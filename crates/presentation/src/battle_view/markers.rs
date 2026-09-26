//! Prewarmed world-space HUD quads from 475DC, 50B94 and 51038.
use super::WARM_LAYER;
use crate::{
    draw_order::{DrawOrder, EFFECTS},
    materials::TitleSurface,
    scene::{SampledImages, sampled_image},
};
use anyhow::{Context, Result, ensure};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    image::{ImageLoaderSettings, ImageSampler},
    mesh::PrimitiveTopology,
    prelude::*,
};
use resonance_battle::BattleFrame;
use resonance_content::{TextureBinding, TextureWrap, battle_ui, font::UiTexture};

const CAPACITY: usize = 48;
const ORDER: u32 = EFFECTS - 65536;

struct Draw {
    entity: Entity,
    mesh: Handle<Mesh>,
}

pub(super) struct Markers {
    diagnostics: resonance_content::diagnostics::Diagnostics,
    art: battle_ui::Markers,
    textures: Vec<UiTexture>,
    images: Vec<Handle<Image>>,
    materials: Vec<Handle<TitleSurface>>,
    warm: Vec<Entity>,
    draws: Vec<Draw>,
}

impl Markers {
    pub fn load(
        art: battle_ui::Markers,
        server: &AssetServer,
        diagnostics: resonance_content::diagnostics::Diagnostics,
    ) -> Result<Self> {
        art.target_background.validate()?;
        ensure!(art.stun_period_ticks != 0, "invalid stun marker period");
        let textures: Vec<_> = std::iter::once(art.target_background.texture.clone())
            .chain(art.target_foregrounds.iter().cloned())
            .chain(std::iter::once(art.stun.clone()))
            .collect();
        for texture in &textures {
            texture.validate()?;
        }
        for texture in &art.target_foregrounds {
            for &rect in &art.target_frames {
                battle_ui::Sprite {
                    texture: texture.clone(),
                    rect,
                }
                .validate()?;
            }
        }
        for &rect in &art.stun_frames {
            battle_ui::Sprite {
                texture: art.stun.clone(),
                rect,
            }
            .validate()?;
        }
        let images = textures
            .iter()
            .map(|texture| {
                server
                    .load_builder()
                    .with_settings(|settings: &mut ImageLoaderSettings| {
                        settings.is_srgb = false;
                        settings.sampler = ImageSampler::linear();
                    })
                    .load(texture.path.clone())
            })
            .collect();
        Ok(Self {
            diagnostics,
            art,
            textures,
            images,
            materials: Vec::new(),
            warm: Vec::new(),
            draws: Vec::new(),
        })
    }

    pub fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.images.iter()
    }
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.warm
            .iter()
            .copied()
            .chain(self.draws.iter().map(|draw| draw.entity))
    }

    pub fn prepare(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        surfaces: &mut Assets<TitleSurface>,
        images: &mut Assets<Image>,
        sampled: &mut SampledImages,
    ) -> Result<()> {
        if !self.draws.is_empty() {
            return Ok(());
        }
        ensure!(
            self.images.iter().all(|image| images.contains(image)),
            "world marker textures are still loading"
        );
        for image in &self.images {
            let texture = sampled_image(
                Some((
                    image.clone(),
                    TextureBinding {
                        texture: 0,
                        wrap_u: TextureWrap::Clamp,
                        wrap_v: TextureWrap::Clamp,
                        nearest_min: false,
                        nearest_mag: false,
                    },
                )),
                images,
                sampled,
            );
            // Native47FA4 flags0x882000: alpha blend, double RGB modulation,
            // disabled depth test, enabled depth writes, no face culling.
            let surface = surfaces.add(TitleSurface {
                tint: Vec4::new(2., 2., 2., 1.),
                blend: true,
                depth_test: false,
                depth_write: true,
                cull: resonance_content::CullFace::None,
                ..TitleSurface::textured(texture)
            });
            self.warm
                .push(spawn(commands, meshes.add(warm_mesh()), surface.clone()));
            self.materials.push(surface);
        }
        for _ in 0..CAPACITY {
            let mesh = meshes.add(warm_mesh());
            let entity = spawn(commands, mesh.clone(), self.materials[0].clone());
            self.draws.push(Draw { entity, mesh });
        }
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
        camera: Mat3,
        meshes: &mut Assets<Mesh>,
        commands: &mut Commands,
    ) -> Result<()> {
        for draw in &self.draws {
            commands.entity(draw.entity).insert(Visibility::Hidden);
        }
        let mut quads = Vec::with_capacity(CAPACITY);
        for marker in &frame.stun_markers {
            let phase = usize::from((marker.phase / self.art.stun_period_ticks) & 1);
            let mut position = marker.position;
            position[1] = (position[1] + 8.) + 4. * phase as f32;
            quads.push(Quad {
                order: marker.actor.index() * 64,
                texture: 5,
                position,
                size: [48., 16.],
                color: [128, 128, 128, 255],
                rect: self.art.stun_frames[phase],
            });
        }
        let mut previous = None;
        let mut alpha = 240_u8;
        for marker in &frame.target_markers {
            if previous != Some(marker.target) {
                alpha = 240;
                previous = Some(marker.target);
            }
            let stage = target_stage(marker.phase);
            for (position, size, opacity) in
                target_quads(marker.position, marker.direction, marker.trail, &mut alpha)
            {
                let order = marker.target.index() * 64 + 1 + marker.owner.index() * 10;
                quads.push(Quad {
                    order,
                    texture: 0,
                    position,
                    size,
                    color: [128, 128, 128, opacity],
                    rect: self.art.target_background.rect,
                });
                quads.push(Quad {
                    order,
                    texture: usize::from(marker.control_slot) + 1,
                    position,
                    size,
                    color: [128, 128, 128, opacity],
                    rect: self.art.target_frames[stage],
                });
            }
        }
        if quads.len() > self.draws.len() {
            self.diagnostics.report(
                "battle world markers",
                anyhow::anyhow!("world markers exceed prepared capacity"),
            )?;
            quads.truncate(self.draws.len());
        }
        quads.sort_by_key(|quad| quad.order);
        for (index, quad) in quads.iter().enumerate() {
            let result = (|| -> Result<()> {
                let draw = &self.draws[index];
                let texture = self
                    .textures
                    .get(quad.texture)
                    .context("unprepared world marker palette")?;
                *meshes
                    .get_mut(&draw.mesh)
                    .context("missing prepared world marker mesh")? =
                    quad.mesh(camera, [texture.width, texture.height]);
                commands.entity(draw.entity).insert((
                    MeshMaterial3d(
                        self.materials
                            .get(quad.texture)
                            .context("unprepared world marker material")?
                            .clone(),
                    ),
                    Visibility::Inherited,
                    DrawOrder(ORDER, index),
                ));
                Ok(())
            })();
            if let Err(error) = result {
                self.diagnostics.report("battle world marker draw", error)?;
            }
        }
        for draw in &self.draws[quads.len()..] {
            commands.entity(draw.entity).insert(Visibility::Hidden);
        }
        Ok(())
    }

    pub fn despawn(self, commands: &mut Commands) {
        for entity in self.entities() {
            commands.entity(entity).despawn();
        }
    }
}

fn target_stage(phase: u16) -> usize {
    if phase < 172 {
        0
    } else if phase < 176 {
        1
    } else {
        2
    }
}

fn target_quads(
    mut position: [f32; 3],
    direction: [f32; 3],
    trail: u8,
    alpha: &mut u8,
) -> Vec<([f32; 3], [f32; 2], u8)> {
    let mut head = position;
    head[1] += 32.;
    let mut quads = vec![(head, [24., 32.], *alpha)];
    for index in 1..=trail {
        *alpha = alpha.wrapping_sub(32);
        position = std::array::from_fn(|i| position[i] + direction[i] * -35.);
        let height = (-1.5_f32).mul_add(f32::from(index), 32.);
        let mut point = position;
        point[1] += height;
        quads.push((point, [24. - f32::from(index), height], *alpha));
        *alpha = alpha.wrapping_sub(8);
    }
    quads
}

struct Quad {
    order: usize,
    texture: usize,
    position: [f32; 3],
    size: [f32; 2],
    color: [u8; 4],
    rect: [u32; 4],
}
impl Quad {
    fn mesh(&self, camera: Mat3, texture: [u32; 2]) -> Mesh {
        let center = Vec3::from_array(self.position);
        let [x, y] = self.size;
        let points = [
            Vec3::new(-x, y, 0.),
            Vec3::new(x, y, 0.),
            Vec3::new(-x, -y, 0.),
            Vec3::new(x, -y, 0.),
        ]
        .map(|point| (center + camera * point).to_array());
        let [u, v, w, h] = self.rect;
        let uv = [[u, v], [u + w, v], [u, v + h], [u + w, v + h]].map(|p| {
            [
                p[0] as f32 / texture[0] as f32,
                p[1] as f32 / texture[1] as f32,
            ]
        });
        let indices = [0, 1, 2, 2, 1, 3];
        let positions: Vec<_> = indices.map(|i| points[i]).into();
        let uv: Vec<_> = indices.map(|i| uv[i]).into();
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 6])
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_COLOR,
            vec![self.color.map(|v| f32::from(v) / 255.); 6],
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uv)
    }
}

fn warm_mesh() -> Mesh {
    Quad {
        order: 0,
        texture: 0,
        position: [0.; 3],
        size: [1.; 2],
        color: [255; 4],
        rect: [0, 0, 1, 1],
    }
    .mesh(Mat3::IDENTITY, [1; 2])
}

fn spawn(commands: &mut Commands, mesh: Handle<Mesh>, surface: Handle<TitleSurface>) -> Entity {
    commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(surface),
            Transform::default(),
            Visibility::Inherited,
            NoFrustumCulling,
            RenderLayers::layer(WARM_LAYER),
            DrawOrder(ORDER, 0),
        ))
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_pointer_phase_and_trail_geometry() {
        assert_eq!(
            [0, 171, 172, 175, 176, 179].map(target_stage),
            [0, 0, 1, 1, 2, 2]
        );
        let mut alpha = 240;
        let quads = target_quads([100., 200., 300.], [1., 0., 0.], 4, &mut alpha);
        assert_eq!(quads[0], ([100., 232., 300.], [24., 32.], 240));
        assert_eq!(quads[1], ([65., 230.5, 300.], [23., 30.5], 208));
        assert_eq!(quads[4], ([-40., 226., 300.], [20., 26.], 88));
        assert_eq!(alpha, 80);
    }
}
