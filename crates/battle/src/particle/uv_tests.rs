use super::*;
use crate::{Side, tests::actor};
use resonance_content::battle_effect::{SourceBank, UvRecord, declaration::Declaration};

fn source() -> SourceBank {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/stun-particle-source.json"
    ))
    .unwrap();
    SourceBank {
        art: None,
        source_sha256: fixture["source_sha256"].as_str().unwrap().into(),
        actors: vec![
            serde_json::from_value::<Declaration>(fixture["declaration"].clone()).unwrap(),
        ],
        uv: serde_json::from_value(fixture["uv"].clone()).unwrap(),
        uv_roots: vec![0],
        programs: vec![],
        modifiers: Default::default(),
    }
}

#[test]
fn original_particle_flag_selects_late_updates_without_admitting_other_controllers() {
    let mut bank = source();
    assert!(!bank.particle(0).unwrap().late);
    bank.actors[0].prefix.flags_or_shake_amplitude |= 0x100;
    assert!(bank.particle(0).unwrap().late);
    bank.actors[0].prefix.secondary.flags = 1;
    assert!(bank.particle(0).is_err());
}

fn particle(data: ParticleTemplate) -> Particle {
    data.validate().unwrap();
    let mut battle = Battle::new(crate::tests::prepared(
        "pub task run() {}",
        vec![actor(Side::Party), actor(Side::Enemy)],
        200,
    ));
    let id = battle
        .spawn_particle(
            Arc::new(ParticleDefinition {
                model: None,
                resource: 1,
                member: 19,
                data,
            }),
            Some(ActionId(1)),
            ActorId(0),
            ActorId(0),
            [0.; 3],
            0.,
        )
        .unwrap()
        .unwrap();
    battle.particles.remove(&id).unwrap()
}

#[test]
fn tolerant_particle_fault_removes_the_bad_particle_and_keeps_the_next_one() {
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(false);
    let mut battle = Battle::new(crate::tests::prepared(
        "pub task run() {}",
        vec![actor(Side::Party), actor(Side::Enemy)],
        200,
    ));
    battle.set_diagnostics(diagnostics.clone());
    let mut bad = particle(source().particle(0).unwrap());
    bad.frame.state.offset[0] = f32::MAX;
    bad.frame.state.velocity[0] = f32::MAX;
    let mut good = particle(source().particle(0).unwrap());
    good.frame.id = ParticleId(2);
    battle.particles.insert(ParticleId(1), bad);
    battle.particles.insert(ParticleId(2), good);
    let mut cues = vec![];
    battle.advance_particles(false, &mut cues).unwrap();
    assert!(!battle.particles.contains_key(&ParticleId(1)));
    assert!(battle.particles[&ParticleId(2)].initialized);
    assert!(cues.contains(&Cue::ParticleExpired {
        particle: ParticleId(1)
    }));
    assert!(battle.is_diagnostic());
    assert_eq!(diagnostics.entries().len(), 1);
    battle.advance_particles(false, &mut cues).unwrap();
    assert_eq!(battle.particles[&ParticleId(2)].frame.age, 1);
    assert_eq!(diagnostics.entries()[0].occurrences, 1);
}

fn row(timing: u8, values: [i16; 4]) -> UvRecord {
    UvRecord {
        timing,
        control: 0,
        values,
    }
}

#[test]
fn ribbon_dimensions_stay_fixed_and_phase_uses_the_signed_age_and_period() {
    for period in [0, 1, -1, 2, -2, -128] {
        let mut data = source().particle(0).unwrap();
        data.state.geometry = ParticleGeometry::Ribbon {
            length: 900.,
            width: 176.,
            jitter: 104.,
            phase: 255,
            phase_period: period,
        };
        data.state.orbit = [10., 2., 3.];
        // Ribbon motion uses the scalar angle step, not this vector.
        data.orbit_velocity = [100.; 3];
        apply_scale(&mut data.state, 0.5).unwrap();
        let mut p = particle(data);
        let mut phase = 255_u8;
        for (step, age) in [0_i16, 1, 2, 127, 128, i16::MAX, i16::MIN, -1]
            .into_iter()
            .enumerate()
        {
            p.age = age;
            p.step(false).unwrap();
            if period != 0 && i32::from(age) % i32::from(period) == 0 {
                phase = phase.wrapping_add(1);
            }
            assert_eq!(
                p.frame.state.geometry,
                ParticleGeometry::Ribbon {
                    length: 450.,
                    width: 88.,
                    jitter: 52.,
                    phase,
                    phase_period: period,
                }
            );
            assert_eq!(p.frame.state.orbit, [12. + 2. * step as f32, 2., 3.]);
        }
    }
}

#[test]
fn original_stun_star_initializes_then_loops_at_its_own_clock() {
    let mut p = particle(source().particle(0).unwrap());
    assert_eq!(p.frame.state.uv, [1, 65, 30, 30]);
    for age in 0..=36 {
        p.step(false).unwrap();
        assert_eq!(
            p.frame.state.uv,
            [
                [0, 64, 32, 32],
                [32, 64, 32, 32],
                [64, 64, 32, 32],
                [32, 64, 32, 32]
            ][(age / 4) % 4]
        );
        assert_eq!(p.frame.state.palettes, [17, 0]);
        assert_eq!(p.frame.age, age as i16);
        assert!(!p.retiring);
    }
}

#[test]
fn unflinching_holds_only_the_uv_clock_and_checks_the_old_clock() {
    let mut p = particle(source().particle(0).unwrap());
    for _ in 0..4 {
        p.step(false).unwrap();
    }
    // An already-due change runs even when the increment is held.
    p.step(true).unwrap();
    assert_eq!(p.frame.state.uv, [32, 64, 32, 32]);
    for _ in 0..8 {
        p.step(true).unwrap();
    }
    assert_eq!(p.frame.age, 12);
    assert_eq!(p.frame.state.angles[2], -32.5);
    assert_eq!(p.frame.state.colors[0][3], 255);
    assert_eq!(p.uv_age, 0);
    for _ in 0..4 {
        p.step(false).unwrap();
    }
    assert_eq!(p.frame.state.uv, [32, 64, 32, 32]);
    p.step(false).unwrap();
    assert_eq!(p.frame.state.uv, [64, 64, 32, 32]);
}

#[test]
fn palette_keys_narrow_to_a_byte_without_replacing_the_rectangle() {
    let mut data = source().particle(0).unwrap();
    data.uv_track = vec![
        row(2, [-32000, -1, 0, 0]),
        row(2, [4, 5, 6, 7]),
        row(255, [0; 4]),
    ];
    let mut p = particle(data);
    p.step(false).unwrap();
    assert_eq!(p.frame.state.uv, [1, 65, 30, 30]);
    assert_eq!(p.frame.state.palettes, [255, 0]);
    for _ in 0..4 {
        p.step(false).unwrap();
    }
    assert_eq!(p.frame.state.uv, [4, 5, 6, 7]);
    assert_eq!(p.uv_row, 0);
}

#[test]
fn scrolling_uses_signed_wrapping_and_one_extent_subtraction() {
    let mut data = source().particle(0).unwrap();
    data.state.uv = [100, 200, 8, 10];
    data.uv_track = vec![row(130, [10, 20, 17, -1])];
    let mut p = particle(data);
    p.step(false).unwrap();
    p.step(false).unwrap();
    assert_eq!(p.frame.state.uv, [100, 200, 8, 10]);
    p.step(false).unwrap();
    assert_eq!(p.frame.state.uv, [19, 19, 8, 10]);
    p.step(false).unwrap();
    p.step(false).unwrap();
    assert_eq!(p.frame.state.uv, [28, 18, 8, 10]);
    p.uv_scroll = [32760, -32768];
    p.uv_age = 2;
    p.step(false).unwrap();
    assert_eq!(p.frame.state.uv, [-32749, -32759, 8, 10]);
}

#[test]
fn high_bit_rows_keep_the_native_scroll_behavior_including_254() {
    let mut data = source().particle(0).unwrap();
    data.uv_track = vec![row(0, [1, 2, 3, 4]), row(254, [10, 20, 5, 6])];
    let mut p = particle(data);
    p.step(false).unwrap();
    assert_eq!(p.frame.state.uv, [10, 20, 5, 6]);
    assert_eq!(p.uv_row, 1);
    p.uv_age = 126;
    p.step(false).unwrap();
    assert_eq!(p.uv_age, 1); // 254 is a scrolling interval, not a stop opcode.
    let mut data = source().particle(0).unwrap();
    data.uv_track = vec![row(128, [10, 20, 1, 1])];
    let mut p = particle(data);
    p.step(true).unwrap();
    p.step(true).unwrap();
    assert_eq!(p.frame.state.uv, [12, 22, 30, 30]);
    assert_eq!(p.uv_age, 0);
    p.uv_age = 255;
    p.step(false).unwrap();
    assert_eq!(p.frame.state.uv, [12, 22, 30, 30]); // signed -1 clock
    assert_eq!(p.uv_age, 0);
}

#[test]
fn preparation_checks_reachable_rows_and_uses_direct_source_row_indices() {
    let mut bank = source();
    let expected = bank.particle(0).unwrap().uv_track;
    bank.uv.insert(0, row(255, [0; 4]));
    bank.actors[0].prefix.uv_track = 1;
    bank.uv_roots.clear(); // 426A4 does not consult this table.
    assert_eq!(bank.particle(0).unwrap().uv_track, expected);
    for index in [-2, 6, 127] {
        bank.actors[0].prefix.uv_track = index;
        assert!(bank.particle(0).is_err());
    }
    bank.actors[0].prefix.uv_track = -1;
    bank.uv.clear();
    assert!(bank.particle(0).unwrap().uv_track.is_empty());
    let mut data = source().particle(0).unwrap();
    for track in [
        vec![row(1, [0; 4])],
        vec![row(1, [0; 4]); 128],
        vec![
            row(1, [0; 4]),
            UvRecord {
                timing: 255,
                control: 128,
                values: [0; 4],
            },
        ],
    ] {
        data.uv_track = track;
        assert!(data.validate().is_err());
    }
    // A loop marker is resolved only on entering the successor, once.
    data.uv_track = vec![
        row(0, [0; 4]),
        UvRecord {
            timing: 255,
            control: 1,
            values: [1, 2, 3, 4],
        },
    ];
    let mut p = particle(data);
    p.step(false).unwrap();
    assert_eq!(p.uv_row, 1);
    assert_eq!(p.frame.state.uv, [1, 2, 3, 4]);
}

#[test]
fn particle_pass_observes_its_owner_without_consuming_rng() {
    let mut battle = Battle::new(crate::tests::prepared(
        "pub task run() {}",
        vec![actor(Side::Party), actor(Side::Enemy)],
        200,
    ));
    let data = Arc::new(ParticleDefinition {
        model: None,
        resource: 1,
        member: 19,
        data: source().particle(0).unwrap(),
    });
    battle
        .spawn_particle(
            data.clone(),
            Some(ActionId(1)),
            ActorId(0),
            ActorId(0),
            [0.; 3],
            0.,
        )
        .unwrap();
    battle
        .spawn_particle(data, Some(ActionId(2)), ActorId(1), ActorId(1), [0.; 3], 0.)
        .unwrap();
    battle.actors[0].reaction.unflinching = true;
    let seed = battle.random_state();
    for _ in 0..5 {
        battle.advance_particles(false, &mut vec![]).unwrap();
    }
    let frames = battle.particle_frames();
    assert_eq!(frames[0].state.uv, [0, 64, 32, 32]);
    assert_eq!(frames[1].state.uv, [32, 64, 32, 32]);
    assert_eq!(frames[0].age, frames[1].age);
    assert_eq!(battle.random_state(), seed);
}

#[test]
fn shared_uv_clock_and_rectangles_match_natural_dolphin_contacts() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/opening-particle-uv.json"
    ))
    .unwrap();
    let track: Vec<UvRecord> = serde_json::from_value(fixture["track"].clone()).unwrap();
    let mut particles = std::collections::BTreeMap::new();
    for observation in fixture["observations"].as_array().unwrap() {
        let address = observation["particle"].as_u64().unwrap();
        let before = &observation["before"];
        if before["particle_age"] == 0 {
            // Isolate the common UV operation. The original attached quad's
            // movement/rendering controller is outside this comparison.
            let mut data = source().particle(0).unwrap();
            data.uv_track = track.clone();
            data.state.uv = serde_json::from_value(before["uv"].clone()).unwrap();
            data.state.palettes = serde_json::from_value(before["palettes"].clone()).unwrap();
            particles.insert(address, particle(data));
        }
        let p = particles.get_mut(&address).unwrap();
        let snapshot = |p: &Particle| {
            serde_json::json!({
                "uv": p.frame.state.uv, "palettes": p.frame.state.palettes,
                "row": p.uv_row, "age": p.uv_age, "scroll": p.uv_scroll,
            })
        };
        for key in ["uv", "palettes", "row", "age", "scroll"] {
            assert_eq!(
                snapshot(p)[key],
                before[key],
                "visit {} before {key}",
                observation["index"]
            );
        }
        assert_eq!(i64::from(p.age), before["particle_age"].as_i64().unwrap());
        p.step(observation["hold_uv"].as_bool().unwrap()).unwrap();
        for key in ["uv", "palettes", "row", "age", "scroll"] {
            assert_eq!(
                snapshot(p)[key],
                observation["after"][key],
                "visit {} after {key}",
                observation["index"]
            );
        }
    }
}
