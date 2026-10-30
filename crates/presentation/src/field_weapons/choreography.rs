//! Transfer the prop's deformation, leaving equipment's own motion intact.
use super::*;
use crate::{
    secondary_motion::Deformation,
    sparse_animation::affine::{Helper, Pose},
};
use bevy::math::Affine3A;
use resonance_content::ScenePart;
use std::sync::Arc;

/// Exact joints retain articulated props (notably Genis's ball). Other joints
/// follow the corresponding body/outline draw, including independent effects.
fn bone_map(
    source: &ScenePart,
    target: &ScenePart,
    parents: &[Option<usize>],
    additive: bool,
) -> Vec<u16> {
    let body = draw_bone(source, false);
    let outline = draw_bone(source, true);
    let mut mapped: Vec<Option<u16>> = target
        .bone_names
        .iter()
        .map(|name| {
            let outline_name = format!("{name}_outline");
            let out_name = format!("{name}_out");
            let names: &[&str] = if target.outline_color.is_some() {
                &[&outline_name, &out_name, name]
            } else {
                &[name]
            };
            names.iter().find_map(|name| {
                source
                    .bone_names
                    .iter()
                    .position(|n| n == name)
                    .map(|i| i as u16)
            })
        })
        .collect();
    for (material, nodes) in target.material_nodes.iter().enumerate() {
        let fallback =
            if target.outline_color.is_some() || (!additive && target.materials[material].blend) {
                outline
            } else {
                body
            };
        for &node in nodes {
            mapped[usize::from(node)].get_or_insert(fallback);
        }
    }
    (0..mapped.len())
        .map(|index| {
            std::iter::successors(Some(index), |&i| parents[i])
                .find_map(|i| mapped[i])
                .unwrap_or(body)
        })
        .collect()
}

fn draw_bone(source: &ScenePart, outline: bool) -> u16 {
    source
        .materials
        .iter()
        .position(|m| m.blend == outline)
        .and_then(|i| source.material_nodes.get(i))
        .and_then(|n| n.first())
        .copied()
        .unwrap_or_else(|| if outline { draw_bone(source, false) } else { 0 })
}

fn mesh_hidden(
    source: &ScenePart,
    target: &ScenePart,
    material: usize,
    mapping: &[u16],
    hidden: &BTreeSet<u16>,
    additive: bool,
) -> bool {
    let nodes = &target.material_nodes[material];
    if nodes.is_empty() {
        // Skinned meshes may live beside the skeleton, outside indexed joints.
        let outline =
            target.outline_color.is_some() || (!additive && target.materials[material].blend);
        hidden.contains(&draw_bone(source, outline))
    } else {
        nodes
            .iter()
            .any(|&node| hidden.contains(&mapping[usize::from(node)]))
    }
}

fn relative(
    bone: Entity,
    root: Entity,
    parents: &Query<&ChildOf>,
    helper: &Helper,
    rest: &BTreeMap<Entity, Transform>,
) -> Result<Affine3A> {
    let mut pose = Affine3A::IDENTITY;
    for node in std::iter::once(bone).chain(parents.iter_ancestors(bone)) {
        if node == root {
            return Ok(pose);
        }
        let local = if let Some(rest) = rest.get(&node) {
            Pose::Trs(*rest)
        } else {
            helper.local(node)?
        };
        pose = local.global().affine() * pose;
    }
    anyhow::bail!("weapon bone is outside its scene")
}

/// Bind-time data. Rebuilt only when either scene receives a new skeleton.
pub(super) struct Mapping {
    source: Arc<()>,
    pub(super) bones: Vec<u16>,
    source_inverse: BTreeMap<u16, (Entity, Option<Affine3A>)>,
    paths: Vec<Vec<Entity>>,
    parents: Vec<(Option<usize>, Affine3A)>,
    default: u16,
    ball: Option<u16>,
}
impl Mapping {
    #[allow(clippy::too_many_arguments)]
    fn bind(
        root: Entity,
        owner: Entity,
        source: &ScenePart,
        target: &ScenePart,
        additive: bool,
        binding: &Binding,
        rig: &crate::field_animation::Rig,
        parents: &Query<&ChildOf>,
        helper: &Helper,
    ) -> Result<Self> {
        let indices: BTreeMap<_, _> = binding
            .0
            .iter()
            .enumerate()
            .map(|(i, &(e, _))| (e, i))
            .collect();
        let paths: Vec<Vec<_>> = binding
            .0
            .iter()
            .map(|&(e, _)| {
                std::iter::once(e)
                    .chain(parents.iter_ancestors(e))
                    .take_while(|&e| e != root)
                    .collect()
            })
            .collect();
        let mut links = Vec::new();
        for path in &paths {
            let mut group = Affine3A::IDENTITY;
            let mut parent = None;
            for &entity in path.iter().skip(1) {
                if let Some(&index) = indices.get(&entity) {
                    parent = Some(index);
                    break;
                }
                group = helper.local(entity)?.global().affine() * group;
            }
            links.push((parent, group));
        }
        let bones = bone_map(
            source,
            target,
            &links.iter().map(|p| p.0).collect::<Vec<_>>(),
            additive,
        );
        let default = draw_bone(source, target.outline_color.is_some());
        let ball = source
            .bone_names
            .iter()
            .position(|n| n == "obj_gen000_tama")
            .map(|i| i as u16);
        let rest = rig.bones().iter().copied().collect();
        let mut source_inverse = BTreeMap::new();
        for index in bones.iter().copied().chain([default]).chain(ball) {
            if source_inverse.contains_key(&index) {
                continue;
            }
            let entity = rig
                .bone_at(index)
                .context("missing prop choreography joint")?;
            let authored = relative(entity, owner, parents, helper, &rest)?;
            let inverse = (authored.matrix3.determinant().abs() > 1e-8).then(|| authored.inverse());
            source_inverse.insert(index, (entity, inverse));
        }
        Ok(Self {
            source: rig.binding_key().clone(),
            bones,
            source_inverse,
            paths,
            parents: links,
            default,
            ball,
        })
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn apply(
    state: State,
    art: Res<Art>,
    cache: Res<EquipmentCache>,
    actors: Query<(&ActorPart, &crate::field_animation::Rig)>,
    parents: Query<&ChildOf>,
    mut roots: Query<(Entity, &mut Instance), With<Loaded>>,
    mut visibility: Query<&mut Visibility, Without<Instance>>,
    mut transforms: ParamSet<(Helper, (Query<&mut Transform>, ResMut<Locals>))>,
    mut failures: Failures,
) {
    let mut poses = Vec::new();
    let mut root_scales = Vec::new();
    {
        let helper = transforms.p0();
        for (entity, mut root) in &mut roots {
            let root = &mut *root;
            if !root.sampled {
                continue;
            }
            let Some(binding) = &root.binding else {
                continue;
            };
            let Ok((owner, rig)) = actors.get(root.owner) else {
                continue;
            };
            if !owner.prepared {
                continue;
            }
            let layer = &cache.equipment[&root.item].layers[root.layer];
            let source = &art.models[&owner.resource][owner.part].spec;
            let result = (|| -> Result<()> {
                if root
                    .choreography
                    .as_ref()
                    .is_none_or(|map| !Arc::ptr_eq(&map.source, rig.binding_key()))
                {
                    root.choreography = Some(Mapping::bind(
                        entity,
                        root.owner,
                        source,
                        &layer.art.spec,
                        layer.preview.additive,
                        binding,
                        rig,
                        &parents,
                        &helper,
                    )?);
                }
                let map = root.choreography.as_ref().unwrap();
                let owner_world = helper.compute_global_transform(root.owner)?.affine();
                let translation =
                    Affine3A::from_translation(Vec3::from_array(layer.preview.scene.translation));
                let deltas: BTreeMap<_, _> = map
                    .source_inverse
                    .iter()
                    .map(|(&index, &(bone, inverse))| {
                        Ok((
                            index,
                            match inverse {
                                Some(inverse) => {
                                    helper.compute_global_transform(bone)?.affine() * inverse
                                }
                                None => owner_world,
                            },
                        ))
                    })
                    .collect::<Result<_>>()?;
                let mut targets = Vec::with_capacity(binding.0.len());
                for (index, path) in map.paths.iter().enumerate() {
                    let local =
                        path.iter()
                            .try_fold(Affine3A::IDENTITY, |pose, &bone| -> Result<_> {
                                Ok(helper.local(bone)?.global().affine() * pose)
                            })?;
                    let local = translation * local;
                    let mut pose = deltas[&map.bones[index]] * local;
                    // The generic prop moves the ball separately from its handle.
                    if let Some(ball) = map.ball
                        && let Some(segment) = layer.art.spec.bone_names[index]
                            .strip_prefix("SP_himo")
                            .and_then(|n| n.parse::<u8>().ok())
                            .filter(|n| (1..=9).contains(n))
                    {
                        pose.translation = pose
                            .translation
                            .lerp((deltas[&ball] * local).translation, f32::from(segment) / 9.);
                    }
                    targets.push(pose);
                }
                let world = deltas[&map.default] * translation;
                let mut scale = Vec3::new(
                    world.matrix3.x_axis.length(),
                    world.matrix3.y_axis.length(),
                    world.matrix3.z_axis.length(),
                );
                if world.matrix3.determinant() < 0. {
                    scale.x = -scale.x;
                }
                root_scales.push((entity, scale));
                poses.push((entity, Deformation::World(world.into())));
                for (index, &(bone, _)) in binding.0.iter().enumerate() {
                    let (parent, group) = map.parents[index];
                    let parent = parent.map_or(world, |parent| targets[parent]) * group;
                    let world = targets[index];
                    poses.push((
                        bone,
                        if parent.matrix3.determinant().abs() > 1e-8 {
                            Deformation::Local(Pose::Affine(parent.inverse() * world))
                        } else {
                            Deformation::World(world.into())
                        },
                    ));
                }
                Ok(())
            })();
            if let Err(error) = result {
                failures.skip("field weapon choreography", error);
                continue;
            }
            root.prepared = true;
            if let Some(actor) = state.get().events.world.actors.get(&root.actor) {
                for &(material, mesh) in &root.geometry {
                    let hidden = mesh_hidden(
                        source,
                        &layer.art.spec,
                        material,
                        &root.choreography.as_ref().unwrap().bones,
                        &actor.appearance.hidden_nodes,
                        layer.preview.additive,
                    );
                    if let Ok(mut visible) = visibility.get_mut(mesh) {
                        *visible = if hidden {
                            Visibility::Hidden
                        } else {
                            Visibility::Inherited
                        };
                    }
                }
            }
        }
    }
    let (mut nodes, mut affine) = transforms.p1();
    for (entity, scale) in root_scales {
        if let Ok(mut transform) = nodes.get_mut(entity) {
            transform.scale = scale;
        }
    }
    for (entity, pose) in poses {
        if let Ok(mut transform) = nodes.get_mut(entity) {
            pose.apply(entity, &mut transform, &mut affine);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_articulated_joints_and_independent_effects() {
        let mut source: ScenePart = serde_json::from_value(
            serde_json::json!({"mesh":"source", "materials":[{"meshes":[0]}, {"meshes":[1],"blend":true}]}),
        )
        .unwrap();
        source.bone_names = vec![
            "body".into(),
            "ball".into(),
            "body_outline".into(),
            "ball_outline".into(),
        ];
        source.material_nodes = vec![vec![0, 1], vec![2, 3]];
        let mut target = source.clone();
        target.bone_names = vec!["body".into(), "string".into(), "ball".into()];
        target.material_nodes = vec![vec![0, 1, 2], vec![]];
        assert_eq!(
            bone_map(&source, &target, &[None, Some(0), Some(1)], false),
            [0, 0, 1]
        );
        target.outline_color = Some([0; 4]);
        assert_eq!(
            bone_map(&source, &target, &[None, Some(0), Some(1)], false),
            [2, 2, 3]
        );
        target.outline_color = None;
        target.bone_names = vec!["electric-arc".into()];
        target.material_nodes = vec![vec![], vec![0]];
        assert_eq!(bone_map(&source, &target, &[None], true), [0]);
        target.material_nodes = vec![vec![], vec![]];
        assert!(mesh_hidden(
            &source,
            &target,
            0,
            &[],
            &BTreeSet::from([0]),
            false
        ));
        assert!(!mesh_hidden(
            &source,
            &target,
            1,
            &[],
            &BTreeSet::from([0]),
            false
        ));
        assert!(mesh_hidden(
            &source,
            &target,
            1,
            &[],
            &BTreeSet::from([2]),
            false
        ));
    }
}
