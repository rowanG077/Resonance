//! Ordinary textured quads attached to animated joints and the field floor.
use super::{ActorPart, Art, State};
use crate::field_audit::{Applied, Request};
use crate::sparse_animation::affine::Helper as TransformHelper;
use crate::{draw_order::DrawOrder, materials::TitleSurface};
use bevy::{
    image::{ImageLoaderSettings, ImageSampler},
    mesh::VertexAttributeValues,
    prelude::*,
};
use resonance_content::field::ContactShadow;
use resonance_events::effect::Blend;
use std::collections::BTreeMap;

pub(super) struct Artwork {
    pub(super) spec: ContactShadow,
    pub(super) texture: Handle<Image>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<TitleSurface>>,
    instances: BTreeMap<i32, Entity>,
}

#[derive(Resource)]
pub(crate) struct Shared(Artwork);

impl Shared {
    pub(crate) fn new(
        spec: &ContactShadow,
        server: &AssetServer,
        meshes: &mut Assets<Mesh>,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Self {
        let mut art = Artwork::load(spec, server);
        art.prepare(meshes, surfaces);
        Self(art)
    }

    pub(crate) fn image(&self) -> &Handle<Image> {
        &self.0.texture
    }
    pub(crate) fn binding(&self) -> (Handle<Mesh>, Handle<TitleSurface>) {
        self.0.binding().unwrap()
    }
}

impl Artwork {
    pub(super) fn reuse(&mut self, shared: &Shared) {
        if self.spec == shared.0.spec {
            self.texture = shared.0.texture.clone();
            self.mesh = shared.0.mesh.clone();
            self.material = shared.0.material.clone();
        }
    }

    fn prepare(&mut self, meshes: &mut Assets<Mesh>, surfaces: &mut Assets<TitleSurface>) {
        if self.mesh.is_some() {
            return;
        }
        let mut mesh = Mesh::from(Rectangle::from_size(Vec2::splat(self.spec.half_size * 2.)));
        if let Some(VertexAttributeValues::Float32x2(uvs)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
        {
            for uv in uvs {
                *uv = std::array::from_fn(|i| uv[i] * self.spec.uv_size[i]);
            }
        }
        self.mesh = Some(meshes.add(mesh));
        self.material = Some(surfaces.add(TitleSurface {
            field_fog: true,
            tint: Vec4::new(0., 0., 0., f32::from(self.spec.alpha) / 255.),
            blend: Some(Blend::Alpha),
            depth_write: false,
            cull: resonance_content::CullFace::None,
            ..TitleSurface::textured(Some(self.texture.clone()))
        }));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_defaults_share_geometry_and_local_settings_get_private_bindings() {
        let spec = resonance_content::field::settings::test_fixture().contact_shadow;
        let art = || Artwork {
            spec: spec.clone(),
            texture: Handle::default(),
            mesh: None,
            material: None,
            instances: BTreeMap::new(),
        };
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<TitleSurface>::default();
        let mut shared = Shared(art());
        shared.0.prepare(&mut meshes, &mut materials);
        let mut first = art();
        first.reuse(&shared);
        first.prepare(&mut meshes, &mut materials);
        let mut second = art();
        second.reuse(&shared);
        assert_eq!(first.binding(), second.binding());
        assert_eq!(first.binding(), Some(shared.binding()));
        assert_eq!((meshes.len(), materials.len()), (1, 1));
        let mut local = art();
        local.spec.half_size = 2.;
        local.spec.alpha = 64;
        local.reuse(&shared);
        assert!(local.binding().is_none());
        local.prepare(&mut meshes, &mut materials);
        assert_ne!(local.binding(), Some(shared.binding()));
        assert_eq!((meshes.len(), materials.len()), (2, 2));
        assert_eq!(shared.0.spec.half_size, 1.);
        assert_eq!(shared.0.spec.alpha, 128);
        assert!(shared.0.instances.is_empty());
    }
}
#[derive(Component)]
pub(super) struct Shadow(i32, u64);

pub(crate) fn diagnostic(world: &mut World) -> serde_json::Value {
    let shadows: BTreeMap<_, _> = world
        .query::<(&Shadow, &Transform, &Visibility)>()
        .iter(world)
        .map(|(shadow, transform, visibility)| {
            (
                shadow.0,
                serde_json::json!({
                    "position": transform.translation.to_array(),
                    "rotation": transform.rotation.to_array(),
                    "visible": *visibility != Visibility::Hidden,
                }),
            )
        })
        .collect();
    let Some(session) = world
        .get_resource::<crate::new_game::Session>()
        .filter(|session| session.is_field())
    else {
        return serde_json::Value::Null;
    };
    let collision = session.field().collision();
    serde_json::Value::Array(
        session
            .field()
            .events
            .world
            .actors
            .iter()
            .map(|(&id, actor)| {
                serde_json::json!({
                    "actor": id,
                    "casts_shadow": actor.casts_shadow,
                    "ground": collision.ground_surface(actor.position).map(|s| s.height),
                    "quad": shadows.get(&id),
                })
            })
            .collect(),
    )
}

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
    shadows.prepare(&mut meshes, &mut surfaces);
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
        let material = surfaces
            .get(shadows.material.as_ref().unwrap())
            .unwrap()
            .clone();
        let entity = commands
            .spawn((
                Mesh3d(shadows.mesh.as_ref().unwrap().clone()),
                MeshMaterial3d(surfaces.add(material)),
                Transform::default(),
                Visibility::Hidden,
                // Contact shadows darken translucent floor effects too.
                DrawOrder(crate::draw_order::Layer::Shadows, 0, 0),
                Shadow(id, actor.instance),
            ))
            .id();
        shadows.instances.insert(id, entity);
    }
}

#[allow(clippy::type_complexity)] // Separate reads of animated joints from writes to shadow quads.
pub(super) fn pose(
    art: Res<Art>,
    state: State,
    actors: Query<(Entity, &ActorPart)>,
    mut transforms: ParamSet<(
        TransformHelper,
        Query<(
            &mut Shadow,
            &mut Transform,
            &mut Visibility,
            &mut DrawOrder,
            &MeshMaterial3d<TitleSurface>,
        )>,
    )>,
    mut applied: ResMut<Applied>,
    mut materials: ResMut<Assets<TitleSurface>>,
) {
    // Animation has run, but propagation has not. Compute this tick's joint
    // transforms explicitly so moving characters do not leave a delayed shadow.
    let helper = transforms.p0();
    for (_, part) in actors
        .iter()
        .filter(|(_, part)| part.part == 0 && !part.prepared)
    {
        applied.loading(Request::Shadow(part.actor));
    }
    let anchors: BTreeMap<_, _> = actors
        .iter()
        .filter(|(_, part)| part.part == 0)
        .filter_map(|(root, part)| {
            let actor = state.get().events.world.actors.get(&part.actor)?;
            let anchor = helper
                .compute_global_transform(part.shadow_anchor?)
                .ok()?
                .translation();
            // Attached script positions are local to the parent's animated bone.
            // Query the floor from the world origin, below the animated hips.
            let ground_point = if actor.attachment.is_some() {
                helper
                    .compute_global_transform(root)
                    .ok()?
                    .translation()
                    .to_array()
            } else {
                actor.position
            };
            Some((part.actor, (anchor, ground_point)))
        })
        .collect();
    let collision = state.get().collision();
    for (mut shadow, mut transform, mut visibility, mut order, material) in &mut transforms.p1() {
        let Some(actor) = state.get().events.world.actors.get(&shadow.0) else {
            continue;
        };
        let alpha = f32::from(actor.shadow_alpha) / 255.;
        if materials
            .get(&material.0)
            .is_some_and(|m| m.tint.w != alpha)
        {
            materials.get_mut(&material.0).unwrap().tint.w = alpha;
        }
        // Overlapping black-alpha quads still round differently when reordered.
        let actor_order = state
            .get()
            .events
            .world
            .actor_order()
            .iter()
            .position(|id| *id == shadow.0)
            .expect("shadow actor has a submission order");
        order.set_if_neq(DrawOrder(crate::draw_order::Layer::Shadows, actor_order, 0));
        let anchor = anchors.get(&shadow.0);
        *visibility = Visibility::Hidden;
        if actor.visible
            && state.get().events.world.tick >= actor.visible_from
            && !actor.appearance.model_hidden
            && actor.casts_shadow
            && let Some((anchor, ground_point)) = anchor
        {
            place(
                &mut transform,
                *anchor,
                *ground_point,
                &collision,
                art.shadows.spec.height_offset,
                shadow.1 != actor.instance,
            );
            shadow.1 = actor.instance;
            *visibility = Visibility::Inherited;
            applied.ack(Request::Shadow(shadow.0));
        }
    }
}

fn place(
    transform: &mut Transform,
    anchor: Vec3,
    ground_point: [f32; 3],
    collision: &resonance_game::field::navigation::WalkMesh,
    height_offset: f32,
    replacement: bool,
) {
    let surface = collision.ground_surface(ground_point);
    // A missing floor keeps the actor's height and last floor tilt.
    // A replacement actor starts with a horizontal shadow.
    transform.translation = Vec3::new(
        anchor.x,
        anchor.y,
        surface.map_or(ground_point[2], |s| s.height) + height_offset,
    );
    if let Some(surface) = surface {
        transform.rotation = Quat::from_rotation_arc(Vec3::Z, Vec3::from_array(surface.normal));
    } else if replacement {
        transform.rotation = Quat::IDENTITY;
    }
}

#[cfg(test)]
mod placement_tests {
    use super::*;
    use resonance_content::field::CollisionGroup;
    use resonance_game::field::navigation::WalkMesh;

    fn floor(origin: [f32; 3], slope: f32) -> CollisionGroup {
        CollisionGroup {
            surface: 0,
            vertices: [[-100., -100.], [100., -100.], [0., 100.]]
                .map(|[x, y]| [origin[0] + x, origin[1] + y, origin[2] + x * slope])
                .into(),
            triangles: vec![[0, 1, 2]],
        }
    }

    #[test]
    fn attached_shadows_query_world_floor_and_follow_the_boat_without_a_floor() {
        let collision = WalkMesh::new(&[
            floor([0., -460., 45.], 0.),
            floor([1733., 3499., -109.], 0.1),
        ])
        .unwrap();
        // Palmacosta passengers retain Z=45 in boat coordinates, while the
        // animated boat places their feet at world Z=-109. The shadow's
        // horizontal anchor is an animated hip node, 85 units above the feet.
        let mut transform = Transform::default();
        place(
            &mut transform,
            Vec3::new(1743., 3499., -24.),
            [1733., 3499., -109.],
            &collision,
            2.,
            true,
        );
        assert_eq!(transform.translation, Vec3::new(1743., 3499., -107.));
        let normal = Vec3::new(-0.1, 0., 1.).normalize();
        assert!((transform.rotation * Vec3::Z).abs_diff_eq(normal, 0.00001));

        // The boat leaves the floor: keep its current world height and last
        // tilt instead of emitting the original overhead quad at Z=47.
        let tilt = transform.rotation;
        place(
            &mut transform,
            Vec3::new(2000., 4000., -21.),
            [2000., 4000., -106.],
            &collision,
            2.,
            false,
        );
        assert_eq!(transform.translation, Vec3::new(2000., 4000., -104.));
        assert_eq!(transform.rotation, tilt);
        place(
            &mut transform,
            Vec3::new(2000., 4000., -21.),
            [2000., 4000., -106.],
            &collision,
            2.,
            true,
        );
        assert_eq!(transform.rotation, Quat::IDENTITY);
    }

    #[test]
    fn ordinary_shadows_keep_the_actor_floor_query_and_animated_horizontal_anchor() {
        let collision = WalkMesh::new(&[floor([0., 0., 10.], 0.1)]).unwrap();
        let mut transform = Transform::default();
        // A walking pose can move the shadow node above or outside the floor.
        // Its original floor lookup still belongs to the unparented actor.
        place(
            &mut transform,
            Vec3::new(150., 0., 100.),
            [0., 0., 10.],
            &collision,
            2.,
            true,
        );
        assert_eq!(transform.translation, Vec3::new(150., 0., 12.));
        assert!(
            (transform.rotation * Vec3::Z)
                .abs_diff_eq(Vec3::new(-0.1, 0., 1.).normalize(), 0.00001)
        );
    }
}
