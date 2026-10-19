//! Small menu state fixture; authored catalogue checks load cooked data separately.
use resonance_content::{
    menu_data::{
        CookingData, ExSkillData, Item, ItemCaptions, ItemText, MenuData, MenuPresentation,
        MenuText, NamedText, STRATEGY_COUNTS, StatusData, StrategyData, StrategyOption,
        StrategyText, Technique, WorldMapData,
    },
    session::{CharacterDefinition, ItemDefinition, SessionData, StatGrowth},
};
use resonance_events::party::Party;
use std::sync::Arc;

pub const SPARE_WEAPON: u16 = 2;

pub fn fixture() -> (Arc<SessionData>, MenuData, Party) {
    let catalogue = vec![2, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    let session = Arc::new(SessionData {
        rules: None,
        version: 1,
        executable_sha256: "0".repeat(64),
        experience: vec![0, 0, 100],
        items: (0..=SPARE_WEAPON)
            .map(|id| ItemDefinition {
                equipment_kind: (id != 0).then_some(0),
                allowed_characters: (1 << 9) - 1,
                stack_limit: 20,
            })
            .collect(),
        characters: vec![
            CharacterDefinition {
                cooking: [0; resonance_content::menu_data::RECIPE_COUNT],
                ex_skills: [0; 4],
                ex_gems: [0; 4],
                compound_ex_skills: vec![],
                recent_compound_ex_skills: vec![],
                technique_balance: 0,
                affinity: 0,
                level: 1,
                experience: 0,
                base_stats: [100, 20, 10, 10, 10, 10, 10],
                luck: 0,
                overlimit: 0,
                equipment: [1, 0, 0, 0, 0, 0],
                techniques: catalogue.clone(),
                allowed_techniques: catalogue,
                shortcuts: [0; 4],
                growth: std::array::from_fn(|_| StatGrowth {
                    base: 0,
                    random: 0,
                    title_bonus: 0,
                }),
                level_techniques: Default::default(),
            };
            9
        ],
    });
    let data = MenuData {
        crafting: Default::default(),
        grade_shop: resonance_content::grade::Shop {
            options: vec![],
            labels: Default::default(),
        },
        version: MenuData::VERSION,
        items: (0..=SPARE_WEAPON)
            .map(|_| Item {
                category: 13,
                field_usable: false,
                battle_usable: false,
                view: None,
                equipment_stats: [0; 7],
                properties: Default::default(),
                price: 0,
                transforms_to: 0,
                field_use: None,
                attention: None,
            })
            .collect(),
        titles: vec![],
        initial_names: std::array::from_fn(|member| format!("Member {member}")),
        techniques: (0..=12)
            .map(|_| Technique {
                tp: 0,
                tp_percent: false,
                unison_usable: true,
                rank: 0,
                element: 0,
                level: 1,
                route: 0,
                prerequisite: 0,
                alternatives: [0; 4],
                field_use: None,
            })
            .collect(),
        strategy: StrategyData {
            groups: std::array::from_fn(|group| {
                (0..STRATEGY_COUNTS[group])
                    .map(|_| StrategyOption {
                        characters: (1 << 9) - 1,
                    })
                    .collect()
            }),
            presets: [[[0; 3]; 9]; 3],
            default_positions: [0; 9],
            positions: [0; 6],
        },
        cooking: CookingData {
            recipes: vec![],
            groups: vec![],
            preferences: vec![],
            bonus_skill: 0,
        },
        status: StatusData {
            equipment_effects: Default::default(),
        },
        world_map: WorldMapData {
            locations: Default::default(),
            field_locations: Default::default(),
            shops: vec![],
        },
        ex_skills: ExSkillData {
            skills: Default::default(),
            characters: vec![],
            gem_items: [0; 5],
        },
        presentation: MenuPresentation {
            labels: Default::default(),
            items: Some(ItemCaptions {
                items: (0..=SPARE_WEAPON)
                    .map(|id| {
                        Some(ItemText {
                            name: format!("Weapon {id}"),
                            description: String::new(),
                            details: String::new(),
                        })
                    })
                    .collect(),
                item_categories: vec![String::new(); 48],
                inventory_categories: vec![String::new(); 9],
                item_group_prompt: Some(MenuText { lines: vec![] }),
                item_bottle_count: Some(MenuText { lines: vec![] }),
            }),
            titles: Some(vec![]),
            techniques: Some(
                (0..=12)
                    .map(|id| {
                        Some(NamedText {
                            name: format!("Technique {id}"),
                            description: format!("Description {id}"),
                        })
                    })
                    .collect(),
            ),
            names: Some(vec!["{name}".into(); 9]),
            strategy: Some(StrategyText {
                groups: std::array::from_fn(|group| {
                    (0..STRATEGY_COUNTS[group])
                        .map(|option| {
                            Some(ItemText {
                                name: format!("Option {option}"),
                                description: String::new(),
                                details: String::new(),
                            })
                        })
                        .collect()
                }),
                presets: Some(["First", "Second", "Third"].map(String::from)),
                keyboard: Some("A".repeat(90)),
                keys: Some(Default::default()),
                labels: Some(Default::default()),
            }),
            ..Default::default()
        },
    };
    let mut party = Party::new(&session, Default::default()).unwrap();
    party.formation = (1..=8).collect();
    party.members[0].technique_uses.insert(3, 7);
    (session, data, party)
}
