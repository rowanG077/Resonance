//! Disposable preview animation and dynamics, after the committed solver step.
use super::*;
use crate::sparse_animation::affine::{Helper, Locals, Pose};

#[allow(clippy::type_complexity)]
pub(super) fn present(
    state: State,
    viewer: Res<Viewer>,
    clips: Res<Assets<crate::sparse_animation::Clip>>,
    mut prediction: Option<ResMut<crate::field_view::Prediction>>,
    rigs: Query<&crate::secondary_motion::Rig>,
    mut transforms: ParamSet<(Helper, (Query<&mut Transform>, ResMut<Locals>))>,
    mut surfaces: ResMut<Assets<Surface>>,
) {
    let Some(menu) = state.menu() else {
        return;
    };
    let Some(current) = menu.preview() else {
        return;
    };
    let Some(prediction) = prediction.as_deref_mut() else {
        return;
    };
    if !viewer.ready {
        return;
    }
    let helper = transforms.p0();
    let result = prediction.scratch_scene(|scene, alpha| -> Result<_> {
        let Some(menu) = scene.menu() else {
            return Ok(None);
        };
        let Some(next) = menu.preview().filter(|p| p.id == current.id) else {
            return Ok(None);
        };
        let overrides = evaluate_behavior(viewer.behavior.as_ref(), menu)?;
        let mut locals = BTreeMap::new();
        let mut authored = Locals::default();
        for (index, part) in viewer.parts.iter().enumerate().filter(|(_, p)| p.ready) {
            let Some(root) = part.root else {
                continue;
            };
            let transform = if part.spec.attached_to.is_some() {
                Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
            } else {
                Transform::from_translation(preview_translation(&overrides, next.model.elevation))
                    .with_rotation(Quat::from_rotation_z(next.yaw.to_radians()))
                    .with_scale(Vec3::splat(next.model.scale))
            };
            locals.insert(root, transform.into());
            let animation = part.clip.and_then(|i| {
                clips.get(&part.clips[i]).map(|c| {
                    (
                        &c.0,
                        next.sample(part.spec.scene.clips[i].duration_ticks()) as f32,
                    )
                })
            });
            for (i, &(entity, rest)) in part.binding.0.iter().enumerate() {
                let pose = animation
                    .and_then(|(m, t)| {
                        m.tracks
                            .iter()
                            .find(|track| usize::from(track.bone) == i)
                            .map(|track| (track, t))
                    })
                    .map_or(Ok(rest.into()), |(track, t)| {
                        crate::sparse_animation::sample_track(track, t, rest)
                    })?;
                let mut transform = rest;
                authored.set(entity, &mut transform, pose);
                for (node, scale) in &overrides.scales {
                    if usize::from(node.part) == index && usize::from(node.bone) == i {
                        authored.scale(entity, &mut transform, Vec3::from_array(*scale));
                    }
                }
                locals.insert(entity, authored.get(entity, transform));
            }
        }
        let empty = BTreeMap::new();
        let mut dynamics = BTreeMap::new();
        if let Some(primary) = viewer.parts.first().filter(|p| p.ready)
            && let Some(root) = primary.root
            && let Ok(rig) = rigs.get(root)
        {
            let roots = primary.clip.map_or(&[][..], |i| {
                primary.spec.scene.clips[i].secondary_pose_nodes.as_slice()
            });
            let mut actor = resonance_events::Actor::new(0, [0.; 3]);
            actor.heading = next.yaw;
            if let Some(pose) = rig.predict(
                &actor,
                menu.tick,
                Vec3::splat(next.model.scale),
                roots,
                |e| {
                    Some((
                        helper
                            .predicted_global(e, &locals, &authored, &empty)
                            .ok()?,
                        helper.predicted_has_affine(e, &locals),
                    ))
                },
            ) {
                for part in viewer
                    .parts
                    .iter()
                    .filter(|p| p.ready && p.spec.attached_to.is_none())
                {
                    for (&(source, _), &(target, _)) in
                        primary.binding.0.iter().zip(&part.binding.0)
                    {
                        if let Some(pose) = pose.get(&source) {
                            dynamics.insert(target, *pose);
                        }
                    }
                }
            }
        }
        let mut output = Vec::new();
        let mut current_cache = bevy::ecs::entity::EntityHashMap::default();
        let mut future_cache = bevy::ecs::entity::EntityHashMap::default();
        for &entity in locals.keys() {
            if !helper.requires_world_interpolation(entity)
                && !dynamics.contains_key(&entity)
                && !authored.has_translation(entity)
            {
                output.push((
                    entity,
                    helper.local(entity)?.interpolate(locals[&entity], alpha),
                    false,
                ));
                continue;
            }
            let from = helper.compute_global_cached(entity, &mut current_cache)?;
            let to = helper.predicted_global_cached(
                entity,
                &locals,
                &authored,
                &dynamics,
                &mut future_cache,
            )?;
            let parents = helper.parent(entity).and_then(|p| {
                Some((
                    helper.compute_global_cached(p, &mut current_cache).ok()?,
                    helper
                        .predicted_global_cached(
                            p,
                            &locals,
                            &authored,
                            &dynamics,
                            &mut future_cache,
                        )
                        .ok()?,
                ))
            });
            let (from, to, global) = if let Some((a, b)) = parents.filter(|(a, b)| {
                a.affine().matrix3.determinant().abs() > 1e-12
                    && b.affine().matrix3.determinant().abs() > 1e-12
            }) {
                (
                    Pose::Affine(a.affine().inverse() * from.affine()),
                    Pose::Affine(b.affine().inverse() * to.affine()),
                    false,
                )
            } else {
                (Pose::Affine(from.affine()), Pose::Affine(to.affine()), true)
            };
            output.push((entity, from.interpolate(to, alpha), global));
        }
        let a = preview_camera(&current);
        let b = preview_camera(&next);
        let camera = Transform {
            translation: a.translation.lerp(b.translation, alpha),
            rotation: a.rotation.slerp(b.rotation, alpha),
            ..a
        };
        let opacity = (f32::from(current.opacity)
            + (f32::from(next.opacity) - f32::from(current.opacity)) * alpha)
            / 255.;
        Ok(Some((output, camera, opacity)))
    });
    let result = result.transpose();
    let output = match result {
        Ok(Some(Some(output))) => output,
        Ok(_) => return,
        Err(e) => {
            prediction.report_error(&e);
            return;
        }
    };
    let (output, camera, opacity) = output;
    let (mut nodes, mut affine) = transforms.p1();
    for &(entity, pose, global) in &output {
        if let Ok(mut transform) = nodes.get_mut(entity) {
            affine.replace_for_render(entity, &mut transform, pose);
            if global {
                affine.set_world(entity, pose.global());
            }
        }
    }
    if let Ok(mut transform) = nodes.get_mut(viewer.camera) {
        *transform = camera;
    }
    for part in viewer.parts.iter().filter(|p| p.ready) {
        for (i, handle) in part.materials.iter().enumerate() {
            if let Some(mut s) = surfaces.get_mut(handle) {
                s.base.tint.w = opacity
                    * part
                        .spec
                        .scene
                        .outline_color_for(&part.spec.scene.materials[i])
                        .map_or(1., |c| f32::from(c[3]) / 255.);
            }
        }
    }
}
