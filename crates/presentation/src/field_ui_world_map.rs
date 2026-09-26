//! Both HUD views reuse the prepared shared-menu world textures.
use super::*;
use resonance_content::overworld::{WORLD_DEPTH, WORLD_WIDTH};

pub(super) struct Artwork {
    layers: [Layer; 2],
    sizes: [[u32; 2]; 2],
}
impl Artwork {
    pub fn prepare(
        menu: &menu::MenuArtwork,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Self {
        let art = [menu.world_map_art(0), menu.world_map_art(1)];
        let sizes = art.clone().map(|(_, size)| size);
        let layers = art.map(|(material, size)| {
            let mut batch = Batch::default();
            batch.quad([0., 0., 1., 1.], [0.; 4], [0.; 4]);
            let mesh = meshes.add(batch.mesh(size));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0., 0., 2.),
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
        });
        Self { layers, sizes }
    }
    pub fn despawn(self, world: &mut World) {
        for layer in self.layers {
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
        let mut map = Batch::default();
        if visible {
            for (index, alpha) in session.travel.map_opacity().into_iter().enumerate() {
                if alpha == 0 {
                    continue;
                }
                let layout = Layout::new(index == 0);
                let alpha = f32::from(alpha) / 255.;
                let [width, height] = self.sizes[world].map(|v| v as f32);
                map.quad(
                    [
                        -layout.size[0] / 2.,
                        -layout.size[1] / 2.,
                        layout.size[0] / 2.,
                        layout.size[1] / 2.,
                    ],
                    [0., 0., width, height],
                    [1., 1., 1., alpha * 0.85],
                );
                // Batch quads use centered, upward-positive render coordinates;
                // transform the four new vertices in the authored UI plane.
                let start = map.positions.len() - 4;
                for point in &mut map.positions[start..] {
                    let [x, y] = layout.transform([point[0] + 320., 240. - point[1]]);
                    *point = [x - 320., 240. - y, 0.];
                }
                if let Some(party) = &session.events.world.party {
                    for (landmark, _) in session.locations.visible(state.world) {
                        if party.travel.visited_locations.contains(&landmark.id) {
                            let [x, y] = layout.point(landmark.position);
                            overlay.quad(
                                [x - 2., y - 2., x + 2., y + 2.],
                                [0.5; 4],
                                [1., 0.85, 0.3, alpha],
                            );
                        }
                    }
                }
                let [x, y, _] = state.position.map();
                let center = layout.point([x, y]);
                draw_direction(overlay, center, state.camera_yaw - layout.angle, alpha);
            }
        }
        let drawn = !map.indices.is_empty();
        if drawn {
            self.layers[world].update_mesh(map, self.sizes[world], meshes)?;
        }
        for (index, layer) in self.layers.iter_mut().enumerate() {
            layer.show(drawn && index == world, commands);
        }
        Ok(())
    }
}

struct Layout {
    center: [f32; 2],
    size: [f32; 2],
    angle: f32,
}
impl Layout {
    fn new(small: bool) -> Self {
        if small {
            Self {
                center: [536., 344.],
                size: [144., 108.],
                angle: 0.,
            }
        } else {
            Self {
                center: [320., 224.],
                size: [384., 288.],
                angle: 0.,
            }
        }
    }
    fn transform(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        let (s, c) = self.angle.sin_cos();
        [
            self.center[0] + c * x - s * y,
            self.center[1] + s * x + c * y,
        ]
    }
    fn point(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        self.transform([
            (x / WORLD_WIDTH - 0.5) * self.size[0],
            (y / WORLD_DEPTH - 0.5) * self.size[1],
        ])
    }
}

// The original HUD draws the camera's viewing cone with vertex opacity,
// rather than a solid character-heading triangle.
fn draw_direction(batch: &mut Batch, center: [f32; 2], yaw: f32, opacity: f32) {
    let (s, c) = yaw.sin_cos();
    let start = batch.positions.len() as u32;
    for ([x, y], [r, g, b, a]) in [
        ([0., 6.], [240., 32., 32., 160.]),
        ([-9., -12.], [32., 0., 0., 32.]),
        ([0., -12.], [8., 0., 0., 8.]),
        ([9., -12.], [32., 0., 0., 32.]),
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
    fn viewing_cone_tracks_camera_cardinals_and_fades_away_from_player() {
        for (yaw, direction) in [
            (0., [0., 1.]),
            (std::f32::consts::FRAC_PI_2, [1., 0.]),
            (std::f32::consts::PI, [0., -1.]),
        ] {
            let mut batch = Batch::default();
            draw_direction(&mut batch, [320., 240.], yaw, 1.);
            let tip = batch.positions[2];
            assert!((tip[0] - direction[0] * 12.).abs() < 0.001);
            assert!((tip[1] - direction[1] * 12.).abs() < 0.001);
            assert!(batch.colors[0][3] > batch.colors[2][3]);
            assert!(batch.colors[0][3] < 1.);
        }
    }
}
