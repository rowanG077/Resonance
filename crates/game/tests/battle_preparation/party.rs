use super::*;
use battle::{
    model::{ModelSetup, ModelSource},
    party::Setup,
};
use resonance_battle::{Affinity, Control, Element, Playback, PreparedBattle};
use resonance_content::{menu_data::MenuData, session::SessionData};
use resonance_events::party::Party;

fn snapshot() -> Result<(Files, MenuData, Party)> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let menus: MenuData = files.json("game/menu-data.json")?;
    menus.validate()?;
    let mut session: SessionData = files.json("game/session-data.json")?;
    session.rules = Some(Arc::new(menus.clone()));
    let mut party = Party::new(&session, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![1, 2, 3, 4, 9];
    let sources: Vec<_> = [1, 2, 3, 4, 9]
        .into_iter()
        .map(ModelSource::Party)
        .chain(
            [135, 143, 159, 175, 190, 228, 357]
                .into_iter()
                .map(ModelSource::Weapon),
        )
        .collect();
    let files = battle::model::load_files(&root, files, &sources, &mut cache, || false)?;
    Ok((files, menus, party))
}

fn setups(count: usize) -> Vec<Setup> {
    (0..count)
        .map(|slot| Setup {
            position: [-300. + slot as f32 * 50., 0., 0.],
            heading: 90.,
            model: Some(ModelSetup {
                resource: slot as u32 + 1,
                initial: Playback {
                    clip: 0,
                    frame: 0.,
                    rate: 0.5,
                    repeat: true,
                },
                suppress_root_translation: [true; 3],
            }),
        })
        .collect()
}

#[test]
#[ignore = "requires current cooked party/item publications; no devices"]
fn cold_activation_applies_signed_technique_drift_once_to_active_candidates() -> Result<()> {
    let (_, mut menus, mut field) = snapshot()?;
    assert_eq!(menus.items[445].properties.technique_drift, -1);
    assert_eq!(menus.items[446].properties.technique_drift, 1);
    assert_eq!(menus.items[476].properties.technique_drift, 2);
    for member in &mut field.members {
        member.equipment = [0; 6];
        member.ex_skills = [0; 4];
        member.technique_balance = 0;
    }
    field.members[0].equipment[4..].copy_from_slice(&[445, 476]);
    field.members[0].ex_skills = [1, 2, 3, 5]; // +1,+1,-1,-1.
    field.members[0].compound_ex_skills.insert(0); // EX Attack contributes zero.
    field.members[1].equipment[4] = 446;
    field.members[1].ex_skills[0] = 2;
    field.members[1].technique_balance = 99;
    field.members[2].ex_skills[0] = 23;
    field.members[2].technique_balance = -99;
    field.members[3].technique_balance = 17;
    field.cooking.full = true;
    let mut candidate = field.clone();
    let active = [1, 2, 3]
        .map(|id| {
            Ok((
                id,
                field.members[usize::from(id - 1)].activated_technique_balance(&menus)?,
            ))
        })
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    candidate.begin_battle(&active)?;
    assert_eq!(
        candidate.members[..4]
            .iter()
            .map(|m| m.technique_balance)
            .collect::<Vec<_>>(),
        [2, 100, -100, 17]
    );
    assert_eq!(candidate.battles.total, 1);
    assert_eq!(candidate.battles.participation, [1, 1, 1, 0, 0, 0, 0, 0, 0]);
    assert!(!candidate.cooking.full);
    let restored: Party = serde_json::from_value(serde_json::to_value(&candidate)?)?;
    assert_eq!(restored.members[0].technique_balance, 2);
    assert_eq!(restored.battles, candidate.battles);

    field.members[0].equipment[3..5].copy_from_slice(&[445, 446]);
    field.members[0].equipment[5] = 0;
    field.members[0].ex_skills = [0; 4];
    for (drift, initial, one_item, expected) in [(64, -100, 28, 100), (-64, 100, -28, -100)] {
        menus.items[445].properties.technique_drift = drift;
        menus.items[446].properties.technique_drift = drift;
        menus.validate()?;
        field.members[0].technique_balance = initial;
        field.members[0].equipment[4] = 0;
        let single = field.members[0].activated_technique_balance(&menus)?;
        assert_eq!(single, one_item);
        field.members[0].equipment[4] = 446;
        let doubled = field.members[0].activated_technique_balance(&menus)?;
        assert_eq!(doubled, expected);
        assert_eq!((doubled - single).signum(), drift.signum());
        candidate = field.clone();
        candidate.begin_battle(&[(1, doubled)])?;
        assert_eq!(candidate.members[0].technique_balance, expected);
    }

    field.members[2].equipment[0] = u16::MAX;
    let before = serde_json::to_value(&field)?;
    assert!(
        field.members[2]
            .activated_technique_balance(&menus)
            .is_err()
    );
    assert!(field.begin_battle(&[(1, 0), (2, 101)]).is_err());
    assert!(field.begin_battle(&[(1, 0), (1, 0)]).is_err());
    assert_eq!(serde_json::to_value(&field)?, before);
    Ok(())
}

#[test]
#[ignore = "requires current cooked party/item publications; no devices"]
fn cold_party_projects_starting_stats_models_and_controller_slots() -> Result<()> {
    let (files, menus, mut session) = snapshot()?;
    // The field leader does not select a battle actor or controller slot.
    session.field_leader = 9;
    session.settings.battle_controls = [0, 1, 2, 0];
    session.members[0].hp = 123;
    session.members[0].tp = 9;
    session.members[0].overlimit = 37;
    let prepared = prepare_party_fixture(&files, &menus, &session, setups(4))?;
    assert_eq!(prepared[0].actor.overlimit.charge(), 370);
    assert!(!prepared[0].actor.overlimit.is_active());
    assert_eq!(
        prepared.iter().map(|p| p.character).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    let expected = [
        (200, 26, [114, 104, 26, 40, 76, 64], 2),
        (172, 32, [102, 34, 19, 50, 65, 68], 2),
        (140, 52, [88, 28, 19, 62, 50, 50], 1),
        (160, 48, [110, 30, 20, 66, 50, 40], 1),
    ];
    for (slot, (value, (hp, tp, stats, weapons))) in prepared.iter().zip(expected).enumerate() {
        let actor = &value.actor;
        assert_eq!((actor.equipment.max_hp, actor.equipment.max_tp), (hp, tp));
        assert_eq!(
            [
                actor.equipment.stats.slash,
                actor.equipment.stats.thrust,
                actor.equipment.stats.defense,
                actor.equipment.stats.intelligence,
                actor.equipment.stats.accuracy,
                actor.equipment.stats.evasion
            ],
            stats
        );
        assert_eq!((actor.equipment.luck, actor.equipment.stats.level), (50, 1));
        assert_eq!(actor.guard.break_pressure, u32::try_from(hp)? / 100 + 3);
        assert_eq!(actor.guard.recovery_bonus, [0, 5, 10, 10][slot]);
        assert_eq!(actor.guard.auto_chance, 0);
        assert_eq!(
            actor.control,
            [
                Control::Manual,
                Control::SemiAuto,
                Control::Auto,
                Control::Manual
            ][slot]
        );
        assert_eq!(value.model.as_ref().unwrap().weapons.len(), weapons);
        assert_eq!(
            value
                .model
                .as_ref()
                .unwrap()
                .weapons
                .iter()
                .map(|part| part.slot)
                .collect::<Vec<_>>(),
            (0..weapons as u8).collect::<Vec<_>>()
        );
    }
    assert_eq!((prepared[0].actor.hp, prepared[0].actor.tp), (123, 9));
    // The prepared body and weapon attachments activate together.
    PreparedBattle::new(
        prepared
            .iter()
            .map(|p| (p.actor.clone(), Default::default()))
            .collect(),
        Default::default(),
        1,
    )?
    .finish()?;
    session.formation = vec![3, 1, 2];
    for (preference, expected) in [(0, [10, 0, 5]), (3, [25, 0, 5])] {
        session.members[2].strategy[2] = preference;
        let reordered = prepare_party_fixture(&files, &menus, &session, setups(3))?;
        assert_eq!(
            reordered
                .iter()
                .map(|p| p.actor.guard.recovery_bonus)
                .collect::<Vec<_>>(),
            expected,
            "character defaults follow identity; explicit position strategy takes precedence"
        );
    }
    session.formation = vec![9, 3, 1];
    // Kratos's starting Long Sword selects the human species bonus.
    let reordered = prepare_party_fixture(&files, &menus, &session, setups(3))?;
    assert_eq!(reordered[0].actor.equipment.damage.weapon_species, Some(6));
    assert_eq!(reordered[0].actor.species, 6);
    let lloyd = &prepared[0];
    assert_eq!(lloyd.model.as_ref().unwrap().weapons.len(), 2);

    // The same saved knockout state governs field actions and battle admission.
    let mut data: SessionData = files.json("game/session-data.json")?;
    data.rules = Some(Arc::new(menus.clone()));
    session.formation = vec![1, 4];
    session.field_leader = 1;
    session.members[0].hp = 1;
    session.members[3].hp = 0;
    session.members[3].techniques.insert(98); // First Aid.
    session.items.insert(1, 1); // Apple Gel.
    session.items.insert(11, 1); // Life Bottle.
    session.validate(&data).map_err(anyhow::Error::msg)?;
    assert!(!session.members[3].can_lead_field());
    let tp = session.members[3].tp;
    assert_eq!(
        session
            .cast_technique(&menus, 3, 0, 98, false)
            .map_err(anyhow::Error::msg)?,
        None
    );
    assert_eq!(session.members[0].hp, 1);
    assert_eq!(session.members[3].tp, tp);
    assert_eq!(
        session
            .use_item(&data, &menus, 1, 3)
            .map_err(anyhow::Error::msg)?,
        None
    );
    assert_eq!(session.items[&1], 1);

    for revived in [false, true] {
        if revived {
            assert!(
                session
                    .use_item(&data, &menus, 11, 3)
                    .map_err(anyhow::Error::msg)?
                    .is_some()
            );
            assert!(session.members[3].hp > 0);
            assert!(session.members[3].can_lead_field());
            assert!(
                session
                    .cast_technique(&menus, 3, 0, 98, false)
                    .map_err(anyhow::Error::msg)?
                    .is_some()
            );
            assert!(session.members[0].hp > 1);
            session.validate(&data).map_err(anyhow::Error::msg)?;
        }
        let members = prepare_party_fixture(&files, &menus, &session, setups(2))?;
        let live = PreparedBattle::new(
            members
                .iter()
                .map(|row| (row.actor.clone(), Default::default()))
                .collect(),
            Default::default(),
            1,
        )?
        .finish()?;
        assert_eq!(live.actors()[1].available(), revived);
        assert_eq!(
            live.actors()[1].availability,
            if revived {
                resonance_battle::ActorAvailability::Active
            } else {
                resonance_battle::ActorAvailability::Dead
            }
        );
        assert_eq!(live.actors()[1].hp, i32::from(session.members[3].hp));
    }
    Ok(())
}

#[test]
#[ignore = "requires current cooked party/item publications; no devices"]
fn ex71_uses_equipped_lloyd_recipe_and_live_admission_not_learned_history_or_reserves() -> Result<()>
{
    use resonance_battle::ActorAvailability;
    let (files, menus, mut session) = snapshot()?;
    session.formation = vec![1];
    session.members[0].equipment = [135, 0, 0, 0, 0, 0];
    session.members[0].ex_skills = [6, 3, 0, 0];
    session.members[0].compound_ex_skills.clear();
    session.members[0].recent_compound_ex_skills.clear();
    for (hp, petrified, availability) in [
        (1, false, ActorAvailability::Active),
        (0, false, ActorAvailability::Dead),
        (1, true, ActorAvailability::Petrified),
    ] {
        session.members[0].hp = hp;
        session.members[0].ailments.petrified = petrified;

        let prepared = prepare_party_fixture(&files, &menus, &session, setups(1))?;
        assert!(
            prepared[0].actor.equipment.quick_escape && prepared[0].actor.equipment.taunt_enabled
        );
        let live = PreparedBattle::new(
            prepared
                .iter()
                .map(|row| (row.actor.clone(), Default::default()))
                .collect(),
            Default::default(),
            1,
        )?
        .finish()?;
        assert_eq!(live.actors()[0].availability, availability);
        assert_eq!(
            live.actors()[0].available(),
            availability == ActorAvailability::Active
        );
    }
    session.members[0].hp = 1;
    session.members[0].ailments.petrified = false;

    // Quick Escape can coexist with Quick Turn or Taunt Cancel.
    for (skills, gems, quick_turn, taunt_cancel) in [
        ([6, 3, 2, 0], [2, 1, 1, 0], true, false),
        ([6, 3, 5, 0], [2, 1, 2, 0], false, true),
    ] {
        session.members[0].ex_skills = skills;
        session.members[0].ex_gems = gems;
        let prepared = prepare_party_fixture(&files, &menus, &session, setups(1))?;
        assert!(
            prepared[0].actor.equipment.quick_escape && prepared[0].actor.equipment.taunt_enabled
        );
        assert_eq!(prepared[0].actor.equipment.quick_turn, quick_turn);
        assert_eq!(prepared[0].actor.equipment.taunt_cancel, taunt_cancel);
    }
    for (skills, gems, compound) in [
        ([6, 3, 1, 2], [2, 1, 1, 1], 54),
        ([6, 3, 5, 7], [2, 1, 2, 2], 55),
    ] {
        session.members[0].ex_skills = skills;
        session.members[0].ex_gems = gems;
        let rows = prepare_party_fixture(&files, &menus, &session, setups(1))?;
        assert_eq!(
            rows[0].actor.equipment.damage.guard_damage_boost,
            compound == 54
        );
        assert_eq!(
            rows[0].actor.equipment.damage.physical_stability,
            compound == 55
        );
    }
    let index = menus.ex_skills.characters[0]
        .compounds
        .iter()
        .position(|recipe| recipe.skill == 71)
        .unwrap();
    session.members[0].compound_ex_skills.insert(index as u8);
    session.members[0].ex_skills = [0; 4];
    assert!(
        !prepare_party_fixture(&files, &menus, &session, setups(1))?[0]
            .actor
            .equipment
            .quick_escape
    );
    session.members[0].ex_skills = [6, 3, 0, 0];
    session.formation = vec![2];
    session.members[1].ex_skills = [0; 4];
    let prepared = prepare_party_fixture(&files, &menus, &session, setups(1))?;
    assert_eq!(prepared[0].character, 2);
    assert!(!prepared[0].actor.equipment.quick_escape);
    Ok(())
}

#[test]
#[ignore = "requires current cooked party/item publications; no devices"]
fn cold_party_equipment_affinities_ex_stats_and_incapacitation_are_prepared() -> Result<()> {
    let (files, mut menus, mut session) = snapshot()?;
    session.formation = vec![1, 3];
    session.members[0].equipment[3] = 398;
    session.members[0].equipment[4] = 399;
    session.members[0].ex_skills = [1, 7, 12, 0];
    // Drop private bindings as loading an ordinary save does; explicit roster
    // identity must still select the loaded EX rules without persistent mutation.
    session = serde_json::from_value(serde_json::to_value(&session)?)?;
    session.members[2].ailments.petrified = true;

    menus.items[135].properties.attack_element = Some(Element::Fire);
    menus.items[398].properties = Default::default();
    menus.items[399].properties = Default::default();
    menus.items[398].properties.attack_element = Some(Element::Water);
    menus.items[399].properties.attack_element = Some(Element::Wind);
    menus.items[398].properties.neutral_resistance = 5;
    menus.items[399].properties.resistance =
        [(Element::Water, -2), (Element::Wind, 2), (Element::Fire, 7)].into();
    let prepared = prepare_party_fixture(&files, &menus, &session, setups(2))?;
    let actor = &prepared[0].actor;
    let expected = session.members[0].stats_for(&menus, 0);
    assert_eq!(actor.equipment.max_hp, i32::from(expected.hp));
    assert_eq!(actor.equipment.stats.slash, i32::from(expected.slash));
    assert_eq!(actor.equipment.stats.defense, i32::from(expected.defense));
    assert_eq!(actor.guard.reduction, 80);
    assert_eq!(actor.equipment.base_element, Some(Element::Water));
    assert_eq!(
        &actor.equipment.affinities[..4],
        &[
            Affinity::Immune,
            Affinity::Weak,
            Affinity::Resistant,
            Affinity::Absorb
        ]
    );
    assert!(prepared[1].actor.is_petrified());
    for item in [398, 399] {
        menus.items[item].properties.neutral_resistance = 64;
    }
    let stacked = prepare_party_fixture(&files, &menus, &session, setups(2))?;
    assert_eq!(stacked[0].actor.equipment.affinities[0], Affinity::Absorb);
    session.members[0].hp = 0;
    let prepared = prepare_party_fixture(&files, &menus, &session, setups(2))?;
    assert_eq!(prepared.len(), 2);
    assert_eq!(prepared[0].actor.hp, 0);
    Ok(())
}

#[test]
#[ignore = "requires current cooked party/item publications; no devices"]
fn cold_party_validation_rejects_invalid_members_and_placements() -> Result<()> {
    let (files, menus, session) = snapshot()?;
    let activate = |session: &Party, placements| {
        let actors = prepare_party_fixture(&files, &menus, session, placements)?;
        PreparedBattle::new(
            actors
                .iter()
                .map(|p| (p.actor.clone(), Default::default()))
                .collect(),
            Default::default(),
            1,
        )?
        .finish()
    };
    activate(&session, setups(4))?;
    type InvalidCase = (&'static str, fn(&mut Party, &mut [Setup]));
    let cases: [InvalidCase; 8] = [
        ("duplicate battle character", |party, _| {
            party.formation[1] = 1
        }),
        ("invalid party character", |party, _| party.formation[1] = 0),
        ("invalid battle control for slot 1", |party, _| {
            party.settings.battle_controls[1] = 3;
        }),
        ("missing equipment item definition 65535", |party, _| {
            party.members[1].equipment[0] = u16::MAX;
        }),
        ("missing EX skill 255", |party, _| {
            party.members[1].ex_skills = [u8::MAX, 0, 0, 0];
        }),
        ("invalid party level", |party, _| party.members[1].level = 0),
        ("invalid battle placement", |_, placements| {
            placements[1].position[0] = f32::NAN;
        }),
        ("invalid party battle vitals", |party, _| {
            party.members[1].hp = u16::MAX;
        }),
    ];
    for (expected, invalidate) in cases {
        let mut invalid = session.clone();
        let mut placements = setups(4);
        invalidate(&mut invalid, &mut placements);
        let error = activate(&invalid, placements)
            .err()
            .context("invalid party candidate was activated")?;
        assert!(
            format!("{error:#}").contains(expected),
            "{expected}: {error:#}"
        );
    }
    Ok(())
}
