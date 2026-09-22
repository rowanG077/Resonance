//! Camera-facing sprites from cooked recipes and live event state.
use super::sparse_animation::affine::Helper as TransformHelper;
use super::{
    field_animation::Rig,
    field_audit::{Applied, Request},
    field_view::{ActorPart, State},
    materials::TitleSurface,
};
use anyhow::Result;
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use resonance_content::{
    effect::{EmoteTrack, FieldEffects, FlutterRecipe, RefractionRecipe, VerticalAnchor},
    field::FieldAssets,
};
use std::{collections::BTreeMap, fs, path::Path};

const EMOTES: usize = 0;
const STATUS: usize = 1;

#[derive(Component)]
pub(super) struct EffectDraw;

#[derive(Resource)]
pub(super) struct Artwork {
    spec: FieldEffects,
    textures: Vec<Handle<Image>>,
    layers: Vec<Option<(Entity, Handle<Mesh>)>>,
    particles: BTreeMap<i32, (FlutterRecipe, usize)>,
    sprites: BTreeMap<u16, usize>,
    additive: Vec<bool>,
    refraction_texture: Handle<Image>,
}
impl Artwork {
    pub(super) fn refraction(&self) -> (&RefractionRecipe, &Handle<Image>) {
        (&self.spec.refraction, &self.refraction_texture)
    }
    pub fn mouth_frame(&self, age: u32) -> u8 {
        self.spec.mouth_cycle[age as usize % self.spec.mouth_cycle.len()]
    }
    pub fn load(root: &Path, field: &FieldAssets, server: &AssetServer) -> Result<Self> {
        Self::load_with(root, field, server, None)
    }
    pub fn load_with(
        root: &Path,
        field: &FieldAssets,
        server: &AssetServer,
        files: Option<&resonance_content::prepared::Files>,
    ) -> Result<Self> {
        let spec: FieldEffects = if let Some(files) = files {
            files.json(&field.effects)?
        } else {
            serde_json::from_slice(&fs::read(root.join(&field.effects))?)?
        };
        spec.validate()?;
        let mut paths = vec![spec.emote_texture.clone(), spec.status_texture.clone()];
        let mut additive = vec![false, false];
        let sprites = spec
            .sprites
            .iter()
            .map(|(&kind, recipe)| {
                paths.push(recipe.texture.clone());
                additive.push(recipe.additive);
                (kind, paths.len() - 1)
            })
            .collect();
        let particles = field
            .particles
            .iter()
            .map(|(&kind, recipe)| {
                let index = paths
                    .iter()
                    .enumerate()
                    .find(|(i, p)| *i > STATUS && !additive[*i] && **p == recipe.texture)
                    .map(|(i, _)| i)
                    .unwrap_or_else(|| {
                        paths.push(recipe.texture.clone());
                        additive.push(false);
                        paths.len() - 1
                    });
                (kind, (recipe.clone(), index))
            })
            .collect();
        let textures: Vec<_> = paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                server
                    .load_builder()
                    .with_settings(move |s: &mut ImageLoaderSettings| {
                        s.is_srgb = false;
                        // AssetServer shares the first load's settings by path.
                        // Match dialogue's sampler for the shared frame/status atlas.
                        s.sampler = if index == STATUS {
                            ImageSampler::Descriptor(ImageSamplerDescriptor {
                                address_mode_u: ImageAddressMode::Repeat,
                                address_mode_v: ImageAddressMode::Repeat,
                                ..ImageSamplerDescriptor::nearest()
                            })
                        } else {
                            ImageSampler::linear()
                        };
                    })
                    .load(path.clone())
            })
            .collect();
        let layers = vec![None; textures.len()];
        let refraction_texture = server
            .load_builder()
            .with_settings(|s: &mut ImageLoaderSettings| {
                s.is_srgb = false;
                s.sampler = ImageSampler::linear();
            })
            .load(spec.refraction.sprite.texture.clone());
        Ok(Self {
            spec,
            textures,
            layers,
            particles,
            sprites,
            additive,
            refraction_texture,
        })
    }
    pub fn despawn(&mut self, world: &mut World) {
        for (entity, _) in self.layers.iter_mut().filter_map(Option::take) {
            world.despawn(entity);
        }
    }
    pub(super) fn prepare(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        surfaces: &mut Assets<TitleSurface>,
    ) {
        for index in 0..self.layers.len() {
            if self.layers[index].is_some() {
                continue;
            }
            let mut batch = Batch::default();
            batch.sprite(
                Vec3::ZERO,
                Quat::IDENTITY,
                [1., 1.],
                [0., 0., 1., 1.],
                [1.; 4],
            );
            let mesh = meshes.add(batch.mesh());
            let surface = surfaces.add(TitleSurface {
                color: Some(self.textures[index].clone()),
                // The UI shares the status atlas with nearest filtering. Reuse
                // the prepared emote sampler for smooth world-space symbols.
                sampling: Some(self.textures[if index == STATUS { EMOTES } else { index }].clone()),
                blend: true,
                additive: self.additive[index],
                // Head emotes ignore depth so hair cannot obscure them; dust tests depth.
                depth_test: index > STATUS,
                depth_write: false,
                cull: resonance_content::CullFace::None,
                ..default()
            });
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(surface),
                    Transform::default(),
                    Visibility::Hidden,
                    NoFrustumCulling,
                    EffectDraw,
                    super::draw_order::DrawOrder(
                        super::draw_order::EFFECTS
                            + if index <= STATUS {
                                u16::MAX as u32
                            } else {
                                index as u32
                            },
                        0,
                    ),
                ))
                .id();
            self.layers[index] = Some((entity, mesh));
        }
    }
}

#[derive(Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}
impl Batch {
    fn sprite(
        &mut self,
        center: Vec3,
        rotation: Quat,
        size: [f32; 2],
        uv: [f32; 4],
        color: [f32; 4],
    ) {
        self.anchored_sprite(center, rotation, size, uv, color, VerticalAnchor::Center);
    }
    fn anchored_sprite(
        &mut self,
        center: Vec3,
        rotation: Quat,
        size: [f32; 2],
        uv: [f32; 4],
        color: [f32; 4],
        anchor: VerticalAnchor,
    ) {
        let right = rotation * Vec3::X * (size[0] / 2.).trunc();
        let [above, below] = match anchor {
            VerticalAnchor::Center => [(size[1] / 2.).trunc(); 2],
            VerticalAnchor::Bottom => [size[1], 0.],
            VerticalAnchor::Top => [0., size[1]],
        };
        let up = rotation * Vec3::Y * above;
        let down = rotation * Vec3::Y * below;
        let base = self.positions.len() as u32;
        self.positions.extend(
            [
                center - right + up,
                center + right + up,
                center + right - down,
                center - right - down,
            ]
            .map(|v| v.to_array()),
        );
        self.uv.extend([
            [uv[0], uv[1]],
            [uv[2], uv[1]],
            [uv[2], uv[3]],
            [uv[0], uv[3]],
        ]);
        self.colors.extend([color; 4]);
        self.indices
            .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    fn mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[allow(clippy::too_many_arguments)] // Cooked images, live state, current joint transforms, and sprite submission.
pub(super) fn render(
    mut commands: Commands,
    state: State,
    art: Res<Artwork>,
    images: Res<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    actors: Query<(&ActorPart, Option<&Rig>)>,
    names: Query<&Name>,
    helper: TransformHelper,
    mut applied: ResMut<Applied>,
) {
    if state.live.as_ref().is_some_and(|s| !s.ready_for_field) {
        return;
    }
    let world = &state.get().events.world;
    let Some(camera) = &world.field_camera else {
        return;
    };
    if art.textures.iter().any(|h| !images.contains(h)) {
        for &id in world.billboards.keys() {
            applied.loading(Request::Billboard(id));
        }
        for &id in world.emotes.keys() {
            applied.loading(Request::Emote(id));
        }
        if world.paralysis.is_some() {
            applied.loading(Request::Paralysis);
        }
        for particle in &world.particles {
            applied.loading(Request::Particle(particle.handle));
        }
        return;
    }
    let camera = Transform::from_translation(Vec3::from_array(camera.position))
        .looking_at(Vec3::from_array(camera.target), Vec3::Z);
    let side = Vec3::new(camera.right().x, camera.right().y, 0.).normalize_or_zero();
    let forward = Vec3::Z.cross(side);
    let brightness = world.brightness();
    let mut batches: Vec<_> = (0..art.layers.len()).map(|_| Batch::default()).collect();
    for particle in &world.particles {
        let Some((recipe, layer)) = art.particles.get(&particle.kind) else {
            continue;
        };
        let Some(flutter) = &particle.flutter else {
            continue;
        };
        let [x, y, z] = flutter.rotation.map(f32::to_radians);
        let rgb = particle.rgba.map(|v| (v * 4. / 255.).min(1.) * brightness);
        batches[*layer].sprite(
            Vec3::from_array(particle.position),
            Quat::from_euler(EulerRot::ZYX, z, y, x),
            [particle.size, particle.size / recipe.aspect_ratio],
            recipe.uv,
            [
                rgb[0],
                rgb[1],
                rgb[2],
                particle.alpha(world.tick).clamp(0., 255.) / 255.,
            ],
        );
        applied.ack(Request::Particle(particle.handle));
    }
    for (&id, effect) in &world.billboards {
        let Some(recipe) = art.spec.sprites.get(&effect.recipe) else {
            continue;
        };
        let rotation = camera.rotation * Quat::from_rotation_z(effect.rotation[2].to_radians());
        // Authored sprite colors use a gain of four.
        let rgb = effect.rgba[..3]
            .iter()
            .map(|v| (f32::from(*v) * 4. / 255.).min(1.) * brightness)
            .collect::<Vec<_>>();
        batches[art.sprites[&effect.recipe]].sprite(
            Vec3::from_array(effect.position),
            rotation,
            effect.size,
            recipe.uv,
            [
                rgb[0],
                rgb[1],
                rgb[2],
                effect.alpha(world.tick).clamp(0., 255.) / 255.,
            ],
        );
        applied.ack(Request::Billboard(id));
    }
    let roots: BTreeMap<_, _> = actors
        .iter()
        .filter(|(p, _)| p.part == 0)
        .map(|(p, rig)| (p.actor, (p, rig)))
        .collect();
    let emotes = world.emotes.iter().map(|(&id, emote)| {
        (
            Request::Emote(id),
            emote.actor,
            art.spec.emotes.get(&emote.kind),
            world.tick.saturating_sub(emote.start_tick) as usize,
            emote.phase,
            emote.offset,
            EMOTES,
        )
    });
    let paralysis = world.paralysis.map(|symbol| {
        (
            Request::Paralysis,
            symbol.actor,
            Some(&art.spec.paralysis),
            usize::from(symbol.frame),
            0,
            [0.; 3],
            STATUS,
        )
    });
    for (request, actor, track, age, phase, offset, layer) in emotes.chain(paralysis) {
        let Some(track) = track else {
            continue;
        };
        let sprites = track.frame_with_phase(age, phase);
        if sprites.is_empty() {
            applied.ack(request);
            continue;
        }
        let Some((part, rig)) = roots.get(&actor) else {
            continue;
        };
        let Some(rig) = rig.filter(|_| part.prepared) else {
            applied.loading(request);
            continue;
        };
        let Some(actor) = world.actors.get(&actor) else {
            continue;
        };
        let Some(anchor) = anchor_position(rig, track, actor.position, &names, &helper) else {
            continue;
        };
        for sprite in sprites {
            let [x, y, z] = std::array::from_fn(|i| sprite.offset[i] + offset[i]);
            let center = anchor + side * x + forward * y + Vec3::Z * z;
            // Snap emote centers to whole world units; keep their rotated vertices
            // and the independently moving dust particles at full precision.
            let center = center.trunc();
            let angle = sprite.rotation + track.rotation.angle(state.get().effect_clock.tick());
            let rotation = camera.rotation * Quat::from_rotation_z(angle.to_radians());
            batches[layer].anchored_sprite(
                center,
                rotation,
                sprite.size,
                sprite.uv,
                [
                    brightness,
                    brightness,
                    brightness,
                    f32::from(sprite.alpha) / 255.,
                ],
                sprite.vertical_anchor,
            );
        }
        applied.ack(request);
    }
    for (index, batch) in batches.into_iter().enumerate() {
        if batch.positions.is_empty() {
            if let Some((entity, _)) = &art.layers[index] {
                commands.entity(*entity).insert(Visibility::Hidden);
            }
            continue;
        }
        let mesh = batch.mesh();
        let (entity, handle) = art.layers[index]
            .as_ref()
            .expect("effect layer was not prepared");
        *meshes.get_mut(handle).expect("effect mesh is retained") = mesh;
        commands.entity(*entity).insert(Visibility::Inherited);
    }
}

fn anchor_position(
    rig: &Rig,
    track: &EmoteTrack,
    actor: [f32; 3],
    names: &Query<&Name>,
    helper: &TransformHelper,
) -> Option<Vec3> {
    match rig.bone(&track.anchor, names).ok()? {
        Some(bone) => helper
            .compute_global_transform(bone)
            .ok()
            .map(|t| t.translation()),
        None => Some(Vec3::from_array(actor) + Vec3::from_array(track.missing_anchor_offset)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_use_model_order_or_logical_position_but_never_hide_transform_errors() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let root = world.spawn(Transform::from_xyz(900., 800., 700.)).id();
        let mut node = |name, position| {
            world
                .spawn((
                    Name::new(name),
                    Transform::from_translation(position),
                    ChildOf(root),
                ))
                .id()
        };
        let _geometry = node("Bone_atama", Vec3::splat(999.));
        let prefix = node("Bone_atama_extra", Vec3::splat(1.));
        let wrong_case = node("bone_atama", Vec3::splat(2.));
        let head_a = node("Bone_atama", Vec3::splat(3.));
        let head_b = node("Bone_atama", Vec3::new(7., 8., 9.));
        let broken = world.spawn((Name::new("Bone_atama"), ChildOf(root))).id();
        let unlabeled = world.spawn(Transform::IDENTITY).id();
        let mut sample = |nodes: &[Entity], offset| {
            let rig = Rig::new(nodes.iter().map(|&e| (e, Transform::IDENTITY)).collect());
            let track = EmoteTrack {
                anchor: "Bone_atama".into(),
                missing_anchor_offset: offset,
                rotation: resonance_content::effect::EmoteRotation::Fixed,
                phase_count: 1,
                intro: Vec::new(),
                cycle: vec![Vec::new()],
            };
            world
                .run_system_once(move |names: Query<&Name>, helper: TransformHelper| {
                    anchor_position(&rig, &track, [10., 20., 30.], &names, &helper)
                })
                .unwrap()
        };
        assert_eq!(
            sample(&[prefix, head_b, head_a], [0., 0., 128.]),
            Some(Vec3::new(907., 808., 709.))
        );
        assert_eq!(
            sample(&[prefix, wrong_case], [0., 0., 128.]),
            Some(Vec3::new(10., 20., 158.))
        );
        assert_eq!(sample(&[], [0.; 3]), Some(Vec3::new(10., 20., 30.)));
        assert_eq!(sample(&[broken], [0., 0., 128.]), None);
        assert_eq!(sample(&[unlabeled], [0., 0., 128.]), None);
    }

    #[test]
    fn emote_vertical_anchors_preserve_odd_integer_heights() {
        for (anchor, expected) in [
            (VerticalAnchor::Center, [1., 1., -1., -1.]),
            (VerticalAnchor::Bottom, [3., 3., 0., 0.]),
            (VerticalAnchor::Top, [0., 0., -3., -3.]),
        ] {
            let mut batch = Batch::default();
            batch.anchored_sprite(
                Vec3::ZERO,
                Quat::IDENTITY,
                [3.; 2],
                [0.; 4],
                [1.; 4],
                anchor,
            );
            assert_eq!(
                batch.positions.iter().map(|p| p[1]).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                batch.positions.iter().map(|p| p[0]).collect::<Vec<_>>(),
                [-1., 1., 1., -1.]
            );
        }
    }
}
