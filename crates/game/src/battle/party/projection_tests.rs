//! Small in-memory inputs for gear and trait projection; no encounter assets are needed.
use super::*;
use resonance_battle::conditions::ConditionSet;
use resonance_battle::{Element, EquipmentReplacement, PreparedBattle, conditions};
use resonance_content::menu_data::{
    CharacterExSkills, CompoundExSkill, CookingData, EquipmentAilment, EquipmentEffect,
    ExActivation, ExSkill, ExSkillData, Item, StatusData, StrategyData, Technique, TpDiscount,
    WorldMapData,
};

pub(in crate::battle) fn fixture() -> Result<(MenuData, Member)> {
    let (_, _, titles, mut party) = crate::battle::rewards::tests::reward_fixture();
    let mut menus = MenuData {
        version: MenuData::VERSION,
        crafting: Default::default(),
        grade_shop: resonance_content::grade::Shop {
            options: vec![],
            labels: Default::default(),
        },
        items: vec![
            Item {
                category: 0,
                field_usable: false,
                battle_usable: false,
                view: None,
                equipment_stats: [0; 7],
                properties: Default::default(),
                price: 0,
                transforms_to: 0,
                field_use: None,
                attention: None,
            };
            7
        ],
        titles,
        initial_names: Default::default(),
        techniques: vec![],
        strategy: StrategyData {
            groups: Default::default(),
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
            equipment_effects: [48, 49, 50, 52]
                .map(|id| (id, EquipmentEffect { suppresses: vec![] }))
                .into(),
        },
        world_map: WorldMapData {
            locations: Default::default(),
            field_locations: Default::default(),
            shops: vec![],
        },
        ex_skills: ExSkillData {
            skills: Default::default(),
            characters: vec![
                CharacterExSkills {
                    levels: [[0; 4]; 4],
                    compounds: vec![]
                };
                9
            ],
            gem_items: [0; 5],
        },
        presentation: Default::default(),
    };
    for id in [1, 2, 6, 69, 75] {
        menus.ex_skills.skills.insert(
            id,
            ExSkill {
                stat_bonuses: vec![],
                save_point_tp_cost: None,
                tendency: None,
                activation: ExActivation::Constant,
            },
        );
    }
    let mut member = party.members.remove(0);
    member.experience = 0;
    member.base_stats = [100, 40, 1000, 500, 300, 200, 400];
    member.hp = 37;
    member.tp = 9;
    member.luck = 1;
    Ok((menus, member))
}

#[test]
fn gear_bonuses_saturate_and_weapon_species_uses_only_the_primary_weapon() -> Result<()> {
    let (mut menus, mut member) = fixture()?;
    member.equipment = [1, 2, 3, 4, 5, 6];
    for item in &mut menus.items[1..=6] {
        item.properties.critical_chance_bonus = 50;
    }
    menus.items[1].properties.species_bonus = Some(6);
    menus.items[2].properties.species_bonus = Some(1);
    menus.items[1].properties.neutral_resistance = 64;
    menus.items[2].properties.neutral_resistance = 64;
    menus.items[1]
        .properties
        .resistance
        .insert(Element::Fire, 64);
    menus.items[2]
        .properties
        .resistance
        .insert(Element::Fire, 64);
    let attributes = loadout(&menus, &member, 0)?.attributes;
    assert_eq!(attributes.damage.weapon_species, Some(6));
    assert_eq!(attributes.damage.critical_chance_bonus, 100);
    assert_eq!(attributes.affinities[0], Affinity::Absorb);
    assert_eq!(
        attributes.affinities[1 + Element::Fire as usize],
        Affinity::Absorb
    );
    menus.items[1].properties.species_bonus = None;
    assert_eq!(
        loadout(&menus, &member, 0)?
            .attributes
            .damage
            .weapon_species,
        None
    );
    menus.items[1].properties.unsupported_modifier = true;
    assert!(loadout(&menus, &member, 0).is_err());
    Ok(())
}

#[test]
fn damage_rings_rebuild_without_stacking_and_keep_live_vitals() -> Result<()> {
    let (mut menus, mut member) = fixture()?;
    menus.items[1].properties.physical_damage_boost = true;
    menus.items[2].properties.physical_damage_reduction = true;
    menus.items[3].properties.magic_damage_boost = true;
    for (items, expected) in [
        ([1, 1], [true, false, false]),
        ([2, 2], [false, true, false]),
        ([3, 3], [false, false, true]),
        ([1, 3], [true, false, true]),
    ] {
        member.equipment[4..].copy_from_slice(&items);
        let damage = loadout(&menus, &member, 0)?.attributes.damage;
        assert_eq!(
            [
                damage.physical_damage_boost,
                damage.physical_damage_reduction,
                damage.magic_damage_boost
            ],
            expected
        );
    }
    let current = actor(
        &loadout(&menus, &member, 0)?,
        &member,
        Control::Manual,
        [0.; 3],
        0.,
    )?;
    let prepared = PreparedBattle::new(vec![(current, Default::default())], Default::default(), 1)?;
    let id = prepared.actor_ids().next().unwrap();
    let mut battle = prepared.finish()?;
    member.equipment[4..].copy_from_slice(&[2, 0]);
    let (attributes, conditions) = loadout(&menus, &member, 0)?.equipment_attributes(
        &member,
        &battle.actors()[0].conditions,
        false,
    )?;
    battle.replace_equipment_batch(vec![EquipmentReplacement {
        actor: id,
        attributes,
        conditions,
        equipment: None,
    }])?;
    let live = &battle.actors()[0];
    assert!(live.equipment.damage.physical_damage_reduction);
    assert!(
        !live.equipment.damage.physical_damage_boost && !live.equipment.damage.magic_damage_boost
    );
    assert_eq!((live.hp, live.tp), (37, 9));
    Ok(())
}

#[test]
fn equipped_compounds_supply_critical_and_cost_traits_without_learned_history() -> Result<()> {
    let (mut menus, mut member) = fixture()?;
    menus.ex_skills.characters[0].compounds = vec![
        CompoundExSkill {
            skill: 69,
            required: vec![1, 2],
        },
        CompoundExSkill {
            skill: 75,
            required: vec![1, 2],
        },
    ];
    member.equipment[0] = 1;
    menus.items[1].properties.critical_chance_bonus = 92;
    member.ex_skills = [1, 2, 0, 0];
    let equipped = loadout(&menus, &member, 0)?;
    assert_eq!(equipped.attributes.damage.critical_chance_bonus, 97);
    assert!(equipped.attributes.tp_cost_reduction);
    menus.items[1].properties.critical_chance_bonus = 100;
    assert_eq!(
        loadout(&menus, &member, 0)?
            .attributes
            .damage
            .critical_chance_bonus,
        100
    );
    member.compound_ex_skills.extend([0, 1]);
    menus.items[1].properties.critical_chance_bonus = 92;
    member.ex_skills[1] = 0;
    let removed = loadout(&menus, &member, 0)?;
    assert!(!removed.attributes.tp_cost_reduction);
    assert_eq!(removed.attributes.damage.critical_chance_bonus, 92);
    Ok(())
}

#[test]
fn movement_ignores_empty_items_and_combines_dash_with_heavy_boots() -> Result<()> {
    let (mut menus, mut member) = fixture()?;
    menus.items[0].properties.movement_bonus = 1;
    menus.items[1].properties.movement_bonus = -1;
    menus.items[1]
        .properties
        .ailments
        .insert(EquipmentAilment::Heavy);
    member.ex_skills = [6, 0, 0, 0];
    let attributes = loadout(&menus, &member, 0)?.attributes;
    assert!((attributes.speed_multiplier - 1.1).abs() < 0.0001);
    member.equipment[3] = 1;
    let (attributes, conditions) =
        loadout(&menus, &member, 0)?.equipment_attributes(&member, &Default::default(), true)?;
    assert!((attributes.speed_multiplier - 1.0).abs() < 0.0001);
    assert_eq!(
        conditions.layers().equipment_overlay,
        conditions::Condition::Heavy.into()
    );
    Ok(())
}

#[test]
fn caption_suppression_does_not_remove_equipment_gameplay() -> Result<()> {
    let (mut menus, mut member) = fixture()?;
    member.equipment[..2].copy_from_slice(&[1, 2]);
    menus.items[1].properties.tp_discount = TpDiscount::Half;
    menus.items[1].properties.caption_ids = vec![49];
    menus.items[1].properties.movement_bonus = 1;
    menus.items[1].properties.experience_percent = 50;
    menus.items[2].properties.tp_discount = TpDiscount::Third;
    menus.items[2].properties.caption_ids = vec![48];
    menus.items[2].properties.experience_percent = 100;
    menus
        .status
        .equipment_effects
        .get_mut(&49)
        .unwrap()
        .suppresses = vec![48];
    let gear = super::super::conditions::GearEffects::equipped(&menus, &member);
    assert_eq!(gear.movement_bonus, 1);
    assert_eq!(gear.experience_percent, 100);
    assert_eq!(member.equipment_captions(&menus).unwrap(), [49]);
    menus.techniques.push(Technique {
        tp: 12,
        tp_percent: false,
        unison_usable: false,
        rank: 0,
        element: 0,
        level: 1,
        route: 0,
        prerequisite: 0,
        alternatives: [0; 4],
        field_use: None,
    });
    let field_cost = member.technique_cost(&menus, 0, false);
    assert_eq!(
        field_cost, 6,
        "discounts belong to properties, regardless of item identity"
    );
    let owner = actor(
        &loadout(&menus, &member, 0)?,
        &member,
        Control::Manual,
        [0.; 3],
        0.,
    )?;
    let action = resonance_battle::ActionDefinition {
        normal: None,
        tp_cost: 12,
        execution: resonance_battle::ActionExecution::Attack(resonance_battle::PreparedAttack {
            opening: None,
            events: vec![],
            chain_at: None,
            end_at: 0,
            recovery: 0,
        }),
    };
    let prepared =
        PreparedBattle::new(vec![(owner, Default::default())], (vec![action]).into(), 1)?;
    let id = prepared.actor_ids().next().unwrap();
    assert_eq!(
        prepared
            .finish()?
            .technique_tp_cost(id, resonance_battle::ActionKey(0)),
        Some(u32::from(field_cost))
    );
    Ok(())
}

#[test]
fn regeneration_stacks_independently_of_caption_suppression() -> Result<()> {
    let (mut menus, mut member) = fixture()?;
    member.equipment = [1, 2, 3, 0, 0, 0];
    menus.items[2].properties.hp_regeneration = 1;
    menus.items[2].properties.caption_ids = vec![50];
    menus.items[3].properties.hp_regeneration = 3;
    menus.items[3].properties.caption_ids = vec![52];
    menus
        .status
        .equipment_effects
        .get_mut(&52)
        .unwrap()
        .suppresses
        .push(50);
    assert_eq!(member.equipment_captions(&menus).unwrap(), [52]);
    let (_, conditions) =
        loadout(&menus, &member, 0)?.equipment_attributes(&member, &Default::default(), true)?;
    assert_eq!(
        conditions.gear_regeneration(),
        conditions::GearRegeneration {
            hp_percent: 4,
            tp_percent: 0
        }
    );
    for (hp, tp, recovery) in [
        (true, false, conditions::Condition::KillHpRecovery),
        (false, true, conditions::Condition::KillTpRecovery),
    ] {
        menus.items[1].properties.kill_hp_recovery = hp;
        menus.items[1].properties.kill_tp_recovery = tp;
        let (_, conditions) = loadout(&menus, &member, 0)?.equipment_attributes(
            &member,
            &Default::default(),
            true,
        )?;
        assert_eq!(
            conditions.layers().intrinsic,
            ConditionSet::of(&[conditions::Condition::RegenerateHp, recovery])
        );
    }
    Ok(())
}

#[test]
fn actor_projection_keeps_saved_overlimit_and_clamps_negative_luck() -> Result<()> {
    let (mut menus, mut member) = fixture()?;
    member.equipment[0] = 1;
    menus.items[1].equipment_stats[6] = -10;
    for value in [0, 99, 100] {
        member.overlimit = value;
        let projected = actor(
            &loadout(&menus, &member, 0)?,
            &member,
            Control::Manual,
            [0.; 3],
            0.,
        )?;
        assert_eq!(projected.overlimit.charge(), u16::from(value) * 10);
        assert_eq!(projected.equipment.luck, 0);
        assert!(!projected.overlimit.is_active());
    }
    for value in [101, 125, 255] {
        member.overlimit = value;
        assert!(loadout(&menus, &member, 0).is_err());
    }
    // Unbound saves must use the explicit character's compound rules for luck too.
    member.overlimit = 0;
    member.luck = 15;
    member.ex_skills = [1, 2, 0, 0];
    menus.ex_skills.characters[1]
        .compounds
        .push(CompoundExSkill {
            skill: 69,
            required: vec![1, 2],
        });
    menus
        .ex_skills
        .skills
        .get_mut(&69)
        .unwrap()
        .stat_bonuses
        .push(resonance_content::menu_data::ExStatBonus {
            stat: resonance_content::menu_data::ExStat::Luck,
            percent: 10,
        });
    assert_eq!(loadout(&menus, &member, 1)?.attributes.luck, 5);
    member.compound_ex_skills.insert(0);
    let saved = serde_json::to_value(&member)?;
    assert!(loadout(&menus, &member, 0).is_err());
    assert_eq!(loadout(&menus, &member, 1)?.attributes.luck, 7);
    assert_eq!(serde_json::to_value(&member)?, saved);
    Ok(())
}

#[test]
fn rescue_equipment_uses_capabilities_and_retains_consumption_slot_order() -> Result<()> {
    use resonance_battle::RescueEquipment;
    use resonance_content::menu_data::EquipmentRescue;
    let (mut menus, mut member) = fixture()?;
    member.equipment = [0, 0, 0, 1, 2, 3];
    menus.items[1].properties.rescue = Some(EquipmentRescue::Consumable);
    menus.items[2].properties.rescue = Some(EquipmentRescue::Chance);
    menus.items[3].properties.rescue = Some(EquipmentRescue::Consumable);
    assert_eq!(
        loadout(&menus, &member, 0)?
            .attributes
            .recovery
            .lethal
            .equipment,
        [
            Some(RescueEquipment::Consumable(5)),
            Some(RescueEquipment::Consumable(3)),
            Some(RescueEquipment::Chance)
        ]
    );
    Ok(())
}
