//! Current battle values and prospective equipment values have different owners.
use super::item_tests::{enter_battle, native_battle, native_fixture};
use super::*;
use resonance_battle::{
    ActionDefinition, ActionExecution, ActionKey, ActionRequest, ActorSetup, AttackEvent,
    BattleInput, Collider, DamageKind, HitElement, HitRule, MeleeDefinition, MeleeVolume, Power,
    PreparedAttack, PreparedBattle, PreparedTechnique,
};

#[test]
fn current_stats_stay_live_after_a_kill_and_preview_matches_equipment_commit() -> Result<()> {
    let mut f = native_fixture(
        &[9, 7, 1],
        |party, _, menus| {
            menus.items[1].properties.kills_damage = true;
            let guilt = resonance_content::menu_data::ex_effect::GUILT;
            menus
                .ex_skills
                .skills
                .insert(guilt, menus.ex_skills.skills[&1].clone());
            party.members[8].ex_skills = [guilt, 0, 0, 0];
            party.members[8].equipment[0] = 1;
            party.battles.kills[8] = 100;
            Ok(())
        },
        |mut actors, setup| {
            setup.devils_arms_unlocked = true;
            // 100 saved kills plus Guilt's 50 attack / 10 defense from Presea.
            actors[0].equipment.stats.slash += 150;
            actors[0].equipment.stats.thrust += 150;
            actors[0].equipment.stats.defense += 10;
            actors[1].availability = ActorAvailability::Absent;
            actors[3].hp = 1;
            actors[3].body.collider = Some(Collider::sphere(2.));
            let mut attack = PreparedAttack::new(1);
            attack.events.push((
                0,
                AttackEvent::Contact {
                    definition: Arc::new(MeleeDefinition {
                        volume: MeleeVolume::Fixed {
                            offset: [0.; 3],
                            radius: 100.,
                            half_height: 100.,
                        },
                        trail: None,
                        hit: HitRule {
                            kind: DamageKind::Magic,
                            effect: None,
                            effect_on_guard: false,
                            sound: None,
                            arte: true,
                            overlimit_pause: false,
                            power: Power::Fixed(1),
                            element: HitElement::Neutral,
                            prevents_defeat: false,
                            finisher_grade: false,
                            hp_drain: false,
                            guard: Default::default(),
                            reaction: Default::default(),
                            condition: None,
                        },
                    }),
                    duration: 1,
                },
            ));
            let actions = vec![ActionDefinition {
                normal: None,
                tp_cost: 0,
                execution: ActionExecution::Attack(attack),
            }];
            PreparedBattle::new(
                actors
                    .into_iter()
                    .enumerate()
                    .map(|(index, actor)| {
                        let mut setup = ActorSetup::default();
                        if index == 0 {
                            setup.techniques.push(PreparedTechnique {
                                action: ActionKey(0),
                                catalogue: TechniqueId(1),
                                player_range: [0., 1000.],
                                ai_range: [0., 1000.],
                                capabilities: Default::default(),
                                element: None,
                            });
                        }
                        (actor, setup)
                    })
                    .collect(),
                actions.into(),
                1,
            )?
            .finish()
        },
    )?;
    enter_battle(&mut f.candidate, &mut f.battle)?;
    let actor = f.candidate.setup.actors[0].0;
    let before = f.battle.actors()[actor.index()].equipment.stats;
    for tick in 0..30 {
        f.battle.step(BattleInput {
            actions: if tick == 0 {
                vec![ActionRequest {
                    actor,
                    target: f.candidate.setup.enemies[0].actor,
                    action: ActionKey(0),
                }]
            } else {
                vec![]
            },
            ..Default::default()
        })?;
        if f.battle.ledger().kills[actor.index()] != 0 {
            break;
        }
    }
    assert_eq!(f.battle.ledger().kills[actor.index()], 1);
    assert_eq!(f.battle.actors()[actor.index()].equipment.stats, before);
    f.candidate.begin_menu(&f.battle)?;
    assert_eq!(f.candidate.menu_party().formation, [9, 1]);
    // Guilt must still count Presea in the saved formation, despite her absence.
    let saved = serde_json::to_value(f.candidate.persistent_party())?;
    let state = crate::menu::equipment::Equipment::default();
    let page = f.candidate.equipment_page(&f.battle, &state, 0);
    let current = page.current_stats()?;
    let preview = page.preview_stats(0, 1)?;
    assert_eq!(i32::from(current.slash), before.slash);
    assert_eq!(i32::from(current.thrust), before.thrust);
    assert_eq!(preview.slash, current.slash + 1);
    assert_eq!(preview.thrust, current.thrust + 1);
    let tech = crate::menu::techniques::Tech::default();
    let connected = [true; 4];
    let target = f
        .candidate
        .battle_tech_view(&tech, &connected, &f.battle)
        .target_stats(Default::default())?;
    assert_eq!(target, current);
    assert_eq!(serde_json::to_value(f.candidate.persistent_party())?, saved);
    let next = f.candidate.persistent_party().clone();
    f.candidate.commit_equipment(&mut f.battle, next)?;
    let applied = f
        .candidate
        .equipment_page(&f.battle, &state, 0)
        .current_stats()?;
    assert_eq!(applied, preview);
    // A committed refresh must not add the pending kill twice in a later preview.
    assert_eq!(
        f.candidate
            .equipment_page(&f.battle, &state, 0)
            .preview_stats(0, 1)?,
        preview
    );
    Ok(())
}

#[test]
fn accepted_draft_changes_do_not_change_current_equipment_or_stats_until_commit() -> Result<()> {
    use crate::menu::{MenuAction, equipment::Focus};
    use resonance_content::menu_data::{ExStat, ExStatBonus};

    let mut f = native_fixture(
        &[9, 1],
        |party, _, menus| {
            party.members[8].equipment[0] = 1;
            party.battles.kills[8] = 123;
            party.items = [(2, 1), (3, 1)].into();
            menus.items[1].equipment_stats = [10, 20, 0, 0, 0, 0, 0];
            menus.items[2].equipment_stats[6] = 30;
            menus.items[2].properties.stat_bonuses = vec![
                ExStatBonus {
                    stat: ExStat::Strength,
                    percent: 50,
                },
                ExStatBonus {
                    stat: ExStat::MaxHp,
                    percent: 20,
                },
            ];
            menus.items[3].properties.kills_damage = true;
            menus.items[3].equipment_stats[6] = -5;
            Ok(())
        },
        |actors, setup| {
            setup.devils_arms_unlocked = true;
            native_battle(actors)
        },
    )?;
    // Give the in-memory items the same slot rules as a weapon and an accessory.
    let session = Arc::make_mut(&mut f.candidate.session);
    session.items[1].equipment_kind = Some(0);
    session.items[2].equipment_kind = Some(4);
    session.items[3].equipment_kind = Some(0);

    let (mut state, mut character) = f.candidate.begin_equipment(&mut f.battle, 0)?;
    for _ in 0..12 {
        f.candidate
            .step_equipment(&mut f.battle, &mut state, &mut character, None)?;
    }
    let committed = serde_json::to_value(f.candidate.persistent_party())?;
    let baseline = f
        .candidate
        .equipment_page(&f.battle, &state, character)
        .current_stats()?;
    assert_eq!(baseline.luck, 10);
    let live = f.battle.actors()[0].equipment.clone();

    // Accept a +30 Luck accessory through the real menu input path.
    for action in [MenuAction::Down; 4]
        .into_iter()
        .chain([MenuAction::Confirm, MenuAction::Confirm])
    {
        f.candidate
            .step_equipment(&mut f.battle, &mut state, &mut character, Some(action))?;
    }
    assert_eq!(state.focus, Focus::Slots);
    assert_eq!(f.candidate.menu_party().members[8].equipment[3], 2);
    let page = f.candidate.equipment_page(&f.battle, &state, character);
    assert_eq!(page.current_member()?.equipment[3], 0);
    assert_eq!(page.current_stats()?, baseline);

    // Reopen the weapon list after the accepted accessory edit. The preview
    // includes the entire draft; the current label and all values stay committed.
    for action in [MenuAction::Up; 4].into_iter().chain([MenuAction::Confirm]) {
        f.candidate
            .step_equipment(&mut f.battle, &mut state, &mut character, Some(action))?;
    }
    assert_eq!(state.focus, Focus::List);
    let page = f.candidate.equipment_page(&f.battle, &state, character);
    assert_eq!(page.items(), [3]);
    let planned = page.preview_stats(0, 3)?;
    assert_eq!(planned.luck, 35);
    assert!(planned.strength > baseline.strength);
    assert!(planned.hp > baseline.hp);
    assert_eq!(planned.slash, planned.strength + 123);
    assert_eq!(page.current_member()?.equipment[0], 1);
    assert_eq!(page.current_stats()?, baseline);

    f.candidate.step_equipment(
        &mut f.battle,
        &mut state,
        &mut character,
        Some(MenuAction::Confirm),
    )?;
    let page = f.candidate.equipment_page(&f.battle, &state, character);
    assert_eq!(page.party.members[8].equipment[0], 3);
    assert_eq!(page.current_member()?.equipment[0], 1);
    assert_eq!(page.current_stats()?, baseline);
    assert_eq!(
        serde_json::to_value(f.candidate.persistent_party())?,
        committed
    );
    assert_eq!(f.battle.actors()[0].equipment, live);

    super::item_tests::close_equipment(
        &mut f.candidate,
        &mut f.battle,
        &mut state,
        &mut character,
    )?;
    let page = f.candidate.equipment_page(&f.battle, &state, character);
    assert_eq!(page.current_member()?.equipment[0], 3);
    assert_eq!(page.current_member()?.equipment[3], 2);
    assert_eq!(page.current_stats()?, planned);
    assert_eq!(f.battle.actors()[0].equipment.luck, 35);
    Ok(())
}

#[test]
fn battle_menu_stats_merge_uncommitted_kills_by_character() -> Result<()> {
    let mut f = native_fixture(
        &[9, 1],
        |party, _, menus| {
            menus.items[1].properties.kills_damage = true;
            for character in [0, 8] {
                party.members[character].equipment[0] = 1;
            }
            party.battles.kills[0] = 100;
            party.battles.kills[8] = 4999;
            Ok(())
        },
        |actors, setup| {
            setup.devils_arms_unlocked = true;
            native_battle(actors)
        },
    )?;
    let saved = serde_json::to_value(f.candidate.persistent_party())?;
    // The ledger is indexed by actor, while saved totals are indexed by character.
    let mut ledger = f.battle.ledger().clone();
    ledger.kills[0] = 20;
    ledger.kills[1] = 7;
    for unlocked in [false, true] {
        f.candidate.setup.devils_arms_unlocked = unlocked;
        let stats = f.candidate.menu_stats(&ledger);
        for (character, total) in [(0, 107), (8, 5000)] {
            let member = &f.candidate.party.members[character];
            let base = member.stats_for(&f.candidate.menus, character);
            let display = stats.member(&f.candidate.menus, member, character);
            assert_eq!(display.slash, base.slash + if unlocked { total } else { 0 });
        }
    }
    assert_eq!(serde_json::to_value(f.candidate.persistent_party())?, saved);
    Ok(())
}
