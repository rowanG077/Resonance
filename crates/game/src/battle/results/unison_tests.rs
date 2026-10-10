//! Prepared Unison pages edit persistent and live shortcuts while battle is paused.
use super::item_tests::{
    Display, PreparedFixture, enter_battle, escape, prepared_fixture_with_story, step,
};
use super::*;
use crate::battle::{command, lifecycle};
use crate::menu::{
    MenuAction,
    unison::{Focus, UNLOCK_STORY, Unison},
};
use resonance_battle::{
    ActionRequest, Activity, BattleFrame, BattleInput, ButtonInput, item::Release,
};

fn fixture(story: i32) -> Result<PreparedFixture> {
    prepared_fixture_with_story(&[3, 1], 2, &[(3, 66), (1, 1)], story, |party, _, _| {
        party.members[0].level = 1;
        party.members[2].level = 2;
        party.members[0].shortcuts = [1, 0, 0, 0];
        party.members[2].shortcuts = [66, 0, 0, 0];
        party.members[0].technique_uses.insert(1, 17);
        party.members[2].technique_uses.insert(66, 49);
        Ok(())
    })
}
fn edge() -> ButtonInput {
    ButtonInput {
        held: true,
        pressed: true,
        released: false,
    }
}
fn ready(f: &mut PreparedFixture) -> Result<()> {
    enter_battle(&mut f.candidate, &mut f.battle)
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
    visit(
        f,
        command::Input {
            controller: 3,
            shared_menu: input,
            ..Default::default()
        },
    )
}
fn open(f: &mut PreparedFixture) -> Result<Vec<command::Event>> {
    visit(
        f,
        command::Input {
            open: edge(),
            ..Default::default()
        },
    )?;
    assert!(matches!(
        f.lifecycle.command_frame().unwrap().view,
        command::View::Strip
    ));
    visit(
        f,
        command::Input {
            step: 1,
            ..Default::default()
        },
    )?;
    Ok(visit(
        f,
        command::Input {
            confirm_a: edge(),
            ..Default::default()
        },
    )?
    .1)
}
fn pose(f: &PreparedFixture) -> Unison {
    let command::View::Unison(page) = f.lifecycle.command_frame().unwrap().view else {
        panic!("missing U. Attack retained frame")
    };
    page
}
fn assign(f: &mut PreparedFixture) -> Result<()> {
    shared(f, Some(MenuAction::Confirm))?;
    assert_eq!(pose(f).focus, Focus::List);
    shared(f, Some(MenuAction::Confirm))?;
    assert_eq!(pose(f).focus, Focus::Slots);
    Ok(())
}
fn close(f: &mut PreparedFixture) -> Result<()> {
    shared(f, Some(MenuAction::Cancel))?;
    assert!(matches!(
        f.lifecycle.command_frame().map(|frame| frame.view),
        Some(command::View::Strip)
    ));
    Ok(())
}

#[test]
#[ignore = "requires prepared opening battle and menus; no devices"]
fn unison_locked_row_stays_locked_and_unlocked_entry_refreshes_party() -> Result<()> {
    let mut f = fixture(UNLOCK_STORY - 1)?;
    ready(&mut f)?;
    let before = serde_json::to_value(&f.candidate.party)?;
    assert!(f.candidate.begin_unison(&f.battle, 0).is_err());
    assert_eq!(serde_json::to_value(&f.candidate.party)?, before);
    assert!(open(&mut f)?.contains(&command::Event::Cue(4)));
    assert!(matches!(
        f.lifecycle.command_frame().unwrap().view,
        command::View::Strip
    ));
    assert_eq!(
        f.lifecycle.command_input_kind(),
        command::InputKind::Command
    );

    let mut f = fixture(UNLOCK_STORY)?;
    ready(&mut f)?;
    f.candidate.party.members[2].hp = 1;
    let state = f.candidate.begin_unison(&f.battle, 1)?;
    assert_eq!(state.character, 1);
    assert_ne!(f.candidate.party.members[2].hp, 1);
    Ok(())
}

#[test]
#[ignore = "requires prepared opening battle and menus; no devices"]
fn empty_unison_choices_are_disabled_and_cancel_keeps_battle_running() -> Result<()> {
    let mut f = prepared_fixture_with_story(&[1], 2, &[], UNLOCK_STORY, |party, _, _| {
        party.members[0].techniques.clear();
        party.members[0].shortcuts = [0; 4];
        Ok(())
    })?;
    ready(&mut f)?;
    open(&mut f)?;
    let before = serde_json::to_value(&f.candidate.party)?;
    let held = f.battle.snapshot();
    let (_, events) = shared(&mut f, Some(MenuAction::Confirm))?;
    assert!(events.contains(&command::Event::Cue(4)));
    assert_eq!(pose(&f).focus, Focus::Slots);
    assert_eq!(serde_json::to_value(&f.candidate.party)?, before);
    assert_eq!(f.battle.snapshot().actors, held.actors);
    close(&mut f)?;
    assert_eq!(f.battle.phase(), resonance_battle::BattlePhase::Combat);
    Ok(())
}

#[test]
#[ignore = "requires prepared opening battle and menus; no devices"]
fn unison_edits_actual_party_and_live_slots_without_reloading_held_cast_or_item() -> Result<()> {
    for casting in [false, true] {
        let mut f = fixture(UNLOCK_STORY)?;
        ready(&mut f)?;
        let actor = f.candidate.setup.actors[0].0;
        if casting {
            let target = f.battle.target(actor).context("missing Fire Ball target")?;
            let action = f.battle.prepared_technique(actor, 66).unwrap().action;
            f.candidate.world_update(
                &mut f.battle,
                BattleInput {
                    actions: vec![ActionRequest {
                        actor,
                        target,
                        action,
                    }],
                    ..Default::default()
                },
            )?;
            for _ in 0..4 {
                step(&mut f.candidate, &mut f.battle)?;
            }
            assert!(matches!(
                f.battle.activity(actor),
                Activity::Casting { held: false }
            ));
        }
        f.battle.queue_item(Release {
            user: actor,
            target: actor,
            item: 1,
        })?;
        if !casting {
            step(&mut f.candidate, &mut f.battle)?;
            assert_eq!(f.battle.activity(actor), Activity::Item);
        }
        open(&mut f)?;
        assert!(matches!(
            f.lifecycle.command_frame().unwrap().view,
            command::View::Unison(_)
        ));
        let held = f.battle.snapshot();
        let pending = f.battle.pending_item();
        shared(&mut f, Some(MenuAction::Alternate))?;
        assert_eq!(f.candidate.party.members[2].shortcuts[0], 0);
        assert_eq!(f.battle.shortcuts(actor).unwrap()[0], 0);
        shared(&mut f, Some(MenuAction::Down))?;
        assign(&mut f)?;
        assert_eq!(f.candidate.party.members[2].shortcuts, [0, 66, 0, 0]);
        assert_eq!(f.battle.shortcuts(actor).unwrap()[1], 66);
        assert_eq!(f.field.members[2].shortcuts, [66, 0, 0, 0]);
        assert_eq!(f.battle.snapshot().actors, held.actors);
        assert_eq!(f.battle.snapshot().actions, held.actions);
        assert_eq!(f.battle.pending_item(), pending);
        close(&mut f)?;
        visit(
            &mut f,
            command::Input {
                cancel_b: edge(),
                ..Default::default()
            },
        )?;
        for _ in 0..600 {
            step(&mut f.candidate, &mut f.battle)?;
            if f.battle.pending_item().is_none() {
                break;
            }
        }
        assert!(f.battle.pending_item().is_none());
        assert_eq!(f.candidate.items().counts[&1], 1);
        assert_eq!(f.battle.ledger().items[actor.index()], 1);
        let outcome = escape(&mut f.candidate, &mut f.battle)?;
        let completed = f.candidate.finish(&f.battle, &outcome)?;
        let saved = serde_json::to_vec(&completed.party)?;
        let reloaded: Party = serde_json::from_slice(&saved)?;
        assert_eq!(reloaded.members[2].shortcuts, [0, 66, 0, 0]);
    }
    Ok(())
}
