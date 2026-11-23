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
fn preseas_early_outfit_is_restored_until_her_recovery() -> anyhow::Result<()> {
    use resonance_content::{
        menu_data::{Costume, Title},
        overworld::PartyCharacter,
    };

    let mut data = super::super::tests::data();
    let mut menus = super::super::tests::technique_menu();
    let index = PartyCharacter::Presea.index();
    menus.titles[index].push(Title {
        growth: [0; 7],
        costume: Some(Costume::Standard),
    });
    data.rules = Some(std::sync::Arc::new(menus));
    let mut legacy = saved(&data);
    assert_eq!(legacy["party"]["members"][index]["costume"], 3);
    legacy["party"]
        .as_object_mut()
        .unwrap()
        .remove("story_costumes_initialized");

    legacy["party"]["members"][index]["costume"] = json!(0);
    let mut restored = load(legacy.clone(), &data)?;
    assert_eq!(restored.members[index].costume, 3);

    // A script may deliberately choose zero after the migration.
    restored.members[index].costume = 0;
    let current = serde_json::to_value(SavedProgress::new(restored))?;
    assert_eq!(load(current, &data)?.members[index].costume, 0);

    for (recovered, costume, owns_standard, expected) in [
        (true, 0, false, 0),
        (false, 2, false, 2),
        (false, 0, true, 0),
    ] {
        let mut legacy = legacy.clone();
        legacy["party"]["members"][index]["costume"] = json!(costume);
        if owns_standard {
            legacy["party"]["members"][index]["titles"] = json!([1, 2]);
        }
        if recovered {
            legacy["event_flags"] = json!([28]);
        }
        let restored = load(legacy, &data)?;
        assert_eq!(restored.members[index].costume, expected);
    }
    Ok(())
}

#[test]
fn unavailable_artes_and_their_references_are_ignored_on_load() -> anyhow::Result<()> {
    use resonance_content::input::{ArteBinding, InputChord, PhysicalInput};
    let data = super::super::tests::data();
    for (owner, id) in [(0, 260), (1, 999)] {
        let assistant = (owner + 1) % data.characters.len();
        let mut modded = data.clone();
        modded.characters[owner].allowed_techniques.push(id);
        let mut party = Party::new(&modded, Default::default())?;
        let member = &mut party.members[owner];
        member.techniques.extend([10, id]);
        member.shortcuts = [id, 10, 0, 0];
        member.unison_arte = Some(id);
        member.disabled_techniques.insert(id);
        member.technique_uses = [(10, 6), (id, 9)].into();
        member.arte_bindings.push(ArteBinding {
            technique: id,
            chord: InputChord(vec![PhysicalInput::Key("F11".into())]),
        });
        party.members[assistant].assist_shortcuts = [id, 10].map(|technique| {
            Some(TechniqueShortcut {
                character: owner,
                technique,
            })
        });
        party.validate(&modded)?;
        let original = serde_json::to_value(SavedProgress::new(party.clone()))?;
        assert_eq!(
            serde_json::to_value(load(original.clone(), &modded)?)?,
            original["party"]
        );

        party.remove_technique(owner, id);
        party.members[owner].technique_uses.remove(&id);
        let expected = serde_json::to_value(&party)?;
        let mut restored = load(original.clone(), &data)?;
        assert_eq!(serde_json::to_value(&restored)?, expected);
        restored.recover(&data)?;
        assert_eq!(serde_json::to_value(restored)?, expected);

        // Historical uses and orphaned assignments must not prevent loading.
        let mut forgotten = original.clone();
        forgotten["party"]["members"][owner]["techniques"] = json!([10]);
        assert_eq!(
            serde_json::to_value(load(forgotten.clone(), &data)?)?,
            expected
        );
        forgotten["party"]["members"][owner]["technique_uses"] = json!({"10": 6});
        assert_eq!(serde_json::to_value(load(forgotten, &data)?)?, expected);

        // Another character's catalogue cannot preserve this owner's removed arte.
        let mut reassigned = data.clone();
        reassigned.characters[assistant].allowed_techniques.push(id);
        assert_eq!(
            serde_json::to_value(load(original, &reassigned)?)?,
            expected
        );
    }
    Ok(())
}

#[test]
fn unlearned_arte_assignments_are_cleared_without_changing_learning() -> anyhow::Result<()> {
    use resonance_content::input::{ArteBinding, InputChord, PhysicalInput};
    let data = super::super::tests::data();
    for owner in [0, 8] {
        let assistant = (owner + 1) % data.characters.len();
        let mut party = Party::new(&data, Default::default())?;
        party.members[assistant].techniques.insert(10);
        party.members[assistant].shortcuts[0] = 10;
        party.members[assistant].assist_shortcuts[1] = Some(TechniqueShortcut {
            character: assistant,
            technique: 10,
        });
        // A forgotten arte's historical uses do not mean it is still learned.
        party.members[owner].technique_uses.insert(10, 9);
        let expected = serde_json::to_value(&party)?;

        let member = &mut party.members[owner];
        member.shortcuts[0] = 10;
        member.unison_arte = Some(10);
        member.disabled_techniques.insert(10);
        member.arte_bindings.push(ArteBinding {
            technique: 10,
            chord: InputChord(vec![PhysicalInput::Key("F11".into())]),
        });
        party.members[assistant].assist_shortcuts[0] = Some(TechniqueShortcut {
            character: owner,
            technique: 10,
        });
        party.members[owner].assist_shortcuts[0] = Some(TechniqueShortcut {
            character: 99,
            technique: 10,
        });
        let original = serde_json::to_value(SavedProgress::new(party))?;
        let mut restored = load(original, &data)?;
        assert_eq!(serde_json::to_value(&restored)?, expected);
        restored.recover(&data)?;
        assert_eq!(serde_json::to_value(restored)?, expected);
    }
    Ok(())
}

#[test]
fn old_horizontal_assignments_migrate_with_the_first_saved_owner() -> anyhow::Result<()> {
    use resonance_content::input::{ArteBinding, AxisDirection::*, InputChord, PhysicalInput};
    let mut data = super::super::tests::data();
    data.characters[0].allowed_techniques = vec![10, 11];
    for (left, right) in [
        (
            PhysicalInput::Axis(LeftLeft),
            PhysicalInput::Axis(LeftRight),
        ),
        (
            PhysicalInput::Key("ArrowLeft".into()),
            PhysicalInput::Key("ArrowRight".into()),
        ),
        (
            PhysicalInput::Button("DPadLeft".into()),
            PhysicalInput::Button("DPadRight".into()),
        ),
    ] {
        for other_member in [0, 1] {
            let mut party = Party::new(&data, Default::default())?;
            party.members[0].techniques.extend([10, 11]);
            party.members[other_member].techniques.insert(10);
            let chord = |input| InputChord(vec![PhysicalInput::Button("East".into()), input]);
            party.members[0].arte_bindings.push(ArteBinding {
                technique: 11,
                chord: chord(left.clone()),
            });
            party.members[other_member].arte_bindings.push(ArteBinding {
                technique: 10,
                chord: chord(right.clone()),
            });
            assert!(party.validate(&data).is_err());
            let value = serde_json::to_value(SavedProgress::new(party))?;
            let mut restored = load(value, &data)?;
            assert_eq!(
                restored.members[0].arte_bindings,
                [ArteBinding {
                    technique: 11,
                    chord: chord(left.clone()).normalized(&restored.settings.preferences.keymap)
                }]
            );
            assert_eq!(
                restored
                    .members
                    .iter()
                    .map(|m| m.arte_bindings.len())
                    .sum::<usize>(),
                1
            );
            assert!(restored.members[other_member].techniques.contains(&10));
            let admitted = serde_json::to_value(&restored)?;
            restored.recover(&data)?;
            assert_eq!(serde_json::to_value(&restored)?, admitted);
        }
    }
    Ok(())
}

#[test]
fn conflicting_and_malformed_assignments_keep_the_first_usable_binding() -> anyhow::Result<()> {
    use resonance_content::input::{ArteBinding, AxisDirection::*, InputChord, PhysicalInput};
    let mut data = super::super::tests::data();
    data.characters[0].allowed_techniques = vec![10, 11];
    let mut party = Party::new(&data, Default::default())?;
    party.members[0].techniques.extend([10, 11]);
    let chord = |axis| {
        InputChord(vec![
            PhysicalInput::Button("East".into()),
            PhysicalInput::Axis(axis),
        ])
    };
    let first = ArteBinding {
        technique: 10,
        chord: chord(LeftLeft),
    };
    let last = ArteBinding {
        technique: 11,
        chord: InputChord(vec![PhysicalInput::Key("F11".into())]),
    };
    party.members[0].arte_bindings = vec![
        ArteBinding {
            technique: 11,
            chord: InputChord(vec![]),
        },
        first.clone(),
        ArteBinding {
            technique: 10,
            chord: InputChord(vec![PhysicalInput::Key("F10".into())]),
        },
        ArteBinding {
            technique: 11,
            chord: chord(LeftRight),
        },
        last.clone(),
    ];
    let expected = vec![
        ArteBinding {
            chord: first.chord.normalized(&party.settings.preferences.keymap),
            ..first
        },
        last,
    ];
    let value = serde_json::to_value(SavedProgress::new(party))?;
    let restored = load(value, &data)?;
    assert_eq!(restored.members[0].arte_bindings, expected);
    Ok(())
}

#[test]
fn unsupported_combined_stick_assignments_become_unassigned_on_load() -> anyhow::Result<()> {
    use resonance_content::input::{ArteBinding, AxisDirection::*, InputChord, PhysicalInput};
    let mut data = super::super::tests::data();
    data.characters[0].allowed_techniques = vec![10, 11];
    let mut party = Party::new(&data, Default::default())?;
    party.members[0].techniques.extend([10, 11]);
    let free_binding = ArteBinding {
        technique: 11,
        chord: InputChord(vec![PhysicalInput::Key("F11".into())]),
    };
    party.members[0].arte_bindings = vec![
        ArteBinding {
            technique: 10,
            chord: InputChord(vec![
                PhysicalInput::Button("East".into()),
                PhysicalInput::Axis(LeftLeft),
                PhysicalInput::Axis(LeftUp),
            ]),
        },
        free_binding.clone(),
    ];
    assert!(party.validate(&data).is_err());
    let value = serde_json::to_value(SavedProgress::new(party))?;
    let restored = load(value, &data)?;
    assert_eq!(restored.members[0].arte_bindings, [free_binding]);
    assert!(restored.members[0].techniques.contains(&10));
    Ok(())
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
fn valid_saves_are_unchanged_and_structurally_invalid_saves_still_fail() -> anyhow::Result<()> {
    let data = super::super::tests::data();
    let original = saved(&data);
    let party = load(original.clone(), &data)?;
    assert_eq!(serde_json::to_value(party)?, original["party"]);
    for (path, invalid) in [
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

#[test]
fn stale_selections_and_history_recover_without_discarding_valid_edits() -> anyhow::Result<()> {
    let data = super::super::tests::data();
    let mut value = saved(&data);
    let party = &mut value["party"];
    party["formation"] = json!([0, 3, 3, 99, 2]);
    party["field_leader"] = json!(99);
    party["items"] = json!({"1": 7, "999": 8});
    party["found_items"] = json!([1, 999]);
    party["recent_items"] = json!([999, 1, 2]);
    party["figurines"] = json!([1, 999]);
    party["viewed_skits"] = json!([0, 4, 999]);
    party["monsters"] = json!({"1": {"seen":true,"scanned":false,"drops":[false,false],"steal":false,"location":false,"variant":99}, "255": {"seen":true,"scanned":false,"drops":[false,false],"steal":false,"location":false,"variant":0}});
    party["encounter_modifier"] = json!({"rate": 99, "remaining": 500});
    party["cooking"]["chef"] = json!(99);
    party["cooking"]["recipe"] = json!(99);
    party["travel"]["saved_formation"] = json!([2, 0, 2, 99, 1]);
    party["travel"]["current_location"] = json!(999);
    party["travel"]["visited_locations"] = json!([1, 999]);
    party["travel"]["visited_shops"] = json!([1, 99]);
    party["travel"]["opened_treasures"] = json!([1, 1024]);
    party["settings"]["battle_rank"] = json!(99);
    party["settings"]["battle_controls"] = json!([99, 0, 1, 2]);
    party["strategy_presets"] = json!(
        (0..3)
            .map(|_| json!({"name":"\n", "members": ([[99, 1, 99]; 9])}))
            .collect::<Vec<_>>()
    );
    let member = &mut party["members"][0];
    member["name"] = json!("Edited\nLloyd name");
    member["costume"] = json!(99);
    member["title"] = json!(99);
    member["titles"] = json!([1, 99]);
    member["strategy"] = json!([99, 2, 99]);
    let mut restored = load(value, &data)?;
    assert_eq!(restored.formation, [3, 2]);
    assert_eq!(restored.field_leader, 3);
    assert_eq!(restored.items, [(1, 7)].into());
    assert_eq!(restored.found_items, [1].into());
    assert_eq!(restored.recent_items, [1, 2]);
    assert_eq!(restored.figurines, [1].into());
    assert_eq!(restored.viewed_skits, [4].into());
    assert_eq!(restored.monsters.len(), 1);
    assert_eq!(restored.monsters[&1].variant, 0);
    assert!(restored.encounter_modifier.is_none());
    assert_eq!((restored.cooking.recipe, restored.cooking.chef), (0, 2));
    assert_eq!(restored.travel.saved_formation, [2, 1]);
    assert_eq!(restored.travel.current_location, None);
    assert_eq!(restored.travel.visited_locations, [1].into());
    assert_eq!(restored.travel.visited_shops, [1].into());
    assert_eq!(restored.travel.opened_treasures, [1].into());
    assert_eq!(restored.settings.preferences.battle_rank, 0);
    assert_eq!(restored.settings.battle_controls, [1, 0, 1, 2]);
    let member = &restored.members[0];
    assert_eq!(member.name.as_deref(), Some("EditedLloyd "));
    assert_eq!((member.costume, member.title), (0, 1));
    assert_eq!(member.titles, [1].into());
    assert_eq!(member.strategy, [0, 2, 0]);
    for preset in restored.strategy_presets.as_ref().unwrap() {
        assert_eq!(preset.name, "Custom");
        assert_eq!(preset.members, [[0, 1, 0]; 9]);
    }
    let expected = serde_json::to_value(&restored)?;
    restored.recover(&data)?;
    assert_eq!(serde_json::to_value(restored)?, expected);
    let mut value = saved(&data);
    value["party"]["formation"] = json!([99]);
    assert_eq!(load(value.clone(), &data)?.formation, [1]);
    value["party"]["field_leader"] = json!(5);
    assert_eq!(load(value, &data)?.formation, [5]);
    Ok(())
}

#[test]
fn unusable_loadout_entries_are_cleared_before_recovering_vitals() -> anyhow::Result<()> {
    let mut data = super::super::tests::data();
    let mut menu = super::super::tests::technique_menu();
    menu.ex_skills.characters[0]
        .compounds
        .push(resonance_content::menu_data::CompoundExSkill {
            skill: 31,
            required: vec![31],
        });
    menu.techniques.resize(11, Default::default());
    data.rules = Some(std::sync::Arc::new(menu));
    let mut value = saved(&data);
    value["party"]["items"] = json!({"1": 18, "2": 20});
    let member = &mut value["party"]["members"][0];
    member["equipment"] = json!([999, 1, 1, 2, 0, 0]);
    member["ex_gems"] = json!([99, 1, 0, 1]);
    member["ex_skills"] = json!([31, 31, 31, 99]);
    member["compound_ex_skills"] = json!([0, 99]);
    member["recent_compound_ex_skills"] = json!([0, 1]);
    member["techniques"] = json!([10]);
    member["unison_arte"] = json!(10);
    member["hp"] = json!(9999);
    let mut restored = load(value, &data)?;
    assert_eq!(restored.items, [(1, 20), (2, 20)].into());
    let member = &restored.members[0];
    assert_eq!(member.equipment, [0; 6]);
    assert_eq!(member.ex_gems, [5, 1, 0, 1]);
    assert_eq!(member.ex_skills, [31, 0, 0, 0]);
    assert_eq!(member.compound_ex_skills, [0].into());
    assert_eq!(member.recent_compound_ex_skills, [0].into());
    assert_eq!(member.unison_arte, None);
    assert_eq!(member.techniques, [10].into());
    assert_eq!(member.hp, 100);
    let expected = serde_json::to_value(&restored)?;
    restored.recover(&data)?;
    assert_eq!(serde_json::to_value(restored)?, expected);
    Ok(())
}

#[test]
fn optional_event_record_metadata_cannot_block_save_loading() -> anyhow::Result<()> {
    let data = super::super::tests::data();
    let mut value = saved(&data);
    value["tick"] = json!(50);
    value["event_records"] = json!({"12":{"value":2,"extra":3,"tick":99,"level":99,"recorded_at":-1},"201":{"value":1,"extra":0,"tick":1}});
    let progress: SavedProgress = serde_json::from_slice(&serde_json::to_vec(&value)?)?;
    let state = progress.into_state(&data)?;
    assert_eq!(state.event_records.len(), 1);
    let record = &state.event_records[&12];
    assert_eq!((record.value, record.extra, record.tick), (2, 3, 50));
    assert_eq!((record.level, record.recorded_at), (None, None));
    Ok(())
}
