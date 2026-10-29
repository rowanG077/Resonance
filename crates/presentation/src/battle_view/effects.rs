//! Reusable draws with every material and pipeline prepared before entry.
//! Particle state and submission order are supplied by the scene animation owner.
use resonance_events::effect::Blend;
#[path = "effects_geometry.rs"]
mod geometry;

use super::{ActorOrder, WARM_LAYER};
use crate::{
    draw_order::{DrawOrder, Layer},
    materials::TitleSurface,
    scene::{SampledImages, sampled_image},
};
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers},
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
};
use resonance_battle::{BattleFrame, ParticleFrame, ResourceId};
use resonance_content::{
    TextureBinding, TextureWrap,
    battle_effect::EffectTexture as Texture,
    battle_effect::{
        declaration::Declaration,
        visual::{ModelVisual, ParticleTexture},
    },
    diagnostics::Diagnostics,
};
use resonance_game::battle::encounter::{ModelAccessory, RenderEffect};
use std::collections::{BTreeMap, BTreeSet};

struct LoadedTexture {
    spec: Texture,
    images: BTreeMap<usize, Handle<Image>>,
}
impl LoadedTexture {
    fn load(
        spec: Texture,
        pages: impl IntoIterator<Item = usize>,
        server: &AssetServer,
    ) -> Result<Self> {
        let images = pages
            .into_iter()
            .map(|page| {
                let image = spec
                    .images
                    .get(page)
                    .context("particle texture page missing")?;
                let handle = server
                    .load_builder()
                    .with_settings(|settings: &mut ImageLoaderSettings| {
                        settings.is_srgb = false;
                        settings.sampler = ImageSampler::linear();
                    })
                    .load(image.path.clone());
                Ok((page, handle))
            })
            .collect::<Result<_>>()?;
        Ok(Self { spec, images })
    }
}

struct Bank {
    declarations: BTreeMap<u16, Declaration>,
    palettes: BTreeMap<u16, BTreeSet<[u8; 2]>>,
    blends: BTreeMap<u16, BTreeSet<u8>>,
    textures: BTreeMap<u8, Vec<LoadedTexture>>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Atlas {
    resource: ResourceId,
    slot: u8,
    color: usize,
    alpha: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MaterialKey {
    atlas: Option<Atlas>,
    blend: Blend,
    depth_test: bool,
    depth_write: bool,
    clip_below_floor: bool,
    cull_back: bool,
}

/// Particle model rendering differs from textured particle geometry.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ModelStyle {
    pub lit: bool,
    pub blend: Blend,
    pub depth_test: bool,
    pub depth_write: bool,
    pub cull_back: bool,
}
impl ModelStyle {
    fn new(visual: &ModelVisual, opaque: bool) -> Self {
        Self {
            lit: visual.lit,
            blend: Blend::try_from(i32::from(visual.blend))
                .expect("validated model particle blend"),
            depth_test: visual.depth_test,
            depth_write: visual.blend == 0 && opaque && visual.depth_test,
            cull_back: visual.cull_back,
        }
    }
}

impl Bank {
    fn material(
        &self,
        resource: ResourceId,
        declaration: &Declaration,
        palettes: [u8; 2],
        cull_back: bool,
        blend: Option<u8>,
    ) -> Result<(MaterialKey, [u32; 2])> {
        let visual = declaration.particle_visual()?;
        let (atlas, size) = match visual.texture {
            ParticleTexture::Untextured => (None, [1; 2]),
            ParticleTexture::Atlas {
                slot,
                dual,
                palette_set,
            } => {
                let textures = &self.textures[&slot];
                let color = textures[0].spec.palette_image(palette_set, palettes[0])?;
                let alpha = dual
                    .then(|| textures[1].spec.palette_image(palette_set, palettes[1]))
                    .transpose()?;
                let page = &textures[0].spec.images[color];
                (
                    Some(Atlas {
                        resource,
                        slot,
                        color,
                        alpha,
                    }),
                    [page.width, page.height],
                )
            }
        };
        Ok((
            MaterialKey {
                atlas,
                blend: *Blend::ALL
                    .get(usize::from(blend.unwrap_or(visual.blend)))
                    .context("unsupported particle blend")?,
                depth_test: visual.depth_test,
                depth_write: visual.depth_write,
                clip_below_floor: declaration.template()?.state.effect_group.is_some(),
                cull_back,
            },
            size,
        ))
    }
}

struct Draw {
    entity: Entity,
    mesh: Handle<Mesh>,
}

pub(super) struct Effects {
    diagnostics: Diagnostics,
    banks: BTreeMap<ResourceId, Bank>,
    materials: BTreeMap<MaterialKey, Handle<TitleSurface>>,
    warm: Vec<Entity>,
    draws: Vec<Draw>,
    // Scene-sampling triangles stay on the CPU instead of being uploaded as a draw.
    sample_mesh: Mesh,
    vertices: geometry::Vertices,
    wing_resource: Option<ResourceId>,
    prepared: bool,
}

impl Effects {
    pub fn model_styles(&self) -> BTreeSet<ModelStyle> {
        self.banks
            .values()
            .flat_map(|bank| bank.declarations.values())
            .filter_map(|d| d.model_visual().ok())
            .flat_map(|v| [ModelStyle::new(v, true), ModelStyle::new(v, false)])
            .collect()
    }

    pub fn model_pass(
        &self,
        particle: &ParticleFrame,
        actors: &ActorOrder,
    ) -> Result<(ModelStyle, Layer)> {
        let declaration = self
            .banks
            .get(&particle.resource)
            .and_then(|bank| bank.declarations.get(&particle.member))
            .context("unprepared model particle declaration")?;
        let visual = declaration.model_visual()?;
        let order = actors.effect(particle.draw_after, !visual.before_actor)?;
        Ok((
            ModelStyle::new(visual, particle.state.colors[0][3] == 255),
            order,
        ))
    }

    pub fn load(
        banks: Vec<RenderEffect>,
        wings: Option<&ModelAccessory>,
        server: &AssetServer,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let mut prepared = BTreeMap::new();
        for bank in banks {
            if !bank
                .palettes
                .keys()
                .chain(bank.blends.keys())
                .all(|m| bank.members.contains(m))
            {
                diagnostics.report(
                    "battle particle palettes",
                    anyhow::anyhow!(
                        "resource {}: palette variants reference an unselected particle",
                        bank.resource
                    ),
                )?;
            }
            let mut declarations = BTreeMap::new();
            let mut palettes = BTreeMap::new();
            let mut blends = BTreeMap::new();
            let mut wanted: BTreeMap<(u8, usize), BTreeSet<usize>> = BTreeMap::new();
            for &member in &bank.members {
                let selected = diagnostics.attempt(
                    &format!("battle particle {}:{member}", bank.resource),
                    select_declaration(&bank, member),
                )?;
                if let Some((declaration, pairs, pages)) = selected {
                    for (key, values) in pages {
                        wanted.entry(key).or_default().extend(values);
                    }
                    palettes.insert(member, pairs);
                    let mut modes = bank.blends.get(&member).cloned().unwrap_or_default();
                    modes.insert(match &declaration {
                        Declaration::Particle { visual, .. } => visual.blend,
                        Declaration::ModelParticle { visual, .. } => visual.blend,
                        _ => continue,
                    });
                    blends.insert(member, modes);
                    declarations.insert(member, declaration);
                }
            }
            let textures = diagnostics.attempt(
                &format!("battle particle textures {}", bank.resource),
                bank.textures
                    .into_iter()
                    .filter(|(slot, _)| wanted.keys().any(|(s, _)| s == slot))
                    .map(|(slot, textures)| {
                        let textures = textures
                            .into_iter()
                            .enumerate()
                            .map(|(index, spec)| {
                                LoadedTexture::load(
                                    spec,
                                    wanted.get(&(slot, index)).into_iter().flatten().copied(),
                                    server,
                                )
                            })
                            .collect::<Result<_>>()?;
                        Ok((slot, textures))
                    })
                    .collect::<Result<_>>(),
            )?;
            let Some(textures) = textures else {
                continue;
            };
            if prepared.contains_key(&bank.resource) {
                diagnostics.report(
                    "battle particles",
                    anyhow::anyhow!("duplicate particle resource {}", bank.resource),
                )?;
                continue;
            }
            prepared.insert(
                bank.resource,
                Bank {
                    declarations,
                    palettes,
                    blends,
                    textures,
                },
            );
        }
        if let Some(wings) = wings {
            let spec = wings
                .sparkle_texture()
                .context("wing sparkle texture is missing")?
                .clone();
            let pages = ModelAccessory::SPARKLE_PALETTES
                .map(|palette| spec.palette_image(0, palette))
                .into_iter()
                .collect::<Result<Vec<_>>>()?;
            let texture = LoadedTexture::load(spec, pages, server)?;
            ensure!(
                prepared
                    .insert(
                        wings.resource,
                        Bank {
                            declarations: BTreeMap::new(),
                            palettes: BTreeMap::new(),
                            blends: BTreeMap::new(),
                            textures: BTreeMap::from([(0, vec![texture])]),
                        }
                    )
                    .is_none(),
                "duplicate wing particle resource"
            );
        }
        Ok(Self {
            diagnostics,
            banks: prepared,
            materials: BTreeMap::new(),
            warm: Vec::new(),
            draws: Vec::new(),
            sample_mesh: warm_mesh(),
            vertices: geometry::Vertices::default(),
            wing_resource: wings.map(|wings| wings.resource),
            prepared: false,
        })
    }

    pub fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.banks
            .values()
            .flat_map(|b| b.textures.values())
            .flatten()
            .flat_map(|t| t.images.values())
    }
    pub fn images_mut(&mut self) -> impl Iterator<Item = &mut Handle<Image>> {
        self.banks
            .values_mut()
            .flat_map(|b| b.textures.values_mut())
            .flatten()
            .flat_map(|t| t.images.values_mut())
    }

    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.warm
            .iter()
            .copied()
            .chain(self.draws.iter().map(|d| d.entity))
    }

    pub fn prepare(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        surfaces: &mut Assets<TitleSurface>,
        images: &mut Assets<Image>,
        sampled: &mut SampledImages,
    ) -> Result<()> {
        if self.prepared {
            return Ok(());
        }
        ensure!(
            self.images().all(|h| images.contains(h)),
            "particle textures are still loading"
        );
        let mut keys: BTreeSet<_> = [false, true].map(shadow_key).into();
        for (&resource, bank) in &self.banks {
            keys.extend(material_keys(resource, bank)?);
        }
        if let Some(resource) = self.wing_resource {
            let texture = &self.banks[&resource].textures[&0][0].spec;
            for palette in ModelAccessory::SPARKLE_PALETTES {
                keys.insert(wing_key(resource, texture.palette_image(0, palette)?));
            }
        }
        for key in keys {
            let texture = |index: usize,
                           page: usize,
                           images: &mut Assets<Image>,
                           sampled: &mut SampledImages| {
                let atlas = key.atlas?;
                let t = &self.banks[&atlas.resource].textures[&atlas.slot][index];
                sampled_image(
                    Some((
                        t.images[&page].clone(),
                        // Particle atlases clamp even when the shared image requests tiling.
                        TextureBinding {
                            texture: 0,
                            wrap_u: TextureWrap::Clamp,
                            wrap_v: TextureWrap::Clamp,
                            nearest_min: false,
                            nearest_mag: false,
                            mipmapped: false,
                        },
                    )),
                    images,
                    sampled,
                )
            };
            let color = key
                .atlas
                .and_then(|atlas| texture(0, atlas.color, images, sampled));
            let alpha = key
                .atlas
                .and_then(|atlas| atlas.alpha)
                .and_then(|page| texture(1, page, images, sampled));
            let surface = surfaces.add(TitleSurface {
                tint: Vec4::new(2., 2., 2., 1.),
                multiply_alpha_only: alpha.is_some(),
                multiply: alpha,
                blend: Some(key.blend),
                depth_test: key.depth_test,
                depth_write: key.depth_write,
                clip_below_floor: key.clip_below_floor,
                depth_equal: true,
                cull: if key.cull_back {
                    resonance_content::CullFace::Back
                } else {
                    resonance_content::CullFace::None
                },
                ..TitleSurface::textured(color)
            });
            let entity = spawn(commands, meshes.add(warm_mesh()), surface.clone());
            self.materials.insert(key, surface);
            self.warm.push(entity);
        }
        if let Some((_, surface)) = self.materials.first_key_value() {
            // Shadows and wing sparkles have fixed slots; particles grow after them.
            let wing_draws = self
                .wing_resource
                .map_or(0, |_| ModelAccessory::SPARKLE_PALETTES.len());
            for _ in 0..2 + wing_draws {
                let mesh = meshes.add(warm_mesh());
                let entity = spawn(commands, mesh.clone(), surface.clone());
                self.draws.push(Draw { entity, mesh });
            }
        }
        self.prepared = true;
        Ok(())
    }

    pub fn set_layer(&self, commands: &mut Commands, layer: usize, visible: bool) {
        for &entity in &self.warm {
            commands.entity(entity).insert((
                RenderLayers::layer(layer),
                if visible && layer == WARM_LAYER {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));
        }
        for draw in &self.draws {
            commands.entity(draw.entity).insert((
                RenderLayers::layer(layer),
                if visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));
        }
    }

    #[expect(
        clippy::too_many_arguments,
        clippy::type_complexity,
        reason = "Keep sampled scene inputs separate from render resources and commands."
    )]
    pub fn apply(
        &mut self,
        (frame, actor_shadows): (&BattleFrame, &[([f32; 3], f32, [u8; 4])]),
        models: &BTreeMap<ResourceId, super::Model>,
        particles: &[ParticleFrame],
        wing_sparkles: &[super::accessories::Sparkle],
        camera: Mat3,
        meshes: &mut Assets<Mesh>,
        commands: &mut Commands,
        actor_order: &ActorOrder,
    ) -> Result<super::scene_sample::Draws> {
        for draw in &self.draws {
            commands.entity(draw.entity).insert(Visibility::Hidden);
        }
        let mut scene_samples = super::scene_sample::Draws::default();
        let mut count = 2 + self
            .wing_resource
            .map_or(0, |_| ModelAccessory::SPARKLE_PALETTES.len());
        for (order, particle) in particles.iter().enumerate() {
            let result = (|| -> Result<()> {
                let bank = self
                    .banks
                    .get(&particle.resource)
                    .context("unprepared particle render resource")?;
                let declaration = bank
                    .declarations
                    .get(&particle.member)
                    .context("unprepared particle render member")?;
                if matches!(declaration, Declaration::ModelParticle { .. }) {
                    return Ok(());
                }
                let (key, size) = bank.material(
                    particle.resource,
                    declaration,
                    particle.state.palettes,
                    particle.state.cull_back,
                    particle.state.blend,
                )?;
                let bone_world = particle
                    .owner_bone
                    .map(|index| -> Result<Mat4> {
                        let model = frame
                            .models
                            .iter()
                            .find(|model| model.actor == particle.owner)
                            .context("missing particle owner model")?;
                        let bone = model
                            .bones
                            .get(usize::from(index))
                            .context("missing particle owner bone")?;
                        Ok(Mat4::from_cols_array_2d(&model.world) * Mat4::from_cols_array_2d(bone))
                    })
                    .transpose()?;
                let scene_sample = declaration.particle_visual()?.scene_sample;
                if scene_sample.is_none() && count == self.draws.len() {
                    let surface = self
                        .materials
                        .get(&key)
                        .context("particle selected an unprepared material")?;
                    let mesh = meshes.add(warm_mesh());
                    let entity = spawn(commands, mesh.clone(), surface.clone());
                    commands
                        .entity(entity)
                        .insert((RenderLayers::layer(super::LAYER), Visibility::Hidden));
                    self.draws.push(Draw { entity, mesh });
                }
                let mut mesh_asset;
                let mesh: &mut Mesh = if scene_sample.is_some() {
                    &mut self.sample_mesh
                } else {
                    mesh_asset = meshes
                        .get_mut(&self.draws[count].mesh)
                        .context("missing prepared particle mesh")?;
                    &mut mesh_asset
                };
                let visible = geometry::update_mesh(mesh, &mut self.vertices, |vertices| {
                    if let Some(group) = particle.state.effect_group {
                        let model = frame
                            .models
                            .iter()
                            .find(|model| model.actor == particle.owner)
                            .context("missing particle group owner")?;
                        if !model.visible {
                            vertices.clear();
                            return Ok(());
                        }
                        let bones = models
                            .get(&model.resource)
                            .and_then(|model| model.effect_groups.get(usize::from(group)))
                            .context("unprepared particle effect group")?;
                        let world = Mat4::from_cols_array_2d(&model.world);
                        let points = bones.iter().map(|&index| {
                            let bone = model
                                .bones
                                .get(index)
                                .context("missing particle group bone")?;
                            Ok((world * Mat4::from_cols_array_2d(bone))
                                .transform_point3(Vec3::ZERO))
                        });
                        geometry::group(
                            vertices,
                            &particle.state,
                            particle.heading,
                            declaration,
                            size,
                            camera,
                            points,
                        )?;
                    } else {
                        geometry::particle(
                            vertices,
                            particle,
                            declaration,
                            size,
                            camera,
                            bone_world,
                        )?;
                    }
                    Ok(())
                })?;
                if !visible {
                    return Ok(());
                }
                if let Some(offset) = scene_sample {
                    let atlas = key.atlas.context("scene particle has no atlas")?;
                    let alpha = atlas
                        .alpha
                        .context("scene particle has no opacity palette")?;
                    let mask = bank.textures[&atlas.slot][1].images[&alpha].clone();
                    scene_samples.push(
                        mesh,
                        mask,
                        offset,
                        declaration.particle_visual()?.depth_test,
                    )?;
                    return Ok(());
                }
                let surface = self
                    .materials
                    .get(&key)
                    .context("particle selected an unprepared material")?;
                let visual = declaration.particle_visual()?;
                let major = actor_order.effect(particle.draw_after, visual.after_actor)?;
                commands.entity(self.draws[count].entity).insert((
                    MeshMaterial3d(surface.clone()),
                    Visibility::Inherited,
                    DrawOrder(major, order, 0),
                ));
                count += 1;
                Ok(())
            })();
            if let Err(error) = result {
                self.diagnostics.report(
                    &format!(
                        "battle particle draw {}:{}",
                        particle.resource, particle.member
                    ),
                    error,
                )?;
            }
        }
        if let Some(resource) = self.wing_resource {
            let texture = &self.banks[&resource].textures[&0][0].spec;
            for (index, palette) in ModelAccessory::SPARKLE_PALETTES.into_iter().enumerate() {
                let mut sparkles = wing_sparkles
                    .iter()
                    .filter(|sparkle| sparkle.palette() == palette)
                    .peekable();
                if sparkles.peek().is_none() {
                    continue;
                }
                let draw = &self.draws[2 + index];
                let mut mesh = meshes
                    .get_mut(&draw.mesh)
                    .context("missing wing sparkle mesh")?;
                geometry::update_mesh(&mut mesh, &mut self.vertices, |vertices| {
                    geometry::wing_sparkles(vertices, sparkles, camera);
                    Ok(())
                })?;
                let key = wing_key(resource, texture.palette_image(0, palette)?);
                let material = self
                    .materials
                    .get(&key)
                    .context("wing sparkle palette is not prepared")?;
                commands.entity(draw.entity).insert((
                    MeshMaterial3d(material.clone()),
                    Visibility::Inherited,
                    DrawOrder(Layer::Effects, particles.len() + index, 0),
                ));
            }
        }
        let shadows = actor_shadows
            .iter()
            .copied()
            .map(|(position, radius, color)| (position, radius, color, false))
            .chain(frame.projectile_shadows.iter().map(|shadow| {
                (
                    shadow.position,
                    shadow.appearance.radius,
                    shadow.appearance.color,
                    shadow.appearance.additive,
                )
            }));
        for additive in [false, true] {
            let mut footprints = shadows
                .clone()
                .filter(|s| s.3 == additive)
                .map(|(p, r, c, _)| (p, r, c))
                .peekable();
            if footprints.peek().is_none() {
                continue;
            }
            let draw = &self.draws[usize::from(additive)];
            let mut mesh = meshes
                .get_mut(&draw.mesh)
                .context("missing prepared shadow mesh")?;
            geometry::update_mesh(&mut mesh, &mut self.vertices, |vertices| {
                geometry::shadows(vertices, footprints);
                Ok(())
            })?;
            let surface = &self.materials[&shadow_key(additive)];
            commands.entity(draw.entity).insert((
                MeshMaterial3d(surface.clone()),
                Visibility::Inherited,
                DrawOrder(Layer::Shadows, usize::from(additive), 0),
            ));
        }
        Ok(scene_samples)
    }

    pub fn despawn(self, commands: &mut Commands) {
        for entity in self.entities() {
            commands.entity(entity).despawn();
        }
    }
}

type Pages = BTreeMap<(u8, usize), BTreeSet<usize>>;

fn select_declaration(
    bank: &RenderEffect,
    member: u16,
) -> Result<(Declaration, BTreeSet<[u8; 2]>, Pages)> {
    let mut selected_pages: Pages = BTreeMap::new();
    let declaration = bank
        .source
        .actors
        .get(usize::from(member))
        .with_context(|| format!("missing particle declaration {}:{member}", bank.resource))?;
    geometry::validate(declaration)
        .with_context(|| format!("particle {}:{member}", bank.resource))?;
    let template = declaration.template()?;
    template.validate()?;
    let is_model = matches!(declaration, Declaration::ModelParticle { .. });
    ensure!(
        bank.blends.get(&member).is_none_or(
            |modes| modes.iter().all(|&mode| mode <= 1) && (!is_model || modes.is_empty())
        ),
        "particle {member} blend variants are not prepared"
    );
    let mut pairs = bank.palettes.get(&member).cloned().unwrap_or_default();
    pairs.insert(template.state.palettes);
    if let Declaration::Particle { visual, .. } = declaration {
        if visual.scene_sample.is_some() {
            ensure!(
                !visual.depth_write
                    && !template.state.cull_back
                    && !visual.after_actor
                    && visual.blend == 0
                    && bank
                        .blends
                        .get(&member)
                        .is_none_or(|modes| modes.iter().all(|&mode| mode == 0)),
                "scene particle requires unsupported render state"
            );
        }
        if let Some(resonance_content::battle_effect::UvAnimation::Frames { frames, .. }) =
            &template.uv_animation
        {
            for frame in frames {
                if let resonance_content::battle_effect::UvChange::Palette { index } = frame.change
                {
                    pairs.insert([index, template.state.palettes[1]]);
                }
            }
        }
        if let Some(slot) = visual.texture.slot() {
            let textures = bank.textures.get(&slot).with_context(|| {
                format!(
                    "particle {}:{member}, missing bound texture slot {slot}",
                    bank.resource
                )
            })?;
            let color = textures
                .first()
                .context("particle texture has no color image")?;
            validate_texture(color)?;
            let set = visual.texture.palette_set();
            for pair in &pairs {
                let page = color.palette_image(set, pair[0])?;
                selected_pages.entry((slot, 0)).or_default().insert(page);
                if visual.texture.dual() {
                    let alpha = textures
                        .get(1)
                        .context("particle texture has no alpha image")?;
                    validate_texture(alpha)?;
                    let alpha_page = alpha.palette_image(set, pair[1])?;
                    ensure!(
                        color.images[page].width == alpha.images[alpha_page].width
                            && color.images[page].height == alpha.images[alpha_page].height,
                        "particle color and alpha atlas dimensions differ"
                    );
                    selected_pages
                        .entry((slot, 1))
                        .or_default()
                        .insert(alpha_page);
                }
            }
        }
    }
    Ok((declaration.clone(), pairs, selected_pages))
}

fn validate_texture(texture: &Texture) -> Result<()> {
    texture.sampler.validate()?;
    ensure!(!texture.images.is_empty(), "empty particle texture");
    for image in &texture.images {
        image.validate()?;
    }
    Ok(())
}

fn shadow_key(additive: bool) -> MaterialKey {
    MaterialKey {
        atlas: None,
        blend: if additive {
            Blend::Additive
        } else {
            Blend::Alpha
        },
        depth_test: true,
        depth_write: false,
        clip_below_floor: false,
        // Cull back faces for the shadow fan.
        cull_back: true,
    }
}

fn wing_key(resource: ResourceId, color: usize) -> MaterialKey {
    MaterialKey {
        atlas: Some(Atlas {
            resource,
            slot: 0,
            color,
            alpha: None,
        }),
        blend: Blend::Additive,
        depth_test: true,
        depth_write: false,
        clip_below_floor: false,
        cull_back: false,
    }
}

fn spawn(commands: &mut Commands, mesh: Handle<Mesh>, surface: Handle<TitleSurface>) -> Entity {
    commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(surface),
            Transform::default(),
            Visibility::Inherited,
            NoFrustumCulling,
            bevy::render::batching::NoAutomaticBatching,
            RenderLayers::layer(WARM_LAYER),
            DrawOrder(Layer::Effects, 0, 0),
        ))
        .id()
}

fn warm_mesh() -> Mesh {
    use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology};
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[-1., -1., 0.], [1., -1., 0.], [0., 1., 0.]],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.; 4]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.], [1., 0.], [0., 1.]])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0., 0.], [1., 0.], [0., 1.]])
}

fn material_keys(resource: ResourceId, bank: &Bank) -> Result<BTreeSet<MaterialKey>> {
    let mut keys = BTreeSet::new();
    for (&member, declaration) in &bank.declarations {
        let Declaration::Particle { visual, .. } = declaration else {
            continue;
        };
        if visual.scene_sample.is_some() {
            continue;
        }
        for pair in &bank.palettes[&member] {
            for cull in [false, true] {
                for &blend in &bank.blends[&member] {
                    keys.insert(
                        bank.material(resource, declaration, *pair, cull, Some(blend))?
                            .0,
                    );
                }
            }
        }
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        battle_effect::visual::{ParticleShape, ParticleVisual},
        font::UiTexture,
        texture::{Filter, Sampler, TextureLod},
    };

    fn masked_particle() -> Declaration {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/masked-particle.json")).unwrap();
        serde_json::from_value(fixture["declaration"].clone()).unwrap()
    }

    fn visual(declaration: &mut Declaration) -> &mut ParticleVisual {
        let Declaration::Particle { visual, .. } = declaration else {
            panic!("sprite fixture");
        };
        visual
    }

    fn mixed_bank() -> RenderEffect {
        let mut healthy = masked_particle();
        visual(&mut healthy).texture = ParticleTexture::Untextured;
        let mut bad_geometry = healthy.clone();
        visual(&mut bad_geometry).geometry = ParticleShape::Unsupported {
            reason: "unsupported drawing geometry".into(),
        };
        let mut bad_blend = healthy.clone();
        visual(&mut bad_blend).blend = 9;
        let mut missing_texture = healthy.clone();
        visual(&mut missing_texture).texture = ParticleTexture::Atlas {
            slot: 1,
            dual: false,
            palette_set: 0,
        };
        RenderEffect {
            resource: ResourceId(7),
            source: std::sync::Arc::new(resonance_content::battle_effect::SourceBank {
                template_names: Vec::new(),
                document: None,
                programs: vec![],
                actors: vec![bad_geometry, bad_blend, healthy, missing_texture],
                art: None,
            }),
            textures: BTreeMap::new(),
            members: BTreeSet::from([0, 1, 2, 3, 4]),
            palettes: BTreeMap::new(),
            blends: BTreeMap::new(),
        }
    }

    #[test]
    fn tolerant_loading_keeps_healthy_members_and_paranoid_stops() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        let diagnostics = Diagnostics::new(false);
        let effects = Effects::load(
            vec![mixed_bank()],
            None,
            app.world().resource(),
            diagnostics.clone(),
        )
        .unwrap();
        assert_eq!(
            effects.banks[&ResourceId(7)]
                .declarations
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![2]
        );
        let entries = diagnostics.entries();
        assert_eq!(entries.len(), 4);
        for (entry, expected) in entries.iter().zip([
            "unsupported drawing geometry",
            "blend 9",
            "missing bound texture slot 1",
            "missing particle declaration 7:4",
        ]) {
            assert!(entry.message.contains(expected), "{}", entry.message);
        }
        let diagnostics = Diagnostics::new(true);
        let result = Effects::load(
            vec![mixed_bank()],
            None,
            app.world().resource(),
            diagnostics.clone(),
        );
        assert!(format!("{:#}", result.err().unwrap()).contains("unsupported drawing geometry"));
        assert_eq!(diagnostics.entries().len(), 1);
    }

    #[test]
    fn palette_and_blend_overrides_have_prepared_materials() {
        let source = resonance_content::battle_effect::SourceBank {
            template_names: Vec::new(),
            document: None,
            programs: vec![],
            actors: vec![
                serde_json::from_str(include_str!(
                    "../../../content/tests/fixtures/material-particle.json"
                ))
                .unwrap(),
            ],
            art: None,
        };
        let mut input = RenderEffect {
            resource: ResourceId(77),
            source: std::sync::Arc::new(source),
            textures: BTreeMap::from([(0, vec![texture(true, 64), texture(true, 32)])]),
            members: BTreeSet::from([0]),
            palettes: BTreeMap::from([(0, BTreeSet::from([[26, 1]]))]),
            blends: BTreeMap::from([(0, BTreeSet::from([0]))]),
        };
        let (declaration, pairs, pages) = select_declaration(&input, 0).unwrap();
        assert_eq!(pairs, BTreeSet::from([[9, 1], [26, 1]]));
        assert_eq!(
            pages,
            BTreeMap::from([
                ((0, 0), BTreeSet::from([9, 26])),
                ((0, 1), BTreeSet::from([1]))
            ])
        );
        for depth_test in [false, true] {
            let mut sampled = declaration.clone();
            let visual = visual(&mut sampled);
            visual.scene_sample = Some([4, 4]);
            visual.blend = 0;
            visual.depth_test = depth_test;
            visual.depth_write = false;
            visual.after_actor = false;
            let Declaration::Particle { template, .. } = &mut sampled else {
                unreachable!();
            };
            template.state.cull_back = false;
            std::sync::Arc::make_mut(&mut input.source).actors[0] = sampled;
            let (prepared, _, sampled_pages) = select_declaration(&input, 0).unwrap();
            assert_eq!(prepared.particle_visual().unwrap().depth_test, depth_test);
            assert_eq!(sampled_pages, pages);
        }
        let mut bank = Bank {
            declarations: BTreeMap::from([(0, declaration.clone())]),
            palettes: BTreeMap::from([(0, pairs)]),
            blends: BTreeMap::from([(0, BTreeSet::from([0, 1]))]),
            textures: input
                .textures
                .into_iter()
                .map(|(slot, textures)| {
                    (
                        slot,
                        textures
                            .into_iter()
                            .map(|spec| LoadedTexture {
                                spec,
                                images: BTreeMap::new(),
                            })
                            .collect(),
                    )
                })
                .collect(),
        };
        let keys = material_keys(ResourceId(77), &bank).unwrap();
        for cull in [false, true] {
            for (color, blend) in [(9, None), (26, Some(0))] {
                let (key, size) = bank
                    .material(ResourceId(77), &declaration, [color, 1], cull, blend)
                    .unwrap();
                assert!(keys.contains(&key));
                assert_eq!(size, [512; 2]);
                let atlas = key.atlas.unwrap();
                assert_eq!(
                    (atlas.resource, atlas.slot, atlas.color, atlas.alpha),
                    (ResourceId(77), 0, usize::from(color), Some(1))
                );
            }
        }
        bank.palettes.insert(0, BTreeSet::from([[64, 1]]));
        assert!(material_keys(ResourceId(77), &bank).is_err());
    }

    #[test]
    fn subtractive_untextured_particles_prepare_both_cull_variants() {
        let mut declaration = masked_particle();
        let visual = visual(&mut declaration);
        visual.texture = ParticleTexture::Untextured;
        visual.blend = 2;
        let bank = Bank {
            declarations: BTreeMap::from([(0, declaration.clone())]),
            palettes: BTreeMap::from([(0, BTreeSet::from([[0, 0]]))]),
            blends: BTreeMap::from([(0, BTreeSet::from([2]))]),
            textures: BTreeMap::new(),
        };
        geometry::validate(&declaration).unwrap();
        let keys = material_keys(ResourceId(1), &bank).unwrap();
        assert_eq!(keys.len(), 2);
        assert!(
            keys.iter()
                .all(|key| key.blend == Blend::Subtractive && key.atlas.is_none())
        );

        let mut model = Declaration::ModelParticle {
            template: declaration.template().unwrap().clone(),
            binding: resonance_content::battle_effect::ModelBinding {
                slot: 0,
                animation: None,
                repeat: false,
                reverse_at: None,
                program_animation: false,
                attachment: None,
            },
            visual: ModelVisual {
                blend: 2,
                lit: false,
                depth_test: true,
                cull_back: true,
                before_actor: false,
            },
        };
        geometry::validate(&model).unwrap();
        for opaque in [false, true] {
            let style = ModelStyle::new(model.model_visual().unwrap(), opaque);
            assert_eq!(style.blend, Blend::Subtractive);
            assert!(!style.depth_write);
            assert!(style.cull_back && style.depth_test);
        }
        let Declaration::ModelParticle { visual, .. } = &mut model else {
            unreachable!();
        };
        visual.blend = 3;
        assert!(geometry::validate(&model).is_err());
    }

    fn texture(indexed: bool, pages: usize) -> Texture {
        Texture {
            palette_sets: vec![if indexed {
                (0..pages).map(Some).collect()
            } else {
                vec![Some(0); 256]
            }],
            sampler: Sampler {
                wrap: [TextureWrap::Clamp; 2],
                min_filter: Filter::Linear,
                mag_filter: Filter::Linear,
                lod: TextureLod::default(),
            },
            images: (0..pages)
                .map(|i| UiTexture {
                    path: format!("page-{i}.ktx2"),
                    width: 512,
                    height: 512,
                })
                .collect(),
        }
    }
}
