//! Character-specific finisher curves, composed on the admitted rig. Hand and
//! foot paths are authored in rig space; contact is measured from the actual
//! equipped artwork after the pose has been solved, never from nominal reach.
use super::rig::{ease, globals, joint, reach, sample, turn};
use super::*;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use resonance_battle::{DamageKind, MotionBinding, PreparedAttack, WeaponDefinition};
use resonance_content::animation::{Channel, Interpolation, Motion, NodeTrack, Transform};

const DURATION: u16 = 112;

fn source<'a>(
    model: &ModelDefinition,
    setup: &ActorSetup,
    actions: &'a ActionDefinitions,
    color: FatalColor,
) -> Result<(&'a PreparedAttack, AttackPose, f32)> {
    let control = setup
        .control
        .as_ref()
        .context("finisher requires normal bindings")?;
    let choices = match color {
        FatalColor::Blue => [
            NormalAttack::Rising,
            NormalAttack::Neutral,
            NormalAttack::Thrust,
        ],
        FatalColor::Red => [
            NormalAttack::Finisher,
            NormalAttack::Low,
            NormalAttack::Neutral,
        ],
        FatalColor::Green => [
            NormalAttack::Thrust,
            NormalAttack::Neutral,
            NormalAttack::Low,
        ],
    };
    let (attack, pose) = choices
        .into_iter()
        .find_map(|normal| {
            let ActionExecution::Attack(attack) = &actions
                .get(control.normals[normal as usize].action)?
                .execution
            else {
                return None;
            };
            let pose = attack.opening.or_else(|| {
                attack.events.iter().find_map(|(_, event)| {
                    if let AttackEvent::Pose(pose) = event {
                        Some(*pose)
                    } else {
                        None
                    }
                })
            })?;
            Some((attack, pose))
        })
        .context("finisher requires a prepared source performance")?;
    let duration = model
        .motions
        .get(&pose.motion.clip)
        .context("missing source motion")?
        .duration_frames;
    let impact = attack
        .events
        .iter()
        .find_map(|(at, event)| {
            let at = match event {
                AttackEvent::Contact { duration, .. } => {
                    f32::from(*at) + f32::from(*duration) * 0.5
                }
                AttackEvent::Throw(_) => f32::from(*at),
                _ => return None,
            };
            Some(pose.start + at * pose.rate)
        })
        .unwrap_or(duration * 0.55)
        .clamp(pose.start, duration);
    Ok((attack, pose, impact))
}

fn curve(keys: &[(f32, Vec3)], time: f32) -> Vec3 {
    let pair = keys.windows(2).find(|p| time <= p[1].0).unwrap();
    pair[0].1.lerp(
        pair[1].1,
        ease((time - pair[0].0) / (pair[1].0 - pair[0].0)),
    )
}

/// The equipped contact markers belong to the primary layer, in attachment
/// space. All seven physical fighters currently carry rigid equipment.
fn marker(weapon: &WeaponDefinition) -> Result<Vec3> {
    let layer = weapon.primary_layer();
    ensure!(
        matches!(layer.playback, resonance_battle::WeaponPlayback::Rigid),
        "finisher contact needs a baked equipment attachment"
    );
    let bind = layer.skeleton.bind_pose()?;
    let points = layer
        .skeleton
        .bones
        .iter()
        .enumerate()
        .filter(|(_, b)| b.name.to_ascii_lowercase().starts_with("at"))
        .map(|(i, _)| Mat4::from_cols_array_2d(&bind.global[i]).transform_point3(Vec3::ZERO))
        .collect::<Vec<_>>();
    ensure!(
        !points.is_empty(),
        "finisher equipment has no contact markers"
    );
    Ok(points.iter().copied().sum::<Vec3>() / points.len() as f32)
}

fn side_of(model: &ModelDefinition, weapon: &WeaponDefinition) -> Result<&'static str> {
    let mut bone = weapon.attachment;
    while let Some(index) = bone {
        let b = &model.skeleton.bones[usize::from(index)];
        if b.name.contains("left") {
            return Ok("left");
        }
        if b.name.contains("right") {
            return Ok("right");
        }
        bone = b.parent;
    }
    anyhow::bail!("equipment is not attached to a limb")
}

fn contact(
    model: &ModelDefinition,
    local: &[Transform],
    weapon: &WeaponDefinition,
) -> Result<Vec3> {
    let global = globals(&model.skeleton, local)?;
    Ok(
        global[usize::from(weapon.attachment.context("unbound finisher weapon")?)]
            .transform_point3(marker(weapon)?),
    )
}

/// These silhouettes differ by fighting style as well as strike direction.
/// Offsets are measured in arm lengths from the shoulder, in Z-up/-Y-forward.
fn hand_path(style: FatalStyle, color: FatalColor, side: &str, time: f32) -> Vec3 {
    // The hands finish their throw at tick 44; the independent chakram flight
    // then has twelve readable ticks to reach the victim at tick 56.
    let time = if style == FatalStyle::Chakram {
        (time + 12.).min(112.)
    } else {
        time
    };
    let s = if side == "left" { 1. } else { -1. };
    let (wind, hit, follow) = match (style, color) {
        (FatalStyle::TwinBlades, FatalColor::Blue) => (
            [s * 0.6, 0.18, -0.65],
            [s * 0.12, -0.78, -0.30],
            [-s * 0.25, -0.40, 0.68],
        ),
        (FatalStyle::TwinBlades, FatalColor::Red) => (
            [s * 0.28, 0.10, 0.82],
            [-s * 0.12, -0.74, -0.28],
            [s * 0.6, -0.12, -0.68],
        ),
        (FatalStyle::TwinBlades, FatalColor::Green) => (
            [s * 0.42, 0.38, -0.04],
            [s * 0.14, -0.94, -0.08],
            [s * 0.34, -0.72, -0.20],
        ),
        (FatalStyle::Chakram, FatalColor::Blue) => (
            [s * 0.85, 0.10, -0.18],
            [s * 0.26, -0.76, 0.30],
            [s * 0.75, -0.04, 0.48],
        ),
        (FatalStyle::Chakram, FatalColor::Red) => (
            [s * 0.5, 0.15, 0.72],
            [s * 0.18, -0.82, -0.24],
            [s * 0.6, -0.24, -0.55],
        ),
        (FatalStyle::Chakram, FatalColor::Green) => (
            [s * 0.85, 0.02, 0.10],
            [s * 0.18, -0.93, 0.02],
            [s * 0.70, -0.28, 0.02],
        ),
        (FatalStyle::Seals, FatalColor::Blue) => (
            [s * 0.34, 0.12, -0.48],
            [s * 0.12, -0.82, 0.26],
            [s * 0.18, -0.42, 0.60],
        ),
        (FatalStyle::Seals, FatalColor::Red) => (
            [s * 0.38, 0.04, 0.66],
            [s * 0.10, -0.86, -0.27],
            [s * 0.36, -0.28, -0.55],
        ),
        (FatalStyle::Seals, FatalColor::Green) => (
            [s * 0.48, 0.20, -0.10],
            [s * 0.06, -0.96, -0.05],
            [s * 0.23, -0.70, -0.12],
        ),
        (FatalStyle::Spellblade, FatalColor::Blue) => (
            [s * 0.82, 0.13, -0.30],
            [s * 0.08, -0.80, -0.20],
            [-s * 0.28, -0.35, 0.72],
        ),
        (FatalStyle::Spellblade, FatalColor::Red) => (
            [s * 0.5, 0.20, 0.74],
            [s * 0.04, -0.82, -0.24],
            [s * 0.72, 0.04, -0.60],
        ),
        (FatalStyle::Spellblade, FatalColor::Green) => (
            [s * 0.55, 0.22, 0.06],
            [s * 0.12, -0.94, 0.00],
            [s * 0.35, -0.68, -0.12],
        ),
        (FatalStyle::HeavyAxe, FatalColor::Blue) => (
            [s * 0.48, 0.15, -0.68],
            [s * 0.15, -0.72, -0.24],
            [s * 0.08, -0.33, 0.76],
        ),
        (FatalStyle::HeavyAxe, FatalColor::Red) => (
            [s * 0.28, 0.10, 0.88],
            [s * 0.12, -0.74, -0.42],
            [s * 0.20, -0.28, -0.78],
        ),
        (FatalStyle::HeavyAxe, FatalColor::Green) => (
            [s * 0.80, 0.22, -0.12],
            [s * 0.10, -0.86, -0.18],
            [-s * 0.52, -0.35, -0.30],
        ),
        (FatalStyle::Seraph, FatalColor::Blue) => (
            [s * 0.28, 0.05, -0.55],
            [s * 0.08, -0.80, -0.20],
            [s * 0.12, -0.44, 0.55],
        ),
        (FatalStyle::Seraph, FatalColor::Red) => (
            [s * 0.16, 0.02, 0.78],
            [s * 0.05, -0.82, -0.20],
            [s * 0.18, -0.34, -0.62],
        ),
        (FatalStyle::Seraph, FatalColor::Green) => (
            [s * 0.18, 0.18, -0.02],
            [s * 0.07, -0.96, 0.02],
            [s * 0.13, -0.70, -0.03],
        ),
        _ => unreachable!(),
    };
    curve(
        &[
            (0., Vec3::from_array(wind)),
            (40., Vec3::from_array(wind) * 0.86),
            (46., Vec3::from_array(wind)),
            (56., Vec3::from_array(hit)),
            (64., Vec3::from_array(hit)),
            (78., Vec3::from_array(follow)),
            (112., Vec3::from_array(follow)),
        ],
        time,
    )
}

fn blade_path(color: FatalColor, side: &str, time: f32) -> Vec3 {
    let s = if side == "left" { 1. } else { -1. };
    let (wind, hit, follow) = match color {
        FatalColor::Blue => (
            [s * 0.7, 0.7, -0.1],
            [s * 0.08, -1., 0.28],
            [-s * 0.18, -0.12, 1.],
        ),
        FatalColor::Red => (
            [s * 0.18, 0.05, 1.],
            [s * 0.08, -1., -0.28],
            [s * 0.18, 0.12, -1.],
        ),
        FatalColor::Green => (
            [s * 0.35, 0.10, 0.65],
            [s * 0.03, -1., 0.],
            [s * 0.08, -1., -0.08],
        ),
    };
    curve(
        &[
            (0., Vec3::from_array(wind)),
            (46., Vec3::from_array(wind)),
            (56., Vec3::from_array(hit)),
            (64., Vec3::from_array(hit)),
            (78., Vec3::from_array(follow)),
            (112., Vec3::from_array(follow)),
        ],
        time,
    )
    .normalize()
}

fn bake(
    model: &ModelDefinition,
    style: FatalStyle,
    color: FatalColor,
    source: AttackPose,
    source_impact: f32,
    head: resonance_battle::HeadAnchor,
) -> Result<(Motion, Vec3, Option<u16>, f32)> {
    let rig = &model.skeleton;
    let idle = sample(
        model,
        model.idle_motions[0].context("finisher requires idle")?,
        0.2,
    )?;
    let idle_global = globals(rig, &idle)?;
    let hips = rig
        .bones
        .iter()
        .position(|b| b.name.ends_with("-hips"))
        .context("missing hips")?;
    let chest = joint(rig, "spine-02")?;
    let source_motion = &model.motions[&source.motion.clip];
    let duration = source_motion.duration_frames;
    let keys = [
        (0., idle.clone()),
        (20., idle.clone()),
        (
            40.,
            rig.sample(source_motion, (source_impact - 5.).max(source.start))?
                .local,
        ),
        (
            46.,
            rig.sample(source_motion, (source_impact - 4.).max(source.start))?
                .local,
        ),
        (56., rig.sample(source_motion, source_impact)?.local),
        (
            64.,
            rig.sample(source_motion, (source_impact + 1.).min(duration))?
                .local,
        ),
        (80., rig.sample(source_motion, duration)?.local),
        (112., idle.clone()),
    ];
    let legs = ["left", "right"]
        .map(|side| -> Result<_> {
            Ok([
                joint(rig, &format!("leg-01-{side}"))?,
                joint(rig, &format!("leg-02-{side}"))?,
                joint(rig, &format!("leg-03-{side}"))?,
            ])
        })
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let arms = ["left", "right"]
        .map(|side| -> Result<_> {
            Ok([
                joint(rig, &format!("arm-01-{side}"))?,
                joint(rig, &format!("arm-03-{side}"))?,
                joint(rig, &format!("wrist-{side}"))?,
            ])
        })
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let weapons = model
        .weapons
        .iter()
        .filter(|w| w.slot.is_some_and(|s| s.is_primary()))
        .collect::<Vec<_>>();
    let primary = weapons
        .iter()
        .find(|w| w.slot == Some(resonance_battle::WeaponSlot(0)))
        .context("missing primary equipment")?;
    let active_side = side_of(model, primary)?;
    let mut frames = Vec::new();
    for tick in 0..=DURATION {
        let time = f32::from(tick);
        let pair = keys.windows(2).find(|p| time <= p[1].0).unwrap();
        let weight = ease((time - pair[0].0) / (pair[1].0 - pair[0].0));
        let mut local = pair[0]
            .1
            .iter()
            .zip(&pair[1].1)
            .map(|(a, b)| a.blend(*b, weight))
            .collect::<Vec<_>>();
        let gather = ease(time / 28.);
        let strike = ease((time - 46.) / 10.);
        let recover = 1. - ease((time - 78.) / 34.);
        let envelope = gather * recover;
        for (i, bone) in rig.bones.iter().enumerate() {
            // Native attack clips couple their spine bend to a stepping pelvis.
            // Keep only their limb/grip references. The authored spine, gaze
            // and pelvis must stay coherent with the planted lower body.
            let limb = ["arm-", "hand-", "wrist-", "finger-"]
                .iter()
                .any(|name| bone.name.contains(name))
                || bone.name.starts_with("kk");
            if !limb || bone.parent.is_none() || i == hips {
                local[i] = idle[i];
            }
        }
        let weight = if style == FatalStyle::HeavyAxe {
            15.
        } else if style == FatalStyle::TwinBlades {
            10.
        } else if style == FatalStyle::Martial {
            4.
        } else {
            7.
        };
        local[hips].translation[2] -=
            (gather * (1. - strike) * weight + strike * weight * 0.4) * recover;
        let twist = match style {
            FatalStyle::TwinBlades => 0.20,
            FatalStyle::Spellblade => 0.32,
            FatalStyle::HeavyAxe => 0.30,
            FatalStyle::Martial => 0.34,
            FatalStyle::Seraph => 0.12,
            _ => 0.18,
        };
        let torso_twist = (-twist + strike * twist * 1.8) * envelope;
        let lean = (0.1
            + strike
                * if style == FatalStyle::HeavyAxe {
                    -0.02
                } else {
                    -0.06
                })
            * envelope;
        turn(
            rig,
            &mut local,
            chest,
            Quat::from_rotation_z(torso_twist) * Quat::from_rotation_x(lean),
        )?;
        turn(
            rig,
            &mut local,
            joint(rig, "head")?,
            Quat::from_rotation_z(-torso_twist * 0.7) * Quat::from_rotation_x(-lean * 0.5),
        )?;
        if style == FatalStyle::Martial {
            // Regal's chained hands remain in his native guard. Only the active
            // leg releases; the supporting foot carries the entire weight shift.
            let active = if active_side == "left" { 0 } else { 1 };
            let chain = legs[active];
            let hip = idle_global[chain[0]].transform_point3(Vec3::ZERO);
            let knee = idle_global[chain[1]].transform_point3(Vec3::ZERO);
            let foot = idle_global[chain[2]].transform_point3(Vec3::ZERO);
            let length = hip.distance(knee) + knee.distance(foot);
            let chamber = match color {
                FatalColor::Blue => [0., -0.36, -0.30],
                FatalColor::Red => [0.10, -0.18, 0.12],
                FatalColor::Green => [0.08, -0.34, -0.34],
            };
            let hit = match color {
                FatalColor::Blue => [0.02, -0.78, -0.08],
                FatalColor::Red => [0.02, -0.76, -0.32],
                FatalColor::Green => [0.02, -0.94, -0.12],
            };
            let offset = curve(
                &[
                    (0., Vec3::from_array(chamber)),
                    (46., Vec3::from_array(chamber)),
                    (56., Vec3::from_array(hit)),
                    (62., Vec3::from_array(hit)),
                    (78., Vec3::from_array(chamber)),
                    (112., Vec3::from_array(chamber)),
                ],
                time,
            );
            reach(
                rig,
                &mut local,
                chain,
                foot.lerp(hip + offset * length, envelope),
                Vec3::new(-0.3, -1., 0.3),
            )?;
            let global = globals(rig, &local)?;
            let (_, q, _) = global[chain[2]].to_scale_rotation_translation();
            let (_, idle_q, _) = idle_global[chain[2]].to_scale_rotation_translation();
            turn(rig, &mut local, chain[2], idle_q * q.inverse())?;
        } else {
            for (side, chain) in ["left", "right"].into_iter().zip(&arms) {
                let global = globals(rig, &local)?;
                let shoulder = global[chain[0]].transform_point3(Vec3::ZERO);
                let wrist = global[chain[2]].transform_point3(Vec3::ZERO);
                let length = shoulder.distance(global[chain[1]].transform_point3(Vec3::ZERO))
                    + global[chain[1]]
                        .transform_point3(Vec3::ZERO)
                        .distance(wrist);
                let support =
                    matches!(style, FatalStyle::Spellblade | FatalStyle::Seraph) && side == "left";
                let path = if support {
                    Vec3::new(0.25, -0.62, -0.18)
                } else {
                    hand_path(style, color, side, time)
                };
                // Preserve a wrist's grip while aiming the blade. Shields follow
                // the bracing forearm and are never treated as a second sword.
                let weapon = weapons
                    .iter()
                    .find(|w| side_of(model, w).ok() == Some(side));
                let desired_rotation = if let Some(weapon) = weapon.filter(|_| !support) {
                    let point = contact(model, &local, weapon)?;
                    let (_, q, _) = global[chain[2]].to_scale_rotation_translation();
                    Some(
                        Quat::from_rotation_arc(
                            (point - wrist).normalize(),
                            blade_path(
                                color,
                                side,
                                if style == FatalStyle::Chakram {
                                    (time + 12.).min(112.)
                                } else {
                                    time
                                },
                            ),
                        ) * q,
                    )
                } else {
                    None
                };
                reach(
                    rig,
                    &mut local,
                    *chain,
                    wrist.lerp(shoulder + path * length, envelope),
                    Vec3::new(if side == "left" { 1. } else { -1. }, 0.2, -0.65),
                )?;
                if let Some(rotation) = desired_rotation {
                    let global = globals(rig, &local)?;
                    let (_, current, _) = global[chain[2]].to_scale_rotation_translation();
                    turn(
                        rig,
                        &mut local,
                        chain[2],
                        current.slerp(rotation, envelope) * current.inverse(),
                    )?;
                }
            }
        }
        for (i, chain) in legs.iter().enumerate() {
            if style == FatalStyle::Martial && i == if active_side == "left" { 0 } else { 1 } {
                continue;
            }
            reach(
                rig,
                &mut local,
                *chain,
                idle_global[chain[2]].transform_point3(Vec3::ZERO),
                -Vec3::Y,
            )?;
            let global = globals(rig, &local)?;
            let (_, a, _) = global[chain[2]].to_scale_rotation_translation();
            let (_, b, _) = idle_global[chain[2]].to_scale_rotation_translation();
            turn(rig, &mut local, chain[2], b * a.inverse())?;
        }
        frames.push(local);
    }
    let contact = contact(model, &frames[56], primary)?;
    // Include the actual equipped blade/shaft markers throughout recovery.
    // A body collider alone cannot frame raised swords or a large axe.
    let bounds = weapons
        .iter()
        .map(|weapon| -> Result<_> {
            let layer = weapon.primary_layer();
            let pose = layer.skeleton.bind_pose()?;
            let points = layer
                .skeleton
                .bones
                .iter()
                .enumerate()
                .filter(|(_, b)| b.name.starts_with("ki") || b.name.starts_with("at"))
                .map(|(i, _)| {
                    Mat4::from_cols_array_2d(&pose.global[i]).transform_point3(Vec3::ZERO)
                })
                .collect::<Vec<_>>();
            Ok((usize::from(weapon.attachment.unwrap()), points))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut visual_height = 1_f32;
    for local in &frames {
        let pose = globals(rig, local)?;
        visual_height = visual_height
            .max(pose[usize::from(head.bone)].transform_point3(Vec3::ZERO).z + head.offset[1]);
        for (bone, matrix) in rig.bones.iter().zip(&pose) {
            if ["hand-", "wrist-", "finger-"]
                .iter()
                .any(|name| bone.name.starts_with(name))
            {
                visual_height = visual_height.max(matrix.transform_point3(Vec3::ZERO).z + 6.);
            }
        }
        for (bone, points) in &bounds {
            for point in points {
                visual_height = visual_height.max(pose[*bone].transform_point3(*point).z);
            }
        }
    }
    let emitter = if style == FatalStyle::Seals {
        Some(joint(rig, &format!("hand-{active_side}"))? as u16)
    } else {
        None
    };
    let times = (0..=DURATION)
        .map(|tick| f32::from(tick) / 60.)
        .collect::<Vec<_>>();
    let motion = Motion {
        duration_frames: 56.,
        tracks: (0..rig.bones.len())
            .map(|bone| {
                let channel = |values| Channel {
                    interpolation: Interpolation::Linear,
                    times: times.clone(),
                    values,
                };
                resonance_content::animation::Track {
                    bone: bone as u16,
                    nodes: vec![Arc::new(NodeTrack {
                        bind: rig.bones[bone].bind,
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
    motion.validate(rig)?;
    Ok((motion, contact, emitter, visual_height))
}

pub(super) fn performances(
    model: &mut ModelDefinition,
    setup: &ActorSetup,
    actions: &ActionDefinitions,
    style: FatalStyle,
    head: resonance_battle::HeadAnchor,
) -> Result<[FatalPerformance; 3]> {
    FatalColor::ALL
        .map(|color| {
            let (attack, source, impact) = source(model, setup, actions, color)?;
            let voice = attack.events.iter().find_map(|(_, e)| {
                if let AttackEvent::Voice { sound, .. } = e {
                    Some(*sound)
                } else {
                    None
                }
            });
            let sound = attack.events.iter().find_map(|(_, e)| {
                if let AttackEvent::Sound(sound) = e {
                    *sound
                } else {
                    None
                }
            });
            let (motion, point, emitter, visual_height) =
                bake(model, style, color, source, impact, head)?;
            let clip = (1000..4096)
                .find(|clip| !model.motions.contains_key(clip))
                .context("no free finisher motion slot")?;
            model.motions.insert(clip, Arc::new(motion));
            Ok(FatalPerformance {
                style,
                emitter,
                poses: vec![(
                    0,
                    AttackPose {
                        motion: MotionBinding {
                            model: model.resource,
                            clip,
                        },
                        start: 3.,
                        end: Some(56.),
                        rate: 0.5,
                        repeat: false,
                        blend: 6,
                    },
                )],
                rates: Vec::new(),
                impact_at: 56,
                duration: DURATION,
                reach: (-point.y).clamp(20., 300.),
                contact: Some(point.to_array()),
                visual_height: Some(visual_height),
                voice,
                charge_sound: None,
                impact_kind: if style == FatalStyle::Martial {
                    DamageKind::Thrust
                } else {
                    DamageKind::Slash
                },
                impact_sound: sound,
                impact_effect: None,
            })
        })
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid finisher count"))
}
