use super::*;
use resonance_content::{battle_projectile::Table, source::FloatOperand};
use resonance_game::battle::projectile::load;

fn request() -> ProjectileResource {
    Resources {
        paths: vec![],
        fail: false,
    }
    .projectile("test")
    .unwrap()
}

fn change(
    files: &mut Files,
    edit: impl FnOnce(&mut resonance_content::battle_projectile::Projectile),
) {
    let mut table: Table = files.json("test-projectiles.json").unwrap();
    edit(&mut table.records[0]);
    files.bytes.insert(
        "test-projectiles.json".into(),
        serde_json::to_vec(&table).unwrap().into(),
    );
}

#[test]
fn verified_row_supplies_motion_contacts_and_caller_selected_effect_banks() {
    let mut files = files("");
    change(&mut files, |row| {
        row.flags |= 0x2000;
        row.velocity = [1., 2., 3.].map(FloatOperand::Value);
        row.acceleration = [0., -0.5, 0.].map(FloatOperand::Value);
        row.active_start = 4;
        row.active_duration = 2;
        row.repeat_limit = 3;
        // The allocator overrides this source slot; the selected resource is used.
        row.birth_effect.bank = 6;
    });
    let mut request = request();
    request.clash = Some(EffectResource {
        models: Default::default(),
        scene: None,
        source: "common.json".into(),
        resource: 9,
        members: vec![11],
    });
    let definition = load(&files, &request).unwrap();
    assert_eq!(definition.velocity, [1., 2., 3.]);
    assert_eq!(definition.acceleration, [0., -0.5, 0.]);
    assert_eq!(definition.active, Some([4, 6]));
    assert_eq!(definition.birth.unwrap().resource, 37);
    assert!(definition.clamp_ground);
    let contact = definition.contact.unwrap();
    assert_eq!(contact.repeat_limit, 3);
    assert_eq!(contact.cooldown, 3);
    assert_eq!(contact.clash_effect.unwrap().resource, 9);
    assert!(contact.survives_contact);
    // Only the floor-clamp flag controls this property.
    change(&mut files, |row| row.flags &= !0x40);
    assert!(!load(&files, &request).unwrap().clamp_ground);
}

#[test]
fn source_and_required_members_must_be_present_and_supported_before_activation() {
    let files = files("");
    let mut request = request();
    assert!(load(&Files::default(), &request).is_err());
    request.member = 1;
    assert!(load(&files, &request).is_err());
    request.member = 0;
    request.hit.rule = 1;
    assert!(load(&files, &request).is_err());
    request.hit.rule = 0;
    request.birth.as_mut().unwrap().members = vec![27];
    assert!(load(&files, &request).is_err());
    request.birth.as_mut().unwrap().members = vec![28];
    for edit in [
        |row: &mut resonance_content::battle_projectile::Projectile| row.flags |= 0x10000,
        |row: &mut resonance_content::battle_projectile::Projectile| row.trail_effect.member = 7,
        |row: &mut resonance_content::battle_projectile::Projectile| {
            row.velocity_jitter[0] = FloatOperand::Value(1.)
        },
        |row: &mut resonance_content::battle_projectile::Projectile| {
            row.velocity[0] = FloatOperand::Bits { bits: 0x7fc12345 }
        },
        |row: &mut resonance_content::battle_projectile::Projectile| row.active_duration = -1,
        |row: &mut resonance_content::battle_projectile::Projectile| {
            row.active_start = 32767;
            row.active_duration = 1;
        },
    ] {
        let mut changed = files.clone();
        change(&mut changed, edit);
        assert!(load(&changed, &request).is_err());
    }
    // Inactive source operands do not prevent preparation of the selected controller.
    let mut changed = files;
    change(&mut changed, |row| {
        row.steering_blend = FloatOperand::Bits { bits: 0xff800000 }
    });
    assert!(load(&changed, &request).is_ok());
}

#[test]
#[ignore = "requires the complete current cooked library; no devices"]
fn cooked_projectiles_load_from_the_field_snapshot_with_real_effect_dependencies() -> Result<()> {
    use resonance_content::{battle_action, battle_effect, battle_projectile, prepared::Cache};
    let root = common::asset_root();
    let files = Files::load(
        &root,
        &["fields/map-340.preload.json"],
        &mut Cache::default(),
        || false,
    )?;
    let observation: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/common-action-sources.json"))?;
    for observation in observation["observations"].as_array().unwrap() {
        let path = if observation["bank"] == "martial" {
            battle_action::MARTIAL_PATH
        } else {
            battle_action::SPELL_PATH
        };
        let table: battle_action::Table = files.json(path)?;
        let member = observation["member"].as_u64().unwrap() as u16;
        let bundle = table.records[usize::from(member)].as_ref().unwrap();
        for (phase, expected) in observation["phases"].as_array().unwrap().iter().enumerate() {
            let descriptor = &bundle.phases[phase];
            let mut scalars = Vec::new();
            for value in [
                descriptor.duration,
                descriptor.recovery_ticks,
                descriptor.buffer_until,
                descriptor.combo_at,
            ] {
                scalars.extend(value.to_be_bytes());
            }
            scalars.extend(descriptor.startup_effect.to_be_bytes());
            let hex = |bytes: &[u8]| {
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            };
            assert_eq!(hex(&scalars), expected["scalars"]);
            let rule = battle::action::hit(
                &files,
                &HitResource {
                    source: path.into(),
                    selection: battle::HitSelection::Technique {
                        member,
                        phase: phase as u8,
                    },
                    rule: 0,
                },
            )?;
            let mut bytes = [0; 28];
            bytes[..2].copy_from_slice(&rule.flags.to_be_bytes());
            bytes[2..8].copy_from_slice(&[
                rule.element,
                rule.hitstun,
                rule.contact_cooldown,
                rule.stun_chance,
                rule.stagger,
                rule.guard_pressure,
            ]);
            bytes[8..12].copy_from_slice(&rule.conditions.to_be_bytes());
            bytes[12..14].copy_from_slice(&[rule.condition_chance, rule.power_mode]);
            bytes[14..16].copy_from_slice(&rule.power.to_be_bytes());
            bytes[16..18].copy_from_slice(&rule.sound.to_be_bytes());
            bytes[20..25].copy_from_slice(&[
                rule.armor_damage,
                rule.knockback_delay,
                rule.impact_effect,
                rule.condition_parameter as u8,
                rule.impact_bank,
            ]);
            for span in &rule.storage {
                bytes[span.offset..span.offset + span.bytes.len()].copy_from_slice(&span.bytes);
            }
            assert_eq!(hex(&bytes), expected["rule"]);
        }
    }
    let table: Table = files.json(battle_projectile::PATH)?;
    assert_eq!(table.records.len(), 26);
    let request = ProjectileResource {
        trail: None,
        ground: None,
        source: battle_projectile::PATH.into(),
        member: 4,
        hit: HitResource {
            source: battle_action::SPELL_PATH.into(),
            selection: battle::HitSelection::Technique {
                member: 16,
                phase: 0,
            },
            rule: 0,
        },
        birth: Some(EffectResource {
            models: Default::default(),
            scene: None,
            source: battle_effect::TECHNIQUES_PATH.into(),
            resource: 1,
            members: vec![28],
        }),
        ..request()
    };
    let hit = battle::action::hit(&files, &request.hit)?;
    assert_eq!((hit.element, hit.power_mode, hit.power), (5, 1, 130));
    assert_eq!(
        (
            hit.hitstun,
            hit.contact_cooldown,
            hit.stun_chance,
            hit.stagger
        ),
        (20, 30, 20, 1)
    );
    let row = &table.records[4];
    assert_eq!((row.lifetime, row.shape, row.active_duration), (20, 1, 0));
    assert_eq!(row.birth_effect.member, 28);
    let definition = load(&files, &request)?;
    let contact = definition.contact.expect("original Lightning contact");
    assert!(contact.hit.arte);
    assert_eq!(contact.hit.kind, resonance_battle::DamageKind::Magic);
    assert_eq!(contact.hit.power, resonance_battle::Power::Percent(130));
    let mut sounds = vec![];
    let effects = battle::effect_program::load(
        &files,
        battle_effect::TECHNIQUES_PATH,
        1,
        &[28],
        &mut |index| {
            sounds.push(index);
            Ok(resonance_battle::SoundBinding { resource: 3, index })
        },
    )?;
    assert_eq!(sounds, [92]);
    assert_eq!(effects.members.len(), 1);
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/lightning-effect-source.json"))?;
    let source: battle_effect::SourceBank = files.json(battle_effect::TECHNIQUES_PATH)?;
    assert_eq!(source.source_sha256, fixture["source_sha256"]);
    assert_eq!(
        serde_json::to_value(source.program(28)?)?,
        fixture["program"]
    );
    Ok(())
}

fn change_hit(
    files: &mut Files,
    edit: impl FnOnce(&mut resonance_content::battle_action::HitRule),
) {
    let mut table: resonance_content::battle_action::Table =
        files.json("test-actions.json").unwrap();
    edit(&mut table.records[0].as_mut().unwrap().hit_rules[0]);
    files.bytes.insert(
        "test-actions.json".into(),
        serde_json::to_vec(&table).unwrap().into(),
    );
}

#[test]
fn hit_power_element_guard_and_contact_cooldown_come_from_the_verified_action() {
    let mut files = files("");
    change_hit(&mut files, |hit| {
        hit.element = 5;
        hit.power_mode = 1;
        hit.power = 130;
        hit.contact_cooldown = 30;
        hit.guard_pressure = 7;
        hit.armor_damage = 3;
        hit.flags = 0x1081;
    });
    change(&mut files, |row| row.hit_class = 1);
    let contact = load(&files, &request()).unwrap().contact.unwrap();
    assert_eq!(contact.cooldown, 30);
    assert_eq!(contact.hit.reaction.armor_damage, 3);
    assert_eq!(contact.hit.power, resonance_battle::Power::Percent(130));
    assert_eq!(
        contact.hit.element,
        resonance_battle::HitElement::Element(resonance_battle::Element::Lightning)
    );
    assert!(contact.hit.prevents_defeat);
    assert_eq!(
        contact.hit.guard,
        resonance_battle::GuardRule {
            enabled: true,
            pressure: 7,
            breaks: true,
            unbreakable: true,
        }
    );
    // A later verified generation supplies new parameters; there is no stale
    // caller descriptor or disk executable cache to override it.
    change_hit(&mut files, |hit| {
        hit.power = 200;
        hit.contact_cooldown = 9;
    });
    let contact = load(&files, &request()).unwrap().contact.unwrap();
    assert_eq!(contact.hit.power, resonance_battle::Power::Percent(200));
    assert_eq!(contact.cooldown, 9);
}

#[test]
fn source_reactions_resolve_direction_impulse_and_timing_before_activation() {
    use resonance_battle::RecoilDirection;
    let mut files = files("");
    change_hit(&mut files, |hit| {
        hit.flags = 0x102;
        hit.hitstun = 40;
        hit.knockback_delay = 2;
    });
    for (selector, direction) in [
        (0, RecoilDirection::Travel),
        (1, RecoilDirection::AwayFromOwner),
        (2, RecoilDirection::AwayFromContact),
        (3, RecoilDirection::TowardContact),
        (255, RecoilDirection::None),
    ] {
        change(&mut files, |row| {
            row.reaction = 0;
            row.knockback = selector;
        });
        let reaction = load(&files, &request())
            .unwrap()
            .contact
            .unwrap()
            .hit
            .reaction;
        assert_eq!(reaction.direction, direction);
        assert_eq!(reaction.hitstun, 40);
        assert!(reaction.alternate_motion && reaction.recoil.lift_guard);
        assert_eq!(reaction.recoil.delay, 2);
        assert_eq!(reaction.recoil.impulse, [4., -10.]);
    }
    change(&mut files, |row| row.reaction = 255);
    assert!(
        load(&files, &request())
            .unwrap_err()
            .to_string()
            .contains("missing recoil impulse")
    );
    change(&mut files, |row| row.reaction = 1);
    files.bytes.remove(resonance_content::battle_recoil::PATH);
    assert!(load(&files, &request()).is_err());
}

#[test]
fn hit_element_keeps_inheritance_distinct_from_explicit_neutral() {
    use resonance_battle::{Element, HitElement};
    for (source, expected) in [(0, HitElement::Inherited), (10, HitElement::Neutral)]
        .into_iter()
        .chain(
            Element::ALL
                .into_iter()
                .enumerate()
                .map(|(i, element)| ((i + 1) as u8, HitElement::Element(element))),
        )
    {
        let mut files = files("");
        change_hit(&mut files, |hit| hit.element = source);
        assert_eq!(
            load(&files, &request())
                .unwrap()
                .contact
                .unwrap()
                .hit
                .element,
            expected
        );
    }
    for source in [9, 11, 255] {
        let mut files = files("");
        change_hit(&mut files, |hit| hit.element = source);
        assert!(
            load(&files, &request())
                .unwrap_err()
                .to_string()
                .contains("invalid action hit element")
        );
    }
}

#[test]
fn stagger_and_down_hit_flag_are_preserved_for_runtime_preparation() {
    let mut files = files("");
    change_hit(&mut files, |hit| {
        hit.stagger = 5;
        hit.flags |= 0x40;
    });
    let definition = load(&files, &request()).unwrap();
    let reaction = definition.contact.unwrap().hit.reaction;
    assert_eq!(reaction.stagger, 5);
    assert!(reaction.hits_down);
}

#[test]
fn unsupported_hit_behavior_cannot_be_dropped_during_preparation() {
    for edit in [
        |hit: &mut resonance_content::battle_action::HitRule| hit.flags = 0x4000,
        |hit: &mut resonance_content::battle_action::HitRule| hit.element = 255,
        |hit: &mut resonance_content::battle_action::HitRule| hit.power_mode = 4,
        |hit: &mut resonance_content::battle_action::HitRule| hit.conditions = 1,
        |hit: &mut resonance_content::battle_action::HitRule| hit.sound = 1,
        |hit: &mut resonance_content::battle_action::HitRule| hit.impact_effect = 1,
    ] {
        let mut files = files("");
        change_hit(&mut files, edit);
        assert!(load(&files, &request()).is_err());
    }
    let files = files("");
    for edit in [
        |r: &mut HitResource| r.source = "missing.json".into(),
        |r: &mut HitResource| {
            r.selection = battle::HitSelection::Technique {
                member: 1,
                phase: 0,
            }
        },
        |r: &mut HitResource| {
            r.selection = battle::HitSelection::Technique {
                member: 0,
                phase: 4,
            }
        },
        |r: &mut HitResource| r.rule = 1,
    ] {
        let mut request = request();
        edit(&mut request.hit);
        assert!(load(&files, &request).is_err());
    }
}

#[test]
fn changed_hit_source_is_checked_on_a_script_cache_hit_without_replacing_live_state() {
    let source = r#"script battle; use battle;
        asset projectile: battle::Projectile = "test";
        pub task run() { await battle::at_age(ticks(2));
            battle::heal_percent(battle::owner(), 40); }
    "#;
    let mut files = files(source);
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: vec![],
        fail: false,
    };
    let prepare = |files: &Files, cache: &mut PreparationCache, resources: &mut Resources| {
        battle::prepare(
            cache,
            files,
            &[binding()],
            vec![actor()],
            1,
            resources,
            vec![],
        )
    };
    let prepared = prepare(&files, &mut cache, &mut resources).unwrap();
    let id = prepared.actor_ids().next().unwrap();
    let mut current = Battle::new(prepared);
    current
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: id,
                action: 1,
                target: id,
            }],
            ..Default::default()
        })
        .unwrap();
    change_hit(&mut files, |hit| hit.stun_chance = 20);
    assert_eq!(
        load(&files, &request())
            .unwrap()
            .contact
            .unwrap()
            .hit
            .reaction
            .stun_chance,
        20
    );
    assert!(prepare(&files, &mut cache, &mut resources).is_err());
    change_hit(&mut files, |hit| hit.stun_chance = 0);
    files.bytes.remove(resonance_content::battle_recoil::PATH);
    assert!(prepare(&files, &mut cache, &mut resources).is_err());
    files.bytes.remove("test-actions.json");
    assert!(prepare(&files, &mut cache, &mut resources).is_err());
    assert_eq!(resources.paths.len(), 4);
    assert_eq!(current.actors()[0].hp, 10);
    for _ in 0..3 {
        current.step(BattleInput::default()).unwrap();
    }
    assert_eq!(current.actors()[0].hp, 50);
}
