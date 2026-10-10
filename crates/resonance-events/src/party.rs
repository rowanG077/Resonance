//! Session-owned party state. Rendering and event bytecode do not own inventory.
use resonance_content::session::SessionData;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
mod battle_flags_tests;
mod battles;
mod bestiary;
pub use battles::BattleStatistics;
mod conditions;
mod cooking;
mod crafting;
pub use crafting::CraftError;
mod ex_skills;
pub use bestiary::MonsterKnowledge;
mod items;
pub mod new_game_plus;
mod stats;
mod strategy;
mod techniques;
mod travel;
pub use conditions::{Ailments, Poison, StatBuff};
pub use cooking::{Cooking, CookingError, Meal};
pub use items::EncounterModifier;
pub use stats::{EquipmentTraits, Stats};
pub use travel::Travel;

fn initial_title() -> u8 {
    1
}
fn initial_titles() -> BTreeSet<u8> {
    [1].into()
}
fn initial_leader() -> u8 {
    1
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Native body variant. The early Colette transformation uses variant 3,
    /// which shares the normal model (variants 1/2/4 need other cooked bodies).
    #[serde(default)]
    pub costume: u8,
    /// Rules and owner identity are rebound on load, not duplicated in saves.
    #[serde(skip)]
    rules: Option<ex_skills::Rules>,
    #[serde(default = "initial_title")]
    /// Events may temporarily equip a title that has not been learned.
    pub title: u8,
    #[serde(default = "initial_titles")]
    pub titles: BTreeSet<u8>,
    /// Negative favors technical arts; positive favors strike arts.
    #[serde(default)]
    pub technique_balance: i8,
    pub affinity: i32,
    pub level: u8,
    pub experience: u32,
    pub base_stats: [u16; 7],
    pub hp: u16,
    pub tp: u16,
    pub ailments: Ailments,
    pub queued_buffs: BTreeSet<StatBuff>,
    pub luck: u8,
    pub overlimit: u8,
    pub equipment: [u16; 6],
    pub techniques: BTreeSet<u16>,
    pub shortcuts: [u16; 4],
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub disabled_techniques: BTreeSet<u16>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub technique_uses: BTreeMap<u16, u16>,
    #[serde(default)]
    pub assist_shortcuts: [Option<TechniqueShortcut>; 2],
    /// Action, skill/magic policy, and starting position.
    #[serde(default)]
    pub strategy: [u8; 3],
    #[serde(default)]
    pub cooking: [u8; resonance_content::menu_data::RECIPE_COUNT],
    #[serde(default)]
    pub ex_skills: [u8; 4],
    #[serde(default)]
    pub ex_gems: [u8; 4],
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub compound_ex_skills: BTreeSet<u8>,
    /// Highlights for recently learned compound skills.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub recent_compound_ex_skills: BTreeSet<u8>,
}

/// Preserve every learned arte, but display only the last one learned at each level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExperienceGain {
    pub techniques: Vec<u16>,
    pub notices: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TechniqueShortcut {
    pub character: usize,
    pub technique: u16,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use resonance_content::session::{CharacterDefinition, ItemDefinition, StatGrowth};

    pub(crate) fn data() -> SessionData {
        SessionData {
            rules: None,
            version: 1,
            executable_sha256: "0".repeat(64),
            experience: vec![0, 0, 10, 30, 60],
            items: (0..4)
                .map(|id| ItemDefinition {
                    equipment_kind: (id != 0).then_some(0),
                    allowed_characters: 511,
                    stack_limit: if id == 3 { 1 } else { 20 },
                })
                .collect(),
            characters: (0..9)
                .map(|_| CharacterDefinition {
                    cooking: [0; resonance_content::menu_data::RECIPE_COUNT],
                    ex_skills: [0; 4],
                    ex_gems: [0; 4],
                    compound_ex_skills: Vec::new(),
                    recent_compound_ex_skills: Vec::new(),
                    technique_balance: 0,
                    affinity: 0,
                    level: 1,
                    experience: 0,
                    base_stats: [100, 20, 30, 40, 50, 60, 70],
                    luck: 10,
                    overlimit: 50,
                    equipment: [0; 6],
                    techniques: vec![],
                    allowed_techniques: vec![10],
                    shortcuts: [0; 4],
                    growth: std::array::from_fn(|_| StatGrowth {
                        base: 1,
                        random: 1,
                        title_bonus: 0,
                    }),
                    level_techniques: [(2, vec![10])].into(),
                })
                .collect(),
        }
    }

    #[test]
    fn crafting_checks_combined_materials_and_capacity_before_mutating_inventory() {
        use resonance_content::menu_data::crafting::Recipe;
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        let recipe = Recipe {
            result: 3,
            ingredients: [(1, 3)].into(),
        };
        party.items = [(1, 2)].into();
        assert_eq!(
            party.craft(&data, &recipe),
            Err(CraftError::MissingMaterials)
        );
        assert_eq!(party.items, [(1, 2)].into());
        party.items = [(1, 3), (3, 1)].into();
        assert_eq!(party.craft(&data, &recipe), Err(CraftError::InventoryFull));
        assert_eq!(party.items, [(1, 3), (3, 1)].into());
        party.items.remove(&3);
        party.craft(&data, &recipe).unwrap();
        assert_eq!(party.items, [(3, 1)].into());
        // An ingredient can also be the result, including a currently full stack.
        party
            .craft(
                &data,
                &Recipe {
                    result: 3,
                    ingredients: [(3, 1)].into(),
                },
            )
            .unwrap();
        assert_eq!(party.items, [(3, 1)].into());
    }

    #[test]
    fn percentage_recovery_and_damage_preserve_conditions_and_clamp_vitals() {
        let mut party = Party::new(&data(), Default::default()).unwrap();
        party.members[0].hp = 60;
        party.members[0].tp = 7;
        party.members[0].ailments.paralysis = true;
        party.adjust_vitals_percent([50, -50]);
        assert_eq!((party.members[0].hp, party.members[0].tp), (100, 0));
        party.adjust_vitals_percent([-100, 100]);
        assert_eq!((party.members[0].hp, party.members[0].tp), (1, 20));
        assert!(party.members[0].ailments.paralysis);
    }

    #[test]
    fn field_damage_leaves_one_hp_without_reviving_or_spending_tp() {
        let mut party = Party::new(&data(), Default::default()).unwrap();
        party.members[0].hp = 1;
        party.members[1].hp = 0;
        party.members[2].hp = 100;
        party.members[3].hp = 9;
        party.damage_hp_percent(10);
        assert_eq!(
            party.members[..4].iter().map(|m| m.hp).collect::<Vec<_>>(),
            [1, 0, 90, 1]
        );
        assert_eq!(party.members[2].tp, 20);
    }

    #[test]
    fn initial_and_saved_overlimit_percentages_stay_within_bounds() {
        let mut data = data();
        for percentage in [0, 100] {
            data.characters[0].overlimit = percentage;
            data.validate().unwrap();
            let mut party = Party::new(&data, Default::default()).unwrap();
            party.validate(&data).unwrap();
            for invalid in [101, 255] {
                party.members[0].overlimit = invalid;
                assert!(party.validate(&data).is_err());
            }
        }
        for invalid in [101, 255] {
            data.characters[0].overlimit = invalid;
            assert!(data.validate().is_err());
        }
    }

    #[test]
    fn new_game_starts_with_empty_battle_history() {
        let party = Party::new(&data(), Default::default()).unwrap();
        assert_eq!(party.battles, BattleStatistics::default());
        let mut saved = serde_json::to_value(party).unwrap();
        saved.as_object_mut().unwrap().remove("battles");
        assert!(serde_json::from_value::<Party>(saved).is_err());
    }

    #[test]
    fn current_techniques_and_forgotten_use_counts_survive_save_restore() {
        let mut data = data();
        data.characters[0].allowed_techniques = vec![10, 11];
        data.characters[0].techniques = vec![10];
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.members[0].technique_uses.insert(11, 42);
        let saved = crate::SavedProgress {
            script_globals: vec![0; 0x100],
            party,
            event_flags: Default::default(),
            event_records: Default::default(),
            script_state: Default::default(),
            random_state: 7,
            gameplay_random: Default::default(),
            tick: 0,
        };
        let encoded = serde_json::to_value(saved).unwrap();
        let restored: crate::SavedProgress = serde_json::from_value(encoded.clone()).unwrap();
        let party = restored.into_state(&data).unwrap().party.unwrap();
        assert_eq!(party.members[0].techniques, [10].into());
        assert_eq!(party.members[0].technique_uses[&11], 42);
        assert_eq!(serde_json::to_value(party).unwrap(), encoded["party"]);
    }

    #[test]
    fn technique_catalogues_validate_membership_without_import_mask_limits() {
        let mut data = data();
        let character = &mut data.characters[0];
        character.allowed_techniques = (1..=65).collect();
        character.techniques = character.allowed_techniques.clone();
        character.level_techniques = [(2, character.allowed_techniques.clone())].into();
        data.validate().unwrap();
        data.characters[0]
            .level_techniques
            .get_mut(&2)
            .unwrap()
            .push(66);
        assert!(data.validate().is_err());
    }

    #[test]
    fn field_leaders_need_hp_and_cannot_be_petrified() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        for hp in [0, 1] {
            let member = &mut party.members[0];
            member.hp = hp;
            assert_eq!(member.knocked_out(), hp == 0);
            assert_eq!(member.can_lead_field(), hp != 0);
            party.validate(&data).unwrap();
        }
        party.members[0].ailments.petrified = true;
        assert!(!party.members[0].can_lead_field());
        party.validate(&data).unwrap();
    }

    #[test]
    fn zero_maximum_hp_cannot_revive_and_small_revivals_restore_one_hp() {
        let mut data = data();
        data.characters[0].base_stats[0] = 0;
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.validate(&data).unwrap();
        assert!(!party.members[0].revive(30));
        party.heal(|| 0);
        assert!(party.members[0].knocked_out());
        party.validate(&data).unwrap();
        party.raise_level(&data, 0, 2, None, || 0).unwrap();
        assert_eq!(party.members[0].hp, 1);
        party.validate(&data).unwrap();
        party.members[0].hp = 0;
        assert!(party.members[0].revive(30));
        assert_eq!(party.members[0].hp, 1);
        party.validate(&data).unwrap();
    }

    #[test]
    fn usage_limit_applies_to_learned_and_forgotten_techniques() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        for learned in [false, true] {
            party.members[0].techniques = if learned {
                [10].into()
            } else {
                Default::default()
            };
            party.members[0].technique_uses.insert(10, 999);
            assert!(party.validate(&data).is_ok());
            party.members[0].technique_uses.insert(10, 1000);
            assert!(party.validate(&data).is_err());
        }
    }

    #[test]
    fn level_acquisition_uses_catalogue_order_and_preserves_disabled_membership() {
        for battle_growth in [false, true] {
            let mut data = data();
            data.characters[0].allowed_techniques = vec![12, 10, 11];
            data.characters[0].techniques = vec![11];
            data.characters[0].level_techniques = [(2, vec![10, 11, 12])].into();

            let mut party = Party::new(&data, Default::default()).unwrap();
            party.members[0].disabled_techniques.insert(11);
            if battle_growth {
                party
                    .gain_experience(&data, 0, 10, [0; 7], |_| Ok(true), || 0)
                    .unwrap();
            } else {
                party.raise_level(&data, 0, 2, None, || 0).unwrap();
            }
            assert_eq!(party.members[0].techniques, [10, 11, 12].into());
            assert_eq!(party.members[0].disabled_techniques, [11].into());
            assert_eq!(party.members[0].shortcuts, [12, 10, 0, 0]);
            assert!(party.members[0].technique_uses.is_empty());
            party.validate(&data).unwrap();
        }
    }

    #[test]
    fn event_acquisition_preserves_disabled_choices_shortcuts_and_use_counts() {
        use std::sync::Arc;
        let main: [u16; 12] = [
            0x0200, 1, 0, 0x3000, 0x4000, 0x0200, 10, 0, 0x3000, 0x4000, 0x202e, 0x20ff,
        ];
        let mut words = vec![10, 0, 0, 1, 0, 2, 0, 42, 0, main.len() as u16];
        words.extend(main);
        words.push(0x20ff);
        let program = Arc::new(
            symphonia_script::Program::decode(
                &words
                    .into_iter()
                    .flat_map(u16::to_be_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        );
        for already_learned in [false, true] {
            let mut data = data();

            if already_learned {
                data.characters[0].techniques.push(10);
            }
            let mut party = Party::new(&data, Default::default()).unwrap();
            party.members[0].technique_uses.insert(10, 42);
            if already_learned {
                party.members[0].disabled_techniques.insert(10);
            }
            let resources = crate::ResourceLibrary {
                session_data: Some(Arc::new(data)),
                ..Default::default()
            };
            let world = crate::GameWorld {
                party: Some(party),
                ..Default::default()
            };
            let mut events = crate::EventRuntime::with_state(
                program.clone(),
                Arc::new(resources),
                world,
                Default::default(),
            )
            .unwrap();
            events.step().unwrap();
            let member = &events.world.party.as_ref().unwrap().members[0];
            assert_eq!(member.techniques, [10].into());
            assert_eq!(member.disabled_techniques.contains(&10), already_learned);
            assert_eq!(member.shortcuts, [0; 4]);
            assert_eq!(member.technique_uses[&10], 42);
        }
    }

    fn technique_menu() -> resonance_content::menu_data::MenuData {
        use resonance_content::menu_data::{Technique, TechniqueUse::*};
        // Field technique operations need only these rules; unrelated page catalogues are empty.
        let techniques: Vec<_> = [
            (0, None),
            (0, None),
            (0, None),
            (0, None),
            (
                8,
                Some(Recover {
                    hp: 30,
                    party: false,
                }),
            ),
            (48, Some(Revive)),
            (10, Some(Cure { party: false })),
            (
                28,
                Some(Recover {
                    hp: 45,
                    party: true,
                }),
            ),
            (24, Some(Cure { party: true })),
        ]
        .into_iter()
        .enumerate()
        .map(|(id, (tp, field_use))| Technique {
            tp,
            tp_percent: false,
            unison_usable: false,
            rank: 0,
            element: 0,
            level: 0,
            route: 0,
            prerequisite: 0,
            alternatives: if id == 2 { [2, 3, 0, 0] } else { [0; 4] },
            field_use,
        })
        .collect();
        serde_json::from_value(serde_json::json!({
            "version": resonance_content::menu_data::MenuData::VERSION,
            "grade_shop": {"options": [], "labels": {}}, "crafting": resonance_content::menu_data::crafting::Data::default(),
            "items": [], "titles": vec![vec![serde_json::json!({"growth":vec![0;7],"costume":null})];9],
            "initial_names":([""; 9]),
            "techniques": techniques,
            "strategy": {"groups": [[], [], []],
                "presets": ([[[0; 3]; 9]; 3]),
                "default_positions": ([0; 9]), "positions": ([0; 6])},
            "cooking": {"recipes": [], "groups": [], "preferences": [], "bonus_skill": 0},
            "status": {"equipment_effects": {}},
            "world_map": {"locations": {}, "field_locations": {}, "shops": []},
            "ex_skills": {"skills": {"31": {
                "stat_bonuses": [], "save_point_tp_cost": 1, "tendency": null, "activation": "constant"}},
                "characters": vec![serde_json::json!({"levels": ([[31; 4]; 4]), "compounds": []}); 9],
                "gem_items": [1, 2, 3, 1, 2]}
        })).unwrap()
    }

    #[test]
    fn saved_titles_are_checked_against_each_characters_prepared_rules() {
        let mut data = data();
        let mut menus = technique_menu();
        menus.titles[0].resize(
            40,
            resonance_content::menu_data::Title {
                growth: [0; 7],
                costume: None,
            },
        );
        data.rules = Some(std::sync::Arc::new(menus));
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.members[0].title = 32;
        party.members[0].titles.insert(32);
        let encoded = serde_json::to_vec(&party).unwrap();
        let mut loaded: Party = serde_json::from_slice(&encoded).unwrap();
        loaded.bind_rules(&data);
        loaded.validate(&data).unwrap();
        let saved = serde_json::to_value(&loaded).unwrap();
        assert!(saved["members"][0].get("rules").is_none());
        for (character, equipped, owned) in [(0, 1, 41), (0, 41, 41), (1, 1, 2), (0, 0, 0)] {
            let mut invalid = saved.clone();
            invalid["members"][character]["title"] = equipped.into();
            invalid["members"][character]["titles"] = serde_json::json!([1, owned]);
            let invalid: Party = serde_json::from_value(invalid).unwrap();
            assert!(invalid.validate(&data).is_err());
        }
        data.rules = None;
        assert!(loaded.validate(&data).is_err());
        Party::new(&data, Default::default())
            .unwrap()
            .validate(&data)
            .unwrap();
    }

    #[test]
    fn menu_forgetting_preserves_use_counts_and_clears_assignments() {
        let menus = technique_menu();
        let mut data = data();
        data.characters[0].allowed_techniques = vec![1, 2, 3, 4];
        data.characters[0].level_techniques.clear();
        data.characters[0].techniques = vec![1, 2, 3];
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.formation = vec![1, 2, 3];
        party.members[1].techniques.insert(10);
        party.members[0].disabled_techniques.insert(2);
        party.members[0].technique_uses.insert(2, 500);
        party.members[0].shortcuts = [1, 0, 3, 0];
        let shortcut = |character, technique| {
            Some(TechniqueShortcut {
                character,
                technique,
            })
        };
        assert!(party.assign_technique(0, 1, shortcut(0, 2)).unwrap());
        assert!(!party.assign_technique(0, 1, shortcut(0, 2)).unwrap());
        assert!(party.assign_technique(2, 4, shortcut(0, 3)).unwrap());
        let before = serde_json::to_value(&party).unwrap();
        for (slot, owner, tech) in [(1, 1, 10), (4, usize::MAX, 3), (4, 0, 20), (6, 0, 2)] {
            assert!(
                party
                    .assign_technique(0, slot, shortcut(owner, tech))
                    .is_err()
            );
        }
        assert_eq!(serde_json::to_value(&party).unwrap(), before);
        assert!(!party.forget_technique(&menus, 0, 1).unwrap());
        assert!(party.forget_technique(&menus, 0, 2).unwrap());
        assert_eq!(party.members[0].techniques, [1].into());
        assert_eq!(party.members[0].technique_uses[&2], 500);
        assert!(party.members[0].disabled_techniques.is_empty());
        assert_eq!(party.members[0].shortcuts, [1, 0, 0, 0]);
        assert_eq!(party.members[2].assist_shortcuts[0], None);
        party.validate(&data).unwrap();
    }

    #[test]
    fn field_techniques_heal_cure_revive_and_reject_without_spending_tp() {
        let menus = technique_menu();
        let mut party = Party::new(&data(), Default::default()).unwrap();
        party.formation = vec![1, 4];
        party.members[3].techniques.extend([4, 5, 6]);
        party.members[3].base_stats[1] = 100;
        party.members[3].tp = 100;
        party.members[0].hp = 1;
        assert_eq!(
            party.cast_technique(&menus, 3, 0, 4, false).unwrap(),
            Some(104)
        );
        assert_eq!((party.members[0].hp, party.members[3].tp), (31, 92));
        for (hp, petrified, tp) in [(100, false, 92), (1, true, 92), (1, false, 7)] {
            party.members[0].hp = hp;
            party.members[3].ailments.petrified = petrified;
            party.members[3].tp = tp;
            let before = serde_json::to_value(&party).unwrap();
            assert_eq!(party.cast_technique(&menus, 3, 0, 4, false).unwrap(), None);
            assert_eq!(serde_json::to_value(&party).unwrap(), before);
        }
        party.members[3].ailments = Default::default();
        party.members[3].tp = 100;
        party.members[0].hp = 0;
        party.members[0].tp = 0;
        party.members[0].ailments.poison = Poison::Both;
        party.members[0].queued_buffs.insert(StatBuff::AttackUp);
        assert_eq!(
            party.cast_technique(&menus, 3, 0, 5, false).unwrap(),
            Some(132)
        );
        assert_eq!(
            (
                party.members[0].hp,
                party.members[0].tp,
                party.members[0].ailments
            ),
            (30, 0, Ailments::default())
        );
        assert_eq!(party.members[3].tp, 52);
        party.members[0].ailments = Ailments {
            poison: Poison::Both,
            paralysis: true,
            petrified: true,
            curse: true,
        };
        assert_eq!(
            party.cast_technique(&menus, 3, 0, 6, false).unwrap(),
            Some(132)
        );
        assert!(party.members[0].ailments.is_empty());
        assert_eq!(party.members[0].queued_buffs, [StatBuff::AttackUp].into());
        assert_eq!(party.members[3].tp, 42);
        let before = serde_json::to_value(&party).unwrap();
        assert!(party.cast_technique(&menus, 9, 0, 4, false).is_err());
        assert!(party.cast_technique(&menus, 3, 0, 99, false).is_err());
        assert_eq!(party.cast_technique(&menus, 3, 8, 4, false).unwrap(), None);
        assert_eq!(serde_json::to_value(&party).unwrap(), before);
    }

    #[test]
    fn group_field_techniques_include_reserves_skip_knockouts_and_charge_once() {
        let menus = technique_menu();
        let mut party = Party::new(&data(), Default::default()).unwrap();
        party.formation = vec![1, 2, 3, 4, 5];
        party.members[3].techniques.extend([7, 8]);
        party.members[3].base_stats[1] = 100;
        party.members[3].tp = 100;
        let ailments = Ailments {
            poison: Poison::Both,
            paralysis: true,
            petrified: true,
            curse: true,
        };
        for index in [0, 2, 4] {
            party.members[index].hp = 1;
            party.members[index].ailments = ailments;
        }
        party.members[1].hp = 0;
        party.members[1].ailments = ailments;
        assert_eq!(
            party.cast_technique(&menus, 3, 1, 7, false).unwrap(),
            Some(104)
        );
        assert_eq!(party.members[3].tp, 72);
        assert_eq!(
            party.cast_technique(&menus, 3, 1, 8, false).unwrap(),
            Some(132)
        );
        assert_eq!(party.members[3].tp, 48);
        for index in [0, 2, 4] {
            assert_eq!(
                (party.members[index].hp, party.members[index].ailments),
                (46, Ailments::default())
            );
        }
        assert_eq!(
            (party.members[1].hp, party.members[1].ailments),
            (0, ailments)
        );
        let before = serde_json::to_value(&party).unwrap();
        assert_eq!(party.cast_technique(&menus, 3, 4, 8, false).unwrap(), None);
        assert_eq!(serde_json::to_value(&party).unwrap(), before);
    }

    #[test]
    fn equipment_and_save_point_skills_apply_the_actual_cast_cost() {
        use resonance_content::menu_data::TpDiscount;
        let mut menus = technique_menu();
        let item = serde_json::from_value(serde_json::json!({
            "category":0,"battle_usable":false,"equipment_stats":vec![0;7],
            "price":0,"transforms_to":0
        }))
        .unwrap();
        menus.items = vec![item; 3];
        menus.items[1].properties.tp_discount = TpDiscount::Third;
        menus.items[2].properties.tp_discount = TpDiscount::Half;
        let mut data = data();
        data.items[1].equipment_kind = Some(4);
        data.items[2].equipment_kind = Some(4);
        data.rules = Some(std::sync::Arc::new(menus.clone()));
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.formation = vec![1, 4];
        party.members[3].techniques.insert(4);
        party.members[3].equipment[3] = 1;
        assert_eq!(party.members[3].technique_cost(&menus, 4, false), 5);
        party.members[3].equipment[3] = 2;
        assert_eq!(party.members[3].technique_cost(&menus, 4, false), 4);
        party.items.insert(1, 1);
        assert!(party.set_ex_gem(&data, 3, 0, 1).unwrap());
        assert!(party.set_ex_skill(&data, 3, 0, 31).unwrap());
        party.members[0].hp = 1;
        party.members[3].tp = 1;
        assert_eq!(party.members[3].technique_cost(&menus, 4, true), 1);
        let before = serde_json::to_value(&party).unwrap();
        assert_eq!(party.cast_technique(&menus, 3, 0, 4, false).unwrap(), None);
        assert_eq!(serde_json::to_value(&party).unwrap(), before);
        assert_eq!(
            party.cast_technique(&menus, 3, 0, 4, true).unwrap(),
            Some(104)
        );
        assert_eq!((party.members[0].hp, party.members[3].tp), (31, 0));
        assert_eq!(party.cast_technique(&menus, 3, 0, 4, true).unwrap(), None);
    }

    #[test]
    fn ailments_and_queued_buffs_roundtrip() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        for petrified in [false, true] {
            let ailments = Ailments {
                poison: Poison::Both,
                paralysis: true,
                petrified,
                curse: true,
            };
            party.members[0].ailments = ailments;
            party.members[0]
                .queued_buffs
                .insert(StatBuff::MagicDefenseUp);
            let value = serde_json::to_value(&party).unwrap();
            let loaded: Party = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(loaded.members[0].ailments, ailments);
            assert_eq!(
                loaded.members[0].queued_buffs,
                [StatBuff::MagicDefenseUp].into()
            );
            loaded.validate(&data).unwrap();
            assert_eq!(serde_json::to_value(loaded).unwrap(), value);
        }
        let native = serde_json::to_value(&party).unwrap();
        for missing in ["ailments", "queued_buffs"] {
            let mut incomplete = native.clone();
            incomplete["members"][0]
                .as_object_mut()
                .unwrap()
                .remove(missing);
            assert!(serde_json::from_value::<Party>(incomplete).is_err());
        }
        let mut legacy = native;
        legacy["members"][0]["conditions"] = serde_json::json!(0);
        assert!(serde_json::from_value::<Party>(legacy).is_err());
    }

    #[test]
    fn equipment_transfers_items_and_tracks_stack_limits_and_discoveries() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.change_item(&data, 1, 3).unwrap();
        party.change_item(&data, 2, 1).unwrap();
        party.equip(&data, 0, 1).unwrap();
        assert_eq!(party.items[&1], 2);
        party.equip(&data, 0, 2).unwrap();
        assert_eq!(party.items[&1], 3);
        assert!(!party.items.contains_key(&2));
        assert_eq!(party.members[0].equipment[0], 2);
        party.unequip(&data, 0, 0).unwrap();
        assert_eq!(party.items[&2], 1);
        assert_eq!(party.members[0].equipment[0], 0);
        assert!(party.change_item(&data, 3, 10).unwrap());
        assert_eq!(party.items[&3], 1);
        assert!(!party.change_item(&data, 3, 1).unwrap());
        party.change_item(&data, 3, -8).unwrap();
        assert!(!party.items.contains_key(&3));
        assert!(party.found_items.contains(&3));
        assert_eq!(party.recent_items[0], 3);
    }

    #[test]
    fn an_equipment_swap_with_no_room_leaves_inventory_and_equipment_intact() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.change_item(&data, 3, 1).unwrap();
        party.equip(&data, 0, 3).unwrap();
        party.change_item(&data, 3, 1).unwrap();
        party.change_item(&data, 1, 2).unwrap();
        let before = party.items.clone();
        assert!(party.equip(&data, 0, 1).is_err());
        assert_eq!(party.items, before);
        assert_eq!(party.members[0].equipment[0], 3);
        assert!(party.unequip(&data, 0, 0).is_err());
        assert_eq!(party.items, before);
        assert_eq!(party.members[0].equipment[0], 3);
    }

    #[test]
    fn malformed_equipped_item_is_a_transaction_error_without_partial_swap() {
        let mut data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.change_item(&data, 1, 1).unwrap();
        party.members[0].equipment[0] = u16::MAX;
        let before = serde_json::to_value(&party).unwrap();

        let error = party.equip_slot(&data, 0, 0, 1).unwrap_err();

        assert_eq!(error, "unknown equipped item");
        assert_eq!(serde_json::to_value(&party).unwrap(), before);

        party.members[0].equipment[0] = 0;
        data.rules = Some(std::sync::Arc::new(technique_menu()));
        party.bind_rules(&data);
        party.validate(&data).unwrap();
        let before = serde_json::to_value(&party).unwrap();
        assert_eq!(
            party.equip_slot(&data, 0, 0, 1).unwrap_err(),
            "equipment properties were not prepared for item 1"
        );
        assert_eq!(serde_json::to_value(&party).unwrap(), before);
        data.characters[0].equipment[0] = 1;
        assert!(Party::new(&data, Default::default()).is_err());
    }

    #[test]
    fn saved_equipment_and_edits_share_slot_and_character_eligibility() {
        let mut data = data();
        data.items[1].allowed_characters = 1;
        data.items[2].equipment_kind = Some(4);
        data.items[3].equipment_kind = None;
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.validate(&data).unwrap(); // Empty equipment is valid.
        for id in 1..=3 {
            party.change_item(&data, id, 2).unwrap();
        }
        for (member, slot, id) in [(0, 3, 1), (1, 0, 1), (0, 0, 3)] {
            let before = serde_json::to_value(&party).unwrap();
            assert!(!party.equip_slot(&data, member, slot, id).unwrap());
            assert_eq!(serde_json::to_value(&party).unwrap(), before);

            let mut saved = party.clone();
            saved.members[member].equipment[slot] = id;
            assert!(saved.validate(&data).is_err());
            data.characters[member].equipment[slot] = id;
            assert!(Party::new(&data, Default::default()).is_err());
            data.characters[member].equipment[slot] = 0;
        }
        assert!(party.equip_slot(&data, 0, 0, 1).unwrap());
        for slot in [3, 4] {
            assert!(party.equip_slot(&data, 0, slot, 2).unwrap());
        }
        party.validate(&data).unwrap();
        assert_eq!(party.members[0].equipment, [1, 0, 0, 2, 2, 0]);
        party.unequip(&data, 0, 0).unwrap();
        party.validate(&data).unwrap();
        assert_eq!(party.items[&1], 2);
        data.rules = Some(std::sync::Arc::new(technique_menu()));
        assert_eq!(
            party.validate(&data).unwrap_err().to_string(),
            "invalid saved equipment"
        );
    }

    #[test]
    fn field_growth_and_healing_use_saved_gameplay_randomness() {
        use std::sync::Arc;
        use symphonia_script::NativeCall;
        let data = Arc::new(data());
        let mut words = vec![4, 0, 0, 0];
        for (call, args) in [
            (NativeCall::RaisePartyMemberLevel, &[1, 3][..]),
            (NativeCall::HealParty, &[0][..]),
        ] {
            for &arg in args {
                words.extend([0x0200, arg, 0, 0x3000, 0x4000]);
            }
            words.push(0x2000 | u16::from(call as u8));
        }
        words.push(0x20ff);
        let program = Arc::new(
            symphonia_script::Program::decode(
                &words
                    .into_iter()
                    .flat_map(u16::to_be_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        );
        let resources = Arc::new(crate::ResourceLibrary {
            session_data: Some(data.clone()),
            ..Default::default()
        });
        let gameplay = crate::GameplayRandom::new(41);
        let run = |cosmetic| {
            let mut party = Party::new(&data, Default::default()).unwrap();
            party.members[0].hp = 1;
            party.members[0].ailments.poison = Poison::Both;
            let events = crate::EventRuntime::with_state(
                program.clone(),
                resources.clone(),
                crate::GameWorld {
                    party: Some(party),
                    random_state: cosmetic,
                    gameplay_random: gameplay,
                    ..Default::default()
                },
                Default::default(),
            )
            .unwrap();
            assert_eq!(events.world.random_state, cosmetic);
            assert_ne!(events.world.gameplay_random, gameplay);
            let party = events.world.party.unwrap();
            assert_eq!(party.members[0].level, 3);
            assert!(
                party
                    .members
                    .iter()
                    .all(|member| [member.hp, member.tp] == member.maximum_vitals()
                        && member.ailments.is_empty())
            );
            (
                party
                    .members
                    .iter()
                    .map(|member| (member.base_stats, member.luck))
                    .collect::<Vec<_>>(),
                events.world.gameplay_random,
            )
        };
        assert_eq!(run(0), run(u32::MAX));
    }

    #[test]
    fn level_recovery_and_currency_update_persistent_state() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        let mut draws = 0;
        party
            .raise_level(&data, 0, 3, None, || {
                draws += 1;
                3
            })
            .unwrap();
        assert_eq!(draws, 14);
        assert_eq!(party.members[0].experience, 30);
        assert_eq!(party.members[0].hp, 104);
        assert_eq!(party.members[0].tp, 24);
        assert_eq!(party.members[0].shortcuts, [10, 0, 0, 0]);
        party
            .raise_level(&data, 0, 2, None, || {
                panic!("lower level must not draw random numbers")
            })
            .unwrap();
        assert_eq!(party.members[0].level, 3);
        party.members[0].ailments.poison = Poison::Both;
        party.members[0].queued_buffs.insert(StatBuff::AttackUp);
        party.members[0].hp = 1;
        party.heal(|| 207);
        assert_eq!(party.members[0].hp, 104);
        assert!(party.members[0].ailments.is_empty());
        assert!(party.members[0].queued_buffs.is_empty());
        assert_eq!(party.members[0].luck, 7);
        assert_eq!(party.members[0].overlimit, 40);
        assert_eq!(party.add_gald(500), 500);
        assert_eq!(party.add_gald(-600), 0);
        assert_eq!(party.spent_gald, 500);
        assert_eq!(party.add_gald(i32::MAX), 99_999_999);
    }
    #[test]
    fn scripted_minimal_recovery_revives_only_incapacitated_members() {
        let mut party = Party::new(&data(), Default::default()).unwrap();
        for (member, hp, paralysis, petrified, curse) in [
            (0, 0, true, false, false),
            (1, 80, false, true, true),
            (2, 60, true, false, false),
        ] {
            party.members[member].ailments = Ailments {
                paralysis,
                petrified,
                curse,
                ..Default::default()
            };
            party.members[member].hp = hp;
            party.members[member].tp = 7;
            party.members[member].luck = 33;
            party.members[member].overlimit = 50;
        }
        party.revive_incapacitated();
        assert_eq!(
            party.members[..3]
                .iter()
                .map(|m| (
                    m.hp,
                    m.ailments.paralysis,
                    m.ailments.petrified,
                    m.ailments.curse
                ))
                .collect::<Vec<_>>(),
            [
                (1, true, false, false),
                (1, false, false, true),
                (60, true, false, false)
            ]
        );
        for member in &party.members[..3] {
            assert_eq!((member.tp, member.luck, member.overlimit), (7, 33, 50));
        }
    }

    #[test]
    fn battle_growth_preserves_experience_vitals_and_technique_owner_order() {
        let mut data = data();
        data.characters[0].allowed_techniques = vec![12, 10, 11, 13, 14, 99];
        data.characters[0].level_techniques =
            [(2, vec![10, 11, 12, 99]), (3, vec![13]), (4, vec![14])].into();
        data.characters[0].growth[0].random = 0;
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.members[0].techniques.insert(99);
        party.members[0].hp = 17;
        party.members[0].tp = 3;
        party.members[0].ailments.poison = Poison::Mild;
        let mut draws = 0;
        let learned = party
            .gain_experience(
                &data,
                0,
                37,
                [1; 7],
                |id| {
                    assert!(
                        !matches!(id, 14 | 99),
                        "policy queried for an ineligible technique"
                    );
                    Ok(id != 11)
                },
                || {
                    draws += 1;
                    3
                },
            )
            .unwrap();
        assert_eq!(draws, 14); // Even a zero-random growth range consumes a draw.
        assert_eq!(party.members[0].level, 3);
        assert_eq!(party.members[0].experience, 37);
        assert_eq!((party.members[0].hp, party.members[0].tp), (17, 3));
        assert_eq!(party.members[0].ailments.poison, Poison::Mild);
        assert_eq!(party.members[0].base_stats[0], 104);
        assert_eq!(learned.techniques, [12, 10, 13]);
        assert_eq!(learned.notices, [10, 13]);
        assert_eq!(party.members[0].shortcuts, [12, 10, 13, 0]);
        party
            .gain_experience(&data, 0, 0, [1; 7], |_| Ok(true), || panic!("no new level"))
            .unwrap();
        assert_eq!(party.members[0].experience, 37);
        party
            .gain_experience(&data, 0, u32::MAX, [1; 7], |_| Ok(true), || 0)
            .unwrap();
        assert_eq!(party.members[0].experience, 9_999_999);
        assert_eq!((party.members[0].hp, party.members[0].tp), (17, 3));
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    #[serde(flatten)]
    pub preferences: resonance_content::menu_data::CustomizeSettings,
    /// Manual 0, semi-auto 1, auto 2; one entry per battle controller.
    pub battle_controls: [u8; 4],
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            preferences: Default::default(),
            battle_controls: [1, 2, 2, 2],
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Party {
    /// Saved Unison gauge; battle units are this value shifted left four.
    #[serde(default)]
    pub unison_gauge: u8,
    pub battles: BattleStatistics,
    #[serde(default)]
    pub new_game_plus: new_game_plus::State,
    /// Completed playthroughs, queried by original field scripts.
    #[serde(default)]
    pub game_clears: u8,
    #[serde(default)]
    pub battle_rules: crate::battle::Rules,
    #[serde(default)]
    pub figurines: BTreeSet<u16>,
    #[serde(default)]
    pub monsters: BTreeMap<u8, MonsterKnowledge>,
    #[serde(default)]
    pub travel: Travel,
    #[serde(default)]
    pub cooking: Cooking,
    /// Unedited saves use the cooked defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_presets: Option<[resonance_content::menu_data::StrategyPreset; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encounter_modifier: Option<EncounterModifier>,
    #[serde(default)]
    pub viewed_skits: BTreeSet<u16>,
    pub members: Vec<Member>,
    pub formation: Vec<u8>,
    /// One-based character ID, independent of the battle formation.
    #[serde(default = "initial_leader")]
    pub field_leader: u8,
    #[serde(default)]
    pub leader_locked: bool,
    pub items: BTreeMap<u16, u8>,
    pub found_items: BTreeSet<u16>,
    pub recent_items: Vec<u16>,
    pub gald: u32,
    /// Grade is stored in hundredths; script purchases use whole Grade amounts.
    #[serde(default)]
    pub grade_hundredths: u32,
    #[serde(default)]
    pub collectors_book_complete: bool,
    #[serde(default)]
    pub monster_book_complete: bool,
    #[serde(default)]
    pub figurine_book_complete: bool,
    pub spent_gald: u32,
    pub settings: Settings,
}
impl Party {
    /// Only the startup-full flag is prepared. Other stored bits
    /// affect damage/rewards and must not silently acquire partial behavior.
    fn validate_battle_flags(flags: u16) -> anyhow::Result<()> {
        anyhow::ensure!(
            flags & !0x0008 == 0,
            "unsupported saved battle flags {flags:#06x}"
        );
        Ok(())
    }

    /// Replace stored battle flags and return their previous value.
    pub fn exchange_battle_flags(&mut self, value: i32) -> anyhow::Result<u16> {
        let flags = u16::try_from(value)
            .map_err(|_| anyhow::anyhow!("unsupported saved battle flags {value}"))?;
        Ok(std::mem::replace(&mut self.battle_rules.modifiers, flags))
    }

    /// The leader flag is independent of story availability.
    pub fn initial_unison_full(&self) -> anyhow::Result<bool> {
        Self::validate_battle_flags(self.battle_rules.modifiers)?;
        Ok(self.battle_rules.modifiers & 0x0008 != 0)
    }

    /// Keep the selected leader unless absent, knocked out or petrified.
    /// The manual selection lock does not prevent this automatic fallback.
    pub fn restore_field_leader(&mut self) -> u8 {
        let available = |id: u8| self.members[usize::from(id - 1)].can_lead_field();
        if (!self.formation.contains(&self.field_leader) || !available(self.field_leader))
            && let Some(id) = self.formation.iter().copied().find(|&id| available(id))
        {
            self.field_leader = id;
        }
        self.field_leader
    }

    pub fn validate(&self, data: &SessionData) -> anyhow::Result<()> {
        use anyhow::ensure;
        ensure!(
            self.members.iter().all(|m| m
                .name
                .as_ref()
                .is_none_or(|name| (1..=12).contains(&name.chars().count())
                    && !name.chars().any(char::is_control))),
            "invalid saved character name"
        );
        self.settings.preferences.validate()?;
        self.battles.validate()?;
        self.travel.validate()?;
        const MAX_GAME_CLEARS: u8 = 100;
        ensure!(
            self.game_clears <= MAX_GAME_CLEARS,
            "invalid saved game clear count"
        );
        ensure!(
            self.monsters.iter().all(|(&id, knowledge)| usize::from(id)
                < resonance_content::monster::MONSTER_COUNT
                && knowledge.variant < 16),
            "invalid saved monster knowledge"
        );
        ensure!(
            usize::from(self.cooking.recipe) < resonance_content::menu_data::RECIPE_COUNT
                && usize::from(self.cooking.chef) < self.members.len()
                && self
                    .members
                    .iter()
                    .all(|m| m.cooking.iter().all(|&v| v <= 8)),
            "invalid saved cooking state"
        );
        ensure!(
            self.strategy_presets
                .as_ref()
                .is_none_or(|presets| presets.iter().all(|p| p.validate().is_ok())),
            "invalid saved strategy presets"
        );
        ensure!(
            self.members.len() == data.characters.len()
                && self.members.iter().all(|m| m.costume < 5)
                && (1..=8).contains(&self.formation.len())
                && self.formation.contains(&self.field_leader)
                && self
                    .formation
                    .iter()
                    .all(|id| *id > 0 && usize::from(*id) <= self.members.len())
                && self.formation.iter().collect::<BTreeSet<_>>().len() == self.formation.len()
                && self.settings.battle_controls.iter().all(|v| *v <= 2)
                && self.gald <= 99_999_999
                && self.grade_hundredths <= 99_999_999
                && self.unison_gauge <= 200
                && self.viewed_skits.iter().all(|&id| (1..=860).contains(&id)),
            "invalid saved party"
        );
        ensure!(
            self.figurines
                .iter()
                .all(|&id| usize::from(id) < resonance_content::figurine::FIGURINE_COUNT),
            "invalid saved figurine"
        );
        ensure!(
            self.encounter_modifier
                .as_ref()
                .is_none_or(|m| (1..=2).contains(&m.rate)
                    && (1..=EncounterModifier::DURATION).contains(&m.remaining)),
            "invalid saved encounter modifier"
        );
        ensure!(
            self.items.iter().all(|(id, count)| data
                .items
                .get(usize::from(*id))
                .is_some_and(|item| *count > 0 && *count <= self.item_limit(item)))
                && self
                    .found_items
                    .iter()
                    .chain(&self.recent_items)
                    .all(|id| usize::from(*id) < data.items.len())
                && self.recent_items.len() <= 32,
            "invalid saved inventory"
        );
        for (index, (member, definition)) in self.members.iter().zip(&data.characters).enumerate() {
            ensure!(
                member.overlimit <= 100,
                "saved Over Limit percentage exceeds 100"
            );
            member.validate_ex(data.rules.as_ref().map(|rules| &rules.ex_skills), index)?;
            ensure!(
                member
                    .equipment
                    .iter()
                    .enumerate()
                    .all(|(slot, &id)| id == 0
                        || data
                            .items
                            .get(usize::from(id))
                            .is_some_and(|item| item.fits_slot(index, slot))
                            && data
                                .rules
                                .as_ref()
                                .is_none_or(|rules| rules.items.get(usize::from(id)).is_some())),
                "invalid saved equipment"
            );
            let title_count = data
                .rules
                .as_ref()
                .map_or(usize::from(initial_title()), |rules| {
                    rules.titles.get(index).map_or(0, Vec::len)
                });
            let [max_hp, max_tp] = member.vitals_with_rules(data.rules.as_deref(), index);
            ensure!(
                member.level > 0
                    && member
                        .strategy
                        .iter()
                        .zip(resonance_content::menu_data::STRATEGY_COUNTS)
                        .all(|(v, count)| usize::from(*v) < count)
                    && member.titles.contains(&member.title)
                    && member
                        .titles
                        .iter()
                        .all(|&id| id != 0 && usize::from(id) <= title_count)
                    && (-100..=100).contains(&member.technique_balance)
                    && usize::from(member.level) < data.experience.len()
                    && member.hp <= max_hp
                    && member.tp <= max_tp
                    && member
                        .base_stats
                        .iter()
                        .zip([9999, 999, 32767, 32767, 32767, 32767, 32767])
                        .all(|(stat, limit)| *stat <= limit)
                    && member
                        .techniques
                        .iter()
                        .all(|id| definition.allowed_techniques.contains(id))
                    && member
                        .shortcuts
                        .iter()
                        .all(|id| *id == 0 || member.techniques.contains(id))
                    && member.disabled_techniques.is_subset(&member.techniques)
                    && member
                        .technique_uses
                        .iter()
                        .all(|(id, count)| definition.allowed_techniques.contains(id)
                            && *count <= resonance_content::arte::MAX_USES)
                    && member.assist_shortcuts.iter().flatten().all(|shortcut| self
                        .members
                        .get(shortcut.character)
                        .is_some_and(|m| m.techniques.contains(&shortcut.technique))),
                "invalid saved party member"
            );
        }
        Ok(())
    }

    pub fn new(data: &SessionData, settings: Settings) -> anyhow::Result<Self> {
        data.validate()?;
        Ok(Self {
            new_game_plus: Default::default(),
            game_clears: 0,
            unison_gauge: 0,
            battles: BattleStatistics::default(),
            cooking: Cooking::default(),
            encounter_modifier: None,
            members: data
                .characters
                .iter()
                .enumerate()
                .map(|(index, character)| Member {
                    name: None,
                    costume: 0,
                    rules: data.rules.clone().map(|data| ex_skills::Rules {
                        data,
                        character: index,
                    }),
                    title: initial_title(),
                    titles: initial_titles(),
                    technique_balance: character.technique_balance,
                    affinity: character.affinity,
                    level: character.level,
                    experience: character.experience,
                    base_stats: character.base_stats,
                    hp: character.base_stats[0],
                    tp: character.base_stats[1],
                    ailments: Ailments::default(),
                    queued_buffs: BTreeSet::new(),
                    luck: character.luck,
                    overlimit: character.overlimit,
                    equipment: character.equipment,
                    techniques: character.techniques.iter().copied().collect(),
                    shortcuts: character.shortcuts,
                    disabled_techniques: BTreeSet::new(),
                    technique_uses: BTreeMap::new(),
                    assist_shortcuts: [None; 2],
                    strategy: [0; 3],
                    cooking: character.cooking,
                    ex_skills: character.ex_skills,
                    ex_gems: character.ex_gems,
                    compound_ex_skills: character.compound_ex_skills.iter().copied().collect(),
                    recent_compound_ex_skills: character
                        .recent_compound_ex_skills
                        .iter()
                        .copied()
                        .collect(),
                })
                .collect(),
            formation: vec![1],
            battle_rules: Default::default(),
            monsters: BTreeMap::new(),
            figurines: BTreeSet::new(),
            travel: Travel::default(),
            field_leader: 1,
            leader_locked: false,
            strategy_presets: None,
            viewed_skits: BTreeSet::new(),
            items: BTreeMap::new(),
            found_items: BTreeSet::new(),
            recent_items: Vec::new(),
            gald: 0,
            grade_hundredths: 0,
            collectors_book_complete: false,
            monster_book_complete: false,
            figurine_book_complete: false,
            spent_gald: 0,
            settings,
        })
    }
    pub fn change_item(&mut self, data: &SessionData, id: u16, delta: i8) -> Result<bool, String> {
        let item = data.items.get(usize::from(id)).ok_or("unknown item")?;
        let limit = self.item_limit(item);
        let previous = self.items.get(&id).copied().unwrap_or(0);
        if delta > 0 && previous == limit || delta <= 0 && previous == 0 {
            return Ok(false);
        }
        let count = (i16::from(previous) + i16::from(delta)).clamp(0, i16::from(limit)) as u8;
        if count == 0 {
            self.items.remove(&id);
        } else {
            self.items.insert(id, count);
        }
        if delta > 0 {
            self.found_items.insert(id);
            self.recent_items.retain(|old| *old != id);
            self.recent_items.insert(0, id);
            self.recent_items.truncate(32);
        }
        Ok(true)
    }
    pub fn unequip(
        &mut self,
        data: &SessionData,
        member: usize,
        slot: usize,
    ) -> Result<(), String> {
        self.equip_slot(data, member, slot, 0).map(|_| ())
    }
    pub fn equip(&mut self, data: &SessionData, member: usize, id: u16) -> Result<(), String> {
        let item = data.items.get(usize::from(id)).ok_or("unknown item")?;
        let character = self.members.get(member).ok_or("unknown party member")?;
        if self.items.get(&id).copied().unwrap_or(0) == 0
            || item.allowed_characters & (1 << member) == 0
        {
            return Ok(());
        }
        let Some(slot) = item
            .equipment_kind
            .and_then(|kind| character.preferred_equipment_slot(kind))
        else {
            return Ok(());
        };
        self.equip_slot(data, member, slot, id).map(|_| ())
    }
    pub fn add_gald(&mut self, amount: i32) -> u32 {
        let previous = self.gald;
        self.gald = (i64::from(previous) + i64::from(amount)).clamp(0, 99_999_999) as u32;
        self.spent_gald = self
            .spent_gald
            .saturating_add(previous.saturating_sub(self.gald));
        self.gald
    }
    pub fn heal(&mut self, mut random: impl FnMut() -> u32) {
        for member in &mut self.members {
            [member.hp, member.tp] = member.maximum_vitals();
            member.ailments = Default::default();
            member.queued_buffs.clear();
            member.luck = (random() % 100) as u8;
            member.overlimit = member.overlimit.saturating_sub(10);
        }
    }
    /// Minimal post-battle recovery used by original field scenes.
    pub fn revive_incapacitated(&mut self) {
        for member in &mut self.members {
            if member.hp == 0 || member.ailments.petrified {
                member.hp = 1;
                member.ailments.petrified = false;
            }
        }
        self.restore_field_leader();
    }

    /// Field hazards spare one HP and never revive knocked-out members.
    pub fn damage_hp_percent(&mut self, percent: u16) {
        self.adjust_vitals_percent([-(percent.min(100) as i16), 0]);
    }

    /// Signed percentages of maximum HP and TP; conditions are unchanged.
    pub fn adjust_vitals_percent(&mut self, percent: [i16; 2]) {
        for member in &mut self.members {
            let maximum = member.maximum_vitals();
            for (index, current) in [&mut member.hp, &mut member.tp].into_iter().enumerate() {
                if index == 0 && *current == 0 && percent[0] < 0 {
                    continue;
                }
                let delta = i32::from(maximum[index]) * i32::from(percent[index]) / 100;
                let minimum = i32::from(index == 0 && percent[0] < 0);
                *current =
                    (i32::from(*current) + delta).clamp(minimum, i32::from(maximum[index])) as u16;
            }
        }
    }
    pub fn raise_level(
        &mut self,
        data: &SessionData,
        index: usize,
        level: u8,
        title_growth: Option<[u8; 7]>,
        mut random: impl FnMut() -> u32,
    ) -> Result<(), String> {
        if level == 0 || usize::from(level) >= data.experience.len() {
            return Err("invalid target level".into());
        }
        let definition = data.characters.get(index).ok_or("unknown party member")?;
        let member = self.members.get_mut(index).ok_or("unknown party member")?;
        while member.level < level {
            member.grow_level(definition, title_growth, &mut random);
            member.experience = data.experience[usize::from(member.level)];
        }
        member.hp = member.base_stats[0];
        member.tp = member.base_stats[1];
        member.acquire_level_techniques(definition, |_| Ok(true))?;
        Ok(())
    }

    /// Battle growth retains accumulated EXP and current HP/TP. The caller
    /// supplies the active/reserve technique gate from the prepared catalogue.
    pub fn gain_experience(
        &mut self,
        data: &SessionData,
        index: usize,
        amount: u32,
        title_growth: [u8; 7],
        can_learn: impl Fn(u16) -> Result<bool, String>,
        mut random: impl FnMut() -> u32,
    ) -> Result<ExperienceGain, String> {
        let definition = data.characters.get(index).ok_or("unknown party member")?;
        let member = self.members.get_mut(index).ok_or("unknown party member")?;
        member.experience = member.experience.saturating_add(amount).min(9_999_999);
        let mut learned = ExperienceGain::default();
        while member.level < 250
            && data
                .experience
                .get(usize::from(member.level) + 1)
                .is_some_and(|&threshold| member.experience >= threshold)
        {
            member.grow_level(definition, Some(title_growth), &mut random);
            let techniques = member.acquire_level_techniques(definition, &can_learn)?;
            if let Some(&technique) = techniques.last() {
                learned.notices.push(technique);
            }
            learned.techniques.extend(techniques);
            member.clamp_vitals();
        }
        Ok(learned)
    }
}
