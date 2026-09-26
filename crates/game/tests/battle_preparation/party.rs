use super::*;
use battle::{
    model::{ModelSetup, ModelSource},
    party::{self, Setup},
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
    session.ex_skills = Some(Arc::new(menus.ex_skills.clone()));
    let mut party = Party::new(&session, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![1, 2, 3, 4, 9];
    let sources: Vec<_> = [1, 2, 3, 4, 9]
        .into_iter()
        .map(ModelSource::Party)
        .chain(
            [135, 159, 175, 190, 228, 357]
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
            model: ModelSetup {
                resource: slot as u32 + 1,
                initial: Playback {
                    clip: 0,
                    frame: 0.,
                    rate: 0.5,
                    repeat: true,
                },
                suppress_root_translation: [true; 3],
                stun: None,
            },
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
    let before = serde_json::to_value(&field)?;
    let mut candidate = field.clone();
    candidate.begin_battle(&menus, &[1, 2, 3])?;
    assert_eq!(serde_json::to_value(&field)?, before);
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

    // EA428 narrows the accumulated drift to a byte before5C38 doubles it.
    menus.items[445].properties.technique_drift = 127;
    field.members[0].equipment[4..].copy_from_slice(&[445, 445]);
    candidate = field.clone();
    candidate.begin_battle(&menus, &[1])?;
    assert_eq!(candidate.members[0].technique_balance, -4);

    field.members[2].equipment[0] = u16::MAX;
    let before = serde_json::to_value(&field)?;
    assert!(field.begin_battle(&menus, &[1, 2, 3]).is_err());
    assert_eq!(serde_json::to_value(&field)?, before);
    Ok(())
}

#[test]
#[ignore = "requires current cooked party/item publications; no devices"]
fn cold_party_projects_original_starting_stats_models_and_controller_slots() -> Result<()> {
    let (files, menus, mut session) = snapshot()?;
    // The field leader does not select a battle actor or controller slot.
    session.field_leader = 9;
    session.settings.battle_controls = [0, 1, 2, 0];
    session.members[0].hp = 123;
    session.members[0].tp = 9;
    session.members[0].overlimit = 37;
    let before = serde_json::to_value(&session)?;
    let prepared = party::prepare(&files, &menus, &session, setups(4))?;
    assert_eq!(prepared[0].actor.overlimit, 370);
    assert!(!prepared[0].actor.overlimit_active);
    assert_eq!(serde_json::to_value(&session)?, before);
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
        assert_eq!((actor.max_hp, actor.max_tp), (hp, tp));
        assert_eq!(
            [
                actor.stats.slash,
                actor.stats.thrust,
                actor.stats.defense,
                actor.stats.intelligence,
                actor.stats.accuracy,
                actor.stats.evasion
            ],
            stats
        );
        assert_eq!((actor.luck, actor.stats.level), (50, 1));
        assert_eq!(actor.guard.break_pressure, (hp / 100 + 3) as i16);
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
        assert_eq!(value.weapons.len(), weapons);
        assert_eq!(
            value.weapons.iter().map(|w| w.slot).collect::<Vec<_>>(),
            (0..weapons as u8).collect::<Vec<_>>()
        );
    }
    assert_eq!((prepared[0].actor.hp, prepared[0].actor.tp), (123, 9));
    // The body candidate can activate independently of weapon contact bindings.
    PreparedBattle::new(
        prepared.iter().map(|p| p.actor.clone()).collect(),
        vec![],
        1,
        prepared.iter().map(|p| Some(p.model.clone())).collect(),
        vec![],
    )?;
    session.formation = vec![3, 1, 2];
    for (preference, expected) in [(0, [10, 0, 5]), (3, [25, 0, 5])] {
        session.members[2].strategy[2] = preference;
        let reordered = party::prepare(&files, &menus, &session, setups(3))?;
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
    // Kratos's starting Long Sword carries original effect 102. Its equipment
    // behavior is outside the opening roster and must still block activation.
    assert!(
        party::prepare(&files, &menus, &session, setups(3))
            .err()
            .unwrap()
            .to_string()
            .contains("battle equipment effects [102] are not prepared")
    );
    // Lloyd's ordinary contact consumer can bind both swords through the same
    // existing rigid-weapon helper used by the original pose oracle tests.
    let lloyd = &prepared[0];
    let mut model = (*lloyd.model).clone();
    let mut groups = Vec::new();
    for binding in &lloyd.weapons {
        let weapon = battle::model::weapon(&files, binding.item)?;
        groups.extend(battle::model::rigid_weapon(
            &mut model,
            &weapon.parts[&binding.part],
            binding.bone,
        )?);
    }
    assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2]);
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
    session.members[2].conditions = 0x100;
    menus.items[135].properties.attack_element = Some(Element::Fire);
    menus.items[398].properties = Default::default();
    menus.items[399].properties = Default::default();
    menus.items[398].properties.attack_element = Some(Element::Water);
    menus.items[399].properties.attack_element = Some(Element::Wind);
    menus.items[398].properties.neutral_resistance = 5;
    menus.items[399].properties.resistance =
        [(Element::Water, -2), (Element::Wind, 2), (Element::Fire, 7)].into();
    let prepared = party::prepare(&files, &menus, &session, setups(2))?;
    let actor = &prepared[0].actor;
    assert_eq!(actor.max_hp, 210);
    assert_eq!(actor.stats.slash, 116);
    assert_eq!(actor.stats.defense, 29);
    assert_eq!(actor.guard.reduction, 80);
    assert_eq!(actor.elements.base, Some(Element::Water));
    assert_eq!(
        &actor.affinities[..4],
        &[
            Affinity::Immune,
            Affinity::Weak,
            Affinity::Resistant,
            Affinity::Absorb
        ]
    );
    assert!(prepared[1].actor.petrified);
    session.members[0].hp = 0;
    session.members[0].conditions = 0x8000_0000;
    let prepared = party::prepare(&files, &menus, &session, setups(2))?;
    assert_eq!(prepared.len(), 2);
    assert_eq!(prepared[0].actor.hp, 0);
    Ok(())
}

#[test]
#[ignore = "requires current cooked party/item publications; no devices"]
fn cold_party_failure_never_mutates_the_suspended_session() -> Result<()> {
    let (files, menus, session) = snapshot()?;
    for fault in 0..10 {
        let mut session = session.clone();
        let mut placements = setups(4);
        match fault {
            0 => session.formation[1] = 1,
            1 => session.formation[1] = 0,
            2 => session.settings.battle_controls[1] = 3,
            3 => session.members[1].equipment[0] = u16::MAX,
            4 => session.members[1].conditions = 0x20,
            5 => session.members[1].ex_skills[0] = 6,
            6 => session.members[1].overlimit = 101,
            7 => placements[1].position[0] = f32::NAN,
            8 => placements[1].model.initial.clip = u16::MAX,
            _ => session.members[1].hp = u16::MAX,
        }
        let before = serde_json::to_value(&session)?;
        let result = party::prepare(&files, &menus, &session, placements).and_then(|actors| {
            // Model playback is validated with the complete generation, after
            // party projection has supplied its owned actor/model candidates.
            PreparedBattle::new(
                actors.iter().map(|p| p.actor.clone()).collect(),
                vec![],
                1,
                actors.iter().map(|p| Some(p.model.clone())).collect(),
                vec![],
            )
        });
        assert!(result.is_err(), "fault {fault}");
        assert_eq!(serde_json::to_value(&session)?, before, "fault {fault}");
    }
    Ok(())
}
