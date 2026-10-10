//! Native CPU scenarios at prepared Candidate and command boundaries.
use super::item_tests::{Display, PreparedFixture, enter_battle, escape, prepared_fixture, step};
use super::*;
use crate::battle::{command, lifecycle};
use crate::menu::{
    MenuAction,
    strategy::{Focus, Strategy},
};
use resonance_battle::{
    ActionRequest, Activity, BattleFrame, BattleInput, ButtonInput, item::Release,
};

fn edge() -> ButtonInput {
    ButtonInput {
        held: true,
        pressed: true,
        released: false,
    }
}
fn visit(
    f: &mut PreparedFixture,
    command: command::Input,
) -> Result<(BattleFrame, Vec<command::Event>)> {
    let frame = f.lifecycle.step(
        &mut f.battle,
        lifecycle::Input {
            command,
            ..Default::default()
        },
        &mut f.candidate,
        &mut Display,
    )?;
    Ok((frame, f.lifecycle.take_command_events()))
}
fn shared(
    f: &mut PreparedFixture,
    input: crate::menu::Input,
) -> Result<(BattleFrame, Vec<command::Event>)> {
    // Slot3 deliberately differs from the slot0 command owner. The physical
    // four-pad merge belongs to presentation; this verifies its game boundary.
    visit(
        f,
        command::Input {
            controller: 3,
            shared_menu: input,
            ..Default::default()
        },
    )
}
fn open(f: &mut PreparedFixture) -> Result<()> {
    let (_, events) = visit(
        f,
        command::Input {
            open: edge(),
            ..Default::default()
        },
    )?;
    assert!(events.contains(&command::Event::VoiceStreamsPaused(true)));
    assert!(matches!(
        f.lifecycle.command_frame().unwrap().view,
        command::View::Strip
    ));
    for _ in 0..2 {
        visit(
            f,
            command::Input {
                step: 1,
                ..Default::default()
            },
        )?;
    }
    let (_, events) = visit(
        f,
        command::Input {
            confirm_a: edge(),
            ..Default::default()
        },
    )?;
    assert!(events.contains(&command::Event::Cue(2)));
    let frame = f.lifecycle.command_frame().unwrap();
    assert!(matches!(frame.view, command::View::Strategy(_)));
    assert_eq!(frame.selected, command::Command::Strategy);
    assert_eq!(
        f.lifecycle.command_input_kind(),
        command::InputKind::SharedMenu
    );
    Ok(())
}
fn strategy_pose(f: &PreparedFixture) -> Strategy {
    let command::View::Strategy(state) = f.lifecycle.command_frame().unwrap().view else {
        panic!("missing Strategy frame")
    };
    state
}
fn settle(f: &mut PreparedFixture) -> Result<()> {
    for _ in 0..12 {
        shared(f, None)?;
        if strategy_pose(f).transition.page_fade == 0 {
            return Ok(());
        }
    }
    anyhow::bail!("Strategy did not settle")
}
fn close(f: &mut PreparedFixture) -> Result<()> {
    shared(f, Some(MenuAction::Cancel))?;
    for _ in 0..12 {
        let (_, events) = shared(f, None)?;
        assert!(!events.contains(&command::Event::VoiceStreamsPaused(false)));
        if f.lifecycle.command_input_kind() == command::InputKind::Command {
            assert!(matches!(
                f.lifecycle.command_frame().unwrap().view,
                command::View::Strip
            ));
            return Ok(());
        }
    }
    anyhow::bail!("Strategy did not close")
}

fn edit_setting(f: &mut PreparedFixture, choice: u8) -> Result<()> {
    shared(f, Some(MenuAction::Confirm))?;
    let state = strategy_pose(f);
    let desired = f
        .candidate
        .strategy_page(&state)
        .options()
        .iter()
        .position(|&id| id == usize::from(choice))
        .context("fixture Strategy option unavailable")?;
    for _ in 0..state.option.abs_diff(desired) {
        shared(
            f,
            Some(if desired > state.option {
                MenuAction::Down
            } else {
                MenuAction::Up
            }),
        )?;
    }
    shared(f, Some(MenuAction::Confirm))?;
    Ok(())
}
pub(super) fn field_call(party: Party) -> Result<resonance_events::EventRuntime> {
    use resonance_events::{EventRuntime, GameWorld, ResourceLibrary};
    use symphonia_script::{NativeCall, Program};
    let mut world = GameWorld::default();
    world.party = Some(party);
    // The same suspended native caller as the existing once-only return tests.
    let mut words = vec![4u16, 0, 0, 0];
    for arg in [1i32, 13, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        words.extend([
            0x0200,
            arg as u16,
            (arg as u32 >> 16) as u16,
            0x3000,
            0x4000,
        ]);
    }
    words.extend([0x2000 | NativeCall::StartBattle as u16, 0x3000, 0x20ff]);
    EventRuntime::with_state(
        Arc::new(Program::decode(
            &words
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        )?),
        Arc::new(ResourceLibrary::default()),
        world,
        Default::default(),
    )
}

#[test]
#[ignore = "requires cooked Strategy and encounter assets; CPU only"]
fn shared_strategy_holds_real_item_and_chant_then_refreshes_before_strip_return() -> Result<()> {
    for casting in [false, true] {
        let mut f = prepared_fixture(&[3, 1], 2, if casting { &[(3, 66)] } else { &[] })?;
        enter_battle(&mut f.candidate, &mut f.battle)?;
        let user = f.candidate.setup.actors[0].0;
        if casting {
            let target = f.battle.target(user).context("missing Fire Ball target")?;
            let action = f.battle.prepared_technique(user, 66).unwrap().action;
            f.candidate.world_update(
                &mut f.battle,
                BattleInput {
                    actions: vec![ActionRequest {
                        actor: user,
                        action,
                        target,
                    }],
                    ..Default::default()
                },
            )?;
            for _ in 0..4 {
                step(&mut f.candidate, &mut f.battle)?;
            }
            assert!(matches!(
                f.battle.activity(user),
                Activity::Casting { held: false }
            ));
        }
        f.battle.queue_item(Release {
            user,
            target: user,
            item: 1,
        })?;
        if !casting {
            step(&mut f.candidate, &mut f.battle)?;
            assert_eq!(f.battle.activity(user), Activity::Item);
        }
        open(&mut f)?;
        let held = f.battle.snapshot();
        let random = f.battle.random_state();
        let ledger = f.battle.ledger().clone();
        let pending = f.battle.pending_item();
        let old_policy = f.battle.companion_policy(user).unwrap();
        settle(&mut f)?;
        assert_eq!(strategy_pose(&f).focus, Focus::Character);
        shared(&mut f, Some(MenuAction::Confirm))?;
        shared(&mut f, Some(MenuAction::Confirm))?;
        shared(&mut f, Some(MenuAction::Down))?;
        let choice = {
            let state = strategy_pose(&f);
            f.candidate.strategy_page(&state).options()[state.option] as u8
        };
        shared(&mut f, Some(MenuAction::Confirm))?;
        assert_eq!(f.candidate.party.members[2].strategy[0], choice);
        assert_eq!(f.field.members[2].strategy, old_policy.choices);
        assert_eq!(f.battle.companion_policy(user), Some(old_policy));
        shared(&mut f, Some(MenuAction::Down))?; // Skill group.
        edit_setting(&mut f, 2)?;
        shared(&mut f, Some(MenuAction::Down))?; // Position group.
        edit_setting(&mut f, 3)?;
        assert_eq!(f.candidate.party.members[2].strategy, [choice, 2, 3]);
        assert_eq!(f.battle.companion_policy(user), Some(old_policy));
        assert_eq!(f.battle.snapshot().actors, held.actors);
        assert_eq!(f.battle.snapshot().actions, held.actions);
        assert_eq!(f.battle.snapshot().targets, held.targets);
        assert_eq!(f.battle.random_state(), random);
        assert_eq!(f.battle.ledger(), &ledger);
        assert_eq!(f.battle.pending_item(), pending);
        assert_eq!(f.battle.item_cooldown(), held.item_cooldown);
        shared(&mut f, Some(MenuAction::Cancel))?; // Setting -> Character.
        close(&mut f)?;
        assert!(matches!(
            f.lifecycle.command_frame().unwrap().view,
            command::View::Strip
        ));
        assert_eq!(
            f.battle.companion_policy(user).unwrap().choices,
            [choice, 2, 3]
        );
        let mut refreshed = held.actors.clone();
        refreshed[user.index()].guard.recovery_bonus = 25;
        let (returned, events) = visit(&mut f, Default::default())?;
        assert_eq!(
            f.battle.companion_policy(user).unwrap().choices,
            [choice, 2, 3]
        );
        assert_eq!(returned.actors, refreshed);
        assert_eq!(returned.actions, held.actions);
        assert_eq!(returned.targets, held.targets);
        assert_eq!(f.battle.random_state(), random);
        assert_eq!(f.battle.ledger(), &ledger);
        assert_eq!(f.battle.pending_item(), pending);
        assert!(matches!(
            f.lifecycle.command_frame().unwrap().view,
            command::View::Strip
        ));
        assert!(!events.contains(&command::Event::VoiceStreamsPaused(false)));
        let (_, events) = visit(
            &mut f,
            command::Input {
                cancel_b: edge(),
                ..Default::default()
            },
        )?;
        assert_eq!(
            events
                .iter()
                .filter(|event| **event == command::Event::VoiceStreamsPaused(false))
                .count(),
            1
        );
        assert!(f.lifecycle.command_frame().is_none());
        // The source action and reservation continue after the actual strip
        // closes; the shared page never restarts or completes either task.
        for _ in 0..600 {
            step(&mut f.candidate, &mut f.battle)?;
            if f.battle.pending_item().is_none() {
                break;
            }
        }
        assert!(f.battle.pending_item().is_none());
        assert_eq!(f.candidate.items().counts[&1], 1);
        assert_eq!(f.battle.ledger().items[user.index()], 1);
        assert!(!f.battle.is_diagnostic());
        let outcome = escape(&mut f.candidate, &mut f.battle)?;
        let completed = f.candidate.finish(&f.battle, &outcome)?;
        let mut events = field_call(f.field.clone())?;
        let request = events
            .world
            .battle_request
            .take()
            .context("missing suspended field caller")?;
        let duplicate = Completed {
            party: completed.party.clone(),
            gameplay_random: completed.gameplay_random,
            result: completed.result,
        };
        completed.commit(&mut events.world, &request)?;
        let saved = serde_json::to_vec(events.world.party.as_ref().unwrap())?;
        assert!(duplicate.commit(&mut events.world, &request).is_err());
        assert_eq!(
            serde_json::to_vec(events.world.party.as_ref().unwrap())?,
            saved
        );
        let reloaded: Party = serde_json::from_slice(&saved)?;
        assert_eq!(reloaded.members[2].strategy, [choice, 2, 3]);
        assert_eq!(reloaded.battles.items[2], f.field.battles.items[2] + 1);
        assert_eq!(f.field.items[&1], 2);
    }
    Ok(())
}

#[test]
#[ignore = "requires cooked Strategy and encounter assets; CPU only"]
fn failed_strategy_return_precedes_strip_input_and_invalidates_the_lifecycle() -> Result<()> {
    let mut f = prepared_fixture(&[3, 1], 2, &[])?;
    enter_battle(&mut f.candidate, &mut f.battle)?;
    open(&mut f)?;
    settle(&mut f)?;
    shared(&mut f, Some(MenuAction::Confirm))?;
    edit_setting(&mut f, 2)?;
    shared(&mut f, Some(MenuAction::Cancel))?;
    let first = f.candidate.setup.actors[0].0;
    let last = f.candidate.setup.actors[1].0;
    let first_policy = f.battle.companion_policy(first);
    let last_policy = f.battle.companion_policy(last);
    // A bad saved row prevents publication when the page closes.
    f.candidate.party.members[0].strategy = [9, 1, 1];
    let held = f.battle.snapshot();
    let random = f.battle.random_state();
    let ledger = f.battle.ledger().clone();
    let error = close(&mut f).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("invalid companion strategy choices")
    );
    assert!(f.lifecycle.command_frame().is_none());
    assert!(f.lifecycle.take_command_events().is_empty());
    assert_eq!(f.battle.phase(), BattlePhase::Finished);
    assert_eq!(f.battle.snapshot().actors, held.actors);
    assert_eq!(f.battle.random_state(), random);
    assert_eq!(f.battle.ledger(), &ledger);
    assert_eq!(f.battle.companion_policy(first), first_policy);
    assert_eq!(f.battle.companion_policy(last), last_policy);
    assert_eq!(f.candidate.party.members[2].strategy[0], 2);
    assert!(visit(&mut f, Default::default()).is_err());
    Ok(())
}
