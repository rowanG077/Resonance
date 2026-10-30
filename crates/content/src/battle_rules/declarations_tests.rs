use super::*;
use crate::{
    battle_victory::{self, document as victories},
    library::Mappings,
};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn characters() -> Result<Mappings> {
    Mappings::new(
        crate::ui_art::characters::NAMES
            .iter()
            .enumerate()
            .map(|(id, name)| ((id + 1) as u32, (*name).into()))
            .collect(),
    )
}

#[test]
fn declaration_authoring_names_preserve_random_bits_and_character_identity() -> Result<()> {
    let names = characters()?;
    for (mask, accepted) in [(1, 1), (7, 0), (4, 4), (3, 0)] {
        let gate = battle_victory::RandomGate { mask, accepted };
        let bits = victories::RandomBits::from(gate);
        let restored = bits.prepare()?;
        for sample in 0..=u16::MAX {
            assert_eq!(gate.accepts(sample), restored.accepts(sample));
        }
    }
    for gate in [
        json!({}),
        json!({"set":[1,1]}),
        json!({"set":[1],"clear":[1]}),
        json!({"clear":[16]}),
    ] {
        assert!(
            serde_json::from_value::<victories::RandomBits>(gate)?
                .prepare()
                .is_err()
        );
    }
    let mut group: victories::NamedGroup = serde_json::from_value(
        json!({"id":1,"character":"lloyd","participants":["lloyd","sheena"],"required_leader":"sheena","voice":null,"condition":"sheena_affinity","selection":{"kind":"ordinary","priority":1,"gate":{"set":[0]}}}),
    )?;
    let before = group.clone().prepare(&names)?;
    group.character = "hero".into();
    group.participants[0] = "hero".into();
    let renamed = Mappings::new(
        names
            .iter()
            .map(|(id, name)| (id, if id == 1 { "hero" } else { name }.into()))
            .collect(),
    )?;
    assert_eq!(
        serde_json::to_value(group.prepare(&renamed)?)?,
        serde_json::to_value(before)?
    );
    let recoil: Recoil = serde_json::from_value(
        json!({"impulses":{"1":{"horizontal":4,"vertical":10},"0":{"horizontal":0,"vertical":0}},"guard_speed":3,"suppression_distance":400,"light_vertical_scale":1.15,"heavy_vertical_scale":0.75,"default_guard_recovery_bonuses":{"hero":25,"colette":5}}),
    )?;
    let retained = recoil.clone().prepare(&renamed)?;
    assert_eq!(retained.impulses, [[0., 0.], [4., 10.]]);
    assert_eq!(retained.default_guard_recovery_bonuses[&1], 25);
    let mut edited = recoil;
    edited.impulses.get_mut(&1).unwrap().vertical = 20.;
    assert_eq!(edited.prepare(&renamed)?.impulses[1], [4., 20.]);
    assert_eq!(retained.impulses[1], [4., 10.]);
    Ok(())
}

#[test]
#[ignore = "requires installed fixed F13 metadata; no media conversion"]
fn declaration_authoring_installed_selections_and_variants_preserve_all_values() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lib = root.join("local/layout-migration/final-integration-20261025-supervised/library");
    let old: Value = serde_json::from_slice(&fs::read(
        root.join("local/structure-review-20261027/battle-declarations-ledger/initial.json"),
    )?)?;
    let characters: Mappings =
        serde_json::from_slice(&fs::read(lib.join(crate::battle_model::document::PARTY))?)?;
    let enemies: Mappings = serde_json::from_slice(&fs::read(
        lib.join(crate::battle_enemy::document::MAPPINGS),
    )?)?;
    let groups: Vec<victories::NamedGroup> =
        serde_json::from_slice(&fs::read(lib.join(victories::GROUPS))?)?;
    for (row, original) in groups
        .into_iter()
        .zip(old[victories::GROUPS]["groups"].as_array().unwrap())
    {
        let expected: battle_victory::Group = serde_json::from_value(original.clone())?;
        let actual = row.prepare(&characters)?;
        ensure!(
            serde_json::to_value(actual)? == serde_json::to_value(expected)?,
            "group identity or selection changed"
        );
    }
    let recoil: Recoil = serde_json::from_slice(&fs::read(lib.join(crate::battle_recoil::PATH))?)?;
    let recoil = recoil.prepare(&characters)?;
    ensure!(
        serde_json::to_value(&recoil.impulses)? == old[crate::battle_recoil::PATH]["impulses"],
        "impulses changed"
    );
    for id in 1..=9 {
        assert_eq!(
            recoil.default_guard_recovery_bonuses[&id] as u64,
            old[crate::battle_recoil::PATH]["default_guard_recovery_bonuses"][id as usize]
                .as_u64()
                .unwrap()
        );
    }
    let defense: DefenseSystem =
        serde_json::from_slice(&fs::read(lib.join(crate::battle_defense_system::PATH))?)?;
    let defense = defense.prepare(&enemies)?;
    assert_eq!(
        [
            defense.core.enemy,
            defense.orbit,
            defense.arm,
            defense.repair.enemy
        ],
        [211, 212, 213, 214]
    );
    assert_eq!(defense.deployment.clip, 32);
    let skeleton = crate::animation::Skeleton::from_glb(&fs::read(
        lib.join("meshes/monsters/defense-system-0211/meshes/body.glb"),
    )?)?;
    let joints = defense
        .arm_joints
        .iter()
        .map(|name| skeleton.bone(name).context("missing core joint"))
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(joints, [6, 3, 11, 9]);
    let current: Formations =
        serde_json::from_slice(&fs::read(lib.join(crate::battle_formation::PATH))?)?;
    let prepared = current.prepare(&enemies)?;
    assert_eq!(prepared.records.len(), 1000);
    assert_eq!(prepared.pools.len(), 736);
    let mut spawns = 0;
    for (id, row) in prepared.records.iter().enumerate() {
        let owner = current.mappings.target(id as u32)?;
        let original = &old[crate::battle_formation::PATH]["formations"][owner];
        let variants = original["actors"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|a| {
                std::iter::repeat_n(
                    a.get("variant").and_then(Value::as_u64).unwrap_or(0) as u8,
                    a.get("count").and_then(Value::as_u64).unwrap_or(1) as usize,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            row.actors.iter().map(|a| a.variant).collect::<Vec<_>>(),
            variants
        );
        spawns += row.actors.len();
    }
    eprintln!(
        "54 groups, 19 impulses, 9 character bonuses, 4 exact core joints, 1,000 formations, 736 pools and {spawns} spawns preserve selection values"
    );
    Ok(())
}
