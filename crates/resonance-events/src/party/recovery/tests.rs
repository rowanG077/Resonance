use super::*;
use crate::SavedProgress;
use serde_json::{Value, json};

fn saved(data: &SessionData) -> Value {
    serde_json::to_value(SavedProgress::new(
        Party::new(data, Default::default()).unwrap(),
    ))
    .unwrap()
}

fn load(value: Value, data: &SessionData) -> anyhow::Result<Party> {
    // Exercise JSON byte decoding as well as admission, including values too
    // large for the serialized field's Rust storage type.
    let saved: SavedProgress = serde_json::from_slice(&serde_json::to_vec(&value)?)?;
    Ok(saved.into_state(data)?.party.unwrap())
}

#[test]
fn integer_amounts_beyond_64_bits_recover_without_float_rounding() -> anyhow::Result<()> {
    let data = super::super::tests::data();
    let original = serde_json::to_string(&saved(&data))?;
    for (amount, expected) in [
        (99_999_999_999_999_999_999_i128, 99_999_999),
        (-99_999_999_999_999_999_999_i128, 0),
        (i128::MAX, 99_999_999),
        (i128::MIN, 0),
    ] {
        // Encode the integer directly: serde_json::Value would first narrow it
        // to a float, losing the original integer token before save decoding.
        let edited = original.replace("\"gald\":0", &format!("\"gald\":{amount}"));
        let progress: SavedProgress = serde_json::from_str(&edited)?;
        let party = progress.into_state(&data)?.party.unwrap();
        assert_eq!(party.gald, expected);
    }
    Ok(())
}

#[test]
fn experience_recovers_to_its_gameplay_cap_and_stays_stable_on_gain() -> anyhow::Result<()> {
    let data = super::super::tests::data();
    for (edited, expected) in [
        (-1_i128, 0),
        (0, 0),
        (123_456, 123_456),
        (9_999_999, 9_999_999),
        (10_000_000, 9_999_999),
        (9_999_999_999, 9_999_999),
        (i128::from(u64::MAX), 9_999_999),
    ] {
        let mut value = saved(&data);
        // Include a reserve character as well as the field leader.
        for index in [0, 8] {
            value["party"]["members"][index]["experience"] = json!(edited);
        }
        let mut party = load(value, &data)?;
        for index in [0, 8] {
            assert_eq!(party.members[index].experience, expected);
        }
        party
            .gain_experience(&data, 0, 0, [0; 7], |_| Ok(true), || 0)
            .map_err(anyhow::Error::msg)?;
        assert_eq!(party.members[0].experience, expected);
    }
    Ok(())
}

#[test]
fn edited_amounts_recover_through_normal_save_admission() -> anyhow::Result<()> {
    let data = super::super::tests::data();
    let mut value = saved(&data);
    let party = &mut value["party"];
    party["gald"] = json!(u64::MAX);
    party["grade_hundredths"] = json!(1_000_000_000);
    party["game_clears"] = json!(1000);
    party["unison_gauge"] = json!(999);
    party["items"] = json!({"1": 99999, "2": 0, "3": 999});
    party["recent_items"] = json!(vec![1; 40]);
    party["encounter_modifier"] = json!({"rate": 1, "remaining": 99999});
    let member = &mut party["members"][0];
    member["hp"] = json!(99999);
    member["tp"] = json!(99999);
    member["level"] = json!(999);
    member["overlimit"] = json!(999);
    member["technique_balance"] = json!(-1000);
    member["cooking"] = json!(([999; resonance_content::menu_data::RECIPE_COUNT]));
    member["technique_uses"] = json!({"10": 999999});
    party["members"][1]["base_stats"] = json!(([999999; 7]));
    party["battles"]["total"] = json!(999999);
    party["battles"]["hunting_streak"] = json!(999);
    party["battles"]["deaths"] = json!(([999; 9]));
    party["battles"]["grade"] = json!(i64::MIN);

    let mut party = load(value, &data)?;
    assert_eq!(
        (party.gald, party.grade_hundredths),
        (99_999_999, 99_999_999)
    );
    assert_eq!((party.game_clears, party.unison_gauge), (100, 200));
    assert_eq!(party.items, [(1, 20), (3, 1)].into());
    assert_eq!(party.recent_items.len(), 32);
    assert_eq!(party.encounter_modifier.as_ref().unwrap().remaining, 3600);
    let member = &party.members[0];
    assert_eq!((member.hp, member.tp), (100, 20));
    assert_eq!(
        (member.level, member.overlimit, member.technique_balance),
        (4, 100, -100)
    );
    assert_eq!(
        member.cooking,
        [8; resonance_content::menu_data::RECIPE_COUNT]
    );
    assert_eq!(
        member.technique_uses[&10],
        resonance_content::arte::MAX_USES
    );
    assert_eq!(
        party.members[1].base_stats,
        [9999, 999, 32767, 32767, 32767, 32767, 32767]
    );
    assert_eq!(
        (party.battles.total, party.battles.hunting_streak),
        (9999, 5)
    );
    assert_eq!(party.battles.deaths, [250; 9]);
    assert_eq!(party.battles.grade, i32::MIN);
    // Repeating admission cannot change an already repaired save.
    let repaired = serde_json::to_value(&party)?;
    party.recover(&data)?;
    assert_eq!(serde_json::to_value(&party)?, repaired);
    Ok(())
}

#[test]
fn zero_and_negative_amounts_do_not_revive_or_invent_items() -> anyhow::Result<()> {
    let data = super::super::tests::data();
    let mut value = saved(&data);
    value["party"]["gald"] = json!(-10);
    value["party"]["items"] = json!({"1": -10});
    value["party"]["members"][0]["hp"] = json!(0);
    value["party"]["members"][0]["tp"] = json!(-10);
    value["party"]["members"][0]["level"] = json!(0);
    value["party"]["encounter_modifier"] = json!({"rate": 1, "remaining": 0});
    let party = load(value, &data)?;
    assert_eq!(party.gald, 0);
    assert!(party.items.is_empty());
    assert_eq!(
        (
            party.members[0].hp,
            party.members[0].tp,
            party.members[0].level
        ),
        (0, 0, 1)
    );
    assert!(party.encounter_modifier.is_none());
    Ok(())
}

#[test]
fn item_recovery_honors_new_game_plus_and_unique_items() -> anyhow::Result<()> {
    let data = super::super::tests::data();
    let mut value = saved(&data);
    value["party"]["new_game_plus"]["benefits"] = json!(["thirty_items"]);
    value["party"]["items"] = json!({"1": 999, "3": 999});
    let party = load(value, &data)?;
    assert_eq!(party.items, [(1, 30), (3, 1)].into());
    Ok(())
}

#[test]
fn recovery_uses_equipment_and_ex_skill_vital_bonuses() -> anyhow::Result<()> {
    use resonance_content::menu_data::{ExStat, ExStatBonus, Item};
    let mut data = super::super::tests::data();
    let mut menu = super::super::tests::technique_menu();
    menu.ex_skills.skills.get_mut(&31).unwrap().stat_bonuses = vec![
        ExStatBonus {
            stat: ExStat::MaxHp,
            percent: 50,
        },
        ExStatBonus {
            stat: ExStat::MaxTp,
            percent: 50,
        },
    ];
    let mut item: Item = serde_json::from_value(json!({
        "category": 0, "battle_usable": false, "equipment_stats": ([0; 7]),
        "price": 0, "transforms_to": 0, "field_use": null, "attention": null
    }))?;
    item.properties.stat_bonuses = vec![
        ExStatBonus {
            stat: ExStat::MaxHp,
            percent: 25,
        },
        ExStatBonus {
            stat: ExStat::MaxTp,
            percent: 25,
        },
    ];
    menu.items = vec![item; data.items.len()];
    data.rules = Some(std::sync::Arc::new(menu));
    let mut value = saved(&data);
    value["party"]["members"][0]["ex_gems"] = json!([1, 0, 0, 0]);
    value["party"]["members"][0]["ex_skills"] = json!([31, 0, 0, 0]);
    value["party"]["members"][0]["equipment"] = json!([1, 0, 0, 0, 0, 0]);
    value["party"]["members"][0]["hp"] = json!(9999);
    value["party"]["members"][0]["tp"] = json!(9999);
    let party = load(value, &data)?;
    assert_eq!((party.members[0].hp, party.members[0].tp), (175, 35));
    Ok(())
}

#[test]
fn valid_saves_are_unchanged_and_unknown_references_still_fail() -> anyhow::Result<()> {
    let data = super::super::tests::data();
    let original = saved(&data);
    let party = load(original.clone(), &data)?;
    assert_eq!(serde_json::to_value(party)?, original["party"]);
    for (path, invalid) in [
        ("/party/members/0/equipment/0", json!(999)),
        ("/party/members/0/ex_skills/0", json!(99)),
        ("/party/members/0/title", json!(99)),
        ("/party/members/0/techniques", json!([999])),
        ("/party/items", json!({"999": 999})),
        ("/party/formation", json!([99])),
        ("/party/members", json!([])),
        ("/script_globals", json!([])),
        ("/party/gald", json!("999999999")),
        ("/party/members/0/tp", json!(1.5)),
        ("/party/members/0/tp", json!(1.0)),
        ("/party/members/0/base_stats", json!([100, 20])),
    ] {
        let mut edited = original.clone();
        *edited.pointer_mut(path).unwrap() = invalid;
        assert!(load(edited, &data).is_err(), "{path}");
    }
    Ok(())
}
