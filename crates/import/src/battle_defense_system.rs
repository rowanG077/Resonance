//! Translate verified native controller parameters and extract its float constants.
use crate::{read::Field, rel::Rel};
use anyhow::{Result, ensure};
use resonance_content::battle_defense_system::{
    Definition, Deployment, FixedMember, Messages, RepairCycle, VERSION,
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    provenance: BTreeMap<String, String>,
    core: u8,
    repair: u8,
    orbit: u8,
    orbit_count: u8,
    arm: u8,
    arm_slots: [u8; 4],
    core_x_offset: usize,
    repair_x_offset: usize,
    zero_offset: usize,
    heading_offset: usize,
    rate_offset: usize,
    deployment_clip: u16,
    deployment_ticks: u16,
    deployment_hp_percent: u8,
    deployment_blend: u8,
    repair_cycle: RepairCycle,
    repair_sound: u16,
    repair_sound_priority: u8,
    message_offsets: [usize; 3],
    message_ticks: u16,
    camera_yaw_limits: [f32; 2],
}

fn read(module: &Rel) -> Result<Definition> {
    let bytes = include_bytes!("battle_defense_system.json");
    let recipe: Recipe = serde_json::from_slice(bytes)?;
    ensure!(
        [
            "members",
            "attachment_slots",
            "deployment",
            "placement",
            "repair",
            "messages",
            "camera"
        ]
        .into_iter()
        .all(|key| recipe
            .provenance
            .get(key)
            .is_some_and(|value| !value.is_empty())),
        "missing Defense System provenance"
    );
    let float = |offset| f32::read(module.at((4, offset))?, 0);
    let fixed = |enemy, x| -> Result<_> {
        Ok(FixedMember {
            enemy,
            position: [
                float(x)?,
                float(recipe.zero_offset)?,
                float(recipe.zero_offset)?,
            ],
            heading: float(recipe.heading_offset)?,
        })
    };
    let definition = Definition {
        version: VERSION,
        source_sha256: crate::digest(&module.bytes),
        recipe_sha256: crate::digest(bytes),
        core: fixed(recipe.core, recipe.core_x_offset)?,
        repair: fixed(recipe.repair, recipe.repair_x_offset)?,
        orbit: recipe.orbit,
        orbit_count: recipe.orbit_count,
        arm: recipe.arm,
        arm_slots: recipe.arm_slots,
        deployment: Deployment {
            clip: recipe.deployment_clip,
            ticks: recipe.deployment_ticks,
            hp_percent: recipe.deployment_hp_percent,
            rate: float(recipe.rate_offset)?,
            blend: recipe.deployment_blend,
        },
        repair_cycle: recipe.repair_cycle,
        repair_sound: recipe.repair_sound,
        repair_sound_priority: recipe.repair_sound_priority,
        messages: Messages {
            automatic: module.text((4, recipe.message_offsets[0]))?,
            orbit: module.text((4, recipe.message_offsets[1]))?,
            deployment: module.text((4, recipe.message_offsets[2]))?,
            duration: recipe.message_ticks,
        },
        camera_yaw_limits: recipe.camera_yaw_limits,
    };
    definition.validate()?;
    Ok(definition)
}

pub(crate) fn publish(file: &Path, output: &Path, prefix: &str) -> Result<String> {
    let definition = read(&Rel::read(file)?)?;
    let path = format!("{prefix}/defense-system.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&definition)?)?;
    Ok(path)
}

#[test]
#[ignore = "requires both original extracted discs; CPU only"]
fn original_defense_system_parameters_and_formation_order() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut first = None;
    for disc in [1, 2] {
        let module = Rel::read(&root.join(format!("disc{disc}/files/US_r_Top2Btl.rel")))?;
        let definition = read(&module)?;
        // Verify formation-order queue writes in the original instructions:
        // child[queue & 15] = actor; queue += 1.
        let queue_words = [
            0x8806_153b_u32,
            0x5403_16ba,
            0x3803_1540,
            0x7e66_012e,
            0x8886_153b,
            0x5483_073e,
            0x3803_0001,
            0x5004_073e,
            0x9886_153b,
            0x8866_1554,
            0x3803_0001,
            0x9806_1554,
        ];
        for (index, word) in queue_words.into_iter().enumerate() {
            assert_eq!(u32::read(module.at((1, 0x49e0 + index * 4))?, 0)?, word);
        }
        assert_eq!(definition.core.position, [850., 0., 0.]);
        assert_eq!(definition.repair.position, [200., 0., 0.]);
        assert_eq!(definition.core.heading, -90.);
        assert_eq!(definition.deployment.rate, 0.175);
        assert_eq!(definition.messages.automatic, "Auto Repair");
        assert_eq!(definition.messages.orbit, "Repair");
        assert_eq!(definition.messages.deployment, "Alert");
        assert_eq!(definition.messages.duration, 90);
        assert_eq!(definition.repair_cycle.self_ticks, 1500);
        assert_eq!(definition.repair_cycle.repeat_ticks, 120);
        assert_eq!(definition.repair_cycle.orbit_ticks, 900);
        assert_eq!(definition.repair_cycle.self_percent, 100);
        assert_eq!(definition.repair_cycle.orbit_percent, 75);
        assert_eq!(definition.repair_sound, 104);
        // Controller dispatch and message dependencies are original relocated records.
        assert_eq!(module.pointer(5, 0x5518)?, (1, 0x72ecc));
        assert_eq!(module.pointer(5, 0x5a40)?, (1, 0x72ddc));
        assert_eq!(module.pointer(5, 0x5a44)?, (1, 0x72980));
        let formations = crate::battle_formation::read(&std::fs::read(
            root.join(format!("disc{disc}/files/BTL/BTLusual.dat")),
        )?)?;
        let matching: Vec<_> = formations
            .records
            .iter()
            .filter_map(|formation| {
                let members: Vec<_> = formation
                    .actors
                    .iter()
                    .map(|actor| formation.resources[usize::from(actor.resource)].enemy)
                    .collect();
                members
                    .contains(&u16::from(definition.core.enemy))
                    .then_some(members)
            })
            .collect();
        assert!(!matching.is_empty());
        for members in matching {
            assert_eq!(members, [211, 213, 213, 213, 213, 214, 212, 212]);
        }
        if let Some(previous) = &first {
            assert_eq!(previous, &definition);
        }
        first = Some(definition);
    }
    Ok(())
}
