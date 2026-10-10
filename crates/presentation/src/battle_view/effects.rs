//! A fixed pool of draws with every material and pipeline prepared before entry.
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
use resonance_battle::{BattleFrame, MAX_PARTICLES, ParticleFrame};
use resonance_content::{
    TextureBinding,
    battle_effect::EffectTexture as Texture,
    battle_effect::{
        declaration::Declaration,
        visual::{ModelVisual, ParticleVisual},
    },
    diagnostics::Diagnostics,
    texture::Filter,
};
use resonance_game::battle::encounter::RenderEffect;
use std::collections::{BTreeMap, BTreeSet};

struct LoadedTexture {
    spec: Texture,
    images: BTreeMap<usize, Handle<Image>>,
}

struct Bank {
    declarations: BTreeMap<u16, Declaration>,
    palettes: BTreeMap<u16, BTreeSet<[u8; 2]>>,
    blends: BTreeMap<u16, BTreeSet<u8>>,
    textures: BTreeMap<u8, Vec<LoadedTexture>>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MaterialKey {
    resource: u32,
    slot: u8,
    color: usize,
    alpha: Option<usize>,
    blend: u8,
    depth_test: bool,
    depth_write: bool,
    cull_back: bool,
}

/// Particle model rendering differs from textured particle geometry.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ModelStyle {
    pub lit: bool,
    pub additive: bool,
    pub depth_test: bool,
    pub depth_write: bool,
    pub cull_back: bool,
}
impl ModelStyle {
    fn new(visual: &ModelVisual, opaque: bool) -> Self {
        Self {
            lit: visual.lit,
            additive: visual.blend == 1,
            depth_test: visual.depth_test,
            depth_write: visual.blend == 0 && opaque && visual.depth_test,
            cull_back: visual.cull_back,
        }
    }
}

impl MaterialKey {
    fn new(
        resource: u32,
        visual: &ParticleVisual,
        color: usize,
        alpha: Option<usize>,
        cull_back: bool,
        blend: Option<u8>,
    ) -> Self {
        Self {
            resource,
            slot: visual.texture.slot().unwrap_or(255),
            color,
            alpha,
            blend: blend.unwrap_or(visual.blend),
            depth_test: visual.depth_test,
            depth_write: visual.depth_write,
            cull_back,
        }
    }
}

struct Draw {
    entity: Entity,
    mesh: Handle<Mesh>,
}

pub(super) struct Effects {
    diagnostics: Diagnostics,
    banks: BTreeMap<u32, Bank>,
    materials: BTreeMap<MaterialKey, Handle<TitleSurface>>,
    warm: Vec<Entity>,
    draws: Vec<Draw>,
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
                            blends,
                            textures
                        }
                    )
                    .is_none(),
                "duplicate particle resource {}",
                bank.resource
            );
        }
        Ok(Self {
            diagnostics,
            banks: prepared,
            materials: BTreeMap::new(),
            warm: Vec::new(),
            draws: Vec::new(),
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
            let surface = surfaces.add(TitleSurface {
                tint: Vec4::new(2., 2., 2., 1.),
                multiply: alpha,
                multiply_alpha_only: key.alpha.is_some(),
                blend: Some(Blend::try_from(i32::from(key.blend)).map_err(anyhow::Error::msg)?),
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
            self.materials.insert(key, surface);
            self.warm.push(entity);
        }
        if let Some((_, surface)) = self.materials.first_key_value() {
            // The final two draws batch ordinary and additive ground shadows.
            for _ in 0..MAX_PARTICLES + 2 {
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

    pub fn apply(
        &mut self,
        frame: &BattleFrame,
        particles: &[ParticleFrame],
        camera: Mat3,
        meshes: &mut Assets<Mesh>,
        commands: &mut Commands,
        actor_order: &ActorOrder,
    ) -> Result<()> {
        ensure!(
            particles.len() <= MAX_PARTICLES,
            "particle snapshot exceeds scene budget"
        );
        for draw in &self.draws {
            commands.entity(draw.entity).insert(Visibility::Hidden);
        }
        let mut count = 0;
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
                let (key, size) = self.key(particle, declaration)?;
                let Some(mesh) = geometry::mesh(particle, declaration, size, camera)? else {
                    return Ok(());
                };
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
                let visual = declaration.particle_visual()?;
                let major = actor_order.effect(particle.draw_after, visual.after_actor)?;
                commands.entity(draw.entity).insert((
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
        let shadows = frame
            .models
            .iter()
            .filter(|model| model.visible)
            .filter_map(|model| {
                model
                    .shadow
                    .map(|shadow| (shadow.position, shadow.radius, shadow.color, false))
            })
            .chain(frame.projectile_shadows.iter().map(|shadow| {
                (
                    shadow.position,
                    shadow.appearance.radius,
                    shadow.appearance.color,
                    shadow.appearance.additive,
                )
            }));
        for additive in [false, true] {
            let Some(mesh) = geometry::shadows(
                shadows
                    .clone()
                    .filter(|s| s.3 == additive)
                    .map(|(p, r, c, _)| (p, r, c)),
            ) else {
                continue;
            };
            let draw = &mut self.draws[MAX_PARTICLES + usize::from(additive)];
            let surface = &self.materials[&shadow_key(additive)];
            *meshes
                .get_mut(&draw.mesh)
                .context("missing prepared shadow mesh")? = mesh;
            commands.entity(draw.entity).insert((
                MeshMaterial3d(surface.clone()),
                Visibility::Inherited,
                DrawOrder(Layer::Shadows, usize::from(additive), 0),
            ));
        }
        Ok(())
    }

    fn key(
        &self,
        particle: &ParticleFrame,
        declaration: &Declaration,
    ) -> Result<(MaterialKey, [u32; 2])> {
        let visual = declaration.particle_visual()?;
        let Some(slot) = visual.texture.slot() else {
            return Ok((
                MaterialKey::new(
                    particle.resource,
                    visual,
                    0,
                    None,
                    particle.state.cull_back,
                    particle.state.blend,
                ),
                [1; 2],
            ));
        };
        let textures = &self.banks[&particle.resource].textures[&slot];
        let set = visual.texture.palette_set();
        let color = palette(&textures[0].spec, particle.state.palettes[0], set)?;
        let alpha = if visual.texture.dual() {
            Some(palette(&textures[1].spec, particle.state.palettes[1], set)?)
        } else {
            None
        };
        let page = &textures[0].spec.images[color];
        Ok((
            MaterialKey::new(
                particle.resource,
                visual,
                color,
                alpha,
                particle.state.cull_back,
                particle.state.blend,
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
                let page = palette(color, pair[0], set)?;
                selected_pages.entry((slot, 0)).or_default().insert(page);
                if visual.texture.dual() {
                    let alpha = textures
                        .get(1)
                        .context("particle texture has no alpha image")?;
                    validate_texture(alpha)?;
                    let alpha_page = palette(alpha, pair[1], set)?;
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
        resource: 0,
        slot: 255,
        color: 0,
        alpha: None,
        blend: u8::from(additive),
        depth_test: true,
        depth_write: false,
        // Cull back faces for the shadow fan.
        cull_back: true,
    }
}

fn palette(texture: &Texture, index: u8, set: u8) -> Result<usize> {
    texture.palette_image(set, index)
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

fn material_keys(resource: u32, bank: &Bank) -> Result<BTreeSet<MaterialKey>> {
    let mut keys = BTreeSet::new();
    for (&member, declaration) in &bank.declarations {
        let Declaration::Particle { visual, .. } = declaration else {
            continue;
        };
        for pair in &bank.palettes[&member] {
            let (color, alpha) = if visual.texture.slot().is_none() {
                (0, None)
            } else {
                let textures = &bank.textures[&visual.texture.slot().unwrap()];
                let color = palette(&textures[0].spec, pair[0], visual.texture.palette_set())?;
                let alpha = if visual.texture.dual() {
                    Some(palette(
                        &textures[1].spec,
                        pair[1],
                        visual.texture.palette_set(),
                    )?)
                } else {
                    None
                };
                (color, alpha)
            };
            for cull in [false, true] {
                for &blend in &bank.blends[&member] {
                    keys.insert(MaterialKey::new(
                        resource,
                        visual,
                        color,
                        alpha,
                        cull,
                        Some(blend),
                    ));
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
        TextureWrap,
        battle_effect::visual::{ParticleShape, ParticleTexture},
        font::UiTexture,
        texture::{Sampler, TextureLod},
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
            resource: 7,
            source: std::sync::Arc::new(resonance_content::battle_effect::SourceBank {
                source_sha256: "fixture".into(),
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
            app.world().resource(),
            diagnostics.clone(),
        );
        assert!(format!("{:#}", result.err().unwrap()).contains("unsupported drawing geometry"));
        assert_eq!(diagnostics.entries().len(), 1);
    }

    #[test]
    fn palette_and_blend_overrides_have_prepared_materials() {
        let source = resonance_content::battle_effect::SourceBank {
            source_sha256: String::new(),
            programs: vec![],
            actors: vec![
                serde_json::from_str(include_str!(
                    "../../../content/tests/fixtures/material-particle.json"
                ))
                .unwrap(),
            ],
            art: None,
        };
        let input = RenderEffect {
            resource: 77,
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
        let keys = material_keys(77, &bank).unwrap();
        for cull in [false, true] {
            for (color, blend) in [(9, None), (26, Some(0))] {
                assert!(keys.contains(&MaterialKey::new(
                    77,
                    declaration.particle_visual().unwrap(),
                    color,
                    Some(1),
                    cull,
                    blend
                )));
            }
        }
        bank.palettes.insert(0, BTreeSet::from([[64, 1]]));
        assert!(material_keys(77, &bank).is_err());
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
        let keys = material_keys(1, &bank).unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.iter().all(|key| key.blend == 2 && key.slot == 255));
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

    #[test]
    fn palette_selection_uses_prepared_sets_and_checks_image_bounds() {
        let mut texture = texture(true, 3);
        texture.palette_sets.push(vec![Some(2), None, Some(0)]);
        assert_eq!(palette(&texture, 0, 1).unwrap(), 2);
        assert_eq!(palette(&texture, 2, 1).unwrap(), 0);
        assert!(palette(&texture, 1, 1).is_err());
        assert!(palette(&texture, 3, 0).is_err());
        assert!(palette(&texture, 0, 2).is_err());
        texture.palette_sets[0][0] = Some(3);
        assert!(palette(&texture, 0, 0).is_err());
    }
}
