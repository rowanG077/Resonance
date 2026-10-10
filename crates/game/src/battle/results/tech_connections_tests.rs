//! Live Tech commands use current controller connections and commit edits to the battle.
use super::item_tests::{Display, PreparedFixture, enter_battle, prepared_fixture_with_party};
use super::*;
use crate::battle::{command, lifecycle};
use crate::menu::{
    MenuAction,
    techniques::{Focus, Tech},
};
use resonance_battle::{ButtonInput, Control};

const PLUGGED: [bool; 4] = [false, true, false, false];
const NO_PADS: [bool; 4] = [false; 4];

fn fixture() -> Result<PreparedFixture> {
    let mut fixture =
        prepared_fixture_with_party(&[1, 3], 2, &[(1, 1), (3, 66)], |party, _, _| {
            party.settings.battle_controls = [0, 2, 0, 0];
            party.members[0].shortcuts[0] = 1;
            party.members[2].shortcuts[0] = 66;
            Ok(())
        })?;
    enter_battle(&mut fixture.candidate, &mut fixture.battle)?;
    Ok(fixture)
}

fn edge() -> ButtonInput {
    ButtonInput {
        pressed: true,
        held: true,
        released: false,
    }
}

#[test]
#[ignore = "requires locally cooked battle assets; CPU only"]
fn tech_page_uses_live_cost_and_rejects_unaffordable_confirmation() -> Result<()> {
    use resonance_battle::EquipmentReplacement;
    let mut f = fixture()?;
    let actor = Candidate::tech_actor(&f.candidate.setup, 2)?;
    let original_conditions = f.battle.actors()[actor.index()].conditions.clone();
    let mut equipped = f.candidate.party.members[2].clone();
    const FAIRY_RING: u16 = 407;
    equipped.equipment[3] = FAIRY_RING;
    let (mut attributes, conditions) = crate::battle::party::loadout(
        &f.candidate.menus,
        &equipped,
        2,
    )?
    .equipment_attributes(&equipped, &original_conditions, false)?;
    attributes.max_tp = 3;
    f.battle
        .replace_equipment_batch(vec![EquipmentReplacement {
            actor,
            attributes,
            conditions,
            equipment: None,
        }])?;
    let mut page = f.candidate.begin_tech(&mut f.battle, 1, NO_PADS)?;
    page.state.focus = Focus::List;
    page.state.row = f
        .candidate
        .battle_tech_view(&page.state, &NO_PADS, &f.battle)
        .technique_list()
        .iter()
        .position(|&id| id == 66)
        .unwrap();
    let view = f
        .candidate
        .battle_tech_view(&page.state, &NO_PADS, &f.battle);
    assert_eq!(view.technique_cost(2, 66), Some(3));
    assert!(view.ready(2, 66));
    assert_eq!(view.technique_cost(2, u16::MAX), None);
    assert!(!view.ready(2, u16::MAX));
    let confirm = Some(MenuAction::Confirm);
    let visit = f
        .candidate
        .step_tech(&mut f.battle, &mut page, actor, confirm)?;
    assert_eq!(visit.exit.and_then(|exit| exit.technique), Some(66));
    assert_eq!(
        f.battle.actors()[actor.index()].tp,
        3,
        "selecting a target has not started a cast"
    );

    let attributes = f.battle.actors()[actor.index()].equipment.clone();
    f.battle
        .replace_equipment_batch(vec![EquipmentReplacement {
            actor,
            attributes,
            conditions: original_conditions,
            equipment: None,
        }])?;
    page.state.focus = Focus::List;
    let view = f
        .candidate
        .battle_tech_view(&page.state, &NO_PADS, &f.battle);
    assert_eq!(view.technique_cost(2, 66), Some(7));
    assert!(!view.ready(2, 66));
    let before = page.state.clone();
    let visit = f
        .candidate
        .step_tech(&mut f.battle, &mut page, actor, confirm)?;
    assert_eq!(visit.cue, Some(4));
    assert_eq!(visit.exit, None);
    assert_eq!(page.state, before);
    assert_eq!(f.battle.actors()[actor.index()].tp, 3);
    Ok(())
}

fn command(f: &mut PreparedFixture, input: command::Input) -> Result<command::Frame> {
    f.lifecycle.step(
        &mut f.battle,
        lifecycle::Input {
            command: input,
            ..Default::default()
        },
        &mut f.candidate,
        &mut Display,
    )?;
    f.lifecycle
        .command_frame()
        .context("Tech command frame is missing")
}

fn page_visit(
    f: &mut PreparedFixture,
    connected: [bool; 4],
    input: crate::menu::Input,
) -> Result<(Tech, bool)> {
    let frame = command(
        f,
        command::Input {
            connected,
            shared_menu: input,
            ..Default::default()
        },
    )?;
    let command::View::Tech(state) = &frame.view else {
        anyhow::bail!("Tech page closed unexpectedly");
    };
    assert_eq!(frame.connected, connected);
    let available = f
        .candidate
        .battle_tech_view(state, &frame.connected, &f.battle)
        .tech_unison_available();
    Ok((state.clone(), available))
}

fn open_character(f: &mut PreparedFixture, connected: [bool; 4]) -> Result<()> {
    f.lifecycle
        .restore_menu_memory(crate::battle::command::Memory {
            tech_character: 1,
            ..Default::default()
        });
    f.lifecycle.step(
        &mut f.battle,
        lifecycle::Input {
            command: command::Input {
                connected,
                open: edge(),
                ..Default::default()
            },
            ..Default::default()
        },
        &mut f.candidate,
        &mut Display,
    )?;
    command(
        f,
        command::Input {
            connected,
            ..Default::default()
        },
    )?;
    command(
        f,
        command::Input {
            connected,
            confirm_a: edge(),
            ..Default::default()
        },
    )?;
    let (state, _) = page_visit(f, connected, Some(MenuAction::Up))?;
    assert_eq!((state.character, state.focus), (1, Focus::Character));
    Ok(())
}

#[test]
#[ignore = "requires prepared battle assets; CPU only"]
fn connections_refresh_an_open_page_and_cancel_returns_to_the_command_strip() -> Result<()> {
    let mut f = fixture()?;
    open_character(&mut f, PLUGGED)?;
    let controls = f.candidate.party.settings.battle_controls;
    let (blocked, available) = page_visit(&mut f, PLUGGED, Some(MenuAction::Menu))?;
    assert!(!available && !blocked.unison);
    let (opened, available) = page_visit(&mut f, NO_PADS, Some(MenuAction::Menu))?;
    assert!(available && opened.unison);
    let (retained, available) = page_visit(&mut f, PLUGGED, None)?;
    assert!(!available && retained.unison);
    let (closed_panel, _) = page_visit(&mut f, PLUGGED, Some(MenuAction::Cancel))?;
    assert!(!closed_panel.unison);
    assert_eq!(closed_panel.focus, Focus::Character);
    let closed = command(
        &mut f,
        command::Input {
            connected: PLUGGED,
            shared_menu: Some(MenuAction::Cancel),
            ..Default::default()
        },
    )?;
    assert!(matches!(closed.view, command::View::Strip));
    assert_eq!(f.candidate.party.settings.battle_controls, controls);
    Ok(())
}

#[test]
#[ignore = "requires prepared battle assets; CPU only"]
fn control_edits_update_live_actors_and_gate_auto_shortcuts() -> Result<()> {
    let mut f = fixture()?;
    open_character(&mut f, NO_PADS)?;
    let actor = f
        .candidate
        .setup
        .actors
        .iter()
        .find(|(_, member)| *member == 3)
        .unwrap()
        .0;
    for (saved, live) in [
        (0, Control::Manual),
        (1, Control::SemiAuto),
        (2, Control::Auto),
    ] {
        page_visit(&mut f, NO_PADS, Some(MenuAction::Details))?;
        assert_eq!(f.candidate.party.settings.battle_controls[1], saved);
        assert_eq!(f.battle.actors()[actor.index()].control, live);
        let (_, available) = page_visit(&mut f, NO_PADS, None)?;
        assert_eq!(available, live == Control::Auto);
    }
    let (_, available) = page_visit(&mut f, PLUGGED, None)?;
    assert!(!available);
    let (opened, available) = page_visit(&mut f, NO_PADS, Some(MenuAction::Menu))?;
    assert!(available && opened.unison);
    Ok(())
}

#[test]
#[ignore = "requires locally cooked battle assets; CPU only"]
fn battle_target_navigation_stays_within_the_active_roster() -> Result<()> {
    let mut f = prepared_fixture_with_party(&[1, 3, 2, 4, 5], 2, &[(3, 66)], |party, _, _| {
        party.settings.battle_controls = [0, 2, 0, 0];
        Ok(())
    })?;
    enter_battle(&mut f.candidate, &mut f.battle)?;
    let actor = Candidate::tech_actor(&f.candidate.setup, 2)?;
    let mut page = f.candidate.begin_tech(&mut f.battle, 1, NO_PADS)?;
    // Exercise the shared recipient panel against the actual battle roster,
    // which contains only the first four of the five saved party members.
    page.state.focus = Focus::Target;
    page.state.target = 0;
    let view = f
        .candidate
        .battle_tech_view(&page.state, &NO_PADS, &f.battle);
    assert_eq!(view.party_count(), 4);
    assert_eq!(view.party.formation.len(), 5);
    for action in [
        MenuAction::Right,
        MenuAction::Down,
        MenuAction::Down,
        MenuAction::Down,
        MenuAction::Down,
    ] {
        let visit = f
            .candidate
            .step_tech(&mut f.battle, &mut page, actor, Some(action))?;
        assert!(visit.exit.is_none());
        assert!(page.state.target < 4);
    }
    assert_eq!(page.state.target, 3);
    assert_eq!(f.battle.pending_technique(actor), None);
    Ok(())
}
