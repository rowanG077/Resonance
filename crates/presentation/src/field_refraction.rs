//! Heat haze and world-space ripples sample the scene before dialogue is composited.
use super::{
    field_audit::{Applied, Request},
    field_effects::{Artwork, effect_rotation},
    field_view::State,
};
use bevy::{prelude::*, render::RenderApp};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Resource, Clone, Default)]
pub(crate) struct Ready(Arc<AtomicBool>);
impl Ready {
    pub fn get(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub(crate) fn mark(&self) {
        self.0.store(true, Ordering::Release);
    }
    #[cfg(test)]
    pub(super) fn completed() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Pulse {
    center: Vec4,
    right: Vec4,
    up: Vec4,
    opacity: Vec4,
    tint: Vec4,
}
#[repr(C)]
#[derive(Component, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Settings {
    uv: [Vec4; 2],
    parameters: Vec4,
    screen_copy: Vec4,
    heat_transform: Vec4,
    heat_uv: Vec4,
}
#[derive(Component, Clone)]
pub(crate) struct Pulses(Vec<Pulse>);
#[derive(Component, Clone)]
pub(crate) struct Atlas([Handle<Image>; 3]);

pub(super) fn install(app: &mut App) {
    super::field_capture::install(app);
    super::field_dissolve::install(app);
    let ready = Ready::default();
    app.insert_resource(ready.clone()).add_systems(
        PostUpdate,
        sync.after(super::field_effects::render)
            .before(super::field_audit::check)
            .run_if(super::battle::field_presenting),
    );
    app.sub_app_mut(RenderApp).insert_resource(ready);
}

#[allow(clippy::too_many_arguments)]
fn sync(
    prediction: Option<Res<crate::field_view::Prediction>>,
    mut commands: Commands,
    state: State,
    art: Option<Res<Artwork>>,
    images: Res<Assets<Image>>,
    ready: Res<Ready>,
    mut applied: ResMut<Applied>,
    views: Query<(Entity, &Projection), With<super::FieldCamera>>,
) {
    let Some(art) = art else {
        for (entity, _) in &views {
            commands
                .entity(entity)
                .remove::<(Settings, Atlas, Pulses)>();
        }
        return;
    };
    let world = &state.get().events.world;
    let (recipe, air, textures) = art.refraction();
    let heat = art.heat_haze();
    let heat_enabled = heat.is_some_and(|(recipe, _)| {
        recipe.gate.active(
            world.texture_animation_enabled,
            &world.texture_bindings,
            |actor| world.actors.contains_key(&actor),
        )
    });
    let heat_transform = heat
        .filter(|_| heat_enabled)
        .map_or(Vec4::ZERO, |(recipe, _)| {
            let (sin, cos) = recipe.rotation_degrees.to_radians().sin_cos();
            let fraction = prediction
                .as_ref()
                .and_then(|p| {
                    (p.field()?.events.world.effect_tick == world.effect_tick.wrapping_add(1))
                        .then_some(p.alpha)
                })
                .unwrap_or_default();
            Vec4::new(
                cos * recipe.scale,
                sin * recipe.scale,
                ((f64::from(world.effect_tick) + f64::from(fraction))
                    * f64::from(recipe.scroll_per_tick)
                    * f64::from(recipe.scale)) as f32,
                f32::from(recipe.alpha) / 255.,
            )
        });
    let textures = [
        textures[0].clone(),
        textures[1].clone(),
        heat.map_or_else(|| textures[1].clone(), |(_, image)| image.clone()),
    ];
    let settings = Settings {
        uv: [recipe.sprite.uv, air.uv].map(Vec4::from_array),
        parameters: Vec4::new(
            world.refractions.len() as f32,
            // Normalize authored displacement across output resolutions.
            recipe.displacement[0] / resonance_content::WIDTH as f32,
            recipe.displacement[1] / resonance_content::SCENE_HEIGHT as f32,
            1. / super::camera::RASTER_SUBDIVISIONS,
        ),
        screen_copy: Vec4::new(
            world.screen_copy_depth[0],
            world.screen_copy_depth[1],
            super::camera::FIELD_NEAR / super::camera::FIELD_FAR,
            0.,
        ),
        heat_transform,
        heat_uv: heat.map_or(Vec4::ZERO, |(recipe, _)| Vec4::from_array(recipe.sprite.uv)),
    };
    let camera = world
        .field_camera
        .as_ref()
        .map_or(Transform::IDENTITY, |c| {
            prediction.as_ref().map_or_else(
                || super::field_view::camera_transform(c),
                |p| p.camera_transform(c),
            )
        });
    let view_from_world = camera.to_matrix().inverse();
    let mut effects: Vec<_> = world.refractions.iter().collect();
    effects.sort_unstable_by_key(|(_, effect)| effect.draw_order);
    for (entity, projection) in &views {
        let clip_from_world = projection.get_clip_from_view() * view_from_world;
        let pulses = effects
            .iter()
            .map(|(id, effect)| {
                let mut size = (effect.size / 2.).trunc() * 2.;
                let mut position = Vec3::from_array(effect.position);
                let mut alpha = effect.alpha(world.tick) / 255.;
                let future = prediction.as_ref().and_then(|p| {
                    let w = &p.field()?.events.world;
                    let n = w.refractions.get(id)?;
                    (n.born == effect.born).then_some((n, w, p.alpha))
                });
                if let Some((n, w, a)) = future {
                    size += ((n.size / 2.).trunc() * 2. - size) * a;
                    position = position.lerp(Vec3::from_array(n.position), a);
                    alpha += (n.alpha(w.tick) / 255. - alpha) * a;
                }
                if size <= 0. {
                    return Pulse::default();
                }
                let mut rotation = effect_rotation(
                    effect.orientation,
                    effect.rotation,
                    effect.rotation_order,
                    camera.rotation,
                );
                if let Some((n, _, a)) = future {
                    rotation = rotation.slerp(
                        effect_rotation(
                            n.orientation,
                            n.rotation,
                            n.rotation_order,
                            camera.rotation,
                        ),
                        a,
                    );
                }
                Pulse {
                    center: clip_from_world * position.extend(1.),
                    right: clip_from_world * (rotation * Vec3::X * size).extend(0.),
                    up: clip_from_world * (rotation * Vec3::Y * size).extend(0.),
                    opacity: Vec4::new(alpha, effect.image as u8 as f32, 0., 0.),
                    tint: Vec4::from_array(
                        art.palette(effect.palette)
                            .map(|v| f32::from(v) * 4. / 255.),
                    ),
                }
            })
            .collect();
        commands
            .entity(entity)
            .insert((settings, Atlas(textures.clone()), Pulses(pulses)));
    }
    let loaded =
        ready.get() && textures.iter().all(|texture| images.contains(texture)) && !views.is_empty();
    for &id in world.refractions.keys() {
        if loaded {
            applied.ack(Request::Refraction(id));
        } else {
            applied.loading(Request::Refraction(id));
        }
    }
}

impl Settings {
    /// Whether the refraction pass has nothing to draw this frame.
    pub(crate) fn idle(&self) -> bool {
        self.parameters.x == 0.
            && self.heat_transform.w == 0.
            && self.screen_copy.x == 0.
            && self.screen_copy.y == 0.
    }
}

/// One frame's refraction for the native frame renderer: the field camera's
/// settings, pulses, atlases and captured meshes, copied when the pass has
/// something to draw.
pub(crate) struct NativeRefraction {
    pub settings: Settings,
    pub pulses: Vec<Pulse>,
    pub atlas: [AssetId<Image>; 3],
    pub material: u32,
}
pub(crate) type NativeQuery = (&'static Settings, &'static Atlas, &'static Pulses);
impl NativeRefraction {
    pub(crate) fn new((settings, atlas, pulses): (&Settings, &Atlas, &Pulses)) -> Option<Self> {
        (!settings.idle()).then(|| Self {
            settings: *settings,
            pulses: pulses.0.clone(),
            atlas: atlas.0.each_ref().map(Handle::id),
            material: 0,
        })
    }
}
