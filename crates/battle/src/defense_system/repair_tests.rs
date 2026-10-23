use super::*;
use crate::{ActorAvailability, BattleInput, RecoveryKind, Sound};

const CORE: ActorId = ActorId(1);
const REPAIR: ActorId = ActorId(6);
const ORBITS: [ActorId; 2] = [ActorId(7), ActorId(8)];

fn prepared() -> Result<(PreparedBattle, DefenseSystemDefinition)> {
    let participants = std::iter::once(crate::tests::actor(Side::Party))
        .chain((0..8).map(|_| crate::tests::actor(Side::Enemy)))
        .enumerate()
        .map(|(index, mut actor)| {
            actor.equipment.stats.defense = 101;
            actor.position = [index as f32 * 100. - 500., 0., 0.];
            (actor, Default::default())
        })
        .collect();
    let definition = DefenseSystemDefinition {
        core: CORE,
        repair: (REPAIR, [200., 0., 0.]),
        repair_heading: -90.,
        orbits: ORBITS.into(),
        repair_cycle: RepairCycle {
            self_ticks: 4,
            repeat_ticks: 2,
            orbit_ticks: 3,
            self_percent: 100,
            orbit_percent: 75,
            defense_divisor: 4,
            defense_multiplier: 3,
        },
        repair_sound: Sound::Cue(104),
        repair_sound_priority: 2,
        messages: Messages {
            automatic: "Auto Repair".into(),
            orbit: "Repair".into(),
            deployment: "Alert".into(),
            duration: 90,
        },
        components: (2..9).map(ActorId).collect(),
        arms: std::array::from_fn(|index| DefenseArm {
            actor: ActorId(index as u8 + 2),
            anchor: MeleeAnchor {
                resource: ResourceId(7),
                bone: 0,
            },
        }),
        deployment: MotionBinding {
            model: ResourceId(7),
            clip: 32,
        },
        deployment_ticks: 120,
        deployment_hp_percent: 66,
        deployment_pose: Pose {
            rate: 0.175,
            blend: 4,
            ..Default::default()
        },
    };
    Ok((
        PreparedBattle::new(participants, Default::default(), 19)?,
        definition,
    ))
}

fn battle() -> Result<Battle> {
    let (prepared, definition) = prepared()?;
    let mut battle = prepared.with_defense_system(definition)?.finish()?;
    let mut models = vec![None; 9];
    models[CORE.index()] = Some(std::sync::Arc::new(crate::tests::model_definition(
        7,
        [(0, 30.), (32, 60.)],
    )));
    battle.set_models(crate::Models::new(
        battle.actors(),
        models,
        Default::default(),
        resonance_content::diagnostics::Diagnostics::new(true),
    )?);
    Ok(battle)
}

fn defeat(battle: &mut Battle, actor: ActorId) {
    battle.actors[actor.index()].hp = 0;
    battle.enter_death(actor, &mut Vec::new());
}

fn visit(battle: &mut Battle) -> Vec<Cue> {
    let mut cues = Vec::new();
    battle.advance_defense_system_on_clock(&mut cues, true);
    cues
}

fn recovered(cues: &[Cue]) -> Vec<(ActorId, i32)> {
    cues.iter()
        .filter_map(|cue| match cue {
            Cue::Recovered {
                actor,
                kind: RecoveryKind::Hp,
                applied,
                ..
            } => Some((*actor, *applied)),
            _ => None,
        })
        .collect()
}

#[test]
fn independent_orbit_progress_survives_self_repair_and_longer_repeated_delay() -> Result<()> {
    let mut battle = battle()?;
    let seed = battle.random.state();
    defeat(&mut battle, ORBITS[0]);
    assert!(recovered(&visit(&mut battle)).is_empty());
    defeat(&mut battle, ORBITS[1]);
    visit(&mut battle);
    assert_eq!(battle.defense_system.orbit_ages, [2, 1]);
    defeat(&mut battle, REPAIR);
    for _ in 0..3 {
        assert!(recovered(&visit(&mut battle)).is_empty());
        assert_eq!(battle.actors[CORE.index()].equipment.stats.defense, 75);
        assert_eq!(battle.defense_system.orbit_ages, [2, 1]);
    }
    let cues = visit(&mut battle);
    assert_eq!(recovered(&cues), [(REPAIR, 100)]);
    assert!(
        matches!(&cues[..2], [Cue::Sound { sound: Sound::Cue(104), priority: 2, .. },
        Cue::EffectNotice { actor: REPAIR, text, duration: 90 }] if text == "Auto Repair")
    );
    assert_eq!(
        battle.actors[REPAIR.index()].availability,
        ActorAvailability::Active
    );
    assert_eq!(
        battle.actors[REPAIR.index()].reaction.protection.remaining,
        120
    );
    // The core's update precedes the repair actor's revival in this visit.
    assert_eq!(battle.actors[CORE.index()].equipment.stats.defense, 75);
    let cues = visit(&mut battle);
    assert_eq!(recovered(&cues), [(ORBITS[0], 75)]);
    assert!(
        matches!(&cues[..2], [Cue::EffectNotice { actor: REPAIR, text, .. },
        Cue::Sound { actor: ActorId(7), .. }] if text == "Repair")
    );
    assert_eq!(battle.actors[CORE.index()].equipment.stats.defense, 101);
    assert_eq!(recovered(&visit(&mut battle)), [(ORBITS[1], 75)]);
    defeat(&mut battle, REPAIR);
    for _ in 0..5 {
        assert!(recovered(&visit(&mut battle)).is_empty());
    }
    assert_eq!(recovered(&visit(&mut battle)), [(REPAIR, 100)]);
    assert_eq!(battle.defense_system.repeat_delay, 4);
    assert_eq!(battle.random.state(), seed);
    Ok(())
}

#[test]
fn authored_parameters_change_repair_health_delay_sound_text_and_defense() -> Result<()> {
    let (prepared, mut definition) = prepared()?;
    definition.repair_cycle.self_ticks = 2;
    definition.repair_cycle.orbit_ticks = 1;
    definition.repair_cycle.self_percent = 25;
    definition.repair_cycle.orbit_percent = 40;
    definition.repair_cycle.defense_divisor = 3;
    definition.repair_cycle.defense_multiplier = 2;
    definition.repair_sound = Sound::Cue(77);
    definition.repair_sound_priority = 1;
    definition.messages.automatic = "Changed repair".into();
    definition.messages.duration = 12;
    let mut battle = prepared.with_defense_system(definition)?.finish()?;
    defeat(&mut battle, REPAIR);
    defeat(&mut battle, ORBITS[0]);
    visit(&mut battle);
    assert_eq!(battle.actors[CORE.index()].equipment.stats.defense, 66);
    let cues = visit(&mut battle);
    assert_eq!(recovered(&cues), [(REPAIR, 25)]);
    assert!(
        matches!(&cues[..2], [Cue::Sound { sound: Sound::Cue(77), priority: 1, .. },
        Cue::EffectNotice { text, duration: 12, .. }] if text == "Changed repair")
    );
    assert_eq!(recovered(&visit(&mut battle)), [(ORBITS[0], 40)]);
    Ok(())
}

#[test]
fn unavailable_repair_divides_defense_before_multiplying_and_restores_initial_value() -> Result<()>
{
    for (base, reduced) in [(101, 75), (102, 75), (103, 75), (104, 78)] {
        let (mut prepared, definition) = prepared()?;
        prepared.actors[CORE.index()].equipment.stats.defense = base;
        let mut battle = prepared.with_defense_system(definition)?.finish()?;
        defeat(&mut battle, REPAIR);
        visit(&mut battle);
        assert_eq!(battle.actors[CORE.index()].equipment.stats.defense, reduced);
        visit(&mut battle);
        assert_eq!(battle.actors[CORE.index()].equipment.stats.defense, reduced);
        battle.enter_revival(REPAIR, &mut Vec::new());
        battle.actors[REPAIR.index()].hp = 100;
        visit(&mut battle);
        assert_eq!(battle.actors[CORE.index()].equipment.stats.defense, base);
    }
    Ok(())
}

#[test]
fn core_cleanup_preempts_due_repairs_and_cannot_restart_them() -> Result<()> {
    let mut battle = battle()?;
    defeat(&mut battle, REPAIR);
    defeat(&mut battle, ORBITS[0]);
    for _ in 0..3 {
        visit(&mut battle);
    }
    defeat(&mut battle, CORE);
    for _ in 0..20 {
        let cues = visit(&mut battle);
        assert!(recovered(&cues).is_empty());
        assert!(
            !cues
                .iter()
                .any(|cue| matches!(cue, Cue::Sound { .. } | Cue::EffectNotice { .. }))
        );
        assert!(
            battle.actors[2..]
                .iter()
                .all(|actor| actor.hp == 0 && !actor.available())
        );
    }
    Ok(())
}

#[test]
fn repair_clocks_pause_with_the_world_and_owner_but_not_core_hit_stop() -> Result<()> {
    let mut battle = battle()?;
    defeat(&mut battle, ORBITS[0]);
    for _ in 0..5 {
        battle.advance_defense_system_on_clock(&mut Vec::new(), false);
        let frame = battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert!(recovered(&frame.cues).is_empty());
    }
    assert_eq!(battle.defense_system.orbit_ages, [0, 0]);
    battle.actors[REPAIR.index()].time_stop = 2;
    visit(&mut battle);
    assert_eq!(battle.defense_system.orbit_ages, [0, 0]);
    battle.actors[REPAIR.index()].time_stop = 0;
    battle.actors[REPAIR.index()].hit_stop = 8;
    battle.actors[CORE.index()].hit_stop = 8;
    for _ in 0..2 {
        assert!(recovered(&visit(&mut battle)).is_empty());
    }
    assert_eq!(recovered(&visit(&mut battle)), [(ORBITS[0], 75)]);
    Ok(())
}

#[test]
fn outside_revival_retains_partial_progress_and_recovery_uses_native_caps() -> Result<()> {
    let mut battle = battle()?;
    defeat(&mut battle, ORBITS[0]);
    visit(&mut battle);
    battle.enter_revival(ORBITS[0], &mut Vec::new());
    battle.actors[ORBITS[0].index()].hp = 100;
    for _ in 0..4 {
        visit(&mut battle);
    }
    assert_eq!(battle.defense_system.orbit_ages, [1, 0]);
    defeat(&mut battle, ORBITS[0]);
    battle.actors[ORBITS[0].index()].equipment.recovery.boost = true;
    battle.actors[ORBITS[0].index()].conditions =
        crate::conditions::Conditions::new(crate::conditions::Layers {
            intrinsic: crate::conditions::Condition::Weak.into(),
            ..Default::default()
        });
    visit(&mut battle);
    let cues = visit(&mut battle);
    assert!(cues.iter().any(|cue| matches!(
        cue,
        Cue::Recovered {
            actor: ActorId(7),
            nominal: 90,
            applied: 50,
            floating: true,
            ..
        }
    )));
    assert_eq!(battle.actors[ORBITS[0].index()].hp, 50);
    Ok(())
}

#[test]
fn ordinary_updates_replay_repair_and_core_cleanup_deterministically() -> Result<()> {
    let replay = || -> Result<Vec<crate::BattleFrame>> {
        let mut battle = battle()?;
        let mut frames = Vec::new();
        for tick in 0..45 {
            match tick {
                0 => defeat(&mut battle, ORBITS[0]),
                1 | 15 => defeat(&mut battle, REPAIR),
                7 => defeat(&mut battle, ORBITS[1]),
                35 => {
                    defeat(&mut battle, REPAIR);
                    defeat(&mut battle, CORE);
                }
                _ => (),
            }
            let mut frame = battle.step(BattleInput {
                paused: (2..5).contains(&tick),
                ..Default::default()
            })?;
            let clock = frame.clock;
            battle.advance_models(&mut frame, clock, true)?;
            if tick >= 35 {
                assert!(
                    frame.actors[2..]
                        .iter()
                        .all(|a| a.hp == 0 && !a.available())
                );
                assert!(recovered(&frame.cues).is_empty());
            }
            frames.push(frame);
        }
        assert!(frames.iter().any(|f| !recovered(&f.cues).is_empty()));
        assert!(!battle.is_diagnostic());
        Ok(frames)
    };
    assert_eq!(replay()?, replay()?);
    Ok(())
}

#[test]
fn prepared_roles_reject_duplicates_unbound_orbits_and_invalid_parameters() -> Result<()> {
    let changes: &[fn(&mut DefenseSystemDefinition)] = &[
        |d| d.orbits[1] = d.orbits[0],
        |d| d.orbits[0] = d.repair.0,
        |d| d.orbits[0] = d.arms[0].actor,
        |d| d.orbits[0] = ActorId(0),
        |d| d.orbits.clear(),
        |d| {
            d.components.pop();
        },
        |d| d.repair_cycle.defense_divisor = 0,
        |d| d.messages.deployment.clear(),
    ];
    for change in changes {
        let (prepared, mut definition) = prepared()?;
        change(&mut definition);
        assert!(prepared.with_defense_system(definition).is_err());
    }
    Ok(())
}
