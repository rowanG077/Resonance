use super::*;
use resonance_content::battle_effect::{
    SourceBank, UvAnimation, UvChange, UvFrame, declaration::Declaration,
};

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
        programs: vec![],
    }
}

fn particle(data: ParticleTemplate) -> Particle {
    data.validate().unwrap();
    let mut effects = Effects::new(vec![], None, 1, Default::default()).unwrap();
    let id = effects
        .spawn_particle(
            Arc::new(ParticleDefinition {
                model: None,
                resource: 1,
                member: 19,
                data,
            }),
            ActorId(0),
            ActorId(0),
            [0.; 3],
            0.,
        )
        .unwrap()
        .unwrap();
    effects.particles.remove(&id).unwrap()
}

#[test]
fn floor_clamp_keeps_downward_motion_without_bouncing() {
    let mut data = source().particle(0).unwrap().clone();
    data.ground_clamp = true;
    data.ground_restitution = None;
    data.bounce_effect = None;
    data.follow_origin = false;
    data.state.offset = [0., -2., 0.];
    data.state.velocity = [0., -3., 0.];
    data.state.acceleration = [0., -1., 0.];
    data.jerk = [0.; 3];
    let mut particle = particle(data);
    particle.frame.origin[1] = 3.;
    for velocity in [-4., -5., -6.] {
        assert!(particle.step(None, None).unwrap().is_none());
        assert_eq!(particle.frame.state.offset[1], -3.);
        assert_eq!(particle.frame.state.velocity[1], velocity);
    }
}

#[test]
fn floor_child_uses_the_final_motion_tick_without_clamping_or_repeating() {
    for origin_motion in [false, true] {
        let mut data = source().particle(0).unwrap().clone();
        data.follow_origin = false;
        data.follow_orientation = false;
        data.ground_clamp = false;
        data.ground_restitution = None;
        data.bounce_effect = None;
        data.floor_effect = Some(2);
        data.origin_motion = origin_motion;
        data.lifetime = Some(13);
        data.state.offset = [0., 800., 0.];
        data.state.velocity = [0., -65., 0.];
        data.state.acceleration = [0.; 3];
        data.jerk = [0.; 3];
        let mut particle = particle(data);
        for _ in 0..12 {
            assert!(particle.step(None, None).unwrap().is_none());
        }
        let request = particle.step(None, None).unwrap().unwrap();
        assert_eq!(request.appearance.member, 2);
        assert_eq!(request.origin, [0., -45., 0.]);
        assert_eq!(particle.frame.state.velocity, [0., -65., 0.]);
        assert_eq!(particle.age, 13);
        // A continuing particle emits no second child while below the floor.
        assert!(particle.step(None, None).unwrap().is_none());
        let id = particle.frame.id;
        let mut effects = Effects::new(vec![], None, 1, Default::default()).unwrap();
        effects.particles.insert(id, particle);
        effects.advance_particles(&BattleFrame::default()).unwrap();
        assert!(effects.particles.is_empty());
    }
}

#[test]
fn tolerant_particle_fault_removes_the_bad_particle_and_keeps_the_next_one() {
    let diagnostics = resonance_content::diagnostics::Diagnostics::new(false);
    let mut effects = Effects::new(vec![], None, 1, Default::default()).unwrap();
    effects.diagnostics = diagnostics.clone();
    let mut bad = particle(source().particle(0).unwrap().clone());
    bad.frame.state.offset[0] = f32::MAX;
    bad.frame.state.velocity[0] = f32::MAX;
    let mut good = particle(source().particle(0).unwrap().clone());
    good.frame.id = ParticleId(2);
    effects.particles.insert(ParticleId(1), bad);
    effects.particles.insert(ParticleId(2), good);
    effects.advance_particles(&BattleFrame::default()).unwrap();
    assert!(!effects.particles.contains_key(&ParticleId(1)));
    assert_eq!(effects.frames().len(), 1);
    assert_eq!(diagnostics.entries().len(), 1);
    effects.advance_particles(&BattleFrame::default()).unwrap();
    assert_eq!(effects.frames().len(), 1);
    assert_eq!(diagnostics.entries()[0].occurrences, 1);
}

fn frame(duration: u32, rect: [i16; 4]) -> UvFrame {
    UvFrame {
        duration,
        change: UvChange::Rectangle { rect },
    }
}

#[test]
fn ribbon_phase_advances_continuously_at_its_period_and_wraps() {
    for period in [0_i8, 1, -1, 2, -2, -128] {
        let mut data = source().particle(0).unwrap().clone();
        data.lifetime = None;
        data.state.geometry = ParticleGeometry::Ribbon {
            length: 900.,
            width: 176.,
            jitter: 104.,
            phase: 254,
            phase_period: period,
        };
        apply_scale(&mut data.state, 0.5).unwrap();
        let mut p = particle(data);
        let interval = u32::from(period.unsigned_abs());
        let mut previous = 254;
        let mut changes = Vec::new();
        let end = if interval == 0 { 8 } else { 2 * interval };
        for age in 0..=end {
            p.step(None, None).unwrap();
            let ParticleGeometry::Ribbon {
                length,
                width,
                jitter,
                phase,
                ..
            } = p.frame.state.geometry
            else {
                unreachable!()
            };
            assert_eq!((length, width, jitter), (450., 88., 52.));
            if phase != previous {
                changes.push((age, phase));
                previous = phase;
            }
        }
        if interval == 0 {
            assert!(changes.is_empty());
        } else {
            assert_eq!(changes, [(0, 255), (interval, 0), (2 * interval, 1)]);
        }
    }
}

#[test]
fn quad_scale_preserves_all_corners_and_their_motion() {
    let vertices = [[-2., 3., 1.], [4., 5., 2.], [6., -7., 3.], [-8., -9., 4.]];
    let velocity = [[1., 2., 3.], [-1., 3., 2.], [2., -3., 1.], [-2., -1., 4.]];
    for scale in [0., 0.5, 2.] {
        let mut data = source().particle(0).unwrap().clone();
        data.geometry_acceleration_until = None;
        data.state.geometry = ParticleGeometry::Quad { vertices, velocity };
        apply_scale(&mut data.state, scale).unwrap();
        let scaled_velocity = velocity.map(|row| row.map(|value| value * scale));
        assert_eq!(
            data.state.geometry,
            ParticleGeometry::Quad {
                vertices: vertices.map(|row| row.map(|value| value * scale)),
                velocity: scaled_velocity,
            }
        );
        let mut particle = particle(data);
        particle.step(None, None).unwrap();
        assert_eq!(
            particle.frame.state.geometry,
            ParticleGeometry::Quad {
                vertices: std::array::from_fn(|corner| {
                    std::array::from_fn(|axis| {
                        (vertices[corner][axis] + velocity[corner][axis]) * scale
                    })
                }),
                velocity: scaled_velocity,
            }
        );
    }
}

#[test]
fn palette_animation_changes_only_the_color_page() {
    let mut data = source().particle(0).unwrap().clone();
    data.uv_animation = Some(UvAnimation::Frames {
        frames: vec![
            UvFrame {
                duration: 2,
                change: UvChange::Palette { index: 255 },
            },
            frame(2, [4, 5, 6, 7]),
        ],
        loop_start: Some(0),
    });
    let mut p = particle(data);
    p.step(None, None).unwrap();
    assert_eq!(p.frame.state.uv, [1, 65, 30, 30]);
    assert_eq!(p.frame.state.palettes, [255, 0]);
    for _ in 0..4 {
        p.step(None, None).unwrap();
    }
    assert_eq!(p.frame.state.uv, [4, 5, 6, 7]);
    assert_eq!(p.frame.state.palettes, [255, 0]);
}

#[test]
fn scrolling_wraps_both_directions_within_the_rectangle() {
    let mut data = source().particle(0).unwrap().clone();
    data.state.uv = [100, 200, 8, 10];
    data.uv_animation = Some(UvAnimation::Scroll {
        origin: [10, 20],
        step: [17, -1],
        interval: 2,
    });
    let mut p = particle(data);
    p.step(None, None).unwrap();
    p.step(None, None).unwrap();
    assert_eq!(p.frame.state.uv, [100, 200, 8, 10]);
    p.step(None, None).unwrap();
    assert_eq!(p.frame.state.uv, [11, 29, 8, 10]);
    for tick in 1..=4096 {
        p.step(None, None).unwrap();
        let [x, y, width, height] = p.frame.state.uv;
        assert!((10..18).contains(&x) && (20..30).contains(&y));
        assert_eq!((width, height), (8, 10));
        if tick == 2 {
            assert_eq!((x, y), (12, 28));
        }
    }
}

#[test]
fn model_scroll_keeps_signed_offsets_across_a_texture_repeat() {
    let mut data = source().particle(0).unwrap().clone();
    data.state.uv = [0, 0, 2500, 2500];
    data.uv_animation = Some(UvAnimation::Scroll {
        origin: [0; 2],
        step: [1, -15],
        interval: 1,
    });
    let mut p = particle(data);
    Arc::make_mut(&mut p.definition).model = Some(resonance_content::battle_effect::ModelBinding {
        slot: 1,
        animation: None,
        repeat: false,
        reverse_at: None,
        program_animation: false,
        attachment: None,
    });
    p.advance_uv(true).unwrap();
    assert_eq!(p.frame.state.uv, [0, 0, 2500, 2500]);
    for _ in 0..167 {
        p.advance_uv(false).unwrap();
    }
    // The retained material samples cross V=-1 without clamping to the atlas.
    assert_eq!(p.frame.state.uv, [167, -2505, 2500, 2500]);
    assert_eq!(f32::from(p.frame.state.uv[0]) / 2500., 0.0668);
    assert_eq!(f32::from(p.frame.state.uv[1]) / 2500., -1.002);
}

#[test]
fn non_looping_animation_keeps_its_last_frame() {
    let mut data = source().particle(0).unwrap().clone();
    data.uv_animation = Some(UvAnimation::Frames {
        frames: vec![frame(2, [1, 2, 3, 4]), frame(2, [5, 6, 7, 8])],
        loop_start: None,
    });
    let mut p = particle(data);
    p.step(None, None).unwrap();
    p.step(None, None).unwrap();
    assert_eq!(p.frame.state.uv, [1, 2, 3, 4]);
    p.step(None, None).unwrap();
    assert_eq!(p.frame.state.uv, [5, 6, 7, 8]);
    for _ in 0..300 {
        p.step(None, None).unwrap();
        assert_eq!(p.frame.state.uv, [5, 6, 7, 8]);
    }
}

#[test]
fn preparation_checks_frame_duration_loop_bounds_and_scroll_extent() {
    let mut data = source().particle(0).unwrap().clone();
    for animation in [
        UvAnimation::Frames {
            frames: vec![],
            loop_start: None,
        },
        UvAnimation::Frames {
            frames: vec![frame(0, [0; 4])],
            loop_start: None,
        },
        UvAnimation::Frames {
            frames: vec![frame(1, [0; 4])],
            loop_start: Some(1),
        },
        UvAnimation::Frames {
            frames: vec![frame(1, [0, 0, -1, 4])],
            loop_start: None,
        },
        UvAnimation::Scroll {
            origin: [0; 2],
            step: [1; 2],
            interval: 0,
        },
        UvAnimation::Scroll {
            origin: [i16::MAX; 2],
            step: [1; 2],
            interval: 1,
        },
    ] {
        data.uv_animation = Some(animation);
        assert!(data.validate().is_err());
    }
    data.uv_animation = Some(UvAnimation::Frames {
        frames: vec![frame(1, [0; 4]), frame(200, [0, 0, 16, 16])],
        loop_start: Some(1),
    });
    assert!(data.validate().is_ok());
    data.uv_animation = Some(UvAnimation::Scroll {
        origin: [0; 2],
        step: [-1, 1],
        interval: 1,
    });
    data.state.uv[2] = 0;
    assert!(data.validate().is_err());
}

#[test]
fn uv_animation_stays_in_its_frames_and_loops() {
    let frames = [[0, 0, 32, 32], [32, 0, 32, 32]];
    let mut data = source().particle(0).unwrap().clone();
    data.uv_animation = Some(UvAnimation::Frames {
        frames: frames.into_iter().map(|rect| frame(2, rect)).collect(),
        loop_start: Some(0),
    });
    let mut p = particle(data);
    let mut seen = std::collections::BTreeSet::new();
    let mut previous = frames[0];
    let mut wrapped = false;
    for _ in 0..32 {
        p.step(None, None).unwrap();
        let current = p.frame.state.uv;
        assert!(frames.contains(&current));
        seen.insert(current);
        wrapped |= previous == frames[1] && current == frames[0];
        previous = current;
    }
    assert_eq!(seen.len(), frames.len());
    assert!(wrapped);
}

#[test]
fn brightening_and_fading_clamp_color_channels_without_overflow() {
    for (brightening, expected) in [
        (true, [[0, 255, 255, 3], [255, 0, 2, 255]]),
        (false, [[0, 255, 252, 0], [255, 0, 0, 253]]),
    ] {
        let mut data = source().particle(0).unwrap().clone();
        data.gradient = true;
        data.state.colors = [[i16::MIN, i16::MAX, 254, 1], [i16::MAX, i16::MIN, 0, 255]];
        data.state.brighten = [1, 1, 2, 2];
        data.state.brighten_until = brightening.then_some(2);
        data.fade = [1, 1, 2, 2];
        data.fade_from = 0;
        let mut p = particle(data);
        p.step(None, None).unwrap();
        assert_eq!(p.frame.state.colors, expected);
        p.step(None, None).unwrap();
        for (actual, before) in p
            .frame
            .state
            .colors
            .iter()
            .flatten()
            .zip(expected.iter().flatten())
        {
            assert!((0..=255).contains(actual));
            assert!(if brightening {
                actual >= before
            } else {
                actual <= before
            });
        }
    }
}
