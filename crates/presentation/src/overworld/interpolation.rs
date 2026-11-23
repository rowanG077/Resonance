//! Retain visual endpoints once per source/input revision. Render frames only blend.
use super::*;

/// Keep retained scenery transforms stable while the camera crosses a tile.
/// Both endpoints of a render frame use the committed scene's anchor.
pub(super) fn render_origin(position: Position) -> Position {
    let tile = position.tile();
    Position::from_map([
        f32::from(tile.column()) * TILE_SIZE,
        f32::from(tile.row()) * TILE_SIZE,
        0.,
    ])
    .unwrap()
}

/// Select the same torus copy as the moving native origin, expressed relative
/// to a fixed tile anchor. The copy can change only at the far world seam.
pub(super) fn render_position(anchor: Position, origin: Position, position: Position) -> Vec3 {
    use resonance_content::overworld::{WORLD_DEPTH, WORLD_WIDTH};
    let center = anchor.displacement_to(origin);
    let mut point = anchor.displacement_to(position);
    for (i, extent) in [WORLD_WIDTH, WORLD_DEPTH].into_iter().enumerate() {
        let relative = point[i] - center[i];
        if relative > extent / 2. {
            point[i] -= extent;
        } else if relative < -extent / 2. {
            point[i] += extent;
        }
    }
    Vec3::new(point[0], point[1], position.map()[2])
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Key {
    pub source: bevy::ecs::change_detection::Tick,
    pub prediction: Option<u64>,
    pub clock: u32,
}

#[derive(Default)]
pub(super) struct Cache {
    pub key: Option<Key>,
    pub from: ModelPose,
    pub to: ModelPose,
    pub bones_key: Option<Key>,
    pub bones: Vec<crate::sparse_animation::affine::PoseEndpoint>,
}

#[derive(Default)]
pub(super) struct ModelPose {
    pub transform: Transform,
    pub visible: bool,
    pub slot: Option<u16>,
    pub materials: Vec<SurfacePose>,
    pub fog_range: Vec4,
}
impl ModelPose {
    pub fn continuous(&self, next: &Self) -> bool {
        self.visible == next.visible
            && self
                .transform
                .translation
                .distance(next.transform.translation)
                < 1024.
            && self.materials.len() == next.materials.len()
    }
}

pub(super) struct SurfacePose {
    pub offsets: Vec4,
    pub scrolls: [bool; 2],
    pub tint: Vec4,
    pub blend: Option<Option<Blend>>,
}

pub(super) fn continuous(current: &Scene, next: &Scene) -> bool {
    current.session.travel.state().world == next.session.travel.state().world
        && current.session.cinematic.as_ref().map(|c| c.id)
            == next.session.cinematic.as_ref().map(|c| c.id)
        && current
            .session
            .travel
            .state()
            .position
            .distance_to(next.session.travel.state().position)
            < 1024.
}

pub(super) fn scroll(
    scene: &Scene,
    instance: &Instance,
    part: &Part,
    texture: Option<usize>,
) -> bool {
    let Some(texture) = texture else { return false };
    match instance.model {
        Model::Sky(_) => true,
        Model::Tile(..) => part
            .spec
            .texture_animations
            .iter()
            .find(|a| a.texture == texture)
            .is_some_and(|a| a.is_scroll()),
        Model::Cinematic(actor) => scene
            .session
            .cinematic
            .as_ref()
            .is_some_and(|playback| cinematic::uv_scrolls(playback, actor, texture)),
        Model::Marker(17) => true,
        Model::Actor(effects::PORTAL_MODEL) => matches!(texture, 1 | 2),
        Model::Actor(effects::BARRIER_MODEL) => texture == 0,
        Model::Actor(203 | 214) => texture == 0,
        _ => instance.wake.is_some() && texture == 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::overworld::{WORLD_DEPTH, WORLD_WIDTH};

    #[test]
    fn retained_space_preserves_native_view_positions_across_tiles_and_both_seams() {
        for x in [0., 0.25, TILE_SIZE - 0.25, TILE_SIZE, WORLD_WIDTH - 0.25] {
            for y in [0., 0.25, TILE_SIZE - 0.25, TILE_SIZE, WORLD_DEPTH - 0.25] {
                let origin = Position::from_map([x, y, 1500.]).unwrap();
                let anchor = render_origin(origin);
                for dx in [
                    -WORLD_WIDTH / 2. + 1.,
                    -10000.,
                    0.,
                    10000.,
                    WORLD_WIDTH / 2. - 1.,
                ] {
                    for dy in [
                        -WORLD_DEPTH / 2. + 1.,
                        -10000.,
                        0.,
                        10000.,
                        WORLD_DEPTH / 2. - 1.,
                    ] {
                        let position = origin.translated([dx, dy, -1250.]).unwrap();
                        let camera = render_position(anchor, origin, origin);
                        let object = render_position(anchor, origin, position);
                        let native = origin.displacement_to(position);
                        let expected = Vec3::new(native[0], native[1], 250.);
                        let camera_plane = Vec3::new(camera.x, camera.y, 0.);
                        assert!(
                            (object - camera_plane - expected).abs().max_element() <= 0.008,
                            "origin={origin:?} position={position:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn traveling_camera_does_not_change_retained_scenery_transforms() {
        let origin = Position::from_map([9770., 23500., 600.]).unwrap();
        let anchor = render_origin(origin);
        let scenery = Position::from_map([16000., 22000., 300.]).unwrap();
        let expected = render_position(anchor, origin, scenery);
        for i in 0..100 {
            let next = origin
                .translated([i as f32 * 0.5, i as f32 * 15., 0.])
                .unwrap();
            assert_eq!(render_origin(next), anchor);
            assert_eq!(render_position(anchor, next, scenery), expected);
        }
    }
}
