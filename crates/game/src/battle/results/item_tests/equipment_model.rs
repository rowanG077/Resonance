//! Cooked equipment-bank composition through the production Candidate path.
use super::*;
use crate::menu::{Input, MenuAction};
use anyhow::{Context, Result};
use resonance_battle::{ActionId, Activity, ActorId, ButtonInput, ContactSource, ControlInput};

#[test]
#[ignore = "requires current cooked equipment resources; CPU only"]
fn equipped_arm_item_transfers_between_members_and_commits_without_inventory_stock() -> Result<()> {
    let PreparedFixture {
        mut candidate,
        mut battle,
        field,
        ..
    } = prepared_fixture_with_party(&[1, 7], 0, &[], |party, session, _| {
        party
            .items
            .retain(|&item, _| session.items[usize::from(item)].equipment_kind != Some(3));
        party.members[0].equipment[5] = 367;
        party.members[6].equipment[5] = 0;
        Ok(())
    })?;
    assert!(!candidate.party.items.contains_key(&367));
    enter_battle(&mut candidate, &mut battle)?;
    let before = battle.snapshot();
    let (mut page, mut character) = candidate.begin_equipment(&mut battle, 0)?;
    for _ in 0..12 {
        candidate.step_equipment(&mut battle, &mut page, &mut character, Input::default())?;
    }
    // Arm row, unequip Lloyd, page to Presea, open its list and choose the returned item.
    for action in [
        MenuAction::Down,
        MenuAction::Down,
        MenuAction::Down,
        MenuAction::Alternate,
        MenuAction::PageDown,
        MenuAction::Confirm,
        MenuAction::Confirm,
    ] {
        candidate.step_equipment(&mut battle, &mut page, &mut character, Some(action))?;
    }
    assert_eq!(character, 1);
    let draft = candidate.equipment_draft.as_ref().unwrap();
    assert_eq!(
        [0, 6].map(|member| draft.members[member].equipment[5]),
        [0, 367]
    );
    assert!(!draft.items.contains_key(&367));
    assert_eq!(
        [0, 6].map(|member| candidate.party.members[member].equipment[5]),
        [367, 0]
    );
    assert_eq!(battle.snapshot().actors, before.actors);
    let expected_defense = [0, 6].map(|member| {
        draft.members[member]
            .stats_for(&candidate.menus, member)
            .defense
    });
    close_equipment(&mut candidate, &mut battle, &mut page, &mut character)?;
    assert_eq!(
        [0, 6].map(|member| candidate.party.members[member].equipment[5]),
        [0, 367]
    );
    assert!(!candidate.party.items.contains_key(&367));
    for (slot, defense) in expected_defense.into_iter().enumerate() {
        let actor = candidate.setup.actors[slot].0;
        assert_eq!(
            battle.actors()[actor.index()].equipment.stats.defense,
            i32::from(defense)
        );
    }
    assert!(!battle.is_diagnostic());
    let outcome = escape(&mut candidate, &mut battle)?;
    let completed = candidate.finish(&battle, &outcome)?;
    assert_eq!(
        [0, 6].map(|member| completed.party.members[member].equipment[5]),
        [0, 367]
    );
    assert!(!completed.party.items.contains_key(&367));
    assert_eq!(
        [0, 6].map(|member| field.members[member].equipment[5]),
        [367, 0]
    );
    Ok(())
}

fn strike(
    candidate: &mut Candidate,
    battle: &mut Battle,
    actor: ActorId,
    previous: Option<ActionId>,
) -> Result<(ActionId, resonance_battle::ActionKey)> {
    for _ in 0..600 {
        let press = !matches!(
            battle.activity(actor),
            Activity::Action | Activity::Recovering
        );
        let cues = candidate.world_update(
            battle,
            BattleInput {
                controllers: vec![ControlInput {
                    attack: ButtonInput {
                        pressed: press,
                        held: press,
                        released: false,
                    },
                    ..ControlInput::neutral(actor)
                }],
                ..Default::default()
            },
        )?;
        for cue in cues {
            if let Cue::Hit {
                source:
                    ContactSource::Melee {
                        actor: owner,
                        action,
                    },
                result,
                ..
            } = cue
                && owner == actor
                && Some(action) != previous
                && result.hp_change < 0
            {
                return Ok((
                    action,
                    battle
                        .action_definition(action)
                        .context("strike action ended before its contact")?,
                ));
            }
        }
    }
    let frame = battle.snapshot();
    let state = &frame.actors[actor.index()];
    anyhow::bail!(
        "Lloyd did not land a new prepared melee attack: activity {:?}, HP {}, position {:?}, result {:?}",
        state.activity,
        state.hp,
        state.position,
        frame.recognized_result
    )
}

/// Swapping weapons during a strike preserves its active contacts and enchantment.
#[test]
#[ignore = "requires current cooked party/profile publications; CPU only"]
fn weapon_swap_preserves_active_contacts_and_native_state() -> Result<()> {
    let PreparedFixture {
        mut candidate,
        mut battle,
        ..
    } = prepared_fixture_with_party(&[1, 2], 0, &[], |party, session, _| {
        party.settings.battle_controls[0] = 1;
        party.settings.preferences.battle_rank = 2;
        party.members[0].equipment[0] = 135;
        party.members[0].equipment[5] = 367;
        party
            .change_item(session, 136, 1)
            .map_err(anyhow::Error::msg)?;
        party
            .change_item(session, 44, 1)
            .map_err(anyhow::Error::msg)?;
        Ok(())
    })?;
    enter_battle(&mut candidate, &mut battle)?;

    let actor = candidate
        .setup
        .actors
        .iter()
        .find(|&&(_, character)| character == 1)
        .map(|&(actor, _)| actor)
        .context("missing Lloyd actor")?;
    release(
        &mut candidate,
        &mut battle,
        Release {
            user: actor,
            target: actor,
            item: 44,
        },
    )?;
    let enchantment = battle.actors()[actor.index()].elements.enchantment;
    assert_eq!(enchantment, Some(resonance_battle::Element::Water));
    let conditions = battle.actors()[actor.index()].conditions.clone();
    refresh_equipment(&mut candidate, &mut battle)?;
    assert_eq!(
        battle.actors()[actor.index()].elements.enchantment,
        enchantment
    );
    assert_eq!(battle.actors()[actor.index()].conditions, conditions);
    // Reach a real contact through SemiAuto approach and the prepared normal action.
    // Swap while that admitted action still owns its contact window.
    let (admitted, definition) = strike(&mut candidate, &mut battle, actor, None)?;
    let (mut page, mut character) = candidate.begin_equipment(&mut battle, 0)?;
    for _ in 0..12 {
        candidate.step_equipment(&mut battle, &mut page, &mut character, Input::default())?;
    }
    let age_before = battle
        .action_age(admitted)
        .context("normal ended before equipment page")?;
    let elements_before = battle.actors()[actor.index()].elements;
    let conditions_before = battle.actors()[actor.index()].conditions.clone();
    assert!(
        candidate
            .equipment_draft
            .as_mut()
            .unwrap()
            .equip_slot(&candidate.session, 0, 0, 136)
            .map_err(anyhow::Error::msg)?
    );
    close_equipment(&mut candidate, &mut battle, &mut page, &mut character)?;
    assert_eq!(battle.action_definition(admitted), Some(definition));
    assert_eq!(battle.action_age(admitted), Some(age_before));
    assert_eq!(
        battle.actors()[actor.index()].elements.action,
        elements_before.action
    );
    assert_eq!(
        battle.actors()[actor.index()].elements.enchantment,
        enchantment
    );
    assert_eq!(battle.actors()[actor.index()].conditions, conditions_before);
    assert_eq!(candidate.party.members[0].equipment[0], 136);

    let (next, _) = strike(&mut candidate, &mut battle, actor, Some(admitted))?;
    assert_ne!(next, admitted);
    assert!(!battle.is_diagnostic());

    Ok(())
}

#[test]
#[ignore = "requires current cooked equipment resources; CPU only"]
fn missing_actor_art_allows_stats_contacts_and_result_commit() -> Result<()> {
    for broken in [Some(135), Some(136), Some(356), None] {
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let loaded = load_fixture(
                &[1, 9],
                0,
                &[],
                diagnostics.clone(),
                |party, session, _, files| {
                    party.settings.battle_controls[0] = 1;
                    party.members[0].equipment[0] = 135;
                    party.members[8].equipment[5] = 356;
                    party.items.insert(136, 1);
                    let Some(broken) = broken else {
                        for character in [1, 9] {
                            files.remove(&resonance_content::battle_model::party_path(character));
                        }
                        return Ok(());
                    };
                    let mut bank: resonance_content::battle_model::Weapons =
                        files.json(resonance_content::battle_model::WEAPONS_PATH)?;
                    // Unusable reserve equipment does not request artwork for this roster.
                    let irrelevant = party.members[2].equipment[0];
                    assert_eq!(
                        session.items[usize::from(irrelevant)].allowed_characters & 0x101,
                        0
                    );
                    bank.records.remove(&irrelevant);
                    if broken == 136 {
                        let resonance_content::battle_model::Attachment::Weapon(weapon) = bank
                            .records
                            .get_mut(&broken)
                            .context("missing spare weapon")?
                        else {
                            anyhow::bail!("spare item is not a weapon");
                        };
                        for part in weapon.parts.values_mut() {
                            part.layers.clear();
                        }
                    } else {
                        bank.records.remove(&broken);
                    }
                    files.insert(
                        resonance_content::battle_model::WEAPONS_PATH.into(),
                        serde_json::to_vec(&bank)?.into(),
                    );
                    Ok(())
                },
            )
            .and_then(|loaded| loaded.prepare(2500, |sound| Ok(Some(sound))));
            if paranoid {
                let error = loaded
                    .err()
                    .context("strict preparation accepted broken artwork")?;
                let expected =
                    broken.map_or_else(|| "battle/party/01.json".into(), |item| item.to_string());
                assert!(format!("{error:#}").contains(&expected), "{error:#}");
                continue;
            }
            let PreparedFixture {
                mut candidate,
                mut battle,
                field,
                ..
            } = loaded?;
            let actor = candidate.setup.actors[0].0;
            assert!(diagnostics.has_errors());

            enter_battle(&mut candidate, &mut battle)?;
            // The initially equipped weapon can hit even when its artwork was omitted.
            let (previous, _) = strike(&mut candidate, &mut battle, actor, None)?;
            let (mut page, mut character) = candidate.begin_equipment(&mut battle, 0)?;
            for _ in 0..12 {
                candidate.step_equipment(
                    &mut battle,
                    &mut page,
                    &mut character,
                    Input::default(),
                )?;
            }
            let draft = candidate.equipment_draft.as_mut().unwrap();
            assert!(
                draft
                    .equip_slot(&candidate.session, 0, 0, 136)
                    .map_err(anyhow::Error::msg)?
            );
            let expected = draft.members[0].stats_for(&candidate.menus, 0);
            close_equipment(&mut candidate, &mut battle, &mut page, &mut character)?;
            assert_eq!(candidate.party.members[0].equipment[0], 136);
            assert_eq!(
                battle.actors()[actor.index()].equipment.stats.slash,
                i32::from(expected.slash)
            );
            assert!(!candidate.party.items.contains_key(&136));

            strike(&mut candidate, &mut battle, actor, Some(previous))?;
            assert!(!battle.is_diagnostic());
            let outcome = escape(&mut candidate, &mut battle)?;
            let completed = candidate.finish(&battle, &outcome)?;
            assert_eq!(completed.party.members[0].equipment[0], 136);
            assert_eq!(completed.party.members[8].equipment[5], 356);
            assert!(!completed.party.items.contains_key(&136));
            assert_eq!(field.members[0].equipment[0], 135);
        }
    }
    Ok(())
}
