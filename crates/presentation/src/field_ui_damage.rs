//! Field-hazard digits use the shared system atlas, after dialogue and before menus.
use super::*;
use resonance_content::{HEIGHT, SCENE_HEIGHT, WIDTH};

const ACTOR_HEIGHT: f32 = 180.;
const DIGIT_WIDTH: f32 = 16.;
const DIGIT_HEIGHT: f32 = 24.;
const ATLAS_TOP: f32 = 32.;
const LOWER_EDGE_SKEW: f32 = 10.;
const DEPTH: f32 = 90.;
const NEAR_CLIP: f32 = 100.;
const FAR_CLIP: f32 = 40000.;

impl Artwork {
    pub(super) fn prepare_damage(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if self.damage_layer.is_some() {
            return;
        }
        let mut batch = Batch::default();
        batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
        let mesh = meshes.add(batch.mesh([1, 1]));
        let material = self.prompt_layers[0].material.clone();
        let entity = commands
            .spawn((
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0., 0., DEPTH),
                Visibility::Hidden,
            ))
            .id();
        self.damage_layer = Some(Layer {
            entity,
            mesh,
            material,
            uploaded: None,
            visible: false,
        });
    }

    pub(super) fn render_damage(
        &mut self,
        session: &FieldSession,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        if session.active_skit.is_some() || session.menu_is_open() {
            return Ok(());
        }
        let world = &session.events.world;
        let mut batch = Batch::default();
        if let Some(actor) = world.actors.get(&world.controlled_actor)
            && let Some(camera) = &world.field_camera
        {
            let transform = super::super::field_view::camera_transform(camera);
            let ndc = Mat4::perspective_rh(
                camera.fov_degrees().to_radians(),
                self.resolution.aspect(),
                NEAR_CLIP,
                FAR_CLIP,
            )
            .project_point3(
                transform
                    .to_matrix()
                    .inverse()
                    .transform_point3(Vec3::from_array(actor.position) + Vec3::Z * ACTOR_HEIGHT),
            );
            let ui = self.resolution.ui_size();
            let position = [
                WIDTH as f32 * 0.5 + ui.x * 0.5 * ndc.x,
                HEIGHT as f32 * 0.5 - ui.y * 0.5
                    + ui.y * (SCENE_HEIGHT as f32 * 0.5 / HEIGHT as f32) * (1. - ndc.y),
            ];
            for number in world.damage_numbers.samples(world.tick) {
                let digits = number.amount.to_string();
                let x = (position[0] - digits.len() as f32 * DIGIT_WIDTH / 2.
                    + DIGIT_WIDTH / 2.
                    + number.drift)
                    .trunc();
                let y = position[1].trunc();
                for (index, digit) in digits.bytes().enumerate() {
                    let x = x + index as f32 * DIGIT_WIDTH;
                    let u = f32::from(digit - b'0') * DIGIT_WIDTH;
                    batch.quad(
                        [
                            x - DIGIT_WIDTH / 2.,
                            y - DIGIT_HEIGHT / 2.,
                            x + DIGIT_WIDTH / 2.,
                            y + DIGIT_HEIGHT / 2.,
                        ],
                        [
                            u,
                            ATLAS_TOP,
                            u + DIGIT_WIDTH - 1.,
                            ATLAS_TOP + DIGIT_HEIGHT - 1.,
                        ],
                        [1., 1., 1., f32::from(number.alpha) / 255.],
                    );
                    for vertex in batch.positions.iter_mut().rev().take(2) {
                        vertex[0] -= LOWER_EDGE_SKEW;
                    }
                }
            }
        }
        let visible = !batch.positions.is_empty();
        let layer = self.damage_layer.as_mut().expect("prepared damage layer");
        if visible {
            layer.update_mesh(
                batch,
                [self.spec.textures[0].width, self.spec.textures[0].height],
                meshes,
            )?;
        }
        layer.show(visible, commands);
        Ok(())
    }
}
