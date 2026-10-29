//! Defense System controller values from the native REL and one maintained recipe.
use crate::rel::Rel;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_defense_system::{
    Definition, Deployment, FixedMember, Messages, RepairCycle,
};
use std::{fs, path::Path};
use symphonia_constants::MonsterId;

// These are controller behavior and named attachment choices, not duplicated
// per-enemy data. Positions, rate and visible strings come from the REL below.
const REPAIR: RepairCycle = RepairCycle {
    self_ticks: 1500,
    repeat_ticks: 120,
    orbit_ticks: 900,
    self_percent: 100,
    orbit_percent: 75,
    defense_divisor: 4,
    defense_multiplier: 3,
};
const ARM_JOINTS: [&str; 4] = ["PA00_Arm_R", "PA01_Arm_R", "PA02_Arm_L", "PA03_Arm_L"];
const CAMERA_YAW_LIMITS: [f32; 2] = [102., 256.];

fn name(id: MonsterId) -> Result<String> {
    Ok(id
        .resource_name()
        .context("Defense System member has no name")?
        .into())
}

fn source(root: &Path) -> Result<Definition<String>> {
    let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    let float = |offset| -> Result<f32> {
        let value = f32::from_bits(crate::dol::u32(module.slice(4, offset, 4)?, 0)?);
        ensure!(value.is_finite(), "non-finite Defense System REL value");
        Ok(value)
    };
    let fixed = |id, x| -> Result<_> {
        Ok(FixedMember {
            enemy: name(id)?,
            position: [float(x)?, float(0x4964)?, float(0x4964)?],
            heading: float(0x4968)?,
        })
    };
    let definition = Definition {
        core: fixed(MonsterId::DefenseSystem, 0x4960)?,
        repair: fixed(MonsterId::AutoRepairUnit, 0x496c)?,
        orbit: name(MonsterId::Orbit)?,
        orbit_count: 2,
        arm: name(MonsterId::GuardArm)?,
        arm_joints: ARM_JOINTS.map(str::to_owned),
        deployment: Deployment {
            clip: 32,
            ticks: 120,
            hp_percent: 66,
            rate: float(0x4984)?,
            blend: 4,
        },
        repair_cycle: REPAIR,
        repair_sound: 104,
        repair_sound_priority: 2,
        messages: Messages {
            automatic: module.text(4, 0x4970)?,
            orbit: module.text(4, 0x497c)?,
            deployment: module.text(4, 0x498c)?,
            duration: 90,
        },
        camera_yaw_limits: CAMERA_YAW_LIMITS,
    };
    REPAIR.validate()?;
    definition.messages.validate()?;
    ensure!(
        definition.deployment.rate > 0.,
        "invalid Defense System deployment rate"
    );
    Ok(definition)
}

pub fn cook_battle_defense(root: &Path, library: &Path) -> Result<()> {
    let definition = source(root)?;
    let output = library.join("battle/defense-system.json");
    ensure!(!output.exists(), "Defense System document already exists");
    crate::output::json(&output, &definition)?;
    Ok(())
}
