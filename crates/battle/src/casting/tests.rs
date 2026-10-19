use super::*;
use crate::{
    ActionDefinition, ActionId, ActionRequest, Battle, BattleInput, Control, Cue, PreparedBattle,
    Side,
};

mod clocks;
mod delay;
mod facing;
mod guard_cancel;
mod items;
mod preparation;
mod spell_charge;
mod targets;

const CAST: crate::ActionKey = crate::ActionKey(0);
const ALTERNATE: crate::ActionKey = crate::ActionKey(1);
const NORMAL: crate::ActionKey = crate::ActionKey(2);
const LEARNED: crate::ActionKey = crate::ActionKey(9);

const CASTER_TP: u16 = 47;

fn battle(control: Control, duration: u16, tp: u16) -> Battle {
    prepared_cast(control, duration, tp).finish().unwrap()
}

fn native_cast(action: &mut ActionDefinition) -> &mut crate::CastingDefinition {
    let crate::ActionExecution::Casting(cast) = &mut action.execution else {
        panic!("expected casting action")
    };
    Arc::make_mut(cast)
}

fn casting_definition(prepared: &mut PreparedBattle) -> &mut crate::CastingDefinition {
    native_cast(Arc::make_mut(&mut prepared.resources.actions.entries[0]))
}

fn casting_technique(action: crate::ActionKey, catalogue: u16) -> crate::PreparedTechnique {
    crate::PreparedTechnique {
        player_range: [0., 1000.],
        ai_range: [0., 1000.],
        capabilities: crate::TechniqueCapabilities {
            target: crate::TechniqueTarget::Enemy,
            uses_weapon_reach: true,
            ..Default::default()
        },
        ..crate::tests::technique(action, catalogue)
    }
}

fn prepared_cast(control: Control, duration: u16, tp: u16) -> PreparedBattle {
    let casting = crate::CastingDefinition {
        chant: vec![],
        release_pose: Default::default(),
        hide_primary_weapons: false,
        duration: u32::from(duration),
        profile_ticks: i16::try_from(duration).unwrap(),
        spell_ticks: 0,
        fixed_timing: false,
        recovery: 90,
        release: Arc::new(crate::tests::volley()),
        threat: None,
    };
    let actions = vec![ActionDefinition {
        tp_cost: 7,
        normal: None,
        execution: crate::ActionExecution::Casting(Arc::new(casting)),
    }];
    let mut actor = crate::tests::actor(Side::Party);
    actor.control = control;
    actor.tp = tp;
    actor.equipment.max_tp = 100;
    let mut prepared = PreparedBattle::new(
        vec![
            (actor, Default::default()),
            (crate::tests::actor(Side::Enemy), Default::default()),
        ],
        actions.into(),
        1,
    )
    .unwrap();
    prepared.resources.actor_setup[0]
        .techniques
        .push(casting_technique(CAST, 66));
    prepared
}

fn request() -> BattleInput {
    BattleInput {
        actions: vec![ActionRequest {
            actor: ActorId(0),
            action: CAST,
            target: ActorId(1),
        }],
        ..Default::default()
    }
}

#[test]
fn cast_learning_replaces_only_available_commit_selected_spells() -> anyhow::Result<()> {
    let mut table = resonance_content::arte::Catalogue {
        definitions: vec![Default::default(); 3],
        learning: vec![vec![1, 2]],
    };
    for row in &mut table.definitions[1..] {
        row.required_level = 1;
        row.capabilities.spell = true;
    }
    let catalogue = crate::learning::LearningCatalogue::new(Arc::new(table));
    let member = catalogue.prepare_member(crate::learning::LearningEntry {
        character: 1,
        level: 1,
        balance: 0,
        story_unlocked: true,
        current: [2].into(),
        counts: [(2, 49)].into(),
    })?;
    let attempt = crate::learning::LearningAttempt {
        mode: crate::learning::LearningMode::Casting,
        current: Some(2),
        airborne: false,
    };
    assert_eq!(member.select_after_count(attempt, |_| true, || 0)?, Some(1));
    for available in [false, true] {
        let mut prepared = prepared_cast(Control::Manual, 1, 7);
        prepared.resources.actor_setup[0].techniques[0].catalogue = 2;
        if available {
            let mut learned = prepared.resources.actions[CAST].as_ref().clone();
            learned.tp_cost = 9;
            let cast = native_cast(&mut learned);
            cast.recovery = 3;
            let spell = Arc::make_mut(&mut cast.release);
            spell.projectiles.clear();
            spell.recovery = vec![crate::SpellRecovery {
                duration: 1,
                effect_placement: Default::default(),
                flash: None,
                at: 0,
                benefit: crate::SpellBenefit::Stat {
                    condition: crate::conditions::Condition::AttackUp,
                    percent: 15,
                    duration: 30,
                },
                targets: crate::RecoveryTargets::Target,
                effect: None,
            }];
            let learned = prepared.resources.actions.insert(learned);
            assert_eq!(learned, ALTERNATE);
            let mut technique = casting_technique(learned, 1);
            technique.capabilities.target = crate::TechniqueTarget::Ally;
            prepared.resources.actor_setup[0].techniques.push(technique);
        }
        let mut battle = prepared
            .with_camera(crate::CameraDefinition {
                leader: ActorId(0),
                stage_pitch: 0.,
                auto_zoom: false,
                yaw_limits: None,
            })?
            .with_technique_learning_members(vec![crate::learning::TechniqueLearningMember {
                actor: ActorId(0),
                member: member.clone(),
            }])?
            .finish()?;
        battle.actors[1].overlimit = crate::OverLimit::active(1000)?;
        let parent = battle.step(request())?.actions[0].0;
        battle.step(BattleInput::default())?;
        assert_eq!(battle.casting_remaining(ActorId(0)), Some(0));
        // Select the eligible base spell at the completed chant's actual commit.
        battle.random = crate::state::Random::new(0);
        let expected = if available { ALTERNATE } else { CAST };
        let mut releases = Vec::new();
        let mut residents = 0;
        let mut learning_start = None;
        for tick in 0..180 {
            let frame = battle.step(BattleInput::default())?;
            releases.extend(frame.cues.iter().filter_map(|cue| match cue {
                Cue::Casting {
                    action,
                    phase: CastPhase::Released,
                    ..
                } => Some(*action),
                _ => None,
            }));
            residents += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Released { .. }))
                .count();
            if tick == 0 {
                let sequence = battle.sequence(&parent).unwrap();
                assert_eq!(sequence.action, expected);
                assert_eq!(sequence.definition.tp_cost, if available { 9 } else { 7 });
                assert_eq!(
                    battle.action_recovery_remaining(parent),
                    if available { None } else { Some(90) }
                );
                if available {
                    learning_start = Some((
                        sequence.age,
                        battle.actors[1].overlimit.remaining(),
                        battle.ledger.elapsed_ticks,
                        frame.camera,
                    ));
                    assert_eq!(battle.timed_hold_remaining(), Some(60));
                }
            }
            if available && tick <= 60 {
                assert!(releases.is_empty());
                assert_eq!(residents, 0);
                assert!(battle.projectiles.is_empty());
                assert_eq!(
                    battle.actors[0]
                        .conditions
                        .magnitude(crate::conditions::Condition::AttackUp),
                    0
                );
                let (age, enemy_timer, elapsed, camera) = learning_start.unwrap();
                assert_eq!(battle.sequence(&parent).unwrap().age, age);
                assert_eq!(battle.actors[1].overlimit.remaining(), enemy_timer);
                assert_eq!(battle.ledger.elapsed_ticks, elapsed + tick);
                if tick > 0 {
                    assert_eq!(frame.clock, crate::BattleClock::Learning(ActorId(0)));
                }
                if tick == 30 {
                    assert_ne!(frame.camera, camera);
                    let remaining = battle.timed_hold_remaining();
                    let paused = battle.step(BattleInput {
                        paused: true,
                        ..Default::default()
                    })?;
                    assert_eq!(paused.camera, frame.camera);
                    assert_eq!(battle.timed_hold_remaining(), remaining);
                    assert_eq!(battle.ledger.elapsed_ticks, elapsed + tick);
                }
            }
            if available && tick == 61 {
                assert_eq!(frame.clock, crate::BattleClock::Running);
                assert_eq!(battle.action_recovery_remaining(parent), Some(3));
                assert_eq!(
                    battle.actors[0]
                        .conditions
                        .magnitude(crate::conditions::Condition::AttackUp),
                    15
                );
                assert!(battle.projectiles.is_empty());
                assert_eq!(releases, [ALTERNATE]);
            }
        }
        assert_eq!(releases, [expected]);
        assert_eq!(residents, 1);
        assert_eq!(battle.actors[0].tp, if available { 7 } else { 0 });
        assert_eq!(
            battle.actors[0].casting_state.previous_spell,
            Some(expected)
        );
        assert!(!battle.is_diagnostic());
        assert_eq!(battle.technique_uses(ActorId(0), 2), Some(50));
        if available {
            assert_eq!(battle.technique_acquisitions().len(), 1);
            assert_eq!(battle.technique_acquisitions()[0].action, ALTERNATE);
            assert_eq!(battle.current_techniques(ActorId(0)), Some(&[1, 2].into()));
            assert_eq!(battle.technique_uses(ActorId(0), 1), Some(0));
        } else {
            assert!(battle.technique_acquisitions().is_empty());
            assert_eq!(battle.current_techniques(ActorId(0)), Some(&[2].into()));
            assert_eq!(battle.technique_counts(ActorId(0)), Some(&[(2, 50)].into()));
        }
    }
    Ok(())
}

#[test]
fn casting_advances_through_local_hit_stop_but_menu_holds_the_cast() {
    let has_phase = |frame: &crate::BattleFrame, phase| {
        frame.cues.iter().any(|cue| {
            matches!(cue,
            Cue::Casting { phase: actual, .. } if *actual == phase)
        })
    };
    let mut prepared = prepared_cast(Control::Auto, 20, CASTER_TP);
    casting_definition(&mut prepared).chant = [0, 5, 12]
        .map(|at| {
            (
                at,
                crate::AttackPose {
                    motion: crate::MotionBinding { model: 7, clip: at },
                    start: 0.,
                    end: None,
                    rate: 0.5,
                    blend: 2,
                    repeat: at == 12,
                },
            )
        })
        .into();
    let mut battle = prepared.finish().unwrap();
    battle.actors[0].hit_stop = 20;
    let started = battle.step(request()).unwrap();
    assert!(has_phase(&started, CastPhase::Started));
    assert!(!has_phase(&started, CastPhase::Chanting));
    assert!(
        started
            .model_requests
            .iter()
            .any(|request| matches!(request,
        crate::ModelRequest::Play { motion, .. } if motion.clip == 0))
    );
    let mut poses = vec![0];
    let mut pulses = Vec::new();
    let initial = cast_remaining(&battle);
    for update in 1..=2 {
        let frame = battle.step(BattleInput::default()).unwrap();
        if has_phase(&frame, CastPhase::Chanting) {
            pulses.push(update);
        }
    }
    assert!(cast_remaining(&battle) < initial);
    assert!(battle.actors[0].hit_stop > 0);
    let remaining = cast_remaining(&battle);
    for _ in 0..5 {
        let frame = battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(cast_remaining(&battle), remaining);
        assert_eq!(frame.actors[0].tp, 47);
        assert!(frame.model_requests.is_empty());
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Casting { .. }))
        );
    }
    let mut releases = 0;
    for update in 3..223 {
        let frame = battle.step(BattleInput::default()).unwrap();
        for request in &frame.model_requests {
            if let crate::ModelRequest::Play { motion, .. } = request {
                poses.push(update);
                assert_eq!(u32::from(motion.clip), update);
            }
        }
        if has_phase(&frame, CastPhase::Chanting) {
            pulses.push(update);
        }
        assert!(!has_phase(&frame, CastPhase::Started));
        releases += frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Released { .. }))
            .count();
    }
    assert_eq!(releases, 1);
    assert_eq!(pulses, [1, 9, 17]);
    assert_eq!(poses, [0, 5, 12]);
    assert_eq!(battle.actors[0].tp, 40);
}

fn start_cast(battle: &mut Battle) -> ActionId {
    let mut cues = Vec::new();
    battle
        .start(
            ActionRequest {
                actor: ActorId(0),
                action: CAST,
                target: ActorId(1),
            },
            &mut cues,
        )
        .expect("casting admission");
    match cues.as_slice() {
        [Cue::Started { action, .. }] => *action,
        _ => panic!("casting fixture rejected: {cues:?}"),
    }
}

fn cast_remaining(battle: &Battle) -> u32 {
    battle.casting_remaining(ActorId(0)).expect("live cast")
}

#[test]
fn release_survives_forgetting_then_interruption_and_counts_and_pays_once() -> anyhow::Result<()> {
    {
        let prepared = delay::controlled_prepared(Control::Manual, 2, CASTER_TP);

        let mut battle = prepared
            .with_technique_learning_members(vec![crate::tests::counted_techniques(
                ActorId(0),
                &[66],
                &[(66, 49)],
            )])?
            .finish()?;
        let mut frame = battle.step(request())?;
        assert!(matches!(
            battle.activity(ActorId(0)),
            crate::Activity::Casting { .. }
        ));
        battle.forget_technique(ActorId(0), CAST)?;
        assert_eq!(battle.technique_is_current(ActorId(0), 66), Some(false));
        let mut releases = 0;
        for _ in 0..200 {
            releases += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Released { .. }))
                .count();
            if releases > 0 {
                break;
            }
            frame = battle.step(BattleInput::default())?;
        }
        assert_eq!(releases, 1);
        assert_eq!(battle.actors[0].tp, 40);
        assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
        battle.step(BattleInput {
            interrupt: vec![ActionId(1)],
            ..Default::default()
        })?;
        assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
        battle.actors[0].equipment.casting.reducer = true;
        assert_eq!(
            battle.action_quote(ActorId(0), CAST),
            5,
            "a released spell keeps its repeat discount after caster interruption"
        );
        for _ in 0..120 {
            let frame = battle.step(BattleInput::default())?;
            releases += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Released { .. }))
                .count();
        }
        assert_eq!(releases, 1);
        assert_eq!(battle.actors[0].tp, 40);
        assert_eq!(battle.technique_uses(ActorId(0), 66), Some(50));
        assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
    }
    Ok(())
}

#[test]
fn interruption_before_release_does_not_pay_or_record_a_use() -> anyhow::Result<()> {
    let prepared = prepared_cast(Control::Manual, 50, CASTER_TP);
    let mut battle = prepared
        .with_technique_learning_members(vec![crate::tests::counted_techniques(
            ActorId(0),
            &[66],
            &[(66, 49)],
        )])?
        .finish()?;
    battle.step(request())?;
    for _ in 0..5 {
        battle.step(BattleInput::default())?;
    }
    battle.step(BattleInput {
        interrupt: vec![ActionId(1)],
        ..Default::default()
    })?;
    for _ in 0..100 {
        let frame = battle.step(BattleInput::default())?;
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Released { .. }))
        );
    }
    assert_eq!(battle.actors[0].tp, 47);
    assert_eq!(battle.technique_uses(ActorId(0), 66), Some(49));
    Ok(())
}

#[test]
fn battle_event_clears_released_spells_and_return_cancels_the_held_cast() -> anyhow::Result<()> {
    for released in [false, true] {
        let mut battle = battle(Control::Manual, if released { 2 } else { 60 }, CASTER_TP);
        let formation: Vec<_> = battle.actors.iter().map(|actor| actor.position).collect();
        battle.step(request())?;
        if released {
            for _ in 0..200 {
                if !battle.projectiles.is_empty() {
                    break;
                }
                battle.step(BattleInput::default())?;
            }
            assert!(!battle.volleys.is_empty());
            assert!(!battle.projectiles.is_empty());
        } else {
            for _ in 0..20 {
                if battle.casting_remaining(ActorId(0)).is_some() {
                    break;
                }
                battle.step(BattleInput::default())?;
            }
            assert!(battle.casting_remaining(ActorId(0)).is_some());
        }
        let actor_action = battle.runtime[0].task().action().map(|(id, _)| *id);
        let pending_spells: Vec<_> = battle.volleys.keys().copied().collect();
        let pending_projectiles: Vec<_> = battle.projectiles.keys().copied().collect();
        let hp = battle.actors[1].hp;
        let tp = battle.actors[0].tp;
        battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        let cues = battle.clear_event_effects()?;
        assert!(cues.contains(&Cue::ClearEffects));
        for action in pending_spells {
            assert!(cues.contains(&Cue::Interrupted { action }));
        }
        for projectile in pending_projectiles {
            assert!(cues.contains(&Cue::ProjectileExpired { projectile }));
        }
        assert!(battle.volleys.is_empty());
        assert!(battle.projectiles.is_empty());
        assert_eq!(
            battle.runtime[0].task().action().map(|(id, _)| *id),
            actor_action
        );
        for _ in 0..30 {
            battle.step(BattleInput {
                paused: true,
                ..Default::default()
            })?;
        }
        battle.resume_event_combat(&formation)?;
        assert!(battle.runtime[0].task().action().is_none());
        assert!(battle.casting_remaining(ActorId(0)).is_none());
        assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
        for _ in 0..120 {
            let frame = battle.step(BattleInput::default())?;
            assert!(frame.projectiles.is_empty());
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Released { .. }))
            );
        }
        assert_eq!(battle.actors[1].hp, hp);
        assert_eq!(battle.actors[0].tp, tp);
    }
    Ok(())
}

#[test]
fn recovery_pauses_with_the_menu_and_expires() -> anyhow::Result<()> {
    let mut battle = battle(Control::Auto, 5, CASTER_TP);
    battle.step(request())?;
    for _ in 0..100 {
        battle.step(BattleInput::default())?;
        if battle.action_recovery_remaining(ActionId(1)).is_some() {
            break;
        }
    }
    let remaining = battle
        .action_recovery_remaining(ActionId(1))
        .expect("cast entered recovery");
    for _ in 0..8 {
        battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(
            battle.action_recovery_remaining(ActionId(1)),
            Some(remaining)
        );
    }
    for _ in 0..=remaining {
        battle.step(BattleInput::default())?;
    }
    assert_eq!(battle.action_recovery_remaining(ActionId(1)), None);
    Ok(())
}

#[test]
fn unaffordable_cast_is_rejected_at_admission() {
    let mut battle = battle(Control::Manual, 1, 6);
    let frame = battle.step(request()).unwrap();
    assert!(frame.cues.contains(&Cue::Rejected {
        actor: ActorId(0),
        reason: crate::Rejection::InsufficientTp
    }));
    assert!(frame.actions.is_empty());
    assert_eq!(battle.actors[0].tp, 6);
}

#[test]
fn spell_admission_and_payment_share_repeat_and_physical_modifiers() {
    for (reducer, previous, cost) in [
        (false, Some(CAST), 7),
        (true, Some(ALTERNATE), 7),
        (true, Some(CAST), 5),
    ] {
        let mut prepared = prepared_cast(Control::Manual, 2, cost);
        prepared.actors[0].equipment.casting.reducer = reducer;
        prepared.actors[0].casting_state.previous_spell = previous;
        prepared.actors[0].equipment.damage.physical_arte_boost = true;
        let mut battle = prepared.finish().unwrap();
        let random = battle.random.state();
        assert_eq!(battle.action_quote(ActorId(0), CAST), u32::from(cost));
        assert_eq!(battle.random.state(), random);
        let started = battle.step(request()).unwrap();
        assert!(
            started
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Started { .. }))
        );
        let mut releases = 0;
        for _ in 0..140 {
            let frame = battle.step(BattleInput::default()).unwrap();
            releases += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Released { .. }))
                .count();
        }
        assert_eq!(releases, 1);
        assert_eq!(battle.actors[0].tp, 0);
    }
}

#[test]
fn zero_hp_revival_releases_before_cast_pose_finishes() -> anyhow::Result<()> {
    let mut prepared = prepared_cast(Control::Manual, 1, CASTER_TP);
    let spell = Arc::make_mut(&mut casting_definition(&mut prepared).release);
    spell.projectiles.clear();
    spell.origin = crate::SpellOrigin::Target;
    spell.duration = 1;
    spell.recovery = vec![crate::SpellRecovery {
        duration: 1,
        effect_placement: Default::default(),
        flash: None,
        at: 1,
        benefit: crate::SpellBenefit::Resurrect(0),
        targets: crate::RecoveryTargets::Target,
        effect: None,
    }];
    prepared.resources.actor_setup[0].techniques[0]
        .capabilities
        .target = crate::TechniqueTarget::Ally;
    prepared.actors[1].side = Side::Party;
    prepared.actors[1].hp = 0;
    let mut actors: Vec<_> = prepared
        .actors
        .into_iter()
        .zip(prepared.resources.actor_setup)
        .collect();
    actors.push((crate::tests::actor(Side::Enemy), Default::default()));
    let mut battle = PreparedBattle::new(actors, prepared.resources.actions, 1)?.finish()?;
    let mut model = crate::tests::model_definition(7, [(0, 1.), (1, 15.)]);
    model.reactions.chant = Some(0);
    model.reactions.chant_rate = 0.5;
    model.reactions.cast = Some(1);
    battle.set_models(crate::Models::new(
        battle.actors(),
        vec![Some(Arc::new(model)), None, None],
        Default::default(),
        resonance_content::diagnostics::Diagnostics::new(true),
    )?);
    battle.step(request())?;
    battle.step(BattleInput::default())?;
    let paid = battle.step(BattleInput::default())?;
    assert_eq!(paid.actors[0].tp, CASTER_TP - 7);
    assert!(
        paid.cues
            .iter()
            .any(|cue| matches!(cue, Cue::Released { .. }))
    );
    assert!(!battle.actors[1].available());
    let next = battle.step(BattleInput::default())?;
    assert_eq!(next.actors[0].tp, CASTER_TP - 7);
    assert!(battle.actors[1].available());
    assert_eq!(battle.actors[1].hp, 0);
    let pose = battle
        .models()
        .unwrap()
        .main_motion_observation(ActorId(0))
        .unwrap();
    assert_eq!(pose.clip, 1);
    assert!(pose.frame < pose.end);
    Ok(())
}

#[test]
fn release_opening_keeps_scene_timing_and_respects_interruption() -> anyhow::Result<()> {
    use resonance_content::animation::{
        Track, TransformChannels, VectorCurve, VectorInterpolation,
    };

    for (has_opening, interrupt_at, long_release, has_release) in [
        (true, None, false, true),
        (true, Some(10), false, true),
        (true, Some(40), false, true),
        (false, None, false, true),
        (false, None, true, true),
        (false, None, false, false),
        (false, Some(10), false, false),
    ] {
        let mut prepared = prepared_cast(Control::Manual, 1, CASTER_TP);
        let cast = casting_definition(&mut prepared);
        cast.hide_primary_weapons = true;
        let opening = crate::AttackPose {
            motion: crate::MotionBinding { model: 7, clip: 13 },
            start: 0.,
            end: None,
            rate: 0.5,
            repeat: false,
            blend: 4,
        };
        let release_pose = crate::AttackPose {
            motion: crate::MotionBinding { model: 7, clip: 12 },
            repeat: long_release,
            ..opening
        };
        let release_at = if long_release || !has_release { 0 } else { 26 };
        cast.release_pose = CastReleasePose::Scene {
            opening: has_opening.then_some(opening),
            release: has_release.then_some(release_pose),
            release_loop_start: 0.,
            at: release_at,
        };
        let release = Arc::make_mut(&mut cast.release);
        release.origin = crate::SpellOrigin::SceneFixed { position: [0.; 3] };
        release.transition_ticks = 60;
        release.duration = 70;
        release.projectiles.truncate(1);
        release.projectiles[0].at = 62;
        let mut battle = prepared.finish()?;
        let release_frames = if long_release { 60. } else { 15. };
        let mut model = crate::tests::model_definition(
            7,
            [
                (0, if has_release { 1. } else { 100. }),
                (12, release_frames),
                (13, 1.),
            ],
        );
        model.reactions.chant = Some(0);
        model.reactions.chant_rate = 0.5;
        model.reactions.cast = Some(12);
        for slot in 0..3 {
            model.weapons.push(Arc::new(crate::WeaponDefinition::new(
                slot,
                0,
                vec![crate::WeaponLayerDefinition {
                    resource: 20 + u32::from(slot),
                    skeleton: model.skeleton.clone(),
                    motions: Default::default(),
                    playback: crate::WeaponPlayback::Rigid,
                    secondary_motion: vec![],
                    texture_animation: None,
                }],
                vec![],
            )?));
        }
        model.motions.get_mut(&12).unwrap().tracks.push(Track {
            bone: 0,
            bind_channels: TransformChannels(8),
            period_frames: 15.,
            times: vec![0., 15.],
            translation: Some(VectorCurve {
                interpolation: VectorInterpolation::Linear,
                values: vec![[0.; 3], [15., 0., 0.]],
                incoming: vec![],
                outgoing: vec![],
                ease: vec![],
            }),
            scale: None,
            rotation: None,
            euler_degrees: None,
            matrices: None,
        });
        model.weapons.push(Arc::new(crate::WeaponDefinition::body(
            None,
            vec![crate::WeaponLayerDefinition {
                resource: 23,
                skeleton: model.skeleton.clone(),
                motions: model
                    .motions
                    .iter()
                    .map(|(&clip, motion)| (clip + 40, motion.clone()))
                    .collect(),
                playback: crate::WeaponPlayback::Body {
                    offset: 40,
                    initial: crate::Playback {
                        clip: 40,
                        ..model.initial
                    },
                },
                secondary_motion: vec![],
                texture_animation: None,
            }],
        )?));
        battle.set_models(crate::Models::new(
            battle.actors(),
            vec![
                Some(Arc::new(model)),
                Some(Arc::new(crate::tests::model_definition(8, [(0, 100.)]))),
            ],
            Default::default(),
            resonance_content::diagnostics::Diagnostics::new(true),
        )?);
        let clip = |battle: &Battle| {
            battle
                .models()
                .unwrap()
                .main_motion_observation(ActorId(0))
                .unwrap()
                .clip
        };
        let mut chanting = battle.step(request())?;
        battle.actors[0].overlimit = crate::OverLimit::active(1000)?;
        let clock = chanting.clock;
        battle.advance_models(&mut chanting, clock, clock == crate::BattleClock::Running)?;
        assert!(
            chanting
                .weapons
                .iter()
                .all(|weapon| weapon.visible == (weapon.slot >= 2))
        );
        battle.step(BattleInput::default())?;
        let paid = battle.step(BattleInput::default())?;
        assert_eq!(paid.actors[0].overlimit.remaining(), 998);
        let (resident, parent) = paid
            .cues
            .iter()
            .find_map(|cue| match cue {
                Cue::Released {
                    action,
                    parent: Some(parent),
                    ..
                } => Some((*action, *parent)),
                _ => None,
            })
            .expect("payment must release the scene immediately");
        assert_eq!(paid.actors[0].tp, CASTER_TP - 7);
        let opening_clip = if long_release {
            12
        } else if has_opening {
            13
        } else {
            0
        };
        assert!(paid.cues.contains(&Cue::Casting {
            actor: ActorId(0),
            action: CAST,
            phase: CastPhase::SceneOpening,
        }));
        assert_eq!(clip(&battle), opening_clip);
        assert_eq!(battle.volleys[&resident].age, 1);
        let held = battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert!(held.cues.is_empty());
        assert_eq!(battle.volleys[&resident].age, 1);
        let enemy_frame = battle
            .models()
            .unwrap()
            .main_motion_observation(ActorId(1))
            .unwrap()
            .frame;
        let mut interrupted_frame = None;
        let mut births = 0;
        let mut previous_cast_blend = 0.;
        for elapsed in 1..=62 {
            if interrupt_at == Some(elapsed) {
                // Use the same ownership cancellation as reactions; gameplay input waits
                // for scene framing, whereas this check must survive any lost parent.
                interrupted_frame = Some(
                    battle
                        .models()
                        .unwrap()
                        .main_motion_observation(ActorId(0))
                        .unwrap()
                        .frame,
                );
                battle.interrupt_actor(ActorId(0), &mut Vec::new());
                battle.enter_idle(ActorId(0));
            }
            let previous_cast_frame = battle
                .models()
                .unwrap()
                .main_motion_observation(ActorId(0))
                .unwrap()
                .frame;
            let mut frame = battle.step(BattleInput::default())?;
            assert_eq!(
                frame.cues.contains(&Cue::Casting {
                    actor: ActorId(0),
                    action: CAST,
                    phase: CastPhase::Released,
                }),
                elapsed == 61 && !long_release && interrupt_at.is_none(),
            );
            assert_eq!(
                frame.model_requests.contains(&crate::ModelRequest::Play {
                    actor: ActorId(0),
                    motion: release_pose.motion,
                    pose: release_pose.playback(),
                }),
                has_release && elapsed == release_at && interrupt_at.is_none_or(|at| at > elapsed)
            );
            // The host consumes published pose requests while scene framing holds
            // other actors; there is no contact-sampling advance in this phase.
            let clock = frame.clock;
            battle.advance_models(&mut frame, clock, clock == crate::BattleClock::Running)?;
            assert!(frame.weapons.iter().all(|weapon| {
                weapon.visible == (weapon.slot >= 2 || interrupted_frame.is_some())
            }));
            let pose = battle
                .models()
                .unwrap()
                .main_motion_observation(ActorId(0))
                .unwrap();
            if elapsed <= 60 {
                assert_eq!(frame.models[1].frame, enemy_frame);
                if let Some(held_frame) = interrupted_frame {
                    assert_eq!(pose.frame, held_frame);
                } else if !has_release {
                    assert_eq!(pose.clip, 0);
                    assert!(
                        pose.frame > previous_cast_frame
                            || frame.models[0].blend_weight > previous_cast_blend,
                        "an unchanged cast pose must advance its blend or clip"
                    );
                } else if elapsed == 10 && has_opening {
                    assert!(pose.frame > 0., "the scene opening must animate");
                } else if elapsed == 27 {
                    assert!(frame.models[0].blend_weight > 0.);
                } else if elapsed == 40 || elapsed == 60 {
                    assert!(pose.frame > 0.);
                    assert!(frame.models[0].bones[0][3][0] > 0.);
                    // The host can republish a frame without advancing it twice.
                    battle.advance_models(&mut frame, clock, false)?;
                    assert_eq!(
                        battle
                            .models()
                            .unwrap()
                            .main_motion_observation(ActorId(0))
                            .unwrap()
                            .frame,
                        pose.frame
                    );
                }
            }
            previous_cast_blend = frame.models[0].blend_weight;
            let secondary = frame
                .weapons
                .iter()
                .find(|weapon| weapon.slot == u8::MAX)
                .unwrap();
            assert_eq!(secondary.clip, Some(pose.clip + 40));
            assert_eq!(secondary.frame, pose.frame);
            births += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::ProjectileStarted { .. }))
                .count();
            assert_eq!(frame.actors[0].tp, CASTER_TP - 7);
            if !has_release || interrupt_at.is_some_and(|at| at < 26) {
                assert_ne!(clip(&battle), 12);
            } else {
                assert_eq!(
                    clip(&battle),
                    if elapsed < release_at {
                        opening_clip
                    } else {
                        12
                    }
                );
                if elapsed <= 60 && interrupted_frame.is_none() {
                    assert!(matches!(
                        &battle.sequence(&parent).unwrap().execution,
                        crate::action::Execution::Casting(_)
                    ));
                }
            }
            assert_eq!(births, usize::from(elapsed == 62));
        }
        assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
        if long_release {
            let mut recovered = false;
            for _ in 0..130 {
                let previous = battle
                    .models()
                    .unwrap()
                    .main_motion_observation(ActorId(0))
                    .unwrap();
                let mut frame = battle.step(BattleInput::default())?;
                let clock = frame.clock;
                battle.advance_models(&mut frame, clock, clock == crate::BattleClock::Running)?;
                if !matches!(
                    &battle.sequence(&parent).unwrap().execution,
                    crate::action::Execution::Casting(_)
                ) {
                    assert_eq!(previous.frame, release_frames);
                    assert!(frame.cues.contains(&Cue::Casting {
                        actor: ActorId(0),
                        action: CAST,
                        phase: CastPhase::Released,
                    }));
                    recovered = true;
                    break;
                }
            }
            assert!(
                recovered,
                "a looping release must enter recovery after its first cycle"
            );
            assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
        }
        // Returning to idle restores held equipment even while the scene survives.
        battle.enter_idle(ActorId(0));
        let mut idle = battle.step(BattleInput::default())?;
        let clock = idle.clock;
        battle.advance_models(&mut idle, clock, clock == crate::BattleClock::Running)?;
        assert!(idle.weapons.iter().all(|weapon| weapon.visible));
    }
    Ok(())
}

#[test]
fn resident_cast_pays_once_then_waits_for_its_release_pose_endpoint() -> anyhow::Result<()> {
    for (clip_frames, recovery_frames) in
        [(3., None), (15., None), (3., Some(6.)), (15., Some(22.))]
    {
        let mut battle = battle(Control::Manual, 1, CASTER_TP);
        let mut model = crate::tests::model_definition(
            7,
            [(0, 1.), (1, clip_frames)]
                .into_iter()
                .chain(recovery_frames.map(|frames| (2, frames))),
        );
        model.idle_motions = [Some(0); 2];
        model.reactions.chant = Some(0);
        model.reactions.chant_rate = 0.5;
        model.reactions.cast = Some(1);
        model.reactions.cast_recovery = recovery_frames.map(|_| 2);
        battle.set_models(crate::Models::new(
            battle.actors(),
            vec![Some(Arc::new(model)), None],
            Default::default(),
            resonance_content::diagnostics::Diagnostics::new(true),
        )?);
        let observation = |battle: &Battle| {
            battle
                .models()
                .unwrap()
                .main_motion_observation(ActorId(0))
                .unwrap()
        };
        let released = |frame: &crate::BattleFrame| {
            frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Released { .. }))
        };
        let chanting = battle.step(request())?;
        assert!(!chanting.actors[0].cast_releasing);
        let chanting = battle.step(BattleInput::default())?;
        assert!(!chanting.actors[0].cast_releasing);
        let paid = battle.step(BattleInput::default())?;
        assert_eq!(paid.actors[0].tp, CASTER_TP - 7);
        assert!(paid.actors[0].cast_releasing);
        assert_eq!(
            paid.actors[0].activity,
            crate::Activity::Casting { held: false }
        );
        assert!(!released(&paid));
        let announcement = Cue::Casting {
            actor: ActorId(0),
            action: CAST,
            phase: CastPhase::Released,
        };
        assert!(!paid.cues.contains(&announcement));
        assert_eq!(observation(&battle).clip, 1);
        assert_eq!(observation(&battle).frame, 0.);
        let held = battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert!(!released(&held));
        assert!(held.actors[0].cast_releasing);
        assert_eq!(observation(&battle).frame, 0.);

        // The start frame and four blend updates precede the half-frame clock.
        // A shorter clip must shorten release too; no spell-specific delay is used.
        for _ in 0..4 + (clip_frames * 2.) as u32 {
            let frame = battle.step(BattleInput::default())?;
            assert!(!released(&frame));
            assert!(!frame.cues.contains(&announcement));
            assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
            assert_eq!(frame.actors[0].tp, CASTER_TP - 7);
        }
        assert_eq!(observation(&battle).frame, clip_frames);
        assert!(!observation(&battle).finished);
        let release = battle.step(BattleInput::default())?;
        assert!(released(&release));
        assert!(release.cues.contains(&announcement));
        assert!(release.actors[0].cast_releasing);
        assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
        assert_eq!(release.actors[0].tp, CASTER_TP - 7);
        assert_eq!(observation(&battle).clip, 1);
        let next = battle.step(BattleInput::default())?;
        assert!(!released(&next));
        assert_eq!(next.actors[0].tp, CASTER_TP - 7);
        assert!(next.actors[0].cast_releasing);
        assert_eq!(next.actors[0].activity, crate::Activity::Recovering);
        assert_eq!(observation(&battle).frame, clip_frames);
        assert_eq!(
            observation(&battle).clip,
            1,
            "recovery retains the completed cast pose"
        );
        let ActionExecution::Casting(cast) = &battle.prepared.actions[CAST].execution else {
            unreachable!()
        };
        let mut closing_frames = vec![];
        for _ in 0..cast.recovery {
            let frame = battle.step(BattleInput::default())?;
            assert_eq!(frame.actors[0].tp, CASTER_TP - 7);
            if frame.actors[0].activity == crate::Activity::Recovering {
                let motion = observation(&battle);
                match motion.clip {
                    1 => assert!(closing_frames.is_empty()),
                    2 => closing_frames.push(motion.frame),
                    clip => panic!("unexpected cast recovery clip {clip}"),
                }
            }
        }
        if let Some(frames) = recovery_frames {
            assert_eq!(closing_frames.first(), Some(&0.));
            assert_eq!(closing_frames.last(), Some(&frames));
            assert!(closing_frames.windows(2).all(|pair| pair[0] <= pair[1]));
        } else {
            assert!(closing_frames.is_empty());
        }
        let idle = battle.step(BattleInput::default())?;
        assert!(!idle.actors[0].cast_releasing);
        assert_eq!(observation(&battle).clip, 0);
    }
    for (duration, clip_frames, rate, blend, stored, loop_start) in [
        (40u16, 14., 0.5, 4, false, None),
        (40, 7., 1., 2, false, None),
        (40, 14., 0.5, 4, true, None),
        (40, 7., 1., 2, true, None),
        (45, 38., 0.5, 4, false, None),
        (45, 38., 0.5, 4, false, Some(15.)),
        (1, 7., 1., 2, false, None),
    ] {
        let mut prepared = prepared_cast(Control::Enemy, duration, CASTER_TP);
        prepared.actors[0].side = Side::Enemy;
        prepared.actors[1].side = Side::Party;
        prepared.resources.actor_setup[0].techniques.clear();
        crate::tests::assign_action(&mut prepared, 0, CAST);
        let pose = crate::AttackPose {
            motion: crate::MotionBinding { model: 7, clip: 1 },
            start: 4. * rate,
            end: None,
            rate,
            blend,
            repeat: loop_start.is_some(),
        };
        let cast = casting_definition(&mut prepared);
        cast.release_pose = CastReleasePose::Anticipated {
            release: Some(pose),
            release_loop_start: loop_start.unwrap_or(0.),
        };
        if stored {
            let release = Arc::make_mut(&mut cast.release);
            release.origin = crate::SpellOrigin::SceneFixed { position: [0.; 3] };
            release.transition_ticks = 60;
            release.duration = 70;
            release.projectiles.truncate(1);
            release.projectiles[0].at = 62;
        }
        let mut battle = prepared.finish()?;
        let mut model = crate::tests::model_definition(7, [(0, 1.), (1, clip_frames)]);
        model.idle_motions = [Some(0); 2];
        model.reactions.chant = Some(0);
        model.reactions.chant_rate = 0.5;
        model.reactions.cast = Some(1);
        battle.set_models(crate::Models::new(
            battle.actors(),
            vec![Some(Arc::new(model)), None],
            Default::default(),
            resonance_content::diagnostics::Diagnostics::new(true),
        )?);
        battle.step(request())?;
        let lead = (clip_frames / rate) as u16 + u16::from(blend);
        let pose_start = duration.saturating_sub(lead) + 1;
        let pose_finished =
            pose_start + u16::from(blend) + ((clip_frames - pose.start) / rate) as u16 + 1;
        let release_clock = if stored {
            duration + 1
        } else {
            (duration + 2).max(pose_finished)
        };
        for clock in 1..=release_clock {
            let frame = battle.step(BattleInput::default())?;
            let motion = battle
                .models()
                .unwrap()
                .main_motion_observation(ActorId(0))
                .unwrap();
            assert_eq!(
                frame.actors[0].tp,
                if clock <= duration {
                    CASTER_TP
                } else {
                    CASTER_TP - 7
                }
            );
            assert_eq!(motion.clip, if clock < pose_start { 0 } else { 1 });
            if clock == pose_start {
                assert_eq!(motion.frame, pose.start);
            }
            assert_eq!(
                frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Released { .. })),
                clock == release_clock
            );
            if clock == duration + 1 {
                assert!(frame.actors[0].cast_releasing);
                if duration >= lead {
                    assert_eq!(motion.frame, clip_frames);
                }
            }
            if clock == release_clock && !stored {
                assert!(motion.finished);
                assert_eq!(
                    motion.frame,
                    loop_start.map_or(clip_frames, |start| start + rate)
                );
            }
        }
    }
    Ok(())
}

#[test]
fn trained_cast_replacement_releases_once_and_pays_for_the_selected_spell() -> anyhow::Result<()> {
    const SONG: u16 = 57;
    const JUDGMENT: u16 = 58;
    const REPLACEMENT: u16 = 251;
    for (uses, tp, seed, replaces) in [
        ([50, 50], 100, 0, true),
        ([49, 50], 100, 0, false),
        ([50, 49], 100, 0, false),
        ([50, 50], 99, 0, false),
        ([50, 50], 100, 1, false),
    ] {
        let mut prepared = prepared_cast(Control::Manual, 1, tp);
        prepared.random_seed = seed;
        let mut replacement = prepared.resources.actions[CAST].as_ref().clone();
        replacement.tp_cost = 100;
        assert_eq!(prepared.resources.actions.insert(replacement), ALTERNATE);
        prepared.resources.actions.insert(crate::tests::cast(1, 7));
        let setup = &mut prepared.resources.actor_setup[0];
        setup.techniques = vec![
            casting_technique(CAST, SONG),
            casting_technique(ALTERNATE, REPLACEMENT),
            casting_technique(NORMAL, JUDGMENT),
        ];
        setup.techniques[0].capabilities.target = crate::TechniqueTarget::SelfTarget;
        setup.cast_replacement = Some(crate::CastReplacement {
            input: CAST,
            output: ALTERNATE,
            required_uses: &[(SONG, 50), (JUDGMENT, 50)],
            one_in: 16,
        });
        let mut battle = prepared
            .with_technique_learning_members(vec![crate::tests::counted_techniques(
                ActorId(0),
                &[SONG, JUDGMENT],
                &[(SONG, uses[0]), (JUDGMENT, uses[1]), (REPLACEMENT, 0)],
            )])?
            .finish()?;
        let mut input = request();
        input.actions[0].target = ActorId(0);
        let mut frame = battle.step(input)?;
        let selected = if replaces { ALTERNATE } else { CAST };
        let mut announcements = Vec::new();
        let mut releases = Vec::new();
        for _ in 0..140 {
            for cue in &frame.cues {
                match cue {
                    Cue::Casting {
                        action,
                        phase: CastPhase::Started,
                        ..
                    } => announcements.push(*action),
                    Cue::Released { action, .. } => releases.push(*action),
                    _ => {}
                }
            }
            frame = battle.step(BattleInput::default())?;
        }
        assert_eq!(announcements, [selected]);
        assert_eq!(releases.len(), 1);
        assert_eq!(battle.actors()[0].tp, tp - if replaces { 100 } else { 7 });
        assert_eq!(
            battle.technique_is_current(ActorId(0), REPLACEMENT),
            Some(false)
        );
        assert_eq!(
            battle.technique_uses(ActorId(0), if replaces { REPLACEMENT } else { SONG }),
            Some(if replaces { 1 } else { uses[0] + 1 })
        );
    }
    Ok(())
}
