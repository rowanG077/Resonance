//! Resolve the Defense System's component identities and body attachments once.
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActorId, DefenseArm, DefenseSystemDefinition, MeleeAnchor, ModelDefinition, Pose,
};
use resonance_content::battle_defense_system::Definition;

#[cfg(test)]
mod tests;

pub fn definition(
    content: &Definition,
    members: &[(ActorId, u8)],
    core_model: &ModelDefinition,
    mut sound: impl FnMut(resonance_battle::Sound) -> Result<Option<resonance_battle::Sound>>,
) -> Result<DefenseSystemDefinition> {
    content.validate()?;
    let instances = |enemy| {
        members
            .iter()
            .filter_map(move |&(actor, id)| (id == enemy).then_some(actor))
    };
    let cores: Vec<_> = instances(content.core.enemy).collect();
    let arms: Vec<_> = instances(content.arm).collect();
    ensure!(
        cores.len() == 1
            && arms.len() == 4
            && instances(content.orbit).count() == usize::from(content.orbit_count)
            && instances(content.repair.enemy).count() == 1
            && members.iter().all(|&(_, enemy)| content.contains(enemy)),
        "Defense System members do not match its composition"
    );
    let mut anchors = Vec::new();
    for (actor, joint) in arms.into_iter().zip(&content.arm_joints) {
        let bones: Vec<_> = core_model
            .skeleton
            .bones
            .iter()
            .enumerate()
            .filter_map(|(index, bone)| (&bone.name == joint).then_some(index))
            .collect();
        ensure!(
            bones.len() == 1,
            "Defense System requires one attachment for {joint}"
        );
        anchors.push(DefenseArm {
            actor,
            anchor: MeleeAnchor {
                resource: core_model.resource,
                bone: u16::try_from(bones[0])?,
            },
        });
    }
    Ok(DefenseSystemDefinition {
        core: cores[0],
        repair: (
            instances(content.repair.enemy).next().unwrap(),
            content.repair.position,
        ),
        repair_heading: content.repair.heading,
        orbits: instances(content.orbit).collect(),
        repair_cycle: content.repair_cycle,
        repair_sound: sound(resonance_battle::Sound::Cue(content.repair_sound))?
            .context("Defense System repair sound is not prepared")?,
        repair_sound_priority: content.repair_sound_priority,
        messages: content.messages.clone(),
        components: members
            .iter()
            .filter_map(|&(actor, enemy)| (enemy != content.core.enemy).then_some(actor))
            .collect(),
        arms: anchors
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid Defense System arm count"))?,
        deployment: super::model::motion(Some(core_model), content.deployment.clip)
            .context("Defense System deployment motion is not prepared")?,
        deployment_ticks: content.deployment.ticks,
        deployment_hp_percent: content.deployment.hp_percent,
        deployment_pose: Pose {
            rate: content.deployment.rate,
            blend: content.deployment.blend,
            ..Default::default()
        },
    })
}
