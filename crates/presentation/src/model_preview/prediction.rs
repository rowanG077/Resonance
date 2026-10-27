//! Disposable preview animation and dynamics, after the committed solver step.
use super::*;
use crate::sparse_animation::affine::{Helper, Locals, PoseEndpoint};

#[derive(Default)]
pub(super) struct Cache {
    key: Option<(u64, Entity)>,
    poses: Vec<PoseEndpoint>,
}

#[allow(clippy::type_complexity)]
pub(super) fn present(
    state: State,
    mut viewer: ResMut<Viewer>,
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
    if viewer.status != super::PreviewStatus::Ready {
        return;
    }
    let Some(next) = prediction
        .menu()
        .and_then(|m| m.preview())
        .filter(|p| p.id == current.id)
    else {
        return;
    };
    let (a, b) = (preview_camera(&current), preview_camera(&next));
    let alpha = prediction.alpha;
    let camera = Transform {
        translation: a.translation.lerp(b.translation, alpha),
        rotation: a.rotation.slerp(b.rotation, alpha),
        ..a
    };
    let opacity = (f32::from(current.opacity)
        + (f32::from(next.opacity) - f32::from(current.opacity)) * alpha)
        / 255.;
    let Some(root) = viewer.parts.first().and_then(|part| part.root) else {
        return;
    };
    // A rebuilt preview has new entities even if the simulation endpoint is unchanged.
    let key = (prediction.counts()[0], root);
    if viewer.prediction.key != Some(key) {
        let helper = transforms.p0();
        let result = prediction.scratch_scene(|scene, _| -> Result<Vec<PoseEndpoint>> {
            let menu = scene.menu().context("preview prediction lost its menu")?;
            let next = menu
                .preview()
                .context("preview prediction lost its model")?;
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
                    Transform::from_translation(preview_translation(
                        &overrides,
                        next.model.elevation,
                    ))
                    .with_rotation(Quat::from_rotation_z(next.yaw.to_radians()))
                    .with_scale(Vec3::splat(next.model.scale))
                };
                locals.insert(root, transform.into());
                let mut poses = part
                    .binding
                    .sample_poses(part.animation_sample(&next, &clips)?)?;
                for (node, scale) in &overrides.scales {
                    if usize::from(node.part) != index {
                        continue;
                    }
                    let i = usize::from(node.bone);
                    let &(entity, rest) = part
                        .binding
                        .0
                        .get(i)
                        .context("behavior references a missing preview bone")?;
                    let mut transform = rest;
                    authored.set(entity, &mut transform, poses[i]);
                    authored.scale(entity, &mut transform, Vec3::from_array(*scale));
                    poses[i] = authored.get(entity, transform);
                }
                locals.extend(
                    part.binding
                        .0
                        .iter()
                        .zip(poses)
                        .map(|(&(entity, _), pose)| (entity, pose)),
                );
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
            Ok(helper
                .pose_endpoints(locals.keys().copied(), &locals, &authored, &dynamics)
                .collect::<Result<Vec<_>, _>>()?)
        });
        let output = match result.transpose() {
            Ok(Some(output)) => output,
            Ok(None) => return,
            Err(e) => {
                prediction.report_error(&e);
                return;
            }
        };
        // Copy out of scratch storage before retaining the cache across renders.
        viewer.prediction.poses.clone_from(&output);
        viewer.prediction.key = Some(key);
    }
    let (mut nodes, mut affine) = transforms.p1();
    for &endpoint in &viewer.prediction.poses {
        endpoint.apply(alpha, &mut nodes, &mut affine);
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
