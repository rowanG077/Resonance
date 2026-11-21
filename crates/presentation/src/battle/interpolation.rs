//! Reusable visual output. Simulation frames remain immutable; only continuous
//! drawing values are blended, with identities and cuts checked once per endpoint.
use super::*;
use crate::sparse_animation::affine::{Matrices, MatrixEndpoint};
use resonance_battle::{ParticleFrame, ParticleGeometry, ParticleOriginAttachment};
use resonance_content::animation::{Matrix, pose::Palette};

fn scalar(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn vector<const N: usize>(a: [f32; N], b: [f32; N], t: f32) -> [f32; N] {
    std::array::from_fn(|i| scalar(a[i], b[i], t))
}
fn color(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    std::array::from_fn(|i| scalar(a[i] as f32, b[i] as f32, t).round() as u8)
}
fn angle(a: f32, b: f32, t: f32) -> f32 {
    a + ((b - a + 180.).rem_euclid(360.) - 180.) * t
}
pub(crate) fn nearby(a: [f32; 3], b: [f32; 3]) -> bool {
    Vec3::from_array(a).distance_squared(Vec3::from_array(b)) <= 128. * 128.
}
pub(crate) fn continuous_camera(
    a: resonance_battle::CameraPose,
    b: resonance_battle::CameraPose,
) -> bool {
    nearby(a.eye, b.eye) && nearby(a.focus, b.focus)
}
struct Skeleton {
    world: MatrixEndpoint,
    bones: Matrices,
    secondary: BTreeMap<usize, SecondaryChain>,
}
struct SecondaryChain {
    pins: [Vec3; 2],
    shift: Vec3,
    joints: Vec<(usize, MatrixEndpoint)>,
}

impl Skeleton {
    fn new(world: (&Matrix, &Matrix), bones: (&Palette, &Palette)) -> Self {
        let endpoints = [(bones.0, world.0), (bones.1, world.1)];
        let mut secondary = BTreeMap::new();
        for (index, (a, b)) in bones.0.joints.iter().zip(&bones.1.joints).enumerate() {
            let Some(root) = a.secondary.or(b.secondary) else {
                continue;
            };
            let chain = secondary.entry(root).or_insert_with(|| SecondaryChain {
                pins: endpoints.map(|(bones, world)| {
                    // A held solver's root can lag the current animated pin.
                    let mut pin = Vec3::ZERO;
                    let mut ancestor = Some(root);
                    while let Some(i) = ancestor {
                        let joint = &bones.joints[i];
                        pin = Mat4::from_cols_array_2d(&joint.local.matrix()).transform_point3(pin);
                        ancestor = joint.parent;
                    }
                    Mat4::from_cols_array_2d(world).transform_point3(pin)
                }),
                shift: Vec3::ZERO,
                joints: Vec::new(),
            });
            let [from, to] = endpoints
                .map(|(bones, world)| resonance_content::animation::multiply(*world, bones[index]));
            chain.joints.push((index, MatrixEndpoint::new(&from, &to)));
        }
        Self {
            world: MatrixEndpoint::new(world.0, world.1),
            bones: Matrices::new(bones.0, bones.1),
            secondary,
        }
    }

    fn sample_bones(&mut self, t: f32, world: &Matrix, bones: &mut Arc<Palette>) {
        let bones = &mut Arc::get_mut(bones)
            .expect("render output bones have one owner")
            .global;
        self.bones.sample(t, bones);
        if self.secondary.is_empty() || t <= 0. || t >= 1. {
            return;
        }
        let world = Mat4::from_cols_array_2d(world);
        let Some(inverse) = (world.determinant() != 0.)
            .then(|| world.inverse())
            .filter(|m| m.is_finite())
        else {
            return;
        };
        // Read every animated pin before replacing any joints with simulated worlds.
        for (&root, chain) in &mut self.secondary {
            let pin = world.transform_point3(Vec3::from_slice(&bones[root][3]));
            chain.shift = pin - chain.pins[0].lerp(chain.pins[1], t);
        }
        for chain in self.secondary.values() {
            for (index, endpoint) in &chain.joints {
                let mut pose = Mat4::from_cols_array_2d(&endpoint.sample(t));
                pose.w_axis += chain.shift.extend(0.);
                bones[*index] = (inverse * pose).to_cols_array_2d();
            }
        }
    }
}
struct ModelPair {
    from: usize,
    to: usize,
    skeleton: Skeleton,
}
struct ParticlePair {
    from: Option<usize>,
    to: Option<usize>,
    skeleton: Option<Skeleton>,
    origin: Option<OriginPair>,
    parent: Option<Parent>,
}
/// Attachment identities resolved once for the lifetime of these endpoints.
#[derive(Clone, Copy)]
enum Parent {
    Actor { index: usize, bone: usize },
    Particle { index: usize, bone: usize },
}
struct OriginPair {
    attachment: ParticleOriginAttachment,
    /// Fade away the held world offset when following resumes.
    offset: [f32; 3],
}

#[derive(Default)]
pub(super) struct Cache {
    pub active: bool,
    key: Option<(u32, u64)>,
    frame: BattleFrame,
    particles: Vec<ParticleFrame>,
    models: Vec<(ModelPair, MatrixEndpoint)>,
    weapons: Vec<ModelPair>,
    shadows: Vec<(usize, usize)>,
    pairs: Vec<ParticlePair>,
}
impl Cache {
    pub fn frame(&self) -> &BattleFrame {
        &self.frame
    }
    pub fn sample<'a>(
        &'a mut self,
        key: (u32, u64),
        from: &BattleFrame,
        particles: &[ParticleFrame],
        to: &prediction::Endpoint,
        t: f32,
    ) -> (&'a BattleFrame, &'a [ParticleFrame]) {
        if self.key != Some(key) {
            self.prepare(key, from, particles, to);
        }
        for (index, out) in self.frame.actors.iter_mut().enumerate() {
            if let Some(b) = to.frame.actors.get(index) {
                let a = &from.actors[index];
                if nearby(a.position, b.position) {
                    out.position = vector(a.position, b.position, t);
                    out.heading = angle(a.heading, b.heading, t);
                }
            }
        }
        if let (Some(a), Some(b)) = (from.camera, to.frame.camera)
            && continuous_camera(a, b)
        {
            self.frame.camera = Some(resonance_battle::CameraPose {
                eye: vector(a.eye, b.eye, t),
                focus: vector(a.focus, b.focus, t),
                pitch: angle(a.pitch, b.pitch, t),
                yaw: angle(a.yaw, b.yaw, t),
                radius: scalar(a.radius, b.radius, t),
            });
            self.frame.camera_fov_degrees = Some(scalar(
                from.camera_fov_degrees
                    .unwrap_or(resonance_battle::VERTICAL_FOV_DEGREES),
                to.frame
                    .camera_fov_degrees
                    .unwrap_or(resonance_battle::VERTICAL_FOV_DEGREES),
                t,
            ));
        }
        for (pair, particle_world) in &mut self.models {
            let a = &from.models[pair.from];
            let b = &to.frame.models[pair.to];
            let out = &mut self.frame.models[pair.from];
            out.world = pair.skeleton.world.sample(t);
            pair.skeleton.sample_bones(t, &out.world, &mut out.bones);
            out.particle_world = particle_world.sample(t);
            out.tint = color(a.tint, b.tint, t);
            out.root_translation = vector(a.root_translation, b.root_translation, t);
            out.light = a
                .light
                .zip(b.light)
                .map(|(a, b)| vector(a, b, t))
                .or(a.light);
            if let (Some(a), Some(b), Some(out)) = (a.shadow, b.shadow, out.shadow.as_mut()) {
                out.position = vector(a.position, b.position, t);
                out.scale = scalar(a.scale, b.scale, t);
                out.color = color(a.color, b.color, t);
            }
        }
        for pair in &self.weapons {
            let a = &from.weapons[pair.from];
            let b = &to.frame.weapons[pair.to];
            let out = &mut self.frame.weapons[pair.from];
            if a.attachment.is_none() {
                out.world = pair.skeleton.world.sample(t);
            }
            out.tint = color(a.tint, b.tint, t);
            if let (Some(a), Some(b), Some(out)) = (&a.texture, &b.texture, &mut out.texture)
                && a.texture == b.texture
                && let (Some(from), Some(to)) = (a.scroll_position, b.scroll_position)
            {
                out.offset = if t >= 1. {
                    b.offset
                } else {
                    std::array::from_fn(|i| a.offset[i] + (to[i] - from[i]) as f32 * t / 1000.)
                };
            }
        }
        // Even an attachment/detachment cut must keep the committed weapon on
        // the rendered owner until the simulation publishes the new binding.
        for weapon in &mut self.frame.weapons {
            if let Some(attachment) = weapon.attachment
                && let Some(owner) = self.frame.models.iter().find(|m| m.actor == weapon.owner)
            {
                weapon.world = attachment.world(owner);
            }
        }
        for pair in &mut self.weapons {
            let out = &mut self.frame.weapons[pair.from];
            pair.skeleton.sample_bones(t, &out.world, &mut out.bones);
        }
        for &(i, j) in &self.shadows {
            self.frame.projectile_shadows[i].position = vector(
                from.projectile_shadows[i].position,
                to.frame.projectile_shadows[j].position,
                t,
            );
        }
        for (pair, out) in self.pairs.iter().zip(&mut self.particles) {
            let a = pair.from.map(|i| &particles[i]);
            let b = pair.to.map(|i| &to.particles[i]);
            let opacity = if a.is_none() {
                t
            } else if b.is_none() {
                1. - t
            } else {
                1.
            };
            let a = a.or(b).unwrap();
            let b = b.unwrap_or(a);
            let origin = vector(a.origin, b.origin, t);
            out.origin = pair
                .origin
                .as_ref()
                .and_then(|pair| {
                    pair.attachment.position(&self.frame).map(|anchor| {
                        std::array::from_fn(|axis| anchor[axis] + pair.offset[axis] * (1. - t))
                    })
                })
                .unwrap_or(origin);
            out.uv_offset = a.scroll_offset(b, t);
            out.heading = angle(a.heading, b.heading, t);
            out.state.offset = vector(a.state.offset, b.state.offset, t);
            out.state.angles =
                std::array::from_fn(|i| angle(a.state.angles[i], b.state.angles[i], t));
            out.state.orbit = vector(a.state.orbit, b.state.orbit, t);
            out.state.colors = std::array::from_fn(|row| {
                std::array::from_fn(|i| {
                    (scalar(
                        a.state.colors[row][i] as f32,
                        b.state.colors[row][i] as f32,
                        t,
                    ) * if i == 3 { opacity } else { 1. })
                    .round() as i16
                })
            });
            geometry(
                &a.state.geometry,
                &b.state.geometry,
                &mut out.state.geometry,
                t,
            );
            if let Some(model) = out.model.as_mut() {
                if let Some(skeleton) = &pair.skeleton {
                    let placement = model
                        .attachment
                        .as_mut()
                        .map_or(&mut model.world, |a| &mut a.local);
                    *placement = skeleton.world.sample(t);
                } else {
                    // Birth/death poses must start fresh on every render sample.
                    model.world = a.model.as_ref().unwrap().world;
                }
                if model.attachment.is_none()
                    && let Some(origin_pair) = &pair.origin
                {
                    for (axis, from) in origin.into_iter().enumerate() {
                        if axis != 1 || !origin_pair.attachment.model_ground_relative {
                            model.world[3][axis] += out.origin[axis] - from;
                        }
                    }
                }
            }
        }
        // Particle IDs preserve birth order; an attached parent is always older.
        // Compose after all local poses have been sampled, including births/deaths.
        for (index, pair) in self.pairs.iter_mut().enumerate() {
            let (parents, current) = self.particles.split_at_mut(index);
            let Some(model) = current[0].model.as_mut() else {
                continue;
            };
            let parent = pair.parent.and_then(|parent| match parent {
                Parent::Actor { index, bone } => {
                    let model = &self.frame.models[index];
                    model.bones.get(bone).map(|b| (model.world, *b))
                }
                Parent::Particle { index, bone } => parents[index]
                    .model
                    .as_ref()
                    .and_then(|m| m.bones.get(bone).map(|b| (m.world, *b))),
            });
            if let Some((world, bone)) = parent
                && let Some(attachment) = model.attachment
            {
                model.world = resonance_content::animation::multiply(
                    resonance_content::animation::multiply(world, bone),
                    attachment.local,
                );
            }
            if let Some(skeleton) = &mut pair.skeleton {
                skeleton.sample_bones(t, &model.world, &mut model.bones);
            }
        }
        (&self.frame, &self.particles)
    }
    fn prepare(
        &mut self,
        key: (u32, u64),
        from: &BattleFrame,
        particles: &[ParticleFrame],
        to: &prediction::Endpoint,
    ) {
        self.frame.clone_from(from);
        self.models.clear();
        self.weapons.clear();
        self.shadows.clear();
        self.pairs.clear();
        self.particles.clear();
        for (index, a) in from.models.iter().enumerate() {
            if let Some((j, b)) = to
                .frame
                .models
                .iter()
                .enumerate()
                .find(|(_, b)| crate::battle_view::interpolation::continuous_model(a, b))
            {
                self.frame.models[index].bones = Arc::new((*a.bones).clone());
                self.models.push((
                    ModelPair {
                        from: index,
                        to: j,
                        skeleton: Skeleton::new((&a.world, &b.world), (&a.bones, &b.bones)),
                    },
                    MatrixEndpoint::new(&a.particle_world, &b.particle_world),
                ));
            }
        }
        for (index, a) in from.weapons.iter().enumerate() {
            if let Some((j, b)) = to.frame.weapons.iter().enumerate().find(|(_, b)| {
                crate::battle_view::interpolation::continuous_weapon(a, b, || {
                    self.models
                        .iter()
                        .any(|(pair, _)| from.models[pair.from].actor == a.owner)
                })
            }) {
                self.frame.weapons[index].bones = Arc::new((*a.bones).clone());
                self.weapons.push(ModelPair {
                    from: index,
                    to: j,
                    skeleton: Skeleton::new((&a.world, &b.world), (&a.bones, &b.bones)),
                });
            }
        }
        for (i, a) in from.projectile_shadows.iter().enumerate() {
            if let Some((j, _)) = to
                .frame
                .projectile_shadows
                .iter()
                .enumerate()
                .find(|(_, b)| {
                    a.projectile == b.projectile
                        && a.appearance == b.appearance
                        && nearby(a.position, b.position)
                })
            {
                self.shadows.push((i, j));
            }
        }
        let mut pairs = BTreeMap::<_, (Option<usize>, Option<usize>)>::new();
        for (i, p) in particles.iter().enumerate() {
            pairs.entry(p.id).or_default().0 = Some(i);
        }
        for (i, p) in to.particles.iter().enumerate() {
            pairs.entry(p.id).or_default().1 = Some(i);
        }
        for (_, (i, j)) in pairs {
            let a = i.map(|i| &particles[i]);
            let b = j.map(|j| &to.particles[j]);
            let first = a.or(b).unwrap();
            let last = b.unwrap_or(first);
            // Held destinations keep their world origin. On resume, retain the
            // held source offset and blend it away along the rendered bone path.
            // Births/deaths use their one active binding without a held offset.
            let origin = last
                .origin_attachment
                .filter(|attachment| first.origin_attachment.is_none_or(|a| a == *attachment))
                .and_then(|attachment| {
                    let offset = if first.origin_attachment.is_some() {
                        [0.; 3]
                    } else {
                        let anchor = attachment.position(from)?;
                        std::array::from_fn(|axis| first.origin[axis] - anchor[axis])
                    };
                    Some(OriginPair { attachment, offset })
                });
            let mut out = a.or(b).unwrap().clone();
            // Birth-only model particles also need render-owned bones: retaining
            // an Arc from the predicted frame would pin its arena after invalidation.
            if let Some(model) = out.model.as_mut() {
                model.bones = Arc::new((*model.bones).clone());
            }
            let skeleton = a
                .zip(b)
                .and_then(|(a, b)| a.model.as_ref().zip(b.model.as_ref()))
                .filter(|(a, b)| {
                    a.resource == b.resource
                        && a.bones.len() == b.bones.len()
                        && a.attachment.map(|a| a.parent) == b.attachment.map(|a| a.parent)
                })
                .map(|(a, b)| {
                    let mut skeleton = Skeleton::new((&a.world, &b.world), (&a.bones, &b.bones));
                    if let (Some(a), Some(b)) = (&a.attachment, &b.attachment) {
                        skeleton.world = MatrixEndpoint::new(&a.local, &b.local);
                    }
                    skeleton
                });
            let parent = out
                .model
                .as_ref()
                .and_then(|model| model.attachment)
                .and_then(|attachment| match attachment.parent {
                    resonance_battle::EffectModelParent::Actor { actor, bone } => self
                        .frame
                        .models
                        .iter()
                        .position(|model| model.actor == actor)
                        .map(|index| Parent::Actor { index, bone }),
                    resonance_battle::EffectModelParent::Particle { particle, bone } => self
                        .particles
                        .iter()
                        .position(|parent| parent.id == particle)
                        .map(|index| Parent::Particle { index, bone }),
                });
            self.particles.push(out);
            self.pairs.push(ParticlePair {
                from: i,
                to: j,
                skeleton,
                origin,
                parent,
            });
        }
        self.key = Some(key);
    }
}
fn geometry(a: &ParticleGeometry, b: &ParticleGeometry, out: &mut ParticleGeometry, t: f32) {
    use ParticleGeometry::*;
    match (a, b, out) {
        (Size { value: a, .. }, Size { value: b, .. }, Size { value: out, .. })
        | (Spiral { value: a, .. }, Spiral { value: b, .. }, Spiral { value: out, .. }) => {
            *out = vector(*a, *b, t)
        }
        (Quad { vertices: a, .. }, Quad { vertices: b, .. }, Quad { vertices: out, .. }) => {
            for i in 0..4 {
                out[i] = vector(a[i], b[i], t);
            }
        }
        (
            BillboardTrail {
                size: a,
                radius: ar,
                ..
            },
            BillboardTrail {
                size: b,
                radius: br,
                ..
            },
            BillboardTrail {
                size: out,
                radius: r,
                ..
            },
        ) => {
            *out = vector(*a, *b, t);
            *r = scalar(*ar, *br, t);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{ActorShadowFrame, ModelFrame, ModelMaterial, ResourceId};

    fn model() -> ModelFrame {
        ModelFrame {
            actor: ActorId::from_index(0).unwrap(),
            visible: true,
            depth_write: true,
            tint: [64, 64, 64, 255],
            material: ModelMaterial::Normal,
            texture_layers: [0; 4],
            texture_tick: 0,
            light: None,
            shadow: Some(ActorShadowFrame {
                position: [0.; 3],
                scale: 1.,
                color: [0, 0, 0, 128],
            }),
            resource: ResourceId(1),
            clip: 0,
            frame: 0.,
            blend_weight: 1.,
            root_translation: [0.; 3],
            world: Mat4::IDENTITY.to_cols_array_2d(),
            particle_world: Mat4::IDENTITY.to_cols_array_2d(),
            bones: Arc::new(resonance_content::animation::pose::Palette::independent(
                vec![Mat4::IDENTITY.to_cols_array_2d(); 64],
            )),
        }
    }
    fn endpoint(frame: BattleFrame) -> prediction::Endpoint {
        prediction::Endpoint {
            frame,
            ..Default::default()
        }
    }

    #[test]
    fn turning_actor_keeps_inertial_cloth_shape_and_animated_pin_and_guide() {
        use resonance_content::animation::{Bone, Skeleton as Rig, Transform as Trs};
        let pin = Vec3::X * 5.;
        let free = Vec3::new(5., 0., -15.);
        let world = |t: f32| Mat4::from_rotation_y(3.1 * t);
        let twist = |t: f32| Quat::from_rotation_z((178. * t - 89.).to_radians());
        let authored = |t: f32| Quat::from_rotation_y(1.6 * t);
        let frame = |t| {
            let rig = Rig {
                bones: [pin, Vec3::NEG_Z * 10., Vec3::NEG_Z * 10.]
                    .into_iter()
                    .enumerate()
                    .map(|(i, position)| Bone {
                        name: i.to_string(),
                        parent: i.checked_sub(1).map(|parent| parent as u16),
                        parent_transform: None,
                        shadow_bounds: true,
                        bind: Trs {
                            translation: position.to_array(),
                            rotation: if i == 0 {
                                authored(t).to_array()
                            } else {
                                Quat::IDENTITY.to_array()
                            },
                            ..Trs::default()
                        },
                    })
                    .collect(),
            };
            let mut bones = Palette::bind(&rig).unwrap();
            // The held pin lags the animation and the free joint counter-rotates.
            let held_pin = world(t).transform_point3(pin) + Vec3::Y * t;
            for (i, (rotation, position)) in [(twist(t), held_pin), (twist(t).inverse(), free)]
                .into_iter()
                .enumerate()
            {
                bones.global[i] = (world(t).inverse()
                    * Mat4::from_rotation_translation(rotation, position))
                .to_cols_array_2d();
                bones.joints[i].secondary = Some(0);
            }
            BattleFrame {
                models: vec![ModelFrame {
                    world: world(t).to_cols_array_2d(),
                    bones: Arc::new(bones),
                    ..model()
                }],
                ..Default::default()
            }
        };
        let from = frame(0.);
        let to = endpoint(frame(1.));
        let mut cache = Cache::default();
        for t in [0., 0.25, 0.5, 1., 0.25] {
            let (frame, _) = cache.sample((1, 1), &from, &[], &to, t);
            let body = &frame.models[0];
            let animated_pin = world(t).transform_point3(pin);
            let free =
                crate::test_support::inertial_joint(free, |t| world(t).transform_point3(pin), t);
            let guide = world(t)
                * Mat4::from_rotation_translation(authored(t), pin)
                * Mat4::from_translation(Vec3::NEG_Z * 20.);
            let expected = [
                Mat4::from_rotation_translation(twist(t), animated_pin + Vec3::Y * t),
                Mat4::from_rotation_translation(twist(t).inverse(), free),
                guide,
            ];
            for (i, expected) in expected.into_iter().enumerate() {
                let actual = Mat4::from_cols_array_2d(&body.world)
                    * Mat4::from_cols_array_2d(&body.bones[i]);
                assert!(actual.abs_diff_eq(expected, 0.0001), "joint {i} at {t}");
            }
        }
    }

    fn weapon(owner: &resonance_battle::ModelFrame) -> resonance_battle::WeaponFrame {
        resonance_battle::WeaponFrame {
            owner: owner.actor,
            slot: Some(resonance_battle::WeaponSlot(0)),
            attachment: Some(resonance_battle::WeaponAttachment::Bone(0)),
            visible: true,
            tint: owner.tint,
            material: owner.material,
            resource: ResourceId(2),
            clip: None,
            frame: 0.,
            world: owner.bones[0],
            bones: Arc::new(resonance_content::animation::pose::Palette::independent(
                vec![Mat4::IDENTITY.to_cols_array_2d()],
            )),
            links: vec![],
            texture: None,
        }
    }

    #[test]
    fn weapons_follow_rendered_owner_bones() {
        let mut owner = model();
        owner.bones = Arc::new(resonance_content::animation::pose::Palette::independent(
            vec![Mat4::from_translation(Vec3::X * 50.).to_cols_array_2d()],
        ));
        let weapon = weapon(&owner);
        let from = BattleFrame {
            models: vec![owner],
            weapons: vec![weapon],
            ..Default::default()
        };
        let mut to = endpoint(from.clone());
        to.frame.models[0].world =
            Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2).to_cols_array_2d();
        to.frame.weapons[0].world = (Mat4::from_cols_array_2d(&to.frame.models[0].world)
            * Mat4::from_cols_array_2d(&to.frame.models[0].bones[0]))
        .to_cols_array_2d();
        let mut cache = Cache::default();
        for alpha in [0., 0.25, 0.5, 0.75, 0.999] {
            let (rendered, _) = cache.sample((1, 1), &from, &[], &to, alpha);
            let owner = &rendered.models[0];
            let expected = (Mat4::from_cols_array_2d(&owner.world)
                * Mat4::from_cols_array_2d(&owner.bones[0]))
            .to_cols_array_2d();
            assert_eq!(rendered.weapons[0].world, expected);
        }
        // A pending detach still renders the current binding up to the committed tick.
        to.frame.weapons[0].attachment = None;
        let (rendered, _) = cache.sample((1, 2), &from, &[], &to, 0.5);
        assert_eq!(
            rendered.weapons[0].world,
            resonance_battle::WeaponAttachment::Bone(0).world(&rendered.models[0])
        );
        // Once detached, flight placement interpolates independently of the owner.
        let mut detached = from.clone();
        detached.weapons[0].attachment = None;
        let (rendered, _) = cache.sample((1, 3), &detached, &[], &to, 0.5);
        let position = Vec3::from_slice(&rendered.weapons[0].world[3][..3]);
        assert!(position.distance(Vec3::new(25., 0., -25.)) < 0.001);
        // Owner replacement holds both the body and the attached weapon.
        to.frame.weapons[0].attachment = from.weapons[0].attachment;
        to.frame.models[0].resource = ResourceId(99);
        let (rendered, _) = cache.sample((1, 4), &from, &[], &to, 0.5);
        assert_eq!(rendered.models, from.models);
        assert_eq!(rendered.weapons, from.weapons);
    }

    #[test]
    fn retained_battle_poses_blend_without_allocations_or_simulation_changes() {
        let from = BattleFrame {
            update: 17,
            models: vec![model()],
            ..Default::default()
        };
        let mut to = endpoint(from.clone());
        to.frame.update += 1;
        to.frame.models[0].world = Mat4::from_translation(Vec3::X * 10.).to_cols_array_2d();
        to.frame.models[0].bones =
            Arc::new(resonance_content::animation::pose::Palette::independent(
                vec![Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2).to_cols_array_2d(); 64],
            ));
        to.frame.models[0].texture_layers = [1; 4];
        let mut cache = Cache::default();
        let (frame, _) = cache.sample((17, 1), &from, &[], &to, 0.5);
        let address = frame.models[0].bones.as_ptr();
        assert_eq!(frame.models[0].world[3][0], 5.);
        assert!((frame.models[0].bones[0][0][0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
        assert_eq!(frame.models[0].texture_layers, [0; 4]);
        let calls = resonance_simulation_memory::system_calls();
        for n in 0..300 {
            let alpha = n as f32 / 300.;
            let (frame, _) = cache.sample((17, 1), &from, &[], &to, alpha);
            assert_eq!(frame.models[0].bones.as_ptr(), address);
            assert_eq!(frame.update, 17);
            assert!((frame.models[0].world[3][0] - 10. * alpha).abs() < 0.00001);
        }
        assert_eq!(resonance_simulation_memory::system_calls(), calls);
        assert_eq!(from.models[0].world, Mat4::IDENTITY.to_cols_array_2d());
        assert_eq!(from.models[0].bones[0], Mat4::IDENTITY.to_cols_array_2d());
        assert_eq!(to.frame.update, 18);
    }

    #[test]
    fn replacement_models_and_camera_cuts_keep_the_committed_endpoint() {
        let from = BattleFrame {
            models: vec![model()],
            camera: Some(resonance_battle::CameraPose {
                eye: [0., 10., 0.],
                focus: [0.; 3],
                pitch: 0.,
                yaw: 0.,
                radius: 10.,
            }),
            ..Default::default()
        };
        let mut to = endpoint(from.clone());
        to.frame.models[0].world = Mat4::from_translation(Vec3::X * 10.).to_cols_array_2d();
        to.frame.models[0].resource = ResourceId(2);
        to.frame.camera.as_mut().unwrap().eye[0] = 1000.;
        let mut cache = Cache::default();
        let (frame, _) = cache.sample((1, 1), &from, &[], &to, 0.9);
        assert_eq!(frame.models, from.models);
        assert_eq!(frame.camera, from.camera);
        to.frame.models[0].resource = ResourceId(1);
        to.frame.models[0].world[3][0] = 1000.;
        let (frame, _) = cache.sample((1, 2), &from, &[], &to, 0.9);
        assert_eq!(frame.models, from.models);
    }

    fn particles() -> Result<Vec<ParticleFrame>> {
        use resonance_battle::{
            EffectAppearance, EffectBank, EffectDefinition, Effects, ParticleDefinition,
        };
        use resonance_content::battle_effect::{
            EffectOperation, ScheduledEvent, declaration::Declaration,
        };
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../battle_view/fixtures/masked-particle.json"))?;
        let declaration: Declaration = serde_json::from_value(data["declaration"].clone())?;
        let particle = ParticleDefinition {
            member: 0,
            model: None,
            data: declaration.particle()?,
        };
        let definition = EffectDefinition {
            events: vec![
                ScheduledEvent {
                    at: 0,
                    operation: EffectOperation::Spawn {
                        particle: 0,
                        blend: None,
                        palette: None,
                        birth: Box::default()
                    }
                };
                3
            ]
            .into(),
            particles: [(0, Arc::new(particle))].into(),
        };
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        let bank = EffectBank::new(
            ResourceId(1),
            BTreeMap::new(),
            [(0, Arc::new(definition))].into(),
            &mut |_| Ok(None),
            &diagnostics,
        )?;
        let mut effects = Effects::new(vec![bank], None, 1, diagnostics)?.with_ambient(Some((
            EffectAppearance {
                resource: ResourceId(1),
                member: 0,
            },
            u16::MAX,
        )))?;
        effects.advance(&BattleFrame::default(), false)?;
        effects.advance(&BattleFrame::default(), false)?;
        Ok(effects.frames())
    }

    #[test]
    fn particle_identity_survives_births_deaths_and_reordering() -> Result<()> {
        let particles = particles()?;
        assert_eq!(particles.len(), 3);
        let mut current = vec![particles[0].clone(), particles[1].clone()];
        for p in &mut current {
            p.state.colors[0][3] = 200;
            p.state.colors[1][3] = 200;
        }
        let mut to = endpoint(BattleFrame::default());
        to.particles = vec![particles[2].clone(), current[1].clone()];
        to.particles[0].state.colors[0][3] = 200;
        to.particles[1].origin[0] += 8.;
        let mut cache = Cache::default();
        let (_, output) = cache.sample((1, 1), &BattleFrame::default(), &current, &to, 0.25);
        assert_eq!(output.len(), 3);
        let find = |id| output.iter().find(|p| p.id == id).unwrap();
        assert_eq!(find(current[0].id).state.colors[0][3], 150);
        assert_eq!(find(to.particles[0].id).state.colors[0][3], 50);
        assert!((find(current[1].id).origin[0] - current[1].origin[0] - 2.).abs() < 0.00001);
        Ok(())
    }

    #[test]
    fn a_new_model_particle_does_not_pin_the_prediction_arena() -> Result<()> {
        let seed = particles()?.remove(0);
        let mut storage = resonance_game::field::PredictionBuffer::new()?;
        let next = storage.execute(|| {
            let mut born = seed.clone();
            born.model = Some(resonance_battle::EffectModelFrame {
                attachment: None,
                resource: ResourceId(1),
                clip: None,
                frame: 0.,
                world: Mat4::IDENTITY.to_cols_array_2d(),
                bones: Arc::new(resonance_content::animation::pose::Palette::independent(
                    vec![Mat4::IDENTITY.to_cols_array_2d()],
                )),
            });
            let mut next = endpoint(BattleFrame::default());
            next.particles.push(born);
            next
        })?;
        let mut cache = Cache::default();
        cache.sample((1, 1), &BattleFrame::default(), &[], &next, 0.5);
        drop(next);
        // Reuse scene storage while the rendered birth remains cached.
        storage.execute(|| ())?;
        assert_eq!(cache.particles[0].model.as_ref().unwrap().bones.len(), 1);
        Ok(())
    }
    #[test]
    fn model_particles_follow_rendered_bones_and_nested_parents() -> Result<()> {
        use resonance_battle::{EffectModelAttachment, EffectModelFrame, EffectModelParent};
        let mut owner = model();
        owner.bones = Arc::new(resonance_content::animation::pose::Palette::independent(
            vec![Mat4::from_translation(Vec3::X * 50.).to_cols_array_2d()],
        ));
        let from = BattleFrame {
            models: vec![owner],
            ..Default::default()
        };
        let mut current = particles()?;
        current.truncate(2);
        let parent_id = current[0].id;
        for (i, particle) in current.iter_mut().enumerate() {
            let parent = if i == 0 {
                EffectModelParent::Actor {
                    actor: from.models[0].actor,
                    bone: 0,
                }
            } else {
                EffectModelParent::Particle {
                    particle: parent_id,
                    bone: 0,
                }
            };
            let local = Mat4::from_translation(Vec3::Y * 3.);
            particle.model = Some(EffectModelFrame {
                resource: ResourceId(10),
                clip: None,
                frame: 0.,
                world: (Mat4::from_translation(Vec3::X * (50. + i as f32 * 10.))
                    * Mat4::from_translation(Vec3::Y * 3. * (i + 1) as f32))
                .to_cols_array_2d(),
                attachment: Some(EffectModelAttachment {
                    parent,
                    local: local.to_cols_array_2d(),
                }),
                bones: Arc::new(resonance_content::animation::pose::Palette::independent(
                    vec![Mat4::from_translation(Vec3::X * 10.).to_cols_array_2d()],
                )),
            });
        }
        let mut to = endpoint(from.clone());
        to.frame.models[0].world =
            Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2).to_cols_array_2d();
        to.particles = current.clone();
        for p in &mut to.particles {
            let model = p.model.as_mut().unwrap();
            if p.id == parent_id {
                model.attachment.as_mut().unwrap().local =
                    Mat4::from_translation(Vec3::Y * 7.).to_cols_array_2d();
            }
            model.world = (Mat4::from_cols_array_2d(&to.frame.models[0].world)
                * Mat4::from_cols_array_2d(&model.world))
            .to_cols_array_2d();
        }
        // Rendering resolves birth order regardless of endpoint vector order.
        to.particles.reverse();
        let before = current.clone();
        let mut cache = Cache::default();
        for (revision, births, deaths, cut) in [
            (1, false, false, false),
            (2, true, false, false),
            (3, false, true, false),
            (4, false, false, true),
            (5, false, false, false),
        ] {
            let mut target = endpoint(to.frame.clone());
            if cut {
                target.frame.models[0].world[3][0] = 1000.;
            }
            if revision == 5 {
                target.frame.models[0].world = Mat4::from_scale(Vec3::ZERO).to_cols_array_2d();
            }
            if !deaths {
                target.particles = to.particles.clone();
            }
            for alpha in [0., 0.25, 0.5, 0.75, 1.] {
                let (rendered, output) = cache.sample(
                    (1, revision),
                    &from,
                    if births { &[] } else { &current },
                    &target,
                    alpha,
                );
                let owner = &rendered.models[0];
                let mut expected = Mat4::from_cols_array_2d(&owner.world)
                    * Mat4::from_cols_array_2d(&owner.bones[0]);
                for particle in output {
                    let model = particle.model.as_ref().unwrap();
                    if particle.id == parent_id {
                        let expected_y = if births {
                            7.
                        } else if deaths {
                            3.
                        } else {
                            3. + 4. * alpha
                        };
                        assert!(
                            (model.attachment.unwrap().local[3][1] - expected_y).abs() < 0.00001
                        );
                    }
                    expected *= Mat4::from_cols_array_2d(&model.attachment.unwrap().local);
                    assert!(Mat4::from_cols_array_2d(&model.world).abs_diff_eq(expected, 0.0001));
                    expected *= Mat4::from_cols_array_2d(&model.bones[0]);
                }
            }
        }
        assert_eq!(
            current, before,
            "rendering mutated committed particle poses"
        );
        // A pending detach retains the current parent through the tick boundary.
        for p in &mut to.particles {
            p.model.as_mut().unwrap().attachment = None;
        }
        let (rendered, output) = cache.sample((1, 6), &from, &current, &to, 0.5);
        let expected = Mat4::from_cols_array_2d(&rendered.models[0].world)
            * Mat4::from_cols_array_2d(&rendered.models[0].bones[0])
            * Mat4::from_translation(Vec3::Y * 3.);
        assert!(
            Mat4::from_cols_array_2d(&output[0].model.as_ref().unwrap().world)
                .abs_diff_eq(expected, 0.0001)
        );
        Ok(())
    }

    #[test]
    fn weapon_atlas_rows_stay_discrete_and_scrolls_wrap() {
        use resonance_battle::WeaponTextureFrame;
        for (start, end, travel) in [
            (0., 0.25, None),
            (0.75, 0., None),
            (0.99, 0.01, Some(20)),
            (0.995, 0.695, Some(700)),
            (0., 0., Some(-1000)),
            (0.3, 0.3, Some(0)),
        ] {
            let owner = model();
            let weapon = resonance_battle::WeaponFrame {
                texture: Some(WeaponTextureFrame {
                    texture: 0,
                    offset: [0., start],
                    scroll_position: travel.map(|_| [0; 2]),
                }),
                ..weapon(&owner)
            };
            let from = BattleFrame {
                models: vec![owner],
                weapons: vec![weapon],
                ..Default::default()
            };
            let mut to = endpoint(from.clone());
            let target = to.frame.weapons[0].texture.as_mut().unwrap();
            target.offset[1] = end;
            target.scroll_position = travel.map(|dy| [0, dy]);
            let mut cache = Cache::default();
            for alpha in [0., 0.25, 0.5, 0.75, 1.] {
                let (rendered, _) = cache.sample((1, 1), &from, &[], &to, alpha);
                let actual = rendered.weapons[0].texture.unwrap().offset[1];
                let expected = if alpha == 1. && travel.is_some() {
                    end
                } else {
                    start + travel.unwrap_or_default() as f32 * alpha / 1000.
                };
                assert!(
                    (actual - expected).abs() < 0.00001,
                    "{travel:?}: {actual} != {expected}"
                );
            }
        }
    }
}
