//! Camera-facing sprites from cooked recipes and live event state.
use super::sparse_animation::affine::Helper as TransformHelper;
use super::{
    draw_order::{CONTACT_SHADOWS, DrawOrder, EFFECT_UI_OFFSET, EFFECTS},
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
use resonance_events::effect::{Blend, SpriteOrientation};
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
    draws: Vec<(Entity, Handle<Mesh>)>,
    particles: BTreeMap<i32, (FlutterRecipe, usize)>,
    shadow: (resonance_content::field::ContactShadow, usize),
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
        let spec: FieldEffects = if let Some(files) = files {
            files.json(&field.effects)?
        } else {
            serde_json::from_slice(&fs::read(root.join(&field.effects))?)?
        };
        spec.validate()?;
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
        let particles = field
            .particles
            .iter()
            .map(|(&kind, recipe)| {
                (
                    kind,
                    (
                        recipe.clone(),
                        register(&recipe.texture, Blend::Alpha, true),
                    ),
                )
            })
            .collect();
        let shadow = (
            field.contact_shadow.clone(),
            register(&field.contact_shadow.texture, Blend::Alpha, true),
        );
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
            particles,
            sprite_materials,
            overlay_materials,
            shadow,
            refraction_texture,
        })
    }
    pub fn despawn(&mut self, world: &mut World) {
        for (entity, _) in self.draws.drain(..) {
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

struct Quad {
    positions: [[f32; 3]; 4],
    uv: [f32; 4],
    color: [f32; 4],
}
impl Quad {
    fn new(
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
            VerticalAnchor::UpperHalf => [(size[1] / 2.).trunc(), 0.],
            VerticalAnchor::LowerHalf => [0., (size[1] / 2.).trunc()],
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
    fn mesh<'a>(quads: impl IntoIterator<Item = &'a Self>) -> Mesh {
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
    camera: Quat,
) -> Quat {
    let [x, y, z] = angles.map(f32::to_radians);
    let rotation = Quat::from_euler(EulerRot::ZYX, z, y, x);
    match orientation {
        SpriteOrientation::Camera => camera * rotation,
        SpriteOrientation::World => rotation,
    }
}

#[allow(clippy::too_many_arguments)] // Cooked images, live state, current joint transforms, and sprite submission.
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
    let camera = super::field_view::camera_transform(camera);
    let side = Vec3::new(camera.right().x, camera.right().y, 0.).normalize_or_zero();
    let forward = Vec3::Z.cross(side);
    let brightness = world.brightness();
    let mut quads = Vec::new();
    for particle in &world.particles {
        let Some((recipe, layer)) = art.particles.get(&particle.kind) else {
            continue;
        };
        let Some(flutter) = &particle.flutter else {
            continue;
        };
        let [x, y, z] = flutter.rotation.map(f32::to_radians);
        let (position, size, rgba) = particle.sample(world.tick);
        let rgb = rgba.map(|v| v * 4. / 255. * brightness);
        let quad = Quad::new(
            Vec3::from_array(position),
            Quat::from_euler(EulerRot::ZYX, z, y, x),
            [size, size / recipe.aspect_ratio],
            recipe.uv,
            [rgb[0], rgb[1], rgb[2], rgba[3].min(255.) / 255.],
            VerticalAnchor::Center,
        );
        quads.push((EFFECTS, particle.handle, *layer, quad));
        applied.ack(Request::Particle(particle.handle));
    }
    let collision = world.ring_shadows().next().map(|_| state.get().collision());
    for position in world.ring_shadows() {
        if let Some(surface) = collision.as_ref().and_then(|c| c.surface_below(position)) {
            let (spec, layer) = &art.shadow;
            let quad = Quad::new(
                Vec3::new(
                    position[0],
                    position[1],
                    surface.height + spec.height_offset,
                ),
                Quat::from_rotation_arc(Vec3::Z, Vec3::from_array(surface.normal)),
                [40.; 2],
                [0., 0., spec.uv_size[0], spec.uv_size[1]],
                [0., 0., 1., 64. / 255.],
                VerticalAnchor::Center,
            );
            quads.push((CONTACT_SHADOWS, 0, *layer, quad));
        }
    }
    for (&id, effect) in &world.billboards {
        let Some(recipe) = art.spec.sprites.get(&effect.recipe) else {
            continue;
        };
        // Camera-facing sprites use only roll; model and refraction planes can tilt.
        let angles = match effect.orientation {
            SpriteOrientation::Camera => [0., 0., effect.rotation[2]],
            SpriteOrientation::World => effect.rotation,
        };
        let rotation = effect_rotation(effect.orientation, angles, camera.rotation);
        let mut rgba = effect.rgba;
        if let Some(palette) = effect
            .palette
            .and_then(|index| art.spec.palette.get(usize::from(index)))
        {
            for channel in 0..3 {
                if rgba[channel] == resonance_events::effect::NEUTRAL_TINT {
                    rgba[channel] = palette[channel];
                }
            }
        }
        // Unlit effects still respect the incoming field's initial black hold.
        let brightness = if effect.field_lighting || world.fade.is_none() {
            brightness
        } else {
            1.
        };
        let rgb = rgba.map(|v| f32::from(v) * 4. / 255. * brightness);
        let mode = effect.blend.unwrap_or(if recipe.additive {
            Blend::Additive
        } else {
            Blend::Alpha
        });
        let (materials, uv) = if let Some((resource, image)) = effect.texture {
            (
                &art.overlay_materials[&resource][usize::from(image)],
                [0., 0., 1., 1.],
            )
        } else {
            (
                &art.sprite_materials[&effect.recipe],
                recipe.uv_at(world.tick.saturating_sub(effect.born) + 1),
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
        quads.push((EFFECTS, id, layer, quad));
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
            [0.; 3],
            STATUS,
        )
    });
    for (request, actor, track, age, offset, layer) in emotes.chain(paralysis) {
        let Some(track) = track else {
            continue;
        };
        let sprites = track.frame(age);
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
            let rotation = camera.rotation * Quat::from_rotation_z(sprite.rotation.to_radians());
            let quad = Quad::new(
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
            quads.push((EFFECTS + EFFECT_UI_OFFSET, layer as i32, layer, quad));
        }
        applied.ack(request);
    }
    // Only adjacent, compatible draws may merge; translucent overlap is ordered.
    quads.sort_by_key(|(pass, id, _, _)| (*pass, *id));
    let mut used = 0;
    for run in quads.chunk_by(|a, b| (a.0, a.2) == (b.0, b.2)) {
        let (pass, _, layer, _) = run[0];
        let batch = Quad::mesh(run.iter().map(|(_, _, _, quad)| quad));
        let (entity, mesh) = if let Some((entity, mesh)) = art.draws.get(used) {
            *meshes.get_mut(mesh).unwrap() = batch;
            (*entity, mesh.clone())
        } else {
            let mesh = meshes.add(batch);
            let entity = commands
                .spawn((Transform::default(), NoFrustumCulling, EffectDraw))
                .id();
            art.draws.push((entity, mesh.clone()));
            (entity, mesh)
        };
        commands.entity(entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(art.materials[layer].clone()),
            Visibility::Inherited,
            DrawOrder(pass, used),
        ));
        used += 1;
    }
    for &(entity, _) in art.draws.iter().skip(used) {
        commands.entity(entity).insert(Visibility::Hidden);
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
            (VerticalAnchor::UpperHalf, [1., 1., 0., 0.]),
            (VerticalAnchor::LowerHalf, [0., 0., -1., -1.]),
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
