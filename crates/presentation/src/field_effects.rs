//! Camera-facing sprites from cooked recipes and live event state.
use super::sparse_animation::affine::Helper as TransformHelper;
use super::{
    draw_order::{DrawOrder, EFFECT_UI_OFFSET, EFFECTS},
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
    effect::{FieldEffects, RefractionRecipe, VerticalAnchor},
    field::FieldAssets,
};
use resonance_events::effect::{Blend, RotationOrder, SpriteOrientation};
use std::{collections::BTreeMap, fs, path::Path};

const EMOTES: usize = 0;
const STATUS: usize = 1;

// Blend mode, then whether field fog is enabled.
type SpriteMaterials = [[usize; 2]; 3];

#[derive(Component)]
pub(super) struct EffectDraw;

#[derive(Resource)]
pub(super) struct Artwork {
    spec: FieldEffects,
    textures: Vec<Handle<Image>>,
    materials: Vec<Handle<TitleSurface>>,
    warm_mesh: Handle<Mesh>,
    draws: Vec<Vec<(Entity, Handle<Mesh>)>>,
    sprite_materials: BTreeMap<u16, SpriteMaterials>,
    overlay_materials: BTreeMap<u32, Vec<SpriteMaterials>>,
    refraction_texture: [Handle<Image>; 2],
}
impl Artwork {
    pub(super) fn palette(&self, index: u8) -> [u8; 4] {
        self.spec.palette[usize::from(index)]
    }
    pub(super) fn refraction(
        &self,
    ) -> (
        &RefractionRecipe,
        &resonance_content::effect::SpriteRecipe,
        &[Handle<Image>; 2],
    ) {
        (
            &self.spec.refraction,
            &self.spec.air_refraction,
            &self.refraction_texture,
        )
    }
    pub fn mouth_frame(&self, age: u32) -> u8 {
        self.spec.mouth_cycle[age as usize % self.spec.mouth_cycle.len()]
    }
    pub fn load(
        root: &Path,
        field: &FieldAssets,
        server: &AssetServer,
        meshes: &mut Assets<Mesh>,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<Self> {
        Self::load_with(root, field, server, meshes, surfaces, None)
    }
    pub fn load_with(
        root: &Path,
        field: &FieldAssets,
        server: &AssetServer,
        meshes: &mut Assets<Mesh>,
        surfaces: &mut Assets<TitleSurface>,
        files: Option<&resonance_content::prepared::Files>,
    ) -> Result<Self> {
        let mut spec: FieldEffects = if let Some(files) = files {
            files.json(&field.effects)?
        } else {
            serde_json::from_slice(&fs::read(root.join(&field.effects))?)?
        };
        spec.validate()?;
        for (&kind, leaf) in &field.particles {
            spec.sprites.insert(
                kind.try_into()?,
                resonance_content::effect::SpriteRecipe {
                    texture: leaf.texture.clone(),
                    uv: leaf.uv,
                    additive: false,
                    frames: Vec::new(),
                    repeat: false,
                },
            );
        }
        let emotes = load_image(server, &spec.emote_texture, false);
        let status = load_image(server, &spec.status_texture, true);
        let mut textures = vec![emotes.clone(), status.clone()];
        let mut materials: Vec<_> = [emotes.clone(), status]
            .map(|color| {
                surfaces.add(TitleSurface {
                    color: Some(color),
                    // World symbols share the UI atlas, with linear sampling.
                    sampling: Some(emotes.clone()),
                    blend: Some(Blend::Alpha),
                    depth_test: false,
                    depth_write: false,
                    cull: resonance_content::CullFace::None,
                    ..default()
                })
            })
            .into();
        let mut shared = BTreeMap::new();
        let mut register = |path: &str, blend, field_fog| {
            let key = (path.to_owned(), blend, field_fog);
            *shared.entry(key).or_insert_with(|| {
                let index = materials.len();
                let color = load_image(server, path, false);
                textures.push(color.clone());
                materials.push(surfaces.add(TitleSurface {
                    color: Some(color.clone()),
                    sampling: Some(color),
                    field_fog,
                    blend: Some(blend),
                    clamp_color: true,
                    depth_test: true,
                    depth_write: false,
                    cull: resonance_content::CullFace::None,
                    ..default()
                }));
                index
            })
        };
        let mut variants = |path: &str| {
            Blend::ALL.map(|mode| std::array::from_fn(|fog| register(path, mode, fog != 0)))
        };
        let sprite_materials = spec
            .sprites
            .iter()
            .map(|(&kind, recipe)| (kind, variants(&recipe.texture)))
            .collect();
        let mut overlay_materials = BTreeMap::new();
        for (&resource, path) in &field.overlays {
            let overlay: resonance_content::effect::OverlayArt = if let Some(files) = files {
                files.json(path)?
            } else {
                serde_json::from_slice(&fs::read(root.join(path))?)?
            };
            overlay_materials.insert(
                resource as u32,
                overlay
                    .textures
                    .iter()
                    .map(|t| variants(&t.images[0].path))
                    .collect(),
            );
        }
        let refraction_texture = [
            &spec.refraction.sprite.texture,
            &spec.air_refraction.texture,
        ]
        .map(|path| load_image(server, path, false));
        let warm = Quad::new(
            Vec3::ZERO,
            Quat::IDENTITY,
            [1.; 2],
            [0., 0., 1., 1.],
            [1.; 4],
            VerticalAnchor::Center,
        );
        Ok(Self {
            spec,
            textures,
            materials,
            warm_mesh: meshes.add(Quad::mesh(std::iter::once(&warm))),
            draws: Vec::new(),
            sprite_materials,
            overlay_materials,
            refraction_texture,
        })
    }
    pub fn despawn(&mut self, world: &mut World) {
        for (entity, _) in self.draws.drain(..).flatten() {
            world.despawn(entity);
        }
    }
    pub(super) fn prepared_bindings(
        &self,
    ) -> impl Iterator<Item = (Handle<Mesh>, Handle<TitleSurface>)> + '_ {
        self.materials
            .iter()
            .map(|material| (self.warm_mesh.clone(), material.clone()))
    }
}

fn load_image(server: &AssetServer, path: &str, nearest: bool) -> Handle<Image> {
    server
        .load_builder()
        .with_settings(move |s: &mut ImageLoaderSettings| {
            s.is_srgb = false;
            // AssetServer shares the first load's settings by path. Match the UI atlas.
            s.sampler = if nearest {
                ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    ..ImageSamplerDescriptor::nearest()
                })
            } else {
                ImageSampler::linear()
            };
        })
        .load(path.to_owned())
}

pub(super) struct Quad {
    positions: [[f32; 3]; 4],
    uv: [f32; 4],
    color: [f32; 4],
}
impl Quad {
    pub(super) fn new(
        center: Vec3,
        rotation: Quat,
        size: [f32; 2],
        uv: [f32; 4],
        color: [f32; 4],
        anchor: VerticalAnchor,
    ) -> Self {
        let right = rotation * Vec3::X * (size[0] / 2.).trunc();
        let [above, below] = match anchor {
            VerticalAnchor::Center => [(size[1] / 2.).trunc(); 2],
            VerticalAnchor::Bottom => [size[1], 0.],
            VerticalAnchor::Top => [0., size[1]],
        };
        let up = rotation * Vec3::Y * above;
        let down = rotation * Vec3::Y * below;
        Self {
            positions: [
                center - right + up,
                center + right + up,
                center + right - down,
                center - right - down,
            ]
            .map(|v| v.to_array()),
            uv,
            color,
        }
    }
    pub(super) fn mesh<'a>(quads: impl IntoIterator<Item = &'a Self>) -> Mesh {
        let mut positions = Vec::new();
        let mut uv = Vec::new();
        let mut colors = Vec::new();
        let mut indices = Vec::new();
        for quad in quads {
            let base = positions.len() as u32;
            positions.extend(quad.positions);
            let [left, top, right, bottom] = quad.uv;
            uv.extend([[left, top], [right, top], [right, bottom], [left, bottom]]);
            colors.extend([quad.color; 4]);
            indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        }
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
    }
}

pub(super) fn effect_rotation(
    orientation: SpriteOrientation,
    angles: [f32; 3],
    order: RotationOrder,
    camera: Quat,
) -> Quat {
    let [x, y, z] = angles.map(f32::to_radians);
    let (order, [a, b, c]) = match order {
        RotationOrder::Zyx => (EulerRot::ZYX, [z, y, x]),
        RotationOrder::Zxy => (EulerRot::ZXY, [z, x, y]),
        RotationOrder::Xyz => (EulerRot::XYZ, [x, y, z]),
        RotationOrder::Xzy => (EulerRot::XZY, [x, z, y]),
        RotationOrder::Yxz => (EulerRot::YXZ, [y, x, z]),
        RotationOrder::Yzx => (EulerRot::YZX, [y, z, x]),
    };
    let rotation = Quat::from_euler(order, a, b, c);
    match orientation {
        SpriteOrientation::Camera => camera * rotation,
        SpriteOrientation::World => rotation,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render(
    mut commands: Commands,
    state: State,
    mut art: ResMut<Artwork>,
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
    if art.draws.is_empty() {
        // Prepare each material before the first visible particle.
        let warm = meshes.get(&art.warm_mesh).unwrap().clone();
        art.draws = art
            .materials
            .iter()
            .map(|material| {
                let mesh = meshes.add(warm.clone());
                let entity = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                        NoFrustumCulling,
                        EffectDraw,
                    ))
                    .id();
                vec![(entity, mesh)]
            })
            .collect();
    }
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
        return;
    }
    let camera = super::field_view::camera_transform(camera);
    let side = Vec3::new(camera.right().x, camera.right().y, 0.).normalize_or_zero();
    let forward = Vec3::Z.cross(side);
    let brightness = world.brightness();
    let mut quads = Vec::new();
    let mut particles: Vec<_> = world.billboards.iter().collect();
    particles.sort_unstable_by_key(|(_, effect)| effect.draw_order);
    let mut previous_blend = Blend::Additive;
    for (&id, effect) in particles {
        if world.tick < effect.born {
            continue;
        }
        let Some(recipe) = art.spec.sprites.get(&effect.recipe) else {
            continue;
        };
        let rotation = effect_rotation(
            effect.orientation,
            effect.rotation,
            effect.rotation_order,
            camera.rotation,
        );
        let rgba = effect.rgba;
        let rgb = rgba.map(|v| f32::from(v) * 4. / 255. * brightness);
        let mut mode = effect.blend.unwrap_or(if recipe.additive {
            Blend::Additive
        } else {
            Blend::Alpha
        });
        if mode == Blend::Previous {
            mode = previous_blend;
        }
        previous_blend = mode;
        let (materials, uv) = if let Some((resource, image)) = effect.texture {
            (
                &art.overlay_materials[&resource][usize::from(image)],
                [0., 0., 1., 1.],
            )
        } else {
            (
                &art.sprite_materials[&effect.recipe],
                recipe.uv_at(world.tick.saturating_sub(effect.born) + effect.texture_phase),
            )
        };
        let layer = materials[mode as usize][usize::from(effect.field_fog)];
        let quad = Quad::new(
            Vec3::from_array(effect.position),
            rotation,
            effect.size,
            effect.uv.unwrap_or(uv),
            [
                rgb[0],
                rgb[1],
                rgb[2],
                effect.alpha(world.tick).clamp(0., 255.) / 255.,
            ],
            effect.anchor,
        );
        quads.push((EFFECTS, layer, quad));
        applied.ack(Request::Billboard(id));
    }
    let roots: BTreeMap<_, _> = actors
        .iter()
        .filter(|(p, _)| p.part == 0)
        .map(|(p, rig)| (p.actor, (p, rig)))
        .collect();
    let mut emotes: Vec<_> = world.emotes.iter().collect();
    emotes.sort_unstable_by_key(|(_, emote)| emote.draw_order);
    let emotes = emotes.into_iter().map(|(&id, emote)| {
        (
            Request::Emote(id),
            emote.actor,
            ("Bone_atama", [0., 0., 128.]),
            resonance_events::emote::sprites(
                emote.kind,
                world.tick.saturating_sub(emote.start_tick),
                emote.phase,
            ),
            emote.offset,
            EMOTES,
        )
    });
    let paralysis = world.paralysis.map(|symbol| {
        (
            Request::Paralysis,
            symbol.actor,
            ("Bone_atama", [0.; 3]),
            vec![resonance_events::emote::paralysis(symbol.frame)],
            [0.; 3],
            STATUS,
        )
    });
    for (request, actor, (anchor_name, fallback), sprites, offset, layer) in emotes.chain(paralysis)
    {
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
        let Some(anchor) =
            anchor_position(rig, anchor_name, fallback, actor.position, &names, &helper)
        else {
            continue;
        };
        for sprite in sprites.iter() {
            let [x, y, z] = std::array::from_fn(|i| sprite.offset[i] + offset[i]);
            let center = (anchor + side * x + forward * y + Vec3::Z * z).trunc();
            let rotation = Quat::from_rotation_z(sprite.rotation.to_radians());
            let mut quad = Quad::new(
                Vec3::ZERO,
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
            for vertex in &mut quad.positions {
                *vertex = (center + camera.rotation * Vec3::from_array(*vertex)).to_array();
            }
            quads.push((EFFECTS + EFFECT_UI_OFFSET, layer, quad));
        }
        applied.ack(request);
    }
    // Only adjacent, compatible draws may merge; translucent overlap is ordered.
    let mut used = vec![0; art.materials.len()];
    for (order, run) in quads.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1)).enumerate() {
        let (pass, layer, _) = run[0];
        let batch = Quad::mesh(run.iter().map(|(_, _, quad)| quad));
        let (entity, mesh) = if let Some((entity, mesh)) = art.draws[layer].get(used[layer]) {
            *meshes.get_mut(mesh).unwrap() = batch;
            (*entity, mesh.clone())
        } else {
            let mesh = meshes.add(batch);
            let entity = commands
                .spawn((Transform::default(), NoFrustumCulling, EffectDraw))
                .id();
            art.draws[layer].push((entity, mesh.clone()));
            (entity, mesh)
        };
        commands.entity(entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(art.materials[layer].clone()),
            Visibility::Inherited,
            DrawOrder(pass, order),
        ));
        used[layer] += 1;
    }
    let warm = meshes.get(&art.warm_mesh).unwrap().clone();
    for (draws, used) in art.draws.iter().zip(used) {
        for (entity, mesh) in draws.iter().skip(used) {
            *meshes.get_mut(mesh).unwrap() = warm.clone();
            commands.entity(*entity).insert(Visibility::Inherited);
        }
    }
}

fn anchor_position(
    rig: &Rig,
    anchor: &str,
    fallback: [f32; 3],
    actor: [f32; 3],
    names: &Query<&Name>,
    helper: &TransformHelper,
) -> Option<Vec3> {
    match rig.bone(anchor, names).ok()? {
        Some(bone) => helper
            .compute_global_transform(bone)
            .ok()
            .map(|t| t.translation()),
        None => Some(Vec3::from_array(actor) + Vec3::from_array(fallback)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_order_changes_the_bolt_axis_before_camera_orientation() {
        let axis = |order, orientation, camera| {
            effect_rotation(orientation, [90.; 3], order, camera) * Vec3::Y
        };
        assert!(
            axis(RotationOrder::Zyx, SpriteOrientation::World, Quat::IDENTITY)
                .abs_diff_eq(Vec3::Y, 0.0001)
        );
        assert!(
            axis(RotationOrder::Yxz, SpriteOrientation::World, Quat::IDENTITY)
                .abs_diff_eq(Vec3::Z, 0.0001)
        );
        let camera = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        assert!(
            axis(RotationOrder::Yxz, SpriteOrientation::Camera, camera)
                .abs_diff_eq(Vec3::NEG_Y, 0.0001)
        );
    }

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
            world
                .run_system_once(move |names: Query<&Name>, helper: TransformHelper| {
                    anchor_position(&rig, "Bone_atama", offset, [10., 20., 30.], &names, &helper)
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
            let quad = Quad::new(
                Vec3::ZERO,
                Quat::IDENTITY,
                [3.; 2],
                [0.; 4],
                [1.; 4],
                anchor,
            );
            assert_eq!(
                quad.positions.iter().map(|p| p[1]).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                quad.positions.iter().map(|p| p[0]).collect::<Vec<_>>(),
                [-1., 1., 1., -1.]
            );
        }
    }
}
