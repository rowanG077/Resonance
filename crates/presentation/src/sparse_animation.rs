//! Shared authored curves: loaded once, evaluated at the requested clip time.
pub(crate) mod affine;
use anyhow::{Context, Result, ensure};
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};
use resonance_content::animation::{FRAME_HZ, Motion, Transform as PoseTransform};
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[derive(Asset, TypePath, Debug)]
pub(super) struct Clip(pub Arc<Motion>);

/// Startup script queries and rendered skeletons share one decoded clip.
#[derive(Resource, Default, Clone)]
pub(super) struct Prepared(BTreeMap<String, Arc<Motion>>);
impl Prepared {
    pub fn load(&mut self, root: &Path, path: &str) -> Result<Arc<Motion>> {
        if let Some(motion) = self.0.get(path) {
            return Ok(motion.clone());
        }
        resonance_content::validate_asset_path(path)?;
        let motion = Arc::new(Motion::decode(&std::fs::read(root.join(path))?)?);
        self.0.insert(path.to_owned(), motion.clone());
        Ok(motion)
    }
}

#[derive(TypePath)]
struct Loader(Prepared);
impl FromWorld for Loader {
    fn from_world(world: &mut World) -> Self {
        Self(
            world
                .get_resource::<Prepared>()
                .cloned()
                .unwrap_or_default(),
        )
    }
}
impl AssetLoader for Loader {
    type Asset = Clip;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<Clip> {
        if let Some(motion) = self.0.0.get(
            context
                .path()
                .path()
                .to_str()
                .context("invalid clip path")?,
        ) {
            return Ok(Clip(motion.clone()));
        }
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let motion = Motion::decode(&bytes)?;
        Ok(Clip(Arc::new(motion)))
    }
    fn extensions(&self) -> &[&str] {
        &["motion"]
    }
}

pub(super) fn install(app: &mut App) {
    affine::install(app);
    app.init_asset::<Clip>().init_asset_loader::<Loader>();
}

/// Indexed model bindings retain separate transforms for body, outline and instances.
#[derive(Default)]
pub(super) struct Binding(pub Vec<(Entity, Transform)>);
impl Binding {
    pub fn new(
        root: Entity,
        bone_count: usize,
        children: &Query<&Children>,
        nodes: &Query<(&Transform, &bevy::gltf::GltfExtras)>,
    ) -> Result<Self> {
        let mut bones = vec![None; bone_count];
        for entity in children.iter_descendants(root) {
            let Ok((rest, extras)) = nodes.get(entity) else {
                continue;
            };
            let extras: serde_json::Value = serde_json::from_str(&extras.value)?;
            let Some(index) = extras.get("resonance_bone") else {
                continue;
            };
            let index = index.as_u64().context("invalid cooked bone index")? as usize;
            let bone = bones
                .get_mut(index)
                .context("cooked bone index exceeds skeleton")?;
            ensure!(
                bone.replace((entity, *rest)).is_none(),
                "duplicate cooked bone index"
            );
        }
        Ok(Self(
            bones
                .into_iter()
                .enumerate()
                .map(|(index, bone)| {
                    bone.with_context(|| format!("missing cooked animation bone {index}"))
                })
                .collect::<Result<_>>()?,
        ))
    }

    pub fn sample(
        &self,
        motion: &Motion,
        seconds: f32,
        nodes: &mut Query<&mut Transform>,
        affine: &mut affine::Locals,
    ) -> Result<()> {
        ensure!(
            seconds.is_finite() && seconds >= 0.,
            "invalid animation time"
        );
        let poses = sample(
            &self.0,
            motion,
            (seconds * FRAME_HZ).min(motion.duration_frames),
            None,
        )?;
        publish(&self.0, &poses, nodes, affine)
    }
}

pub(super) fn sample(
    bones: &[(Entity, Transform)],
    motion: &Motion,
    frame: f32,
    previous: Option<&[affine::Pose]>,
) -> Result<Vec<affine::Pose>> {
    use resonance_content::animation::pose::{BonePose, sample_bone};
    ensure!(
        motion
            .tracks
            .iter()
            .all(|track| usize::from(track.bone) < bones.len()),
        "animation bone outside bound model"
    );
    bones
        .iter()
        .enumerate()
        .map(|(index, &(_, rest))| {
            let bind = PoseTransform {
                translation: rest.translation.to_array(),
                rotation: rest.rotation.to_array(),
                scale: rest.scale.to_array(),
            };
            let track = motion
                .tracks
                .iter()
                .find(|track| usize::from(track.bone) == index);
            let mut sampled = sample_bone(bind, track, frame)?;
            if let Some(previous) = previous.and_then(|rows| rows.get(index)) {
                sampled.retain_unwritten(BonePose::complete(previous.native()));
            }
            Ok(sampled.value().into())
        })
        .collect()
}

pub(super) fn publish(
    bones: &[(Entity, Transform)],
    poses: &[affine::Pose],
    nodes: &mut Query<&mut Transform>,
    affine: &mut affine::Locals,
) -> Result<()> {
    for (&(entity, _), &pose) in bones.iter().zip(poses) {
        affine.set(entity, &mut *nodes.get_mut(entity)?, pose);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn joint_twist_uses_the_bind_axes_without_moving_its_origin() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let mut transform = Transform::from_xyz(4., 5., 6.)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2));
        let mut locals = affine::Locals::default();
        for affine_pose in [false, true] {
            let rest = transform;
            if affine_pose {
                locals.set(
                    entity,
                    &mut transform,
                    affine::Pose::Affine(rest.compute_affine()),
                );
            }
            locals.rotate_local(entity, &mut transform, Quat::from_rotation_x(0.5));
            let pose = locals.get(entity, transform).global();
            assert!(pose.translation().abs_diff_eq(rest.translation, 0.00001));
            assert!(
                pose.affine()
                    .transform_vector3(Vec3::X)
                    .abs_diff_eq(Vec3::Z, 0.00001)
            );
            transform = rest;
            locals = affine::Locals::default();
        }
    }

    #[test]
    fn indexed_binding_keeps_distinct_bones_with_repeated_names() {
        let mut world = World::new();
        let root = world.spawn_empty().id();
        let bones: Vec<_> = [1, 0]
            .into_iter()
            .map(|index| {
                world
                    .spawn((
                        Name::new("repeated"),
                        Transform::IDENTITY,
                        ChildOf(root),
                        bevy::gltf::GltfExtras {
                            value: format!("{{\"resonance_bone\":{index}}}"),
                        },
                    ))
                    .id()
            })
            .collect();
        let bound = world
            .run_system_once(
                move |children: Query<&Children>,
                      nodes: Query<(&Transform, &bevy::gltf::GltfExtras)>| {
                    Binding::new(root, 2, &children, &nodes)
                        .unwrap()
                        .0
                        .into_iter()
                        .map(|(entity, _)| entity)
                        .collect::<Vec<_>>()
                },
            )
            .unwrap();
        assert_eq!(bound, [bones[1], bones[0]]);
    }

    #[test]
    fn sparse_playback_evaluates_fractional_curves_and_restores_omitted_channels() {
        let motion: Motion = serde_json::from_value(serde_json::json!({
            "duration_frames":2., "tracks":[{"bone":0,"bind_channels":0,"period_frames":2.,"times":[0.,2.],
            "translation":{"interpolation":"bezier","values":[[0.,0.,0.],[8.,0.,0.]],
                "incoming":[[0.,0.,0.],[0.,0.,0.]],"outgoing":[[0.,4.,0.],[0.,0.,0.]]},
            "scale":null,"rotation":null}]
        }))
        .unwrap();
        motion.validate_bones(1).unwrap();
        assert_eq!(motion.tracks[0].times.len(), 2);
        let expected = motion.tracks[0]
            .sample(0.125, PoseTransform::default())
            .unwrap();
        let mut world = World::new();
        let bone = world
            .spawn(Transform::from_xyz(99., 99., 99.).with_scale(Vec3::splat(5.)))
            .id();
        let binding = Binding(vec![(bone, Transform::IDENTITY)]);
        world
            .run_system_once(move |mut nodes: Query<&mut Transform>| {
                binding
                    .sample(
                        &motion,
                        0.125 / FRAME_HZ,
                        &mut nodes,
                        &mut affine::Locals::default(),
                    )
                    .unwrap();
            })
            .unwrap();
        let actual = world.get::<Transform>(bone).unwrap();
        assert!(
            actual
                .translation
                .abs_diff_eq(Vec3::from_array(expected.translation), 0.000001)
        );
        assert_eq!(actual.scale, Vec3::ONE);
        assert_eq!(actual.rotation, Quat::IDENTITY);
    }
}
