//! A fixed pool of draws with every material and pipeline prepared before entry.
//! Particle state and submission order are supplied by the battle core.
#[path = "effects_geometry.rs"]
mod geometry;

use super::{ActorOrder, WARM_LAYER, refraction::ScreenDraw};
use crate::{
    draw_order::{DrawOrder, EFFECTS},
    materials::TitleSurface,
    scene::{SampledImages, sampled_image},
};
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers},
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
};
use resonance_battle::{BattleFrame, ParticleFrame};
use resonance_content::{
    TextureBinding,
    battle_effect::declaration::Declaration,
    battle_effect::{EffectTexture as Texture, SourceBank},
    diagnostics::Diagnostics,
    texture::Filter,
};
use std::collections::{BTreeMap, BTreeSet};

const CAPACITY: usize = 416;

/// The caller selects the complete encounter closure. Unused declarations in an
/// original source bank do not imply support for their controllers or bindings.
pub(crate) struct EffectBank {
    pub resource: u32,
    pub source: SourceBank,
    pub textures: BTreeMap<u8, Vec<Texture>>,
    pub members: BTreeSet<u16>,
    /// Additional reachable palette pairs from selected scripts, modifiers and element tints.
    /// Declaration and UV-track palette pairs are included automatically.
    pub palettes: BTreeMap<u16, BTreeSet<[u8; 2]>>,
}

struct LoadedTexture {
    spec: Texture,
    images: BTreeMap<usize, Handle<Image>>,
}

struct Bank {
    declarations: BTreeMap<u16, Declaration>,
    palettes: BTreeMap<u16, BTreeSet<[u8; 2]>>,
    textures: BTreeMap<u8, Vec<LoadedTexture>>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MaterialKey {
    resource: u32,
    slot: u8,
    color: usize,
    alpha: Option<usize>,
    screen: bool,
    blend: u8,
    depth_test: bool,
    depth_write: bool,
    cull_back: bool,
}

/// 3FC24's model pass differs from ordinary textured particle geometry.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ModelStyle {
    pub lit: bool,
    pub additive: bool,
    pub depth_test: bool,
    pub depth_write: bool,
    pub cull_back: bool,
}
impl ModelStyle {
    fn new(declaration: &Declaration, opaque: bool) -> Self {
        let p = &declaration.prefix;
        let flags = p.flags_or_shake_amplitude;
        Self {
            lit: flags & 0x4000 != 0,
            additive: p.blend == 1,
            depth_test: flags & 0x82000 == 0,
            depth_write: p.blend == 0 && opaque && flags & 0x82000 == 0,
            cull_back: p.blend == 0 || flags & 0x10000000 != 0,
        }
    }
}

impl MaterialKey {
    fn new(
        resource: u32,
        declaration: &Declaration,
        color: usize,
        alpha: Option<usize>,
        cull_back: bool,
    ) -> Self {
        let p = &declaration.prefix;
        Self {
            resource,
            slot: texture_slot(p.resource_slot),
            color,
            alpha,
            screen: p.resource_slot == 10,
            blend: if p.resource_slot == 10 { 0 } else { p.blend },
            depth_test: p.flags_or_shake_amplitude & 0x80000 == 0,
            depth_write: p.flags_or_shake_amplitude & 0x800000 != 0,
            cull_back,
        }
    }
}

struct Draw {
    entity: Entity,
    mesh: Handle<Mesh>,
    screen: bool,
}

pub(super) struct Effects {
    diagnostics: Diagnostics,
    banks: BTreeMap<u32, Bank>,
    materials: BTreeMap<MaterialKey, Handle<TitleSurface>>,
    warm: Vec<Entity>,
    screen_warm: Vec<Entity>,
    draws: Vec<Draw>,
    shadow: Option<u32>,
    shadow_circle: Option<[[f32; 2]; 15]>,
    sine: Box<[f32; 450]>,
    prepared: bool,
}

impl Effects {
    pub fn model_styles(&self) -> BTreeSet<ModelStyle> {
        self.banks
            .values()
            .flat_map(|bank| bank.declarations.values())
            .filter(|d| d.prefix.kind == 3)
            .flat_map(|d| [ModelStyle::new(d, true), ModelStyle::new(d, false)])
            .collect()
    }

    pub fn model_pass(
        &self,
        particle: &ParticleFrame,
        actors: &ActorOrder,
    ) -> Result<(ModelStyle, u32)> {
        let declaration = self
            .banks
            .get(&particle.resource)
            .and_then(|bank| bank.declarations.get(&particle.member))
            .context("unprepared model particle declaration")?;
        ensure!(
            declaration.prefix.kind == 3,
            "non-model particle has a model pose"
        );
        let flags = declaration.prefix.flags_or_shake_amplitude;
        // 12670/4979C send ordinary-alpha models to the early global list,
        // irrespective of owner flags. Additive models use separate before/
        // after owner lists (the selector is reversed from sprite lists).
        let order = if declaration.prefix.blend == 0 {
            super::SHADOWS - 65536
        } else if let Some(actor) = particle.draw_after {
            actors.get(actor)? + if flags & 0x200000 != 0 { 0 } else { 53248 }
        } else {
            EFFECTS - 16384
        };
        Ok((
            ModelStyle::new(declaration, particle.state.colors[0][3] == 255),
            order,
        ))
    }

    pub fn load(
        banks: Vec<EffectBank>,
        sine: &[f32],
        server: &AssetServer,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let lookup = diagnostics.attempt(
            "battle particle degree lookup",
            (|| {
                let table: [f32; 450] = sine
                    .try_into()
                    .context("particle degree lookup requires 450 original samples")?;
                ensure!(
                    table
                        .iter()
                        .all(|v| v.is_finite() && (-1. ..=1.).contains(v)),
                    "invalid particle degree lookup samples"
                );
                Ok(table)
            })(),
        )?;
        let sine = lookup.unwrap_or([0.; 450]);
        let mut shadow = banks
            .iter()
            .find(|b| b.textures.get(&10).is_some_and(|t| !t.is_empty()))
            .map(|b| b.resource);
        let mut prepared = BTreeMap::new();
        for bank in banks {
            if !bank.palettes.keys().all(|m| bank.members.contains(m)) {
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
            let mut wanted: BTreeMap<(u8, usize), BTreeSet<usize>> = BTreeMap::new();
            if shadow == Some(bank.resource) {
                if diagnostics
                    .attempt(
                        "battle shadow texture",
                        validate_texture(&bank.textures[&10][0]),
                    )?
                    .is_some()
                {
                    wanted.entry((10, 0)).or_default().insert(0);
                } else {
                    shadow = None;
                }
            }
            for &member in &bank.members {
                let selected = diagnostics.attempt(
                    &format!("battle particle {}:{member}", bank.resource),
                    select_declaration(&bank, member, lookup.is_some()),
                )?;
                if let Some((declaration, pairs, pages)) = selected {
                    for (key, values) in pages {
                        wanted.entry(key).or_default().extend(values);
                    }
                    palettes.insert(member, pairs);
                    declarations.insert(member, declaration);
                }
            }
            let textures = diagnostics.attempt(
                &format!("battle particle textures {}", bank.resource),
                (|| {
                    let textures = bank
                        .textures
                        .into_iter()
                        .filter(|(slot, _)| wanted.keys().any(|(s, _)| s == slot))
                        .map(|(slot, textures)| {
                            let textures = textures
                                .into_iter()
                                .enumerate()
                                .map(|(index, spec)| {
                                    let images = wanted
                                        .get(&(slot, index))
                                        .into_iter()
                                        .flatten()
                                        .map(|&page| {
                                            let image = spec
                                                .images
                                                .get(page)
                                                .context("particle texture page missing")?;
                                            let handle = server
                                                .load_builder()
                                                .with_settings(|s: &mut ImageLoaderSettings| {
                                                    s.is_srgb = false;
                                                    s.sampler = ImageSampler::linear();
                                                })
                                                .load(image.path.clone());
                                            Ok((page, handle))
                                        })
                                        .collect::<Result<_>>()?;
                                    Ok(LoadedTexture { spec, images })
                                })
                                .collect::<Result<_>>()?;
                            Ok((slot, textures))
                        })
                        .collect::<Result<_>>()?;
                    Ok(textures)
                })(),
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
            ensure!(
                prepared
                    .insert(
                        bank.resource,
                        Bank {
                            declarations,
                            palettes,
                            textures
                        }
                    )
                    .is_none(),
                "duplicate particle resource {}",
                bank.resource
            );
        }
        let shadow = shadow.filter(|resource| prepared.contains_key(resource));
        Ok(Self {
            diagnostics,
            banks: prepared,
            materials: BTreeMap::new(),
            warm: Vec::new(),
            screen_warm: Vec::new(),
            draws: Vec::new(),
            shadow,
            shadow_circle: None,
            sine: Box::new(sine),
            prepared: false,
        })
    }

    pub fn disable_shadows(&mut self) {
        self.shadow = None;
    }

    pub fn set_shadow_circle(&mut self, circle: [[f32; 2]; 15]) -> Result<()> {
        ensure!(
            !self.prepared && circle.iter().flatten().all(|v| v.is_finite()),
            "invalid prepared shadow circle"
        );
        self.shadow_circle = Some(circle);
        Ok(())
    }

    pub fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.banks
            .values()
            .flat_map(|b| b.textures.values())
            .flatten()
            .flat_map(|t| t.images.values())
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
        scene: &Handle<Image>,
    ) -> Result<()> {
        if self.prepared {
            return Ok(());
        }
        ensure!(
            self.shadow.is_none() || self.shadow_circle.is_some(),
            "missing prepared shadow circle"
        );
        ensure!(
            self.images().all(|h| images.contains(h)),
            "particle textures are still loading"
        );
        let mut keys = BTreeSet::new();
        if let Some(resource) = self.shadow {
            keys.extend([false, true].map(|additive| shadow_key(resource, additive)));
        }
        for (&resource, bank) in &self.banks {
            for (&member, declaration) in &bank.declarations {
                let p = &declaration.prefix;
                if p.kind == 3 {
                    continue;
                }
                for pair in &bank.palettes[&member] {
                    let (color, alpha) = if p.resource_slot == 255 {
                        (0, None)
                    } else {
                        let textures = &bank.textures[&texture_slot(p.resource_slot)];
                        let color = palette(&textures[0].spec, pair[0], p.palette_stride)?;
                        let alpha = if p.flags_or_shake_amplitude & 0x4000000 != 0
                            || p.resource_slot == 10
                        {
                            Some(palette(&textures[1].spec, pair[1], p.palette_stride)?)
                        } else {
                            None
                        };
                        (color, alpha)
                    };
                    for cull in [false, true] {
                        keys.insert(MaterialKey::new(resource, declaration, color, alpha, cull));
                    }
                }
            }
        }
        for key in keys {
            let texture = |index: usize,
                           page: usize,
                           images: &mut Assets<Image>,
                           sampled: &mut SampledImages| {
                if key.slot == 255 {
                    return None;
                }
                let t = &self.banks[&key.resource].textures[&key.slot][index];
                sampled_image(
                    Some((t.images[&page].clone(), binding(&t.spec))),
                    images,
                    sampled,
                )
            };
            let color = texture(0, key.color, images, sampled);
            let alpha = key.alpha.and_then(|page| texture(1, page, images, sampled));
            let (color, alpha) = if key.screen {
                (Some(scene.clone()), alpha)
            } else {
                (color, alpha)
            };
            let surface = surfaces.add(TitleSurface {
                tint: Vec4::new(2., 2., 2., 1.),
                multiply: alpha,
                multiply_alpha_only: key.alpha.is_some(),
                screen_texture: key.screen,
                blend: true,
                additive: key.blend == 1,
                depth_test: key.depth_test,
                depth_write: key.depth_write,
                depth_equal: true,
                cull: if key.cull_back {
                    resonance_content::CullFace::Back
                } else {
                    resonance_content::CullFace::None
                },
                ..TitleSurface::textured(color)
            });
            let entity = spawn(commands, meshes.add(warm_mesh()), surface.clone());
            commands.entity(entity).insert(ScreenDraw(key.screen));
            if key.screen {
                commands
                    .entity(entity)
                    .insert(DrawOrder(super::refraction::ORDER, 0));
                self.screen_warm.push(entity);
            }
            self.materials.insert(key, surface);
            self.warm.push(entity);
        }
        if let Some((key, surface)) = self.materials.first_key_value() {
            for _ in 0..CAPACITY {
                let mesh = meshes.add(warm_mesh());
                let entity = spawn(commands, mesh.clone(), surface.clone());
                commands.entity(entity).insert((
                    ScreenDraw(key.screen),
                    DrawOrder(
                        if key.screen {
                            super::refraction::ORDER
                        } else {
                            EFFECTS
                        },
                        0,
                    ),
                ));
                self.draws.push(Draw {
                    entity,
                    mesh,
                    screen: key.screen,
                });
            }
        }
        self.prepared = true;
        Ok(())
    }

    pub fn hide_screen(&self, commands: &mut Commands) {
        for entity in self.screen_warm.iter().copied().chain(
            self.draws
                .iter()
                .filter(|draw| draw.screen)
                .map(|draw| draw.entity),
        ) {
            commands.entity(entity).insert(Visibility::Hidden);
        }
    }

    pub fn screen_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.screen_warm.iter().copied()
    }

    pub fn warm_screen(&self, commands: &mut Commands, layer: usize, visible: bool) {
        for &entity in &self.screen_warm {
            commands.entity(entity).insert((
                RenderLayers::layer(layer),
                if visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));
        }
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

    pub fn apply(
        &mut self,
        frame: &BattleFrame,
        camera: Mat3,
        meshes: &mut Assets<Mesh>,
        commands: &mut Commands,
        actor_order: &ActorOrder,
    ) -> Result<()> {
        for draw in &self.draws {
            commands.entity(draw.entity).insert(Visibility::Hidden);
        }
        let mut count = 0;
        for (order, particle) in frame.particles.iter().enumerate() {
            let result = (|| -> Result<()> {
                let bank = self
                    .banks
                    .get(&particle.resource)
                    .context("unprepared particle render resource")?;
                let declaration = bank
                    .declarations
                    .get(&particle.member)
                    .context("unprepared particle render member")?;
                if declaration.prefix.kind == 3 {
                    return Ok(());
                }
                let (key, size) = self.key(particle, declaration)?;
                let Some(mut mesh) =
                    geometry::mesh(particle, declaration, size, camera, &self.sine)?
                else {
                    return Ok(());
                };
                if key.screen {
                    geometry::screen_uv(
                        &mut mesh,
                        declaration,
                        frame.camera.context("screen particle has no camera")?,
                    )?;
                }
                let draw = self
                    .draws
                    .get_mut(count)
                    .context("battle particles exceeded prepared draw capacity")?;
                let surface = self
                    .materials
                    .get(&key)
                    .context("particle selected an unprepared material")?;
                *meshes
                    .get_mut(&draw.mesh)
                    .context("missing prepared particle mesh")? = mesh;
                let pass = u32::from(declaration.prefix.blend == 1);
                let group = if key.alpha.is_some() {
                    2
                } else if key.slot == 255 {
                    0
                } else {
                    1
                };
                let major = if let Some(actor) = particle.draw_after {
                    let body = actor_order.get(actor)?;
                    let base = if declaration.prefix.flags_or_shake_amplitude & 0x2000 != 0 {
                        body
                    } else {
                        body + 53248
                    };
                    base + 1024 + pass * 512 + group * 128
                } else {
                    EFFECTS + pass * 4096 + group * 1024
                };
                draw.screen = key.screen;
                commands.entity(draw.entity).insert((
                    MeshMaterial3d(surface.clone()),
                    Visibility::Inherited,
                    DrawOrder(
                        if key.screen {
                            super::refraction::ORDER
                        } else {
                            major
                        },
                        order,
                    ),
                    ScreenDraw(key.screen),
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
        let shadows = frame
            .models
            .iter()
            .filter(|model| model.visible)
            .filter_map(|model| {
                model
                    .shadow
                    .map(|shadow| (shadow.position, shadow.radius, shadow.color, false))
            })
            .chain(frame.projectiles.iter().filter_map(|projectile| {
                projectile
                    .shadow
                    .filter(|_| projectile.position[1] >= 0.)
                    .map(|shadow| {
                        (
                            [projectile.position[0], 1.1, projectile.position[2]],
                            shadow.radius,
                            shadow.color,
                            shadow.additive,
                        )
                    })
            }));
        // 48E6C appends shadows to their own early list; actor body depth
        // sorting (495E4) does not reorder that list.
        for (order, (position, radius, color, additive)) in shadows.enumerate() {
            let result = (|| -> Result<()> {
                let resource = self
                    .shadow
                    .context("battle shadow texture was not prepared")?;
                let mesh = geometry::shadow(
                    position,
                    radius,
                    color,
                    self.shadow_circle
                        .as_ref()
                        .context("missing prepared shadow circle")?,
                );
                let draw = self
                    .draws
                    .get_mut(count)
                    .context("battle effects exceeded prepared draw capacity")?;
                let surface = self
                    .materials
                    .get(&shadow_key(resource, additive))
                    .context("battle shadow material was not prepared")?;
                *meshes
                    .get_mut(&draw.mesh)
                    .context("missing prepared shadow mesh")? = mesh;
                draw.screen = false;
                commands.entity(draw.entity).insert((
                    MeshMaterial3d(surface.clone()),
                    Visibility::Inherited,
                    DrawOrder(super::SHADOWS, order),
                    ScreenDraw(false),
                ));
                count += 1;
                Ok(())
            })();
            if let Err(error) = result {
                self.diagnostics.report("battle shadow draw", error)?;
            }
        }
        for draw in &self.draws[count..] {
            commands.entity(draw.entity).insert(Visibility::Hidden);
        }
        Ok(())
    }

    fn key(
        &self,
        particle: &ParticleFrame,
        declaration: &Declaration,
    ) -> Result<(MaterialKey, [u32; 2])> {
        let p = &declaration.prefix;
        if p.resource_slot == 255 {
            return Ok((
                MaterialKey::new(
                    particle.resource,
                    declaration,
                    0,
                    None,
                    particle.state.cull_back,
                ),
                [1; 2],
            ));
        }
        let textures = &self.banks[&particle.resource].textures[&texture_slot(p.resource_slot)];
        let color = palette(
            &textures[0].spec,
            particle.state.palettes[0],
            p.palette_stride,
        )?;
        let alpha = if p.flags_or_shake_amplitude & 0x4000000 != 0 || p.resource_slot == 10 {
            Some(palette(
                &textures[1].spec,
                particle.state.palettes[1],
                p.palette_stride,
            )?)
        } else {
            None
        };
        let page = &textures[0].spec.images[color];
        Ok((
            MaterialKey::new(
                particle.resource,
                declaration,
                color,
                alpha,
                particle.state.cull_back,
            ),
            [page.width, page.height],
        ))
    }

    pub fn despawn(self, commands: &mut Commands) {
        for entity in self.entities() {
            commands.entity(entity).despawn();
        }
    }
}

type Pages = BTreeMap<(u8, usize), BTreeSet<usize>>;

fn select_declaration(
    bank: &EffectBank,
    member: u16,
    has_lookup: bool,
) -> Result<(Declaration, BTreeSet<[u8; 2]>, Pages)> {
    let mut selected_pages: Pages = BTreeMap::new();
    let declaration = bank
        .source
        .actors
        .get(usize::from(member))
        .with_context(|| format!("missing particle declaration {}:{member}", bank.resource))?;
    geometry::validate(declaration)
        .with_context(|| format!("particle {}:{member}", bank.resource))?;
    let p = &declaration.prefix;
    ensure!(
        p.kind != 7 || has_lookup,
        "orbit particle requires the missing degree lookup"
    );
    ensure!(
        p.blend <= 1,
        "particle {}:{member} needs unprepared blend {}",
        bank.resource,
        p.blend
    );
    let mut pairs = bank.palettes.get(&member).cloned().unwrap_or_default();
    pairs.insert(p.palettes);
    if p.kind != 3 {
        for row in bank.source.particle(usize::from(member))?.uv_track {
            if row.values[0] == -32000 {
                pairs.insert([row.values[1] as u8, p.palettes[1]]);
            }
        }
    }
    if p.kind != 3 && p.resource_slot != 255 {
        let slot = texture_slot(p.resource_slot);
        let textures = bank.textures.get(&slot).with_context(|| {
            format!(
                "particle {}:{member}, authored slot {}, missing bound texture slot {slot}",
                bank.resource, p.resource_slot
            )
        })?;
        let color = textures.first().with_context(|| {
            format!(
                "particle {}:{member}, authored slot {}, bound slot {slot}: no color image",
                bank.resource, p.resource_slot
            )
        })?;
        validate_texture(color)?;
        for pair in &pairs {
            let page = palette(color, pair[0], p.palette_stride).with_context(|| {
                format!(
                    "particle {}:{member}, texture slot {} color, source {}",
                    bank.resource, p.resource_slot, bank.source.source_sha256
                )
            })?;
            selected_pages.entry((slot, 0)).or_default().insert(page);
            if p.flags_or_shake_amplitude & 0x4000000 != 0 || p.resource_slot == 10 {
                let alpha = textures.get(1).with_context(|| {
                    format!(
                        "particle {}:{member}, authored slot {}, bound slot {slot}: no alpha image",
                        bank.resource, p.resource_slot
                    )
                })?;
                validate_texture(alpha)?;
                let alpha_page = palette(alpha, pair[1], p.palette_stride).with_context(|| {
                    format!(
                        "particle {}:{member}, texture slot {} alpha, source {}",
                        bank.resource, p.resource_slot, bank.source.source_sha256
                    )
                })?;
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

fn shadow_key(resource: u32, additive: bool) -> MaterialKey {
    MaterialKey {
        resource,
        slot: 10,
        color: 0,
        alpha: None,
        screen: false,
        blend: u8::from(additive),
        depth_test: true,
        depth_write: false,
        // 48D3C sets GX cull mode to back immediately before 7A90C.
        cull_back: true,
    }
}

// 47FA4 selects atlas zero for the screen list. Slot ten remains the actual
// shadow texture only in the separate 7A90C shadow pass.
fn texture_slot(authored: u8) -> u8 {
    if authored == 10 { 0 } else { authored }
}

fn palette(texture: &Texture, index: u8, stride: u8) -> Result<usize> {
    texture.palette_page(index, stride)
}

fn binding(texture: &Texture) -> TextureBinding {
    let nearest = |filter| {
        matches!(
            filter,
            Filter::Nearest | Filter::NearestMipmapNearest | Filter::NearestMipmapLinear
        )
    };
    TextureBinding {
        texture: 0,
        wrap_u: texture.sampler.wrap[0],
        wrap_v: texture.sampler.wrap[1],
        nearest_min: nearest(texture.sampler.min_filter),
        nearest_mag: nearest(texture.sampler.mag_filter),
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
            DrawOrder(EFFECTS, 0),
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

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        TextureWrap,
        font::UiTexture,
        texture::{Sampler, TextureLod},
    };

    fn mixed_bank() -> EffectBank {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/screen-particle.json")).unwrap();
        let mut healthy: Declaration =
            serde_json::from_value(fixture["declaration"].clone()).unwrap();
        healthy.prefix.resource_slot = 255;
        healthy.prefix.flags_or_shake_amplitude = 0;
        let mut bad_geometry = healthy.clone();
        bad_geometry.prefix.kind = 99;
        let mut bad_blend = healthy.clone();
        bad_blend.prefix.blend = 9;
        let mut missing_texture = healthy.clone();
        missing_texture.prefix.resource_slot = 1;
        EffectBank {
            resource: 7,
            source: SourceBank {
                source_sha256: "fixture".into(),
                programs: vec![],
                actors: vec![bad_geometry, bad_blend, healthy, missing_texture],
                modifiers: BTreeMap::new(),
                uv: vec![],
                uv_roots: vec![],
                art: None,
            },
            textures: BTreeMap::new(),
            members: BTreeSet::from([0, 1, 2, 3, 4]),
            palettes: BTreeMap::new(),
        }
    }

    #[test]
    fn tolerant_particle_loading_keeps_healthy_members_after_multiple_failures() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        let diagnostics = Diagnostics::new(false);
        let effects = Effects::load(
            vec![mixed_bank()],
            &[0.; 450],
            app.world().resource(),
            diagnostics.clone(),
        )
        .unwrap();
        assert_eq!(
            effects.banks[&7]
                .declarations
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![2]
        );
        let entries = diagnostics.entries();
        assert_eq!(entries.len(), 4);
        assert!(entries[0].message.contains("geometry 99"));
        assert!(entries[1].message.contains("blend 9"));
        assert!(entries[2].message.contains("missing bound texture slot 1"));
        assert!(
            entries[3]
                .message
                .contains("missing particle declaration 7:4")
        );
        // The accepted member has everything its ordinary untextured draw needs.
        assert!(select_declaration(&mixed_bank(), 2, true).is_ok());
    }

    #[test]
    fn paranoid_particle_loading_stops_at_the_first_unsupported_member() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        let diagnostics = Diagnostics::new(true);
        let result = Effects::load(
            vec![mixed_bank()],
            &[0.; 450],
            app.world().resource(),
            diagnostics.clone(),
        );
        assert!(format!("{:#}", result.err().unwrap()).contains("geometry 99"));
        assert_eq!(diagnostics.entries().len(), 1);
    }

    #[test]
    fn common_screen_ring_uses_atlas_zero_and_keeps_the_shadow_binding() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/screen-particle.json")).unwrap();
        let declaration: Declaration =
            serde_json::from_value(fixture["declaration"].clone()).unwrap();
        let p = &declaration.prefix;
        assert_eq!(
            (p.kind, p.resource_slot, p.flags_or_shake_amplitude),
            (4, 10, 0x04000040)
        );
        // Original atlas zero has CI4 color and alpha images (64/32 pages).
        // Actual shadow slot ten has just one image; it is not the mask atlas.
        let atlas = [texture(16, 64), texture(16, 32)];
        let key = MaterialKey::new(
            1,
            &declaration,
            palette(&atlas[0], p.palettes[0], p.palette_stride).unwrap(),
            Some(palette(&atlas[1], p.palettes[1], p.palette_stride).unwrap()),
            false,
        );
        assert!(key.screen);
        assert_eq!(
            (key.slot, key.color, key.alpha, key.blend),
            (0, 0, Some(1), 0)
        );
        let shadow = shadow_key(1, false);
        assert!(!shadow.screen);
        assert_eq!((shadow.slot, shadow.alpha), (10, None));
        assert!(shadow.cull_back);
    }

    fn texture(step: u16, pages: usize) -> Texture {
        Texture {
            palette_step: step,
            page_step: step,
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

    #[test]
    fn native_palette_stride_selects_published_tlut_windows() {
        // Address the entry offset before converting it to a cooked page.
        assert_eq!(palette(&texture(16, 42), 20, 32).unwrap(), 40);
        assert_eq!(palette(&texture(256, 3), 2, 0).unwrap(), 2);
        assert_eq!(palette(&texture(256, 3), 0, 32).unwrap(), 0);
        assert_eq!(palette(&texture(256, 3), 8, 32).unwrap(), 1);
        assert_eq!(palette(&texture(16, 3), 2, 8).unwrap(), 1);
        assert!(palette(&texture(16, 40), 20, 32).is_err());
        assert!(palette(&texture(256, 3), 1, 16).is_err());
        assert_eq!(palette(&texture(0, 1), 255, 32).unwrap(), 0);
        assert!(palette(&texture(0, 0), 0, 0).is_err());

        // Opening contact particle 7 uses offset 0 for color and offset 32
        // for alpha. Demon Fang particle 2 uses color offset 20 * 32 = 640.
        let mut color = texture(256, 41);
        color.page_step = 32;
        let mut alpha = texture(256, 9);
        alpha.page_step = 32;
        assert_eq!(palette(&color, 0, 32).unwrap(), 0);
        assert_eq!(palette(&alpha, 1, 32).unwrap(), 1);
        assert_eq!(palette(&color, 20, 32).unwrap(), 20);
        assert_eq!(palette(&color, 5, 0).unwrap(), 40);
        assert_eq!(palette(&alpha, 1, 0).unwrap(), 8);
        assert!(palette(&alpha, 9, 32).is_err());
        assert!(palette(&alpha, 1, 16).is_err());
        alpha.page_step = 0;
        assert!(palette(&alpha, 0, 32).is_err());
    }
}
