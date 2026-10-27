//! Retain visual endpoints once per source/input revision. Render frames only blend.
use super::*;

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
    pub fog_range: Vec4,
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
        Model::Tile(_) => part
            .spec
            .texture_animations
            .iter()
            .find(|a| a.texture == texture)
            .is_some_and(|a| a.is_scroll()),
        Model::Cinematic(actor) => scene.session.cinematic.as_ref().is_some_and(|c| {
            c.definition.presentation.actors[&actor]
                .uv
                .iter()
                .find(|a| a.animation.texture == texture)
                .is_some_and(|a| a.animation.is_scroll())
        }),
        Model::Marker(17) => true,
        Model::Actor(effects::PORTAL_MODEL) => matches!(texture, 1 | 2),
        Model::Actor(effects::BARRIER_MODEL) => texture == 0,
        Model::Actor(203 | 214) => texture == 0,
        _ => instance.wake.is_some() && texture == 0,
    }
}
