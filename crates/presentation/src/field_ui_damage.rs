//! Field-hazard digits use the shared system atlas, after dialogue and before menus.
use super::*;

const ACTOR_HEIGHT: f32 = 180.;
const DIGIT_WIDTH: f32 = 16.;
const DIGIT_HEIGHT: f32 = 24.;
const ATLAS_TOP: f32 = 32.;
const LOWER_EDGE_SKEW: f32 = 10.;
const DEPTH: f32 = 90.;

impl Artwork {
    pub(super) fn prepare_damage(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if self.damage_layer.is_some() {
            return;
        }
        self.damage_layer = Some(Layer::new(
            self.prompt_layers[0].material.clone(),
            DEPTH,
            commands,
            meshes,
        ));
    }

    fn sample_damage(
        &self,
        session: &FieldSession,
        prediction: Option<&crate::field_view::Prediction>,
    ) -> Batch {
        let world = &session.events.world;
        let future = prediction.and_then(|p| Some((p, &p.field()?.events.world)));
        let mut numbers = world
            .damage_numbers
            .sample_pairs(
                world.tick,
                future.map(|(_, next)| (&next.damage_numbers, next.tick)),
            )
            .peekable();
        let mut batch = Batch::default();
        let (Some(_), Some(actor), Some(camera)) = (
            numbers.peek(),
            world.actors.get(&world.controlled_actor),
            &world.field_camera,
        ) else {
            return batch;
        };
        let project = |camera: Transform, fov: f32, position: Vec3| {
            let ndc = scene_ndc(
                &camera,
                fov,
                position + Vec3::Z * ACTOR_HEIGHT,
                self.resolution,
            );
            scene_ui_point(ndc, self.resolution)
        };
        let position = Vec3::from_array(actor.position);
        let current = project(
            crate::field_view::camera_transform(camera),
            camera.fov_degrees(),
            position,
        );
        let (next, presented, fraction) = future.map_or((current, current, 0.), |(p, next)| {
            let target = next
                .actors
                .get(&world.controlled_actor)
                .filter(|a| {
                    world.controlled_actor == next.controlled_actor
                        && crate::field_view::prediction::continuous_actor(actor, a)
                })
                .map_or(position, |a| Vec3::from_array(a.position));
            let target_camera = p.camera(camera).unwrap_or(camera);
            let end = project(
                crate::field_view::camera_transform(target_camera),
                target_camera.fov_degrees(),
                target,
            );
            let presented = project(
                p.camera_transform(camera),
                p.camera_fov(camera),
                position.lerp(target, p.alpha),
            );
            (end, presented, p.alpha)
        });
        for (from, to) in numbers {
            let digits = from.amount.to_string();
            let center = (1. - digits.len() as f32) * DIGIT_WIDTH / 2.;
            let at = |anchor: Vec2, drift| anchor + Vec2::new(center + drift, 0.);
            let [x, y] = pixel_aligned_position(
                at(current, from.drift),
                at(next, to.drift),
                at(presented, from.drift.lerp(to.drift, fraction)),
                fraction,
            )
            .to_array();
            let alpha = f32::from(from.alpha).lerp(f32::from(to.alpha), fraction) / 255.;
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
                    [1., 1., 1., alpha],
                );
                for vertex in batch.positions.iter_mut().rev().take(2) {
                    vertex[0] -= LOWER_EDGE_SKEW;
                }
            }
        }
        batch
    }

    pub(super) fn render_damage(
        &mut self,
        session: &FieldSession,
        prediction: Option<&crate::field_view::Prediction>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        if session.active_skit.is_some() || session.menu_is_open() {
            return Ok(());
        }
        let batch = self.sample_damage(session, prediction);
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
