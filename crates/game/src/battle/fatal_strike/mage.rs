//! Prepared, continuous caster performances. Curves are baked on the destination
//! rig, with a free casting hand and planted feet, before combat admission.
use super::*;
use anyhow::{Context, Result};
use glam::{Quat, Vec3};
use resonance_content::animation::{Channel, Interpolation, Motion, NodeTrack, Track};

const FIRST_CLIP: u16 = 1_000;
const DURATION: u16 = 112;

use super::rig::{ease, globals, joint, reach, sample, turn};

fn bake(model: &ModelDefinition, style: FatalStyle, color: FatalColor) -> Result<(Motion, u16)> {
    let skeleton = &model.skeleton;
    let idle = model.idle_motions[0].context("caster finisher requires idle")?;
    let chant = model
        .reactions
        .chant
        .context("caster finisher requires chant")?;
    let release = model
        .reactions
        .cast
        .context("caster finisher requires release")?;
    let opening = model
        .motions
        .contains_key(&13)
        .then_some(13)
        .unwrap_or(chant);
    let idle_pose = sample(model, idle, 0.2)?;
    let keys = [
        (0., idle_pose.clone()),
        (16., sample(model, opening, 1.)?),
        (36., sample(model, chant, 0.35)?),
        (46., sample(model, release, 0.)?),
        (56., sample(model, release, 0.5)?),
        (70., sample(model, release, 1.)?),
        (88., sample(model, release, 1.)?),
        (112., idle_pose.clone()),
    ];
    let side = "left";
    let arm = [
        joint(skeleton, &format!("arm-01-{side}"))?,
        joint(skeleton, &format!("arm-03-{side}"))?,
        joint(skeleton, &format!("wrist-{side}"))?,
    ];
    let emitter = joint(skeleton, &format!("hand-{side}"))? as u16;
    let support = [
        joint(skeleton, "arm-01-right")?,
        joint(skeleton, "arm-03-right")?,
        joint(skeleton, "wrist-right")?,
    ];
    let fingers = skeleton
        .bones
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            (b.name.starts_with("finger-") && b.name.ends_with("-left")).then_some(i)
        })
        .collect::<Vec<_>>();
    let feet = ["left", "right"]
        .map(|side| -> Result<_> {
            Ok([
                joint(skeleton, &format!("leg-01-{side}"))?,
                joint(skeleton, &format!("leg-02-{side}"))?,
                joint(skeleton, &format!("leg-03-{side}"))?,
            ])
        })
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let idle_global = globals(skeleton, &idle_pose)?;
    let chest = joint(skeleton, "spine-02")?;
    let hips = skeleton
        .bones
        .iter()
        .position(|bone| bone.name.ends_with("-hips"))
        .context("missing caster pelvis")?;
    let mut frames = Vec::new();
    for tick in 0..=DURATION {
        let time = f32::from(tick);
        let pair = keys.windows(2).find(|pair| time <= pair[1].0).unwrap();
        let weight = ease((time - pair[0].0) / (pair[1].0 - pair[0].0));
        let mut local = pair[0]
            .1
            .iter()
            .zip(&pair[1].1)
            .map(|(a, b)| a.blend(*b, weight))
            .collect::<Vec<_>>();
        let gather = ease(time / 24.);
        let release = ease((time - 44.) / 12.);
        let settle = 1. - ease((time - 72.) / 40.);
        // Start from a stable lower-body stance. Native release clips contain
        // a step; copying it into a stationary ranged performance caused skating.
        for (index, bone) in skeleton.bones.iter().enumerate() {
            if bone.name.contains("leg-") || index == hips || bone.parent.is_none() {
                local[index] = idle_pose[index];
            }
        }
        local[hips].translation[2] -= (1.5 * gather + 0.8 * release) * settle;
        let sign = 1.;
        let twist = (-0.1 * gather + 0.19 * release) * settle * sign;
        turn(
            skeleton,
            &mut local,
            chest,
            Quat::from_rotation_z(twist)
                * Quat::from_rotation_x((0.025 * gather + 0.055 * release) * settle),
        )?;
        let global = globals(skeleton, &local)?;
        let shoulder = global[arm[0]].transform_point3(Vec3::ZERO);
        let length = shoulder.distance(global[arm[1]].transform_point3(Vec3::ZERO))
            + global[arm[1]]
                .transform_point3(Vec3::ZERO)
                .distance(global[arm[2]].transform_point3(Vec3::ZERO));
        // Native model axes are Z-up and -Y-forward. These are casting gestures,
        // independent of the victim's distance and of the actor's melee weapon.
        let (mut gather_hand, release_hand) = match color {
            FatalColor::Blue => (
                Vec3::new(sign * 0.24, -0.25, 0.65),
                Vec3::new(sign * 0.12, -0.65, 0.55),
            ),
            FatalColor::Red => (
                Vec3::new(sign * 0.32, -0.12, 0.82),
                Vec3::new(sign * 0.18, -0.82, -0.18),
            ),
            FatalColor::Green => (
                Vec3::new(sign * 0.62, 0.05, 0.05),
                Vec3::new(sign * 0.08, -0.95, 0.03),
            ),
        };
        if style == FatalStyle::Arcane && color == FatalColor::Blue {
            gather_hand = Vec3::new(0.5, -0.55, 0.35);
        }
        let desired = shoulder + gather_hand.lerp(release_hand, release) * length;
        let current = global[arm[2]].transform_point3(Vec3::ZERO);
        reach(
            skeleton,
            &mut local,
            arm,
            current.lerp(desired, gather * settle),
            Vec3::new(sign, 0.2, -0.6),
        )?;
        if style == FatalStyle::Arcane {
            let global = globals(skeleton, &local)?;
            let shoulder = global[support[0]].transform_point3(Vec3::ZERO);
            let wrist = global[support[2]].transform_point3(Vec3::ZERO);
            let length = shoulder.distance(global[support[1]].transform_point3(Vec3::ZERO))
                + global[support[1]]
                    .transform_point3(Vec3::ZERO)
                    .distance(wrist);
            let offset = Vec3::new(0.15, -0.55, 0.18).lerp(Vec3::new(0.25, -0.42, -0.25), release);
            reach(
                skeleton,
                &mut local,
                support,
                wrist.lerp(shoulder + offset * length, gather * settle),
                Vec3::new(-1., 0.2, -0.6),
            )?;
        }
        for &bone in &fingers {
            local[bone].rotation = Quat::from_array(local[bone].rotation)
                .slerp(
                    Quat::from_array(skeleton.bones[bone].bind.rotation),
                    gather * settle * 0.8,
                )
                .to_array();
        }
        for chain in &feet {
            let target = idle_global[chain[2]].transform_point3(Vec3::ZERO);
            reach(skeleton, &mut local, *chain, target, -Vec3::Y)?;
            let current = globals(skeleton, &local)?;
            let (_, from, _) = current[chain[2]].to_scale_rotation_translation();
            let (_, to, _) = idle_global[chain[2]].to_scale_rotation_translation();
            turn(skeleton, &mut local, chain[2], to * from.inverse())?;
        }
        frames.push(local);
    }
    let times = (0..=DURATION)
        .map(|tick| f32::from(tick) / 60.)
        .collect::<Vec<_>>();
    let motion = Motion {
        duration_frames: f32::from(DURATION) * 0.5,
        tracks: (0..skeleton.bones.len())
            .map(|bone| {
                let channel = |values| Channel {
                    interpolation: Interpolation::Linear,
                    times: times.clone(),
                    values,
                };
                Track {
                    bone: bone as u16,
                    nodes: vec![Arc::new(NodeTrack {
                        bind: skeleton.bones[bone].bind,
                        translation: Some(channel(
                            frames.iter().map(|f| f[bone].translation).collect(),
                        )),
                        rotation: Some(Channel {
                            interpolation: Interpolation::Linear,
                            times: times.clone(),
                            values: frames.iter().map(|f| f[bone].rotation).collect(),
                        }),
                        scale: Some(channel(frames.iter().map(|f| f[bone].scale).collect())),
                    })],
                }
            })
            .collect(),
    };
    motion.validate(skeleton)?;
    Ok((motion, emitter))
}

pub(super) fn performances(
    model: &mut ModelDefinition,
    setup: &ActorSetup,
    actions: &ActionDefinitions,
    style: FatalStyle,
    feedback: &crate::battle::feedback::Feedback,
) -> Result<[FatalPerformance; 3]> {
    let audio = setup
        .techniques
        .iter()
        .filter_map(|technique| feedback.casting.get(&technique.action))
        .next();
    let voice = setup
        .control
        .as_ref()
        .and_then(|control| actions.get(control.normals[NormalAttack::Neutral as usize].action))
        .and_then(|action| {
            let ActionExecution::Attack(attack) = &action.execution else {
                return None;
            };
            attack.events.iter().find_map(|(_, event)| match event {
                AttackEvent::Voice { sound, .. } => Some(*sound),
                _ => None,
            })
        });
    FatalColor::ALL
        .map(|color| {
            let (motion, emitter) = bake(model, style, color)?;
            let clip = (FIRST_CLIP..4_096)
                .find(|clip| !model.motions.contains_key(clip))
                .context("no free caster finisher motion slot")?;
            model.motions.insert(clip, Arc::new(motion));
            Ok(FatalPerformance {
                style,
                emitter: Some(emitter),
                poses: vec![(
                    0,
                    AttackPose {
                        motion: resonance_battle::MotionBinding {
                            model: model.resource,
                            clip,
                        },
                        // The player holds the incoming pose clock during its six
                        // blend visits. Start six authored ticks into the bake so
                        // its release still lands exactly on the gameplay impact.
                        start: 3.,
                        end: Some(f32::from(DURATION) * 0.5),
                        rate: 0.5,
                        repeat: false,
                        blend: 6,
                    },
                )],
                rates: Vec::new(),
                impact_at: 56,
                duration: DURATION,
                reach: 60.,
                contact: None,
                visual_height: None,
                voice,
                charge_sound: audio.and_then(|audio| audio.start_sound),
                impact_kind: resonance_battle::DamageKind::Magic,
                impact_sound: audio.and_then(|audio| audio.release_sound),
                impact_effect: None,
            })
        })
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid caster finisher count"))
}
