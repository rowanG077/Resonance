use super::*;

#[cfg(feature = "test-support")]
mod lifecycle;

#[test]
#[ignore = "requires a full current cook at RESONANCE_TEST_ASSETS; CPU only"]
fn prepared_defense_system_follows_content_and_rejects_missing_bindings() -> Result<()> {
    use crate::battle::encounter::{Assets, PrepareOptions};
    use resonance_content::{
        prepared::{Cache, Files},
        session::SessionData,
    };
    use resonance_events::{
        battle::{DefeatPolicy, Encounter, Setup},
        party::Party,
    };
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked")
        });
    let mut cache = Cache::default();
    let files = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
        false
    })?;
    let session = SessionData::load(&files)?;
    let menus = session.rules.as_deref().unwrap();
    let party = Party::new(&session, Default::default())?;
    let options = || PrepareOptions {
        map: 332,
        story: 2500,
        random_seed: 0x2345,
        event_flags: Default::default(),
        hunting_symbol: None,
        devils_arms_unlocked: false,
        world_music: 0,
        colette_state: 0,
        victory_story_flags: [false; 2],
        overlimit_boost: false,
    };
    let (assets, prepared) = Assets::load(
        &root,
        &files,
        menus,
        &session,
        &party,
        Setup {
            route: [0; 5],
            encounter: Encounter::Formation(43),
            arena: 13,
            music: None,
            defeat: DefeatPolicy::ResumeEvent,
        },
        options(),
        &mut cache,
        || false,
    )?;
    let content: Definition = files.json(resonance_content::battle_defense_system::PATH)?;
    let first_enemy = prepared.characters.len();
    let members: Vec<_> = assets
        .enemies
        .spawns
        .iter()
        .enumerate()
        .map(|(index, spawn)| {
            Ok((
                ActorId::from_index(first_enemy + index)?,
                assets.enemies.resources[spawn.resource].monster.id,
            ))
        })
        .collect::<Result<_>>()?;
    let core = members
        .iter()
        .find(|(_, enemy)| *enemy == content.core.enemy)
        .unwrap()
        .0;
    let core_model = prepared.core.models().unwrap().definition(core).unwrap();
    let audio = |sound| Ok(Some(sound));
    let native = definition(&content, &members, core_model, audio)?;
    assert_eq!(native.arms.map(|arm| arm.anchor.bone), [6, 3, 11, 9]);
    assert_eq!(
        prepared.core.actors()[core.index()].position,
        content.core.position
    );
    assert_eq!(
        prepared.core.actors()[core.index()].heading,
        content.core.heading
    );
    assert_eq!(native.deployment.clip, 32);
    assert_eq!(native.deployment_ticks, 120);
    assert_eq!(native.deployment_hp_percent, 66);

    let mut changed = content.clone();
    changed.core.enemy = 11;
    changed.arm = 12;
    changed.orbit = 13;
    changed.repair.enemy = 14;
    changed.arm_slots.reverse();
    changed.repair.position = [300., 20., -50.];
    changed.repair.heading = 27.;
    changed.deployment.clip = 0;
    changed.deployment.ticks = 17;
    changed.deployment.hp_percent = 42;
    changed.deployment.rate = 0.75;
    changed.deployment.blend = 2;
    changed.repair_cycle.self_ticks = 7;
    changed.repair_cycle.orbit_ticks = 3;
    changed.messages.automatic = "Test repair".into();
    changed.repair_sound = 77;
    let renamed: Vec<_> = members
        .iter()
        .map(|&(actor, enemy)| {
            (
                actor,
                if enemy == content.core.enemy {
                    changed.core.enemy
                } else if enemy == content.arm {
                    changed.arm
                } else if enemy == content.orbit {
                    changed.orbit
                } else {
                    changed.repair.enemy
                },
            )
        })
        .collect();
    let translated = definition(&changed, &renamed, core_model, audio)?;
    assert_eq!(translated.arms.map(|arm| arm.anchor.bone), [9, 11, 3, 6]);
    assert_eq!(translated.repair.1, changed.repair.position);
    assert_eq!(translated.repair_heading, 27.);
    assert_eq!(translated.deployment.clip, 0);
    assert_eq!(translated.deployment_ticks, 17);
    assert_eq!(translated.deployment_hp_percent, 42);
    assert_eq!(translated.deployment_pose.rate, 0.75);
    assert_eq!(translated.deployment_pose.blend, 2);
    assert_eq!(translated.repair_cycle, changed.repair_cycle);
    assert_eq!(translated.messages.automatic, "Test repair");
    assert_eq!(translated.repair_sound, resonance_battle::Sound::Cue(77));
    assert_eq!(translated.orbits.len(), usize::from(changed.orbit_count));
    assert!(definition(&changed, &renamed, core_model, |_| Ok(None)).is_err());
    assert!(definition(&changed, &members, core_model, audio).is_err());
    assert!(definition(&changed, &renamed[1..], core_model, audio).is_err());
    let mut missing = changed.clone();
    missing.arm_slots[0] = 9;
    assert!(definition(&missing, &renamed, core_model, audio).is_err());
    missing = changed.clone();
    missing.deployment.clip = u16::MAX;
    assert!(definition(&missing, &renamed, core_model, audio).is_err());
    let mut ambiguous = (**core_model).clone();
    ambiguous.skeleton.bones[0].name = "pa03_duplicate".into();
    assert!(definition(&changed, &renamed, &ambiguous, audio).is_err());
    assert!(!assets.files.diagnostics().has_errors() && !prepared.core.is_diagnostic());
    #[cfg(feature = "test-support")]
    {
        use resonance_battle::{ActorAvailability, BattleClock, BattleInput, Cue, Sound};
        let mut assets = assets;
        let mut recipe = content.clone();
        recipe.repair_cycle.self_ticks = 4;
        recipe.repair_cycle.orbit_ticks = 3;
        recipe.repair_cycle.self_percent = 40;
        recipe.repair_cycle.orbit_percent = 30;
        recipe.repair_sound = 77;
        recipe.messages.automatic = "Prepared repair".into();
        recipe.repair.heading = 27.;
        recipe.camera_yaw_limits = [145., 190.];
        assets.inputs.defense_system = recipe.clone();
        let mut selected = resonance_content::battle_audio::Selection::default();
        let mut live = assets.inputs.prepare_fixture(
            menus,
            options(),
            |sound| {
                crate::battle::audio::collect(
                    assets.audio.as_ref(),
                    &mut selected,
                    assets.files.diagnostics(),
                    sound,
                )
            },
            |actors| {
                actors[core.index()].0.hp = actors[core.index()].0.equipment.max_hp / 2;
                for id in std::iter::once(native.repair.0).chain(native.orbits.iter().copied()) {
                    actors[id.index()].0.hp = 0;
                    actors[id.index()].0.availability = ActorAvailability::Dead;
                }
                Ok(())
            },
        )?;
        assert!(selected.sounds.contains(&77));
        assert!(live.feedback.recovery_tint.is_some());
        live.core.suppress_automatic_decisions();
        let base_defense = live.core.actors()[core.index()].equipment.stats.defense;
        let mut combat_ticks = 0;
        let mut paused = 0;
        let mut repairs = Vec::new();
        let mut notices = Vec::new();
        let mut sounds = 0;
        for _ in 0..240 {
            let pause = combat_ticks == 2 && paused < 5;
            paused += u32::from(pause);
            let mut frame = live.core.step(BattleInput {
                paused: pause,
                ..Default::default()
            })?;
            let running = frame.combat_active && frame.clock == BattleClock::Running;
            combat_ticks += u32::from(running);
            let clock = frame.clock;
            live.core.advance_models(&mut frame, clock, true)?;
            let camera = frame
                .camera
                .as_ref()
                .context("prepared Defense System has no camera")?;
            assert!((145. ..=190.).contains(&camera.yaw));
            if running {
                assert_eq!(frame.actors[native.repair.0.index()].heading, 27.);
                assert_eq!(
                    frame.actors[core.index()].equipment.stats.defense,
                    if combat_ticks <= 4 {
                        base_defense / 4 * 3
                    } else {
                        base_defense
                    }
                );
            }
            for cue in &frame.cues {
                match cue {
                    Cue::Recovered { actor, applied, .. } => {
                        let percent = if *actor == native.repair.0 { 40 } else { 30 };
                        assert_eq!(
                            *applied,
                            frame.actors[actor.index()].equipment.max_hp * percent / 100
                        );
                        let model = frame
                            .models
                            .iter()
                            .find(|model| model.actor == *actor)
                            .context("revived Defense System member has no model frame")?;
                        assert!(model.visible && model.tint[3] != 0 && model.depth_write);
                        repairs.push((combat_ticks, *actor));
                    }
                    Cue::EffectNotice { text, .. } => notices.push(text.clone()),
                    Cue::Sound {
                        sound: Sound::Cue(77),
                        priority: 2,
                        ..
                    } => sounds += 1,
                    _ => (),
                }
            }
            if combat_ticks >= 12 {
                break;
            }
        }
        assert_eq!(paused, 5);
        assert_eq!(
            repairs,
            [
                (4, native.repair.0),
                (7, native.orbits[0]),
                (7, native.orbits[1])
            ]
        );
        assert_eq!(sounds, 3);
        assert_eq!(notices, ["Alert", "Prepared repair", "Repair", "Repair"]);
        assert!(live.core.actors().iter().all(|actor| actor.available()));
        assert!(!assets.files.diagnostics().has_errors() && !live.core.is_diagnostic());
        assets.inputs.defense_system.repair_cycle.self_ticks = 60;
        assets.inputs.defense_system.repair_cycle.repeat_ticks = 20;
        lifecycle::check_repeated_contact_deaths(&assets, menus, options, &native)?;
    }
    Ok(())
}
