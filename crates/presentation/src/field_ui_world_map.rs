//! World map and compass share the prepared menu atlas.
use super::*;
use resonance_content::overworld::{WORLD_DEPTH, WORLD_WIDTH};
use std::f32::consts::{FRAC_PI_2, PI};

pub(super) struct Artwork {
    maps: [Layer; 2],
    map_sizes: [[u32; 2]; 2],
    decorations: Layer,
    atlas_size: [u32; 2],
    sprites: Vec<[u32; 4]>,
}
impl Artwork {
    pub fn prepare(
        menu: &menu::MenuArtwork,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<Self> {
        let art = [menu.world_map_art(0), menu.world_map_art(1)];
        let map_sizes = art.clone().map(|(_, size)| size);
        let mut layer = |material: Handle<Surface>, z| {
            let mut batch = Batch::default();
            batch.quad([0., 0., 1., 1.], [0.; 4], [0.; 4]);
            let mesh = meshes.add(batch.mesh([1, 1]));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., z),
                    Visibility::Hidden,
                ))
                .id();
            Layer {
                entity,
                mesh,
                material,
                uploaded: None,
                visible: false,
            }
        };
        let maps = art.map(|(material, _)| layer(material, 2.));
        let (material, atlas_size, sprites) = menu.world_hud_art()?;
        anyhow::ensure!(sprites.len() == 7, "world HUD artwork is incomplete");
        Ok(Self {
            maps,
            map_sizes,
            decorations: layer(material, 3.1),
            atlas_size,
            sprites,
        })
    }
    pub fn despawn(self, world: &mut World) {
        for layer in self.maps.into_iter().chain([self.decorations]) {
            world.despawn(layer.entity);
        }
    }
    pub fn render(
        &mut self,
        session: &resonance_game::overworld::Session,
        overlay: &mut Batch,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let state = session.travel.state();
        let world = state.world.index();
        let visible = session.cinematic.is_none()
            && session.active_skit.is_none()
            && session.menu.is_none()
            && session.prompt().is_none();
        let overlay_start = overlay.positions.len();
        let mut map = Batch::default();
        let mut decoration = Batch::default();
        if visible {
            self.compass(
                &mut decoration,
                state.camera_yaw,
                session.travel.camera_turn_input(),
            );
            let [x, y, _] = state.position.map();
            for (index, opacity) in session.travel.map_opacity().into_iter().enumerate() {
                if opacity == 0 {
                    continue;
                }
                let alpha = f32::from(opacity) / 255.;
                let slide = f32::from(255 - opacity) / 2.;
                let layout = Layout::new(index == 0, [x, y], slide);
                layout.map(&mut map, self.map_sizes[world], alpha);
                if let Some(party) = &session.events.world.party {
                    for (landmark, _) in session.locations.visible(state.world) {
                        let point = layout.point(landmark.position);
                        if !layout.contains(point) {
                            continue;
                        }
                        let color = if party.travel.visited_locations.contains(&landmark.id) {
                            [1., 0.5, 0.5, alpha]
                        } else {
                            [0.5, 0.5, 0.5, alpha]
                        };
                        overlay.quad(
                            [
                                point[0] - 1.5,
                                point[1] - 1.5,
                                point[0] + 1.5,
                                point[1] + 1.5,
                            ],
                            [0.5; 4],
                            color,
                        );
                    }
                }
                draw_direction(overlay, layout.point([x, y]), state.camera_yaw, alpha);
                if index == 0 {
                    sprite(
                        &mut decoration,
                        self.sprites[0],
                        [460. + slide, 281.],
                        alpha,
                    );
                }
            }
        }
        // World HUD artwork spans the full 448-line scene viewport.
        for p in map
            .positions
            .iter_mut()
            .chain(&mut decoration.positions)
            .chain(&mut overlay.positions[overlay_start..])
        {
            p[1] = 240. - (240. - p[1]) * 480. / 448.;
        }
        for (index, layer) in self.maps.iter_mut().enumerate() {
            let shown = index == world && !map.indices.is_empty();
            if shown {
                layer.update_mesh(map.clone(), self.map_sizes[world], meshes)?;
            }
            layer.show(shown, commands);
        }
        let shown = !decoration.indices.is_empty();
        if shown {
            self.decorations
                .update_mesh(decoration, self.atlas_size, meshes)?;
        }
        self.decorations.show(shown, commands);
        Ok(())
    }
    fn compass(&self, batch: &mut Batch, yaw: f32, steering: f32) {
        let center = [80., 64.];
        // The centre tilts with steering; the directional layer follows yaw.
        let tilt = (steering / 8.).trunc().to_radians();
        for (index, angle) in [(1, tilt), (2, yaw)] {
            let start = batch.positions.len();
            sprite(
                batch,
                self.sprites[index],
                [center[0] - 28., center[1] - 28.],
                1.,
            );
            let (s, c) = angle.sin_cos();
            for p in &mut batch.positions[start..] {
                let [x, y] = [p[0] + 320. - center[0], 240. - p[1] - center[1]];
                *p = [
                    center[0] + c * x - s * y - 320.,
                    240. - center[1] - s * x - c * y,
                    0.,
                ];
            }
        }
        for (index, angle) in [(3, PI), (4, FRAC_PI_2), (5, -FRAC_PI_2), (6, 0.)] {
            let (s, c) = (yaw + angle).sin_cos();
            sprite(
                batch,
                self.sprites[index],
                [68. + (48. * s).trunc(), 52. + (32. * c).trunc()],
                1.,
            );
        }
    }
}
fn sprite(batch: &mut Batch, rect: [u32; 4], [x, y]: [f32; 2], alpha: f32) {
    batch.quad(
        [x, y, x + rect[2] as f32, y + rect[3] as f32],
        glyph_uv(rect),
        [1., 1., 1., alpha],
    );
}

struct Layout {
    rect: [f32; 4],
    /// Upper-left point and span in wrapped world coordinates.
    origin: [f32; 2],
    span: [f32; 2],
}
impl Layout {
    fn new(small: bool, player: [f32; 2], slide: f32) -> Self {
        if small {
            let span = [28_800., 23_200.];
            Self {
                rect: [468. + slide, 300., 612. + slide, 416.],
                origin: [player[0] - span[0] / 2., player[1] - span[1] / 2.],
                span,
            }
        } else {
            Self {
                rect: [364. + slide, 228., 616. + slide, 417.],
                origin: [0.; 2],
                span: [WORLD_WIDTH, WORLD_DEPTH],
            }
        }
    }
    fn point(&self, point: [f32; 2]) -> [f32; 2] {
        std::array::from_fn(|i| {
            self.rect[i]
                + (point[i] - self.origin[i]).rem_euclid([WORLD_WIDTH, WORLD_DEPTH][i])
                    / self.span[i]
                    * (self.rect[i + 2] - self.rect[i])
        })
    }
    fn contains(&self, point: [f32; 2]) -> bool {
        (0..2).all(|i| point[i] >= self.rect[i] && point[i] < self.rect[i + 2])
    }
    fn map(&self, batch: &mut Batch, size: [u32; 2], alpha: f32) {
        // Split at world seams so a clamped map texture can wrap in either axis.
        let world = [WORLD_WIDTH, WORLD_DEPTH];
        let origin: [f32; 2] = std::array::from_fn(|i| self.origin[i].rem_euclid(world[i]));
        let first: [f32; 2] = std::array::from_fn(|i| self.span[i].min(world[i] - origin[i]));
        for row in 0..2 {
            for column in 0..2 {
                let cell = [column, row];
                let offset: [f32; 2] =
                    std::array::from_fn(|i| if cell[i] == 0 { 0. } else { first[i] });
                let span: [f32; 2] = std::array::from_fn(|i| {
                    if cell[i] == 0 {
                        first[i]
                    } else {
                        self.span[i] - first[i]
                    }
                });
                if span.contains(&0.) {
                    continue;
                }
                let source: [f32; 2] =
                    std::array::from_fn(|i| if cell[i] == 0 { origin[i] } else { 0. });
                let mut rect = [0.; 4];
                let mut uv = [0.; 4];
                for i in 0..2 {
                    let scale = (self.rect[i + 2] - self.rect[i]) / self.span[i];
                    rect[i] = self.rect[i] + offset[i] * scale;
                    rect[i + 2] = rect[i] + span[i] * scale;
                    uv[i] = source[i] / world[i] * size[i] as f32;
                    uv[i + 2] = (source[i] + span[i]) / world[i] * size[i] as f32;
                }
                batch.quad(rect, uv, [1., 1., 1., alpha * 0.5]);
            }
        }
    }
}

// The viewing cone fades away from the player.
fn draw_direction(batch: &mut Batch, center: [f32; 2], yaw: f32, opacity: f32) {
    let (s, c) = yaw.sin_cos();
    let start = batch.positions.len() as u32;
    for ([x, y], [r, g, b, a]) in [
        ([0., 16.], [240., 32., 32., 160.]),
        ([-24., -32.], [32., 0., 0., 32.]),
        ([0., -32.], [8., 0., 0., 8.]),
        ([24., -32.], [32., 0., 0., 32.]),
    ] {
        let [x, y] = [center[0] + c * x - s * y, center[1] + s * x + c * y];
        batch.positions.push([x - 320., 240. - y, 0.]);
        batch.uv.push([0.5; 2]);
        batch
            .colors
            .push([r / 255., g / 255., b / 255., opacity * a / 256.]);
    }
    batch
        .indices
        .extend([start, start + 1, start + 2, start, start + 2, start + 3]);
    batch.quad(
        [
            center[0] - 1.5,
            center[1] - 1.5,
            center[0] + 1.5,
            center[1] + 1.5,
        ],
        [0.5; 4],
        [1., 1., 0.5, opacity],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_map_wraps_both_seams_and_keeps_player_centered() {
        let layout = Layout::new(true, [30., 50.], 0.);
        assert_eq!(layout.point([30., 50.]), [540., 358.]);
        let mut map = Batch::default();
        layout.map(&mut map, [384, 288], 1.);
        assert_eq!(map.positions.len(), 16);
        assert!(
            map.uv
                .iter()
                .all(|p| (0. ..=384.).contains(&p[0]) && (0. ..=288.).contains(&p[1]))
        );
        assert!(layout.contains(layout.point([WORLD_WIDTH - 20., WORLD_DEPTH - 20.])));
        assert!(!layout.contains(layout.point([WORLD_WIDTH / 2., WORLD_DEPTH / 2.])));
    }
    #[test]
    fn viewing_cone_tracks_camera_cardinals_and_fades_away_from_player() {
        for (yaw, direction) in [
            (0., [0., 1.]),
            (std::f32::consts::FRAC_PI_2, [1., 0.]),
            (std::f32::consts::PI, [0., -1.]),
        ] {
            let mut batch = Batch::default();
            draw_direction(&mut batch, [320., 240.], yaw, 1.);
            let tip = batch.positions[2];
            assert!((tip[0] - direction[0] * 32.).abs() < 0.001);
            assert!((tip[1] - direction[1] * 32.).abs() < 0.001);
            assert!(batch.colors[0][3] > batch.colors[2][3]);
            assert!(batch.colors[0][3] < 1.);
        }
    }
}
