//! Camera-facing sprites from cooked recipes and live event state.
use super::sparse_animation::affine::Helper as TransformHelper;
use super::{
    draw_order::{DrawOrder, Layer},
    field_animation::Rig,
    field_audit::{Applied, Request},
    field_view::{ActorPart, State},
    materials::TitleSurface,
};
use anyhow::{Context as ContextExt, Result};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use resonance_content::{
    effect::{FieldEffects, RefractionRecipe, VerticalAnchor},
    field::{AmbientEmission, FieldAssets, HeatHaze},
};
use resonance_events::effect::{Blend, RotationOrder, SpriteOrientation};
use std::collections::BTreeMap;

const EMOTES: usize = 0;
const STATUS: usize = 1;

// Blend mode, then whether field fog is enabled.
type SpriteMaterials = [[usize; 2]; 3];
type Bindings = BTreeMap<(String, Blend, bool), usize>;

/// Fixed default bindings have one owner; live draws and overrides remain scene-owned.
#[derive(Resource)]
pub(crate) struct Shared(Artwork);

impl Shared {
    pub(crate) fn new(
        settings: &resonance_content::field::settings::Settings,
        server: &AssetServer,
        meshes: &mut Assets<Mesh>,
        surfaces: &mut Assets<TitleSurface>,
        files: &resonance_content::prepared::Files,
    ) -> Result<Self> {
        let mut spec = settings.effects.clone();
        merge_particles(&mut spec, &settings.particles)?;
        Ok(Self(Artwork::build(
            spec, None, server, meshes, surfaces, files, None,
        )?))
    }

    pub(crate) fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.0.textures.iter().chain(&self.0.refraction_texture)
    }

    pub(crate) fn bindings(
        &self,
    ) -> impl Iterator<Item = (Handle<Mesh>, Handle<TitleSurface>)> + '_ {
        self.0.prepared_bindings()
    }
}

fn merge_particles(
    spec: &mut FieldEffects,
    particles: &BTreeMap<i32, resonance_content::effect::FlutterRecipe>,
) -> Result<()> {
    for (&kind, leaf) in particles {
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
    Ok(())
}

#[derive(Component)]
pub(super) struct EffectDraw;

#[derive(Resource)]
pub(super) struct Artwork {
    drawn: std::collections::BTreeSet<Request>,
    drawn_tick: Option<u32>,
    spec: FieldEffects,
    textures: Vec<Handle<Image>>,
    materials: Vec<Handle<TitleSurface>>,
    bindings: Bindings,
    warm_mesh: Handle<Mesh>,
    draws: Vec<Vec<(Entity, Handle<Mesh>)>>,
    sprite_materials: BTreeMap<u16, SpriteMaterials>,
    overlay_materials: BTreeMap<u32, Vec<Option<SpriteMaterials>>>,
    refraction_texture: [Handle<Image>; 2],
    heat_haze: Option<(HeatHaze, Handle<Image>)>,
    ambient: super::field_ambient::Ambient,
}
impl Artwork {
    pub(super) fn drawn(&self, request: &Request, tick: u32) -> bool {
        self.drawn_tick == Some(tick) && self.drawn.contains(request)
    }
    pub(super) fn diagnostic(&self, world: &resonance_events::GameWorld) -> serde_json::Value {
        serde_json::json!({
            "emotes": world.emotes.iter().filter(|(id, _)| self.drawn(&Request::Emote(**id), world.tick))
                .map(|(id, emote)| serde_json::json!({"id":id,"actor":emote.actor,"kind":emote.kind})).collect::<Vec<_>>(),
            "billboards": world.billboards.iter().filter(|(id, _)| self.drawn(&Request::Billboard(**id), world.tick))
                .map(|(id, effect)| serde_json::json!({"id":id,"recipe":effect.recipe})).collect::<Vec<_>>()
        })
    }

    pub(super) fn palette(&self, index: u8) -> [u8; 4] {
        self.spec.palette[usize::from(index)]
    }
    pub(super) fn heat_haze(&self) -> Option<(&HeatHaze, &Handle<Image>)> {
        self.heat_haze
            .as_ref()
            .map(|(recipe, image)| (recipe, image))
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
        field: &FieldAssets,
        server: &AssetServer,
        meshes: &mut Assets<Mesh>,
        surfaces: &mut Assets<TitleSurface>,
        files: &resonance_content::prepared::Files,
        common: Option<&resonance_content::field::document::Common>,
        shared: Option<&Shared>,
    ) -> Result<Self> {
        let settings = resonance_content::field::document::Common::settings(
            &files.read(&field.effects)?,
            common,
        )
        .with_context(|| format!("decode {}", field.effects))?;
        let mut spec = settings.effects.clone();
        resonance_content::field::document::Common::validate_effects(&settings, common)?;
        merge_particles(&mut spec, &field.particles)?;
        Self::build(spec, Some(field), server, meshes, surfaces, files, shared)
    }

    fn build(
        spec: FieldEffects,
        field: Option<&FieldAssets>,
        server: &AssetServer,
        meshes: &mut Assets<Mesh>,
        surfaces: &mut Assets<TitleSurface>,
        files: &resonance_content::prepared::Files,
        shared: Option<&Shared>,
    ) -> Result<Self> {
        let base = shared.map(|shared| &shared.0).filter(|base| {
            base.spec.emote_texture == spec.emote_texture
                && base.spec.status_texture == spec.status_texture
        });
        let mut textures = base.map_or_else(
            || {
                vec![
                    load_image(server, &spec.emote_texture, false),
                    load_image(server, &spec.status_texture, true),
                ]
            },
            |base| base.textures.clone(),
        );
        let emotes = textures[EMOTES].clone();
        let status = textures[STATUS].clone();
        let mut materials: Vec<_> = base.map_or_else(
            || {
                [emotes.clone(), status]
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
                    .into()
            },
            |base| base.materials.clone(),
        );
        let mut bindings = base.map_or_else(BTreeMap::new, |base| base.bindings.clone());
        let mut register = |path: &str, blend, field_fog| {
            let key = (path.to_owned(), blend, field_fog);
            *bindings.entry(key).or_insert_with(|| {
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
        for (&resource, path) in field.into_iter().flat_map(|field| &field.overlays) {
            let overlay =
                resonance_content::effect::OverlayArt::load(path, |path| files.read(path))?;
            overlay_materials.insert(
                resource as u32,
                overlay
                    .textures
                    .iter()
                    .map(|t| t.as_ref().map(|t| variants(&t.images[0])))
                    .collect(),
            );
        }
        let mut ambient_bindings: Vec<_> = field
            .into_iter()
            .flat_map(|field| &field.environment.particles)
            .map(|recipe| {
                let blend = if recipe.sprite.additive {
                    Blend::Additive
                } else {
                    Blend::Alpha
                };
                (
                    recipe.clone(),
                    register(&recipe.sprite.texture, blend, true),
                )
            })
            .collect();
        for (recipe, material) in &mut ambient_bindings {
            if let AmbientEmission::Snow {
                fog_color,
                fog_range,
                ..
            } = recipe.emission
            {
                let mut surface = surfaces.get(&materials[*material]).unwrap().clone();
                surface.field_fog = false;
                surface.fog_color = Vec4::from_array(fog_color.map(|v| f32::from(v) / 255.));
                // Snow fades into exponential fog rather than the scene's lighting.
                surface.fog_range = Vec4::new(fog_range[0], fog_range[1], 1., 0.);
                *material = materials.len();
                materials.push(surfaces.add(surface));
            }
        }
        let ambient = super::field_ambient::Ambient::new(ambient_bindings);
        let refraction_texture = [
            &spec.refraction.sprite.texture,
            &spec.air_refraction.texture,
        ]
        .map(|path| load_image(server, path, false));
        let heat_haze = field
            .and_then(|field| field.environment.heat_haze.as_ref())
            .map(|recipe| {
                let image = load_image(server, &recipe.sprite.texture, false);
                textures.push(image.clone());
                (recipe.clone(), image)
            });
        let warm = Quad::new(
            Vec3::ZERO,
            Quat::IDENTITY,
            [1.; 2],
            [0., 0., 1., 1.],
            [1.; 4],
            VerticalAnchor::Center,
        );
        Ok(Self {
            drawn: Default::default(),
            drawn_tick: None,
            spec,
            textures,
            materials,
            bindings,
            warm_mesh: base.map_or_else(
                || meshes.add(Quad::mesh(std::iter::once(&warm))),
                |base| base.warm_mesh.clone(),
            ),
            draws: Vec::new(),
            sprite_materials,
            overlay_materials,
            refraction_texture,
            heat_haze,
            ambient,
        })
    }
    pub fn despawn(&mut self, world: &mut World) {
        self.ambient.reset();
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
    fn visible(&self) -> bool {
        self.color[3] > 0.
            && Vec3::from_array(self.positions[0]).distance(Vec3::from_array(self.positions[1]))
                > 1.
            && Vec3::from_array(self.positions[0]).distance(Vec3::from_array(self.positions[3]))
                > 1.
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
    let field = state.get();
    let world = &field.events.world;
    let resources = field.events.resources();
    art.drawn.clear();
    art.drawn_tick = Some(world.tick);
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
    art.ambient.render(world, camera.rotation, &mut quads);
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
            let Some(materials) = &art.overlay_materials[&resource][usize::from(image)] else {
                continue;
            };
            (materials, [0., 0., 1., 1.])
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
        if quad.visible() {
            art.drawn.insert(Request::Billboard(id));
        }
        quads.push((Layer::Effects, layer, quad));
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
            ("head", [0., 0., 128.]),
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
            ("head", [0.; 3]),
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
        let Some(actor_state) = world.actors.get(&actor) else {
            continue;
        };
        let anchor = if let Some(anchor) = locator_anchor_position(resources, actor_state, fallback)
        {
            anchor
        } else {
            let Some((part, rig)) = roots.get(&actor) else {
                continue;
            };
            let Some(rig) = rig.filter(|_| part.prepared) else {
                applied.loading(request);
                continue;
            };
            let Some(anchor) = anchor_position(
                rig,
                anchor_name,
                fallback,
                actor_state.position,
                &names,
                &helper,
            ) else {
                continue;
            };
            anchor
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
            if quad.visible() {
                art.drawn.insert(request.clone());
            }
            quads.push((Layer::EffectOverlay, layer, quad));
        }
        applied.ack(request);
    }
    let art = &mut *art;
    sync_draws(
        &mut commands,
        &mut meshes,
        &art.materials,
        &mut art.draws,
        &quads,
    );
}

fn sync_draws(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &[Handle<TitleSurface>],
    draws: &mut Vec<Vec<(Entity, Handle<Mesh>)>>,
    quads: &[(Layer, usize, Quad)],
) {
    // field_warm prepares every binding offscreen. Live draws only need meshes
    // for actual effects; rewriting placeholder meshes would upload all unused
    // material variants every frame, even in fields without any particles.
    draws.resize_with(materials.len(), Vec::new);
    // Only adjacent, compatible draws may merge; translucent overlap is ordered.
    let mut used = vec![0; materials.len()];
    for (order, run) in quads.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1)).enumerate() {
        let (pass, layer, _) = run[0];
        let batch = Quad::mesh(run.iter().map(|(_, _, quad)| quad));
        let (entity, mesh) = if let Some((entity, mesh)) = draws[layer].get(used[layer]) {
            *meshes.get_mut(mesh).unwrap() = batch;
            (*entity, mesh.clone())
        } else {
            let mesh = meshes.add(batch);
            let entity = commands
                .spawn((Transform::default(), NoFrustumCulling, EffectDraw))
                .id();
            draws[layer].push((entity, mesh.clone()));
            (entity, mesh)
        };
        commands.entity(entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(materials[layer].clone()),
            Visibility::Inherited,
            DrawOrder(pass, order, 0),
        ));
        used[layer] += 1;
    }
    for (draws, used) in draws.iter().zip(used) {
        for (entity, _) in draws.iter().skip(used) {
            commands.entity(*entity).insert(Visibility::Hidden);
        }
    }
}

fn locator_anchor_position(
    resources: &resonance_events::ResourceLibrary,
    actor: &resonance_events::Actor,
    fallback: [f32; 3],
) -> Option<Vec3> {
    resources
        .locators
        .contains(&i32::try_from(actor.resource).ok()?)
        .then(|| Vec3::from_array(actor.position) + Vec3::from_array(fallback))
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
    fn default_effect_bindings_are_reused_without_retaining_field_overrides_or_draws() -> Result<()>
    {
        let root = tempfile::tempdir()?;
        std::fs::create_dir(root.path().join("effects"))?;
        std::fs::write(
            root.path().join("effects/default.ktx2"),
            resonance_asset_writer::ktx2::encode_levels(1, 1, &[&[255; 4]])?,
        )?;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: root.path().to_string_lossy().into_owned(),
                ..default()
            },
            bevy::image::ImagePlugin::default(),
        ));
        app.finish();
        app.cleanup();
        let server = app.world().resource::<AssetServer>().clone();
        let settings = resonance_content::field::settings::test_fixture();
        let mut meshes = Assets::<Mesh>::default();
        let mut surfaces = Assets::<TitleSurface>::default();
        let files = resonance_content::prepared::Files::default();
        let shared = Shared::new(&settings, &server, &mut meshes, &mut surfaces, &files)?;
        let counts = (meshes.len(), surfaces.len());
        let mut first = Artwork::build(
            settings.effects.clone(),
            None,
            &server,
            &mut meshes,
            &mut surfaces,
            &files,
            Some(&shared),
        )?;
        let second = Artwork::build(
            settings.effects.clone(),
            None,
            &server,
            &mut meshes,
            &mut surfaces,
            &files,
            Some(&shared),
        )?;
        assert_eq!(
            (meshes.len(), surfaces.len()),
            counts,
            "field reentry allocated default GPU bindings again"
        );
        assert_eq!(first.warm_mesh.id(), shared.0.warm_mesh.id());
        assert_eq!(first.materials, shared.0.materials);
        assert_eq!(second.materials, shared.0.materials);

        let mut overridden = settings.effects.clone();
        overridden.sprites.get_mut(&0).unwrap().texture = "effects/override.ktx2".into();
        let local = Artwork::build(
            overridden,
            None,
            &server,
            &mut meshes,
            &mut surfaces,
            &files,
            Some(&shared),
        )?;
        let key = ("effects/override.ktx2".into(), Blend::Alpha, false);
        assert!(local.bindings.contains_key(&key));
        assert!(
            !shared.0.bindings.contains_key(&key),
            "a local variant enlarged the startup inventory"
        );
        assert_eq!(shared.0.materials.len(), counts.1);
        first.draws.push(vec![(
            app.world_mut().spawn_empty().id(),
            meshes.add(Mesh::from(Rectangle::new(1., 1.))),
        )]);
        first.spec.palette[0] = [0; 4];
        assert!(second.draws.is_empty() && shared.0.draws.is_empty());
        assert_eq!(second.spec.palette[0], [255; 4]);
        assert_eq!(shared.0.spec.palette[0], [255; 4]);
        Ok(())
    }

    #[test]
    fn field522_question_emote_anchors_to_its_invisible_locator() {
        let mut resources = resonance_events::ResourceLibrary::default();
        resources.locators.insert(24);
        // Both original field522 scripts spawn this model-less actor101, then
        // emit Question(-100) on it during Sheena's two-world explanation.
        let mut actor = resonance_events::Actor::new(24, [-464., 933., -87.]);
        actor.visible = false;
        actor.interaction_anchor = true;
        let mut world = resonance_events::GameWorld::default();
        world.actors.insert(101, actor);
        world.emotes.insert(
            -100,
            resonance_events::Emote {
                draw_order: 0,
                phase: 0,
                actor: 101,
                kind: resonance_events::emote::Kind::Question,
                offset: [0.; 3],
                start_tick: 0,
                duration: Some(60),
            },
        );
        let emote = &world.emotes[&-100];
        let actor = world.actors.get_mut(&emote.actor).unwrap();
        assert!(!resonance_events::emote::sprites(emote.kind, 8, emote.phase).is_empty());
        assert_eq!(
            locator_anchor_position(&resources, &actor, [0., 0., 128.]),
            Some(Vec3::new(-464., 933., 41.))
        );
        actor.resource = 5;
        assert_eq!(locator_anchor_position(&resources, &actor, [0.; 3]), None);
        actor.resource = 24;
        resources.locators.clear();
        assert_eq!(locator_anchor_position(&resources, &actor, [0.; 3]), None);
    }

    #[test]
    fn inactive_effects_stop_uploading_and_reused_draws_preserve_order() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Mesh>();
        let mut surfaces = Assets::<TitleSurface>::default();
        let materials: Vec<_> = (0..3)
            .map(|_| surfaces.add(TitleSurface::default()))
            .collect();
        let mut draws = Vec::new();
        let quad = |layer, x| {
            (
                Layer::Effects,
                layer,
                Quad::new(
                    Vec3::new(x, 0., 0.),
                    Quat::IDENTITY,
                    [16.; 2],
                    [0., 0., 1., 1.],
                    [1.; 4],
                    VerticalAnchor::Center,
                ),
            )
        };
        let mut update = |app: &mut App, quads: &[_]| {
            app.world_mut()
                .resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
                    sync_draws(
                        &mut world.commands(),
                        &mut meshes,
                        &materials,
                        &mut draws,
                        quads,
                    );
                });
            app.world_mut().flush();
            app.update();
            app.world_mut()
                .resource_mut::<Messages<AssetEvent<Mesh>>>()
                .drain()
                .collect::<Vec<_>>()
        };
        assert!(update(&mut app, &[]).is_empty());
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
        update(&mut app, &[quad(1, 0.), quad(0, 10.), quad(1, 20.)]);
        let mut query = app
            .world_mut()
            .query::<(Entity, &Mesh3d, &MeshMaterial3d<TitleSurface>, &DrawOrder)>();
        let mut original: Vec<_> = query
            .iter(app.world())
            .map(|(e, mesh, material, order)| (order.1, e, mesh.0.clone(), material.0.clone()))
            .collect();
        original.sort_by_key(|(order, ..)| *order);
        assert_eq!(original.len(), 3);
        assert_eq!(
            original.iter().map(|(_, _, _, m)| m).collect::<Vec<_>>(),
            [&materials[1], &materials[0], &materials[1]]
        );
        for _ in 0..3 {
            assert!(
                update(&mut app, &[]).is_empty(),
                "inactive meshes must not be uploaded"
            );
            for (_, entity, mesh, _) in &original {
                assert_eq!(
                    app.world().get::<Visibility>(*entity),
                    Some(&Visibility::Hidden)
                );
                assert_eq!(
                    app.world()
                        .resource::<Assets<Mesh>>()
                        .get(mesh)
                        .unwrap()
                        .count_vertices(),
                    4
                );
            }
        }
        update(
            &mut app,
            &[quad(1, 30.), quad(1, 40.), quad(0, 50.), quad(1, 60.)],
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 3);
        for (order, entity, mesh, _) in &original {
            assert_eq!(
                app.world().get::<Visibility>(*entity),
                Some(&Visibility::Inherited)
            );
            assert_eq!(app.world().get::<Mesh3d>(*entity).unwrap().0, *mesh);
            assert_eq!(app.world().get::<DrawOrder>(*entity).unwrap().1, *order);
            let mesh = app.world().resource::<Assets<Mesh>>().get(mesh).unwrap();
            assert_eq!(mesh.count_vertices(), if *order == 0 { 8 } else { 4 });
            let bevy::mesh::VertexAttributeValues::Float32x3(positions) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
            else {
                panic!("positions")
            };
            assert_eq!(positions[0][0], [22., 42., 52.][*order]);
        }
    }

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
        let _geometry = node("head", Vec3::splat(999.));
        let prefix = node("Bone_atama_extra", Vec3::splat(1.));
        let wrong_case = node("bone_atama", Vec3::splat(2.));
        let head_a = node("head", Vec3::splat(3.));
        let head_b = node("head", Vec3::new(7., 8., 9.));
        let broken = world.spawn((Name::new("head"), ChildOf(root))).id();
        let unlabeled = world.spawn(Transform::IDENTITY).id();
        let mut sample = |nodes: &[Entity], offset| {
            let rig = Rig::new(nodes.iter().map(|&e| (e, Transform::IDENTITY)).collect());
            world
                .run_system_once(move |names: Query<&Name>, helper: TransformHelper| {
                    anchor_position(&rig, "head", offset, [10., 20., 30.], &names, &helper)
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
