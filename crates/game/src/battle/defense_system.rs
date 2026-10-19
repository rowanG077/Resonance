//! Resolve the Defense System's component identities and body attachments once.
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActorId, DefenseArm, DefenseSystemDefinition, MeleeAnchor, ModelDefinition, Pose,
};

/// Keep the door behind the combatants instead of letting the view pass through it.
pub const CAMERA_YAW_LIMITS: [f32; 2] = [102., 256.];

const CORE: u8 = 211;
const ORBIT: u8 = 212;
const ARM: u8 = 213;
const REPAIR: u8 = 214;
const ARM_BONES: [&str; 4] = ["PA00_Arm_R", "PA01_Arm_R", "PA02_Arm_L", "PA03_Arm_L"];
const DEPLOYMENT_CLIP: u16 = 32;
const DEPLOYMENT_TICKS: u16 = 120;
const DEPLOYMENT_RATE: f32 = 0.175;
const DEPLOYMENT_BLEND: u8 = 4;

pub fn initial_position(enemy: u8) -> Option<[f32; 3]> {
    match enemy {
        CORE => Some([850., 0., 0.]),
        REPAIR => Some([200., 0., 0.]),
        _ => None,
    }
}

pub fn definition(
    members: &[(ActorId, u8)],
    core_model: &ModelDefinition,
) -> Result<DefenseSystemDefinition> {
    let instances = |enemy| {
        members
            .iter()
            .filter_map(move |&(actor, id)| (id == enemy).then_some(actor))
    };
    let cores: Vec<_> = instances(CORE).collect();
    let arms: Vec<_> = instances(ARM).collect();
    ensure!(
        cores.len() == 1
            && arms.len() == 4
            && instances(ORBIT).count() == 2
            && instances(REPAIR).count() == 1,
        "Defense System requires one core, four arms, two orbits and one repair unit"
    );
    let mut anchors = Vec::new();
    for (actor, name) in arms.into_iter().zip(ARM_BONES) {
        let bone = core_model
            .skeleton
            .bones
            .iter()
            .position(|bone| bone.name == name)
            .with_context(|| format!("Defense System is missing attachment {name}"))?;
        anchors.push(DefenseArm {
            actor,
            anchor: MeleeAnchor {
                resource: core_model.resource,
                bone: u16::try_from(bone)?,
            },
        });
    }
    Ok(DefenseSystemDefinition {
        core: cores[0],
        repair: (
            instances(REPAIR).next().unwrap(),
            initial_position(REPAIR).unwrap(),
        ),
        components: members
            .iter()
            .filter_map(|&(actor, enemy)| matches!(enemy, ORBIT | ARM | REPAIR).then_some(actor))
            .collect(),
        arms: anchors
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid Defense System arm count"))?,
        deployment: super::model::motion(Some(core_model), DEPLOYMENT_CLIP)
            .context("Defense System deployment motion is not prepared")?,
        deployment_ticks: DEPLOYMENT_TICKS,
        deployment_pose: Pose {
            rate: DEPLOYMENT_RATE,
            blend: DEPLOYMENT_BLEND,
            ..Default::default()
        },
    })
}
