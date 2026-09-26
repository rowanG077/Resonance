use super::*;
use resonance_content::battle_action::{NORMAL_PATH, NormalTable};

#[test]
#[ignore = "requires the complete current cooked library; no devices"]
fn verified_normal_sources_match_original_lloyd_contacts_and_preserve_aliases() -> Result<()> {
    let files = Files::load(
        &common::asset_root(),
        &["fields/map-340.preload.json"],
        &mut resonance_content::prepared::Cache::default(),
        || false,
    )?;
    let table: NormalTable = files.json(NORMAL_PATH)?;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/normal-action-sources.json"))?;
    assert_eq!(table.source_sha256, fixture["source_module_sha256"]);
    assert_eq!(table.groups.len(), 9);
    for group in &table.groups {
        assert_eq!(group.selectors.len(), 7);
        assert_eq!(group.actions.len(), 7);
        assert_eq!(group.descriptors.len(), 7);
        for selector in &group.selectors {
            assert!(usize::from(selector.action) < group.actions.len());
            assert!(
                selector.fallback == 255 || usize::from(selector.fallback) < group.selectors.len()
            );
        }
    }
    let lloyd = &table.groups[0];
    // Original ordinary hand groups are resolved to prepared pose-anchor IDs;
    // the loader retains their order and reads damage/recoil from verified data.
    for (selection, row, anchors, kind) in [
        (0, 0, vec![2, 3], resonance_battle::DamageKind::Slash),
        (4, 0, vec![2, 3], resonance_battle::DamageKind::Slash),
        (4, 1, vec![7, 8], resonance_battle::DamageKind::Slash),
        (2, 0, vec![7, 8], resonance_battle::DamageKind::Thrust),
    ] {
        let contact = battle::melee::load(
            &files,
            &MeleeResource {
                impact: None,
                source: NORMAL_PATH.into(),
                selection: battle::MeleeSelection::Normal {
                    character: 0,
                    selection,
                },
                row,
                anchor_groups: vec![vec![2, 3], vec![7, 8]],
            },
        )?;
        assert_eq!(contact.anchors, anchors);
        assert_eq!(contact.hit.kind, kind);
        assert_eq!(contact.hit.power, resonance_battle::Power::Normal);
        assert_eq!(contact.hit.element, resonance_battle::HitElement::Inherited);
        assert_eq!(
            (
                contact.cooldown,
                contact.hit.reaction.hitstun,
                contact.hit.reaction.stagger
            ),
            (120, 30, 2)
        );
        assert_eq!((contact.radius, contact.height), (30., 30.));
        assert_eq!(
            contact.hit.reaction.direction,
            resonance_battle::RecoilDirection::AwayFromOwner
        );
    }
    for row in fixture["observations"].as_array().unwrap() {
        let hit = &lloyd.hits[row["hit_index"].as_u64().unwrap() as usize];
        for (actual, key) in [
            (i64::from(hit.start), "start"),
            (i64::from(hit.emission), "emission"),
            (i64::from(hit.attachment_count), "attachment_count"),
            (i64::from(hit.shape), "shape"),
            (i64::from(hit.damage_kind), "damage_kind"),
            (i64::from(hit.rule), "rule_index"),
            (i64::from(hit.hit_class), "hit_class"),
            (i64::from(hit.reaction), "reaction"),
        ] {
            assert_eq!(actual, row[key].as_i64().unwrap(), "{key}");
        }
        assert_eq!(
            hit.emission_operands,
            serde_json::from_value::<[u8; 4]>(row["emission_operands"].clone())?
        );
        assert_eq!(
            hit.radius.bits(),
            row["radius_bits"].as_u64().unwrap() as u32
        );
        assert_eq!(
            hit.height.bits(),
            row["height_bits"].as_u64().unwrap() as u32
        );
        let rule = &lloyd.hit_rules[usize::from(hit.rule)];
        for (actual, key) in [
            (u64::from(rule.flags), "flags"),
            (u64::from(rule.element), "element"),
            (u64::from(rule.hitstun), "hitstun"),
            (u64::from(rule.contact_cooldown), "cooldown"),
            (u64::from(rule.stun_chance), "stun_chance"),
            (u64::from(rule.stagger), "stagger"),
            (u64::from(rule.guard_pressure), "guard_pressure"),
        ] {
            assert_eq!(actual, row[key].as_u64().unwrap(), "{key}");
        }
    }
    // Raine's action table aliases records; direct descriptor indexing for reach
    // still needs all seven physical entries, including the unused selections.
    let raine = &table.groups[3];
    assert_eq!(raine.actions[1].descriptor, 3);
    assert_eq!(raine.actions[6].descriptor, 5);
    assert_ne!(raine.descriptors[1].duration, raine.descriptors[3].duration);
    Ok(())
}

fn request() -> MeleeResource {
    MeleeResource {
        impact: None,
        source: "test-normal.json".into(),
        selection: battle::MeleeSelection::Normal {
            character: 0,
            selection: 0,
        },
        row: 0,
        anchor_groups: vec![vec![1, 2], vec![7, 8]],
    }
}

fn change(files: &mut Files, edit: impl FnOnce(&mut NormalTable)) {
    let mut table: NormalTable = files.json("test-normal.json").unwrap();
    edit(&mut table);
    files.bytes.insert(
        "test-normal.json".into(),
        serde_json::to_vec(&table).unwrap().into(),
    );
}

#[test]
fn normal_contact_selection_uses_action_roots_and_preserves_attachment_order() {
    let mut files = files("");
    change(&mut files, |table| {
        let group = &mut table.groups[0];
        group.selectors[0].action = 1;
        let mut action = group.actions[0].clone();
        action.hit = 1;
        group.actions.push(action);
        let mut hit = group.hits[0].clone();
        hit.attachment_count = 3;
        hit.emission_operands = [1, 0, 1, 255]; // Inactive fourth operand is not interpreted.
        hit.radius = resonance_content::source::FloatOperand::Value(17.);
        group.hits.push(hit);
    });
    let contact = battle::melee::load(&files, &request()).unwrap();
    assert_eq!(contact.anchors, [7, 8, 1, 2, 7, 8]);
    assert_eq!(contact.radius, 17.);
}

#[test]
fn unsupported_normal_routes_and_missing_bindings_fail_loading() {
    for edit in [
        |t: &mut NormalTable| t.groups[0].selectors[0].action = 255,
        |t: &mut NormalTable| t.groups[0].actions[0].hit = u32::MAX,
        |t: &mut NormalTable| t.groups[0].hits[0].start = -1,
        |t: &mut NormalTable| t.groups[0].hits[0].emission = -2,
        |t: &mut NormalTable| t.groups[0].hits[0].attachment_count = 0,
        |t: &mut NormalTable| t.groups[0].hits[0].attachment_count = 5,
        |t: &mut NormalTable| t.groups[0].hits[0].emission_operands[0] = 255,
        |t: &mut NormalTable| t.groups[0].hits[0].rule = 255,
        |t: &mut NormalTable| t.groups[0].hits[0].hit_class = 2,
        |t: &mut NormalTable| t.groups[0].hit_rules[0].conditions = 1,
    ] {
        let mut files = files("");
        change(&mut files, edit);
        assert!(battle::melee::load(&files, &request()).is_err());
    }
    let files = files("");
    for edit in [
        |r: &mut MeleeResource| r.source = "missing.json".into(),
        |r: &mut MeleeResource| {
            r.selection = battle::MeleeSelection::Normal {
                character: 255,
                selection: 0,
            }
        },
        |r: &mut MeleeResource| {
            r.selection = battle::MeleeSelection::Normal {
                character: 0,
                selection: 255,
            }
        },
        |r: &mut MeleeResource| r.row = 255,
        |r: &mut MeleeResource| r.anchor_groups.clear(),
        |r: &mut MeleeResource| r.anchor_groups[0].clear(),
    ] {
        let mut request = request();
        edit(&mut request);
        assert!(battle::melee::load(&files, &request).is_err());
    }
}

#[test]
fn lloyd_binding_durations_follow_selector_and_descriptor_aliases() -> Result<()> {
    let mut files = files("");
    let mut table: NormalTable = files.json("test-normal.json")?;
    let group = &mut table.groups[0];
    group.selectors = vec![group.selectors[0].clone(); 7];
    group.actions = vec![group.actions[0].clone(); 7];
    group.descriptors = vec![
        resonance_content::battle_action::NormalDescriptor {
            duration: 30,
            recovery_ticks: 10,
            combo_at: [15, 0],
            buffer_until: 60,
            recovery_clip: 0,
            recovery_rate: resonance_content::source::FloatOperand::Value(0.5),
            startup_effect: 0,
            reach: [120, 0],
            storage: vec![],
        };
        7
    ];
    group.selectors[2].action = 5;
    group.actions[5].descriptor = 3;
    group.descriptors[3].duration = 77;
    files
        .bytes
        .insert(NORMAL_PATH.into(), serde_json::to_vec(&table)?.into());
    let bindings = battle::normal::lloyd_bindings(&files, [10, 11, 12, 13, 14, 15, 16])?;
    assert_eq!((bindings[2].id, bindings[2].duration), (12, 77));
    assert_eq!(bindings[2].entry, "thrust");
    assert_eq!(bindings[6].entry, "aerial_thrust");
    table.groups[0].actions[5].descriptor = 255;
    files
        .bytes
        .insert(NORMAL_PATH.into(), serde_json::to_vec(&table)?.into());
    assert!(battle::normal::lloyd_bindings(&files, [1; 7]).is_err());
    assert!(battle::normal::lloyd_melee("battle/melee/lloyd/unknown", &[]).is_err());
    Ok(())
}
