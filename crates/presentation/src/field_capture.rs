//! Meshes whose texture is the completed field image, before UI composition.
use super::{
    field_audit::{Applied, Request},
    field_view::{Art, State},
};
use crate::native::SceneSample;
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct Part {
    pub request: Request,
    pub particle: i32,
    pub vertex_color: bool,
}

pub(super) fn install(app: &mut App) {
    app.add_systems(
        PostUpdate,
        sync.after(bevy::transform::TransformSystems::Propagate)
            .after(super::field_model_particles::sync)
            .before(super::field_audit::check)
            .run_if(resource_exists::<super::field_view::Art>),
    );
}

#[allow(clippy::too_many_arguments)]
fn sync(
    state: State,
    art: Res<Art>,
    prediction: Option<Res<crate::field_view::Prediction>>,
    assets: Res<Assets<Mesh>>,
    mut parts: Query<(&Part, &Mesh3d, &mut SceneSample)>,
    views: Query<&Msaa, With<super::FieldCamera>>,
    ready: Res<super::field_refraction::Ready>,
    mut applied: ResMut<Applied>,
) {
    let world = &state.get().events.world;
    for (part, mesh, mut sample) in &mut parts {
        let Some(particle) = world.model_particles.get(&part.particle) else {
            continue;
        };
        if !assets.contains(&mesh.0) {
            applied.loading(part.request.clone());
            continue;
        }
        let color = |rgba: [u8; 4]| {
            Vec4::from_array(rgba.map(|v| f32::from(v) / 255.)) * Vec4::new(4., 4., 4., 1.)
        };
        let mut tint = color(particle.rgba);
        if let Some((next, alpha)) = prediction.as_ref().and_then(|p| {
            let next = p
                .field()?
                .events
                .world
                .model_particles
                .get(&part.particle)?;
            (next.born == particle.born && next.resource == particle.resource)
                .then_some((next, p.alpha))
        }) {
            tint = tint.lerp(color(next.rgba), alpha);
        }
        sample.tint = tint;
        sample.vertex_color = part.vertex_color;
        if ready.get()
            && views
                .single()
                .is_ok_and(|msaa| art.gpu == Some(msaa.samples()))
        {
            applied.ack(part.request.clone());
        } else {
            applied.loading(part.request.clone());
        }
    }
}
