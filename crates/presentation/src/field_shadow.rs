//! Ordinary textured quads attached to animated joints and the field floor.
use super::{ActorPart, Art, State};
use crate::field_audit::{Applied, Request};
use crate::{draw_order::DrawOrder, materials::TitleSurface};
use bevy::{
    image::{ImageLoaderSettings, ImageSampler},
    mesh::VertexAttributeValues,
    prelude::*,
    transform::helper::TransformHelper,
};
use resonance_content::field::ContactShadow;
use std::collections::BTreeMap;

pub(super) struct Artwork {
    pub(super) spec: ContactShadow,
    pub(super) texture: Handle<Image>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<TitleSurface>>,
    instances: BTreeMap<i32, Entity>,
}
impl Artwork {
    pub(super) fn binding(&self) -> Option<(Handle<Mesh>, Handle<TitleSurface>)> {
        Some((self.mesh.clone()?, self.material.clone()?))
    }
    pub(super) fn load(spec: &ContactShadow, server: &AssetServer) -> Self {
        Self {
            spec: spec.clone(),
            texture: server
                .load_builder()
                .with_settings(|s: &mut ImageLoaderSettings| {
                    s.is_srgb = false;
                    s.sampler = ImageSampler::linear();
                })
                .load(spec.texture.clone()),
            mesh: None,
            material: None,
            instances: BTreeMap::new(),
        }
    }
    pub(super) fn despawn(&mut self, world: &mut World) {
        for entity in std::mem::take(&mut self.instances).into_values() {
            world.despawn(entity);
        }
    }
}
#[derive(Component)]
pub(super) struct Shadow(i32);

pub(super) fn sync(
    mut commands: Commands,
    mut art: ResMut<Art>,
    state: State,
    mut meshes: ResMut<Assets<Mesh>>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
) {
    if !art.ready {
        return;
    }
    let shadows = &mut art.shadows;
    if shadows.mesh.is_none() {
        let mut mesh = Mesh::from(Rectangle::from_size(Vec2::splat(
            shadows.spec.half_size * 2.,
        )));
        if let Some(VertexAttributeValues::Float32x2(uvs)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
        {
            for uv in uvs {
                *uv = std::array::from_fn(|i| uv[i] * shadows.spec.uv_size[i]);
            }
        }
        shadows.mesh = Some(meshes.add(mesh));
        shadows.material = Some(surfaces.add(TitleSurface {
            tint: Vec4::new(0., 0., 0., f32::from(shadows.spec.alpha) / 255.),
            blend: true,
            depth_write: false,
            cull: resonance_content::CullFace::None,
            ..TitleSurface::textured(Some(shadows.texture.clone()))
        }));
    }
    let removed: Vec<_> = shadows
        .instances
        .keys()
        .filter(|id| !state.get().events.world.actors.contains_key(id))
        .copied()
        .collect();
    for id in removed {
        commands
            .entity(shadows.instances.remove(&id).unwrap())
            .despawn();
    }
    for (&id, actor) in &state.get().events.world.actors {
        if !actor.casts_shadow || shadows.instances.contains_key(&id) {
            continue;
        }
        let entity = commands
            .spawn((
                Mesh3d(shadows.mesh.as_ref().unwrap().clone()),
                MeshMaterial3d(shadows.material.as_ref().unwrap().clone()),
                Transform::default(),
                Visibility::Hidden,
                // Contact shadows darken translucent floor effects too.
                DrawOrder(crate::draw_order::CONTACT_SHADOWS),
                Shadow(id),
            ))
            .id();
        shadows.instances.insert(id, entity);
    }
}

#[allow(clippy::type_complexity)] // Separate reads of animated joints from writes to shadow quads.
pub(super) fn pose(
    art: Res<Art>,
    state: State,
    actors: Query<&ActorPart>,
    mut transforms: ParamSet<(
        TransformHelper,
        Query<(&Shadow, &mut Transform, &mut Visibility)>,
    )>,
    mut applied: ResMut<Applied>,
) {
    // Animation has run, but propagation has not. Compute this tick's joint
    // transforms explicitly so moving characters do not leave a delayed shadow.
    let helper = transforms.p0();
    for part in actors
        .iter()
        .filter(|part| part.part == 0 && !part.prepared)
    {
        applied.loading(Request::Shadow(part.actor));
    }
    let anchors: BTreeMap<_, _> = actors
        .iter()
        .filter(|part| part.part == 0)
        .filter_map(|part| {
            Some((
                part.actor,
                helper
                    .compute_global_transform(part.shadow_anchor?)
                    .ok()?
                    .translation(),
            ))
        })
        .collect();
    for (shadow, mut transform, mut visibility) in &mut transforms.p1() {
        let Some(actor) = state.get().events.world.actors.get(&shadow.0) else {
            continue;
        };
        let surface = state.get().ground_surface(actor.position);
        let anchor = anchors.get(&shadow.0);
        *visibility = Visibility::Hidden;
        if actor.visible
            && !actor.appearance.model_hidden
            && actor.casts_shadow
            && let (Some(surface), Some(anchor)) = (surface, anchor)
        {
            transform.translation = Vec3::new(
                anchor.x,
                anchor.y,
                surface.height + art.shadows.spec.height_offset,
            );
            transform.rotation = Quat::from_rotation_arc(Vec3::Z, Vec3::from_array(surface.normal));
            *visibility = Visibility::Inherited;
            applied.ack(Request::Shadow(shadow.0));
        }
    }
}
