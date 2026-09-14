//! Classroom-only Bevy Solari prototype. Authored geometry is retained for F6
//! comparison; opaque scenery and posed characters receive deferred lighting.
mod actors;
pub(super) mod denoise;
mod matte;
mod mesh;
mod output;
pub(super) mod pathtracer;
mod smaa;
mod symbols;
mod windows;

use super::{
    field_view::{ActorPart, Art},
    materials::TitleSurface,
};
use bevy::{
    anti_alias::{
        smaa::{Smaa, SmaaPreset},
        taa::TemporalAntiAliasing,
    },
    camera::{CameraMainTextureUsages, CameraOutputMode, Hdr, visibility::VisibilitySystems},
    core_pipeline::{
        prepass::{
            DeferredPrepass, DeferredPrepassDoubleBuffer, DepthPrepass, DepthPrepassDoubleBuffer,
            MotionVectorPrepass,
        },
        tonemapping::Tonemapping,
    },
    material::OpaqueRendererMethod,
    mesh::skinning::SkinnedMesh,
    pbr::DefaultOpaqueRendererMethod,
    prelude::*,
    render::{
        camera::{MipBias, TemporalJitter},
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_resource::{BlendState, TextureUsages},
        renderer::{RenderAdapterInfo, RenderDevice},
        view::{ColorGrading, ColorGradingGlobal},
    },
    solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins},
};
use resonance_content::field::SCENERY_RESOURCE_BASE;
use std::collections::HashMap;

#[derive(Resource)]
pub(super) struct State {
    enabled: bool,
    supported: bool,
    software: bool,
    active: bool,
    pathtracing: bool,
}
impl State {
    /// Keep warmup and live views on identical material specialization keys.
    pub(super) fn prepare_view(&self, camera: &mut EntityCommands) {
        camera.insert((
            ModernView,
            Hdr,
            AmbientLight {
                color: Color::srgb(0.83, 0.9, 1.),
                brightness: 250.,
                ..default()
            },
            Msaa::Off,
            Smaa {
                preset: SmaaPreset::Ultra,
            },
            Tonemapping::ReinhardLuminance,
            // A bright, painted daylight palette. Grade only this 3D camera;
            // the separate dialogue compositor retains its authored colors.
            ColorGrading {
                global: ColorGradingGlobal {
                    exposure: 0.6,
                    post_saturation: 1.15,
                    ..default()
                },
                ..default()
            },
        ));
        if self.supported {
            camera.insert(CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING));
            if self.pathtracing {
                camera.insert(pathtracer::Pathtraced);
            } else {
                camera.insert(SolariLighting::default());
            }
        }
    }
}
#[derive(Component, Clone, ExtractComponent)]
struct ModernView;
#[derive(Component)]
struct Lighting;
#[derive(Component)]
struct Converted {
    original_mesh: Handle<Mesh>,
    original_material: Handle<TitleSurface>,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    active: bool,
}
#[derive(Resource, Default)]
struct Cache {
    meshes: HashMap<AssetId<Mesh>, Handle<Mesh>>,
    images: HashMap<AssetId<Image>, Handle<Image>>,
}

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "ray_tracing_output.wgsl");
    app.add_plugins((
        SolariPlugins,
        ExtractComponentPlugin::<ModernView>::default(),
    ))
    // Solari globally defaults standard materials to deferred. Limit that
    // choice to our converted room so other views keep their renderer.
    .insert_resource(DefaultOpaqueRendererMethod::forward())
    .insert_resource(State {
        enabled: true,
        supported: false,
        software: false,
        active: false,
        pathtracing: false,
    })
    .init_resource::<Cache>()
    .add_systems(Startup, check_support)
    .add_systems(PreUpdate, toggle.after(bevy::input::InputSystems))
    .add_systems(
        PostUpdate,
        (activate, convert, sync)
            .chain()
            .after(VisibilitySystems::VisibilityPropagate)
            .before(bevy::asset::AssetEventSystems),
    );
    app.add_systems(
        PostUpdate,
        (actors::sync, windows::sync, symbols::sync)
            .after(sync)
            .after(bevy::transform::TransformSystems::Propagate)
            .before(bevy::asset::AssetEventSystems),
    );
    output::install(app);
    denoise::install(app);
    matte::install(app);
}
fn check_support(
    device: Res<RenderDevice>,
    adapter: Res<RenderAdapterInfo>,
    mut state: ResMut<State>,
) {
    state.software = adapter.name.contains("llvmpipe");
    state.supported = device
        .features()
        .contains(SolariPlugins::required_wgpu_features());
    if state.supported {
        info!("Bevy Solari enabled for the Iselia classroom; F6 compares the original renderer");
        if state.software {
            info!(
                "Lavapipe: resetting Solari temporal reservoirs; ray-traced shadows and spatial sampling remain enabled"
            );
        }
    } else {
        warn!(
            "This GPU cannot run Bevy Solari (missing {:?}); the classroom will use Bevy PBR lighting and shadow maps",
            SolariPlugins::required_wgpu_features().difference(device.features())
        );
    }
}

pub(super) fn configure_capture(app: &mut App, spec: &crate::ClassroomShowcase) {
    if spec.pathtracing {
        pathtracer::install(app);
    }
    if spec.strong_smaa {
        smaa::install(app);
    }
}
fn toggle(keys: Res<ButtonInput<KeyCode>>, mut state: ResMut<State>) {
    if keys.just_pressed(KeyCode::F6) {
        state.enabled = !state.enabled;
        info!(
            "Classroom modern lighting: {}",
            if state.enabled { "on" } else { "off" }
        );
    }
}
#[allow(clippy::too_many_arguments)] // Camera state, field lifetime and lighting assets.
fn activate(
    mut commands: Commands,
    art: Option<Res<Art>>,
    mut state: ResMut<State>,
    cameras: Query<(Entity, Option<&ModernView>), With<super::FieldCamera>>,
    mut overlays: Query<&mut Camera, With<super::FieldOverlayCamera>>,
    lights: Query<Entity, With<Lighting>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<Cache>,
) {
    let active = state.enabled && art.as_ref().is_some_and(|art| art.map == 340 && art.ready);
    if active != state.active {
        // HDR 3D and LDR UI cameras have separate intermediate textures. The
        // overlay must alpha-composite onto the completed HDR output instead
        // of overwriting it with its otherwise empty LDR texture.
        for mut camera in &mut overlays {
            camera.clear_color = if active {
                ClearColorConfig::Custom(Color::NONE)
            } else {
                ClearColorConfig::None
            };
            camera.output_mode = if active {
                CameraOutputMode::Write {
                    blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    clear_color: ClearColorConfig::None,
                }
            } else {
                CameraOutputMode::default()
            };
        }
    }
    for (entity, traced) in &cameras {
        if active && traced.is_none() {
            state.prepare_view(&mut commands.entity(entity));
        } else if !active && traced.is_some() {
            // Required components aren't removed automatically with Solari.
            commands
                .entity(entity)
                .remove::<(
                    ModernView,
                    SolariLighting,
                    Hdr,
                    AmbientLight,
                    DeferredPrepass,
                    DeferredPrepassDoubleBuffer,
                    DepthPrepass,
                    DepthPrepassDoubleBuffer,
                    MotionVectorPrepass,
                    TemporalAntiAliasing,
                    Smaa,
                    TemporalJitter,
                    MipBias,
                )>()
                .insert((
                    CameraMainTextureUsages::default(),
                    Tonemapping::None,
                    ColorGrading::default(),
                ));
        }
    }
    if active && !state.active {
        commands.spawn((
            Lighting,
            DirectionalLight {
                color: Color::srgb(1., 0.93, 0.78),
                illuminance: 19000.,
                shadow_maps_enabled: !state.supported,
                ..default()
            },
            Transform::default().looking_to(Vec3::new(-1., 0.35, -0.65), Vec3::Z),
            bevy::light::CascadeShadowConfigBuilder {
                minimum_distance: 100.,
                first_cascade_far_bound: 1200.,
                maximum_distance: 5000.,
                ..default()
            }
            .build(),
        ));
        // Window emitters follow the actual panes on both walls (windows::sync).
        // A restrained ceiling-height fill approximates bounced skylight.
        // The windows must dominate: a bright room-sized panel erases the
        // difference between their light and the sheltered sides of objects.
        // Keeping it above the windows avoids shading their upper surrounds.
        // No Mesh3d: the lighting proxy doesn't paint over the original art.
        if state.supported {
            let panel = mesh::prepare(&Mesh::from(Rectangle::new(1240., 1400.))).unwrap();
            commands.spawn((
                Lighting,
                RaytracingMesh3d(meshes.add(panel)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    emissive: LinearRgba::rgb(650., 715., 845.),
                    ..default()
                })),
                Transform::from_xyz(-290., -25., 380.).looking_to(Vec3::Z, Vec3::Y),
            ));
        }
    } else if !active && state.active {
        for entity in &lights {
            commands.entity(entity).despawn();
        }
    }
    if art.as_ref().is_none_or(|art| art.map != 340) {
        cache.meshes.clear();
        cache.images.clear();
    }
    state.active = active;
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn convert(
    mut commands: Commands,
    state: Res<State>,
    roots: Query<(Entity, &ActorPart)>,
    children: Query<&Children>,
    candidates: Query<
        (&Mesh3d, &MeshMaterial3d<TitleSurface>),
        (Without<Converted>, Without<SkinnedMesh>),
    >,
    surfaces: Res<Assets<TitleSurface>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<Cache>,
) {
    if !state.active {
        return;
    }
    for (root, part) in &roots {
        if part.resource < SCENERY_RESOURCE_BASE || !part.prepared {
            continue;
        }
        for entity in children.iter_descendants(root) {
            let Ok((source_mesh, source_material)) = candidates.get(entity) else {
                continue;
            };
            let Some(surface) = surfaces.get(&source_material.0) else {
                continue;
            };
            // Solari 0.19 treats every traced triangle as opaque. Keep glass,
            // cutouts, shafts, outlines and particles on the authored path.
            if surface.blend || surface.additive || surface.constant_color || !surface.depth_write {
                continue;
            }
            let mesh = if let Some(mesh) = cache.meshes.get(&source_mesh.0.id()) {
                mesh.clone()
            } else {
                let Some(source) = meshes.get(&source_mesh.0) else {
                    continue;
                };
                let converted = match mesh::prepare(source) {
                    Ok(mesh) => meshes.add(mesh),
                    Err(error) => {
                        warn!("Cannot prepare classroom mesh for Solari: {error}");
                        continue;
                    }
                };
                cache.meshes.insert(source_mesh.0.id(), converted.clone());
                converted
            };
            let texture = if let Some(source) = &surface.color {
                if let Some(texture) = cache.images.get(&source.id()) {
                    Some(texture.clone())
                } else {
                    let Some(image) = images.get(source) else {
                        continue;
                    };
                    // The authored shader uses encoded color; PBR needs sRGB
                    // texture decoding. Retain the imported sampler settings.
                    let mut image = image.clone();
                    image.texture_descriptor.format =
                        image.texture_descriptor.format.add_srgb_suffix();
                    let texture = images.add(image);
                    cache.images.insert(source.id(), texture.clone());
                    Some(texture)
                }
            } else {
                None
            };
            let material = materials.add(StandardMaterial {
                base_color_texture: texture,
                perceptual_roughness: 0.72,
                reflectance: 0.35,
                opaque_render_method: if state.supported {
                    OpaqueRendererMethod::Deferred
                } else {
                    OpaqueRendererMethod::Forward
                },
                // Script cameras can sit above the ceiling or outside a wall.
                // Preserve the authored raster culling so those back faces
                // don't cover the room. Solari still traces the solid shell.
                double_sided: surface.cull == resonance_content::CullFace::None,
                cull_mode: match surface.cull {
                    resonance_content::CullFace::Back => {
                        Some(bevy::render::render_resource::Face::Back)
                    }
                    resonance_content::CullFace::Front => {
                        Some(bevy::render::render_resource::Face::Front)
                    }
                    resonance_content::CullFace::None => None,
                },
                ..default()
            });
            commands.entity(entity).insert(Converted {
                original_mesh: source_mesh.0.clone(),
                original_material: source_material.0.clone(),
                mesh,
                material,
                active: false,
            });
        }
    }
}
fn sync(
    mut commands: Commands,
    state: Res<State>,
    mut converted: Query<(Entity, &InheritedVisibility, &mut Converted)>,
    surfaces: Res<Assets<TitleSurface>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut views: Query<&mut SolariLighting>,
) {
    // Mesa 26.2 Lavapipe loses lighting energy when temporal and spatial
    // reservoir reuse are combined. Fresh temporal history keeps Solari's
    // ray-traced lighting, spatial reuse and shadows working on the CPU.
    if state.software {
        for mut lighting in &mut views {
            lighting.reset = true;
        }
    }
    for (entity, visibility, mut converted) in &mut converted {
        // Solari doesn't inspect Visibility. Explicitly remove hidden meshes
        // from its scene, including the room's scripted wall visibility.
        let active = state.active && visibility.get();
        if active != converted.active {
            let mut entity = commands.entity(entity);
            if active {
                entity.remove::<MeshMaterial3d<TitleSurface>>().insert((
                    Mesh3d(converted.mesh.clone()),
                    MeshMaterial3d(converted.material.clone()),
                ));
                if state.supported {
                    entity.insert(RaytracingMesh3d(converted.mesh.clone()));
                }
            } else {
                entity
                    .remove::<(RaytracingMesh3d, MeshMaterial3d<StandardMaterial>)>()
                    .insert((
                        Mesh3d(converted.original_mesh.clone()),
                        MeshMaterial3d(converted.original_material.clone()),
                    ));
            }
            converted.active = active;
        }
        if active && let Some(surface) = surfaces.get(&converted.original_material) {
            let tint = surface.tint;
            let color = Color::srgb(tint.x, tint.y, tint.z);
            if materials
                .get(&converted.material)
                .is_some_and(|m| m.base_color != color)
            {
                materials.get_mut(&converted.material).unwrap().base_color = color;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_scenery_and_toggle_restore_original_bindings() {
        for supported in [false, true] {
            let mut app = App::new();
            app.insert_resource(State {
                enabled: true,
                supported,
                software: false,
                active: true,
                pathtracing: false,
            })
            .init_resource::<Assets<TitleSurface>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Mesh>>()
            .add_systems(Update, sync);
            let original_mesh = app
                .world_mut()
                .resource_mut::<Assets<Mesh>>()
                .add(Rectangle::new(1., 1.));
            let traced_mesh = app
                .world_mut()
                .resource_mut::<Assets<Mesh>>()
                .add(Rectangle::new(2., 2.));
            let original_material = app
                .world_mut()
                .resource_mut::<Assets<TitleSurface>>()
                .add(TitleSurface::default());
            let material = app
                .world_mut()
                .resource_mut::<Assets<StandardMaterial>>()
                .add(StandardMaterial::default());
            let entity = app
                .world_mut()
                .spawn((
                    InheritedVisibility::VISIBLE,
                    Mesh3d(original_mesh.clone()),
                    MeshMaterial3d(original_material.clone()),
                    Converted {
                        original_mesh: original_mesh.clone(),
                        original_material: original_material.clone(),
                        mesh: traced_mesh.clone(),
                        material,
                        active: false,
                    },
                ))
                .id();
            app.update();
            assert_eq!(app.world().get::<Mesh3d>(entity).unwrap().0, traced_mesh);
            assert_eq!(
                app.world().get::<RaytracingMesh3d>(entity).is_some(),
                supported
            );
            assert!(
                app.world()
                    .get::<MeshMaterial3d<TitleSurface>>(entity)
                    .is_none()
            );
            app.world_mut()
                .entity_mut(entity)
                .insert(InheritedVisibility::HIDDEN);
            app.update();
            assert!(app.world().get::<RaytracingMesh3d>(entity).is_none());
            assert_eq!(app.world().get::<Mesh3d>(entity).unwrap().0, original_mesh);
            assert_eq!(
                app.world()
                    .get::<MeshMaterial3d<TitleSurface>>(entity)
                    .unwrap()
                    .0,
                original_material
            );
            app.world_mut()
                .entity_mut(entity)
                .insert(InheritedVisibility::VISIBLE);
            app.update();
            assert_eq!(
                app.world().get::<RaytracingMesh3d>(entity).is_some(),
                supported
            );
            app.world_mut().resource_mut::<State>().active = false;
            app.update();
            assert!(app.world().get::<RaytracingMesh3d>(entity).is_none());
            assert!(
                app.world()
                    .get::<MeshMaterial3d<StandardMaterial>>(entity)
                    .is_none()
            );
            assert_eq!(app.world().get::<Mesh3d>(entity).unwrap().0, original_mesh);
        }
    }

    #[test]
    fn leaving_classroom_removes_hdr_prepasses_and_lights() {
        let mut app = App::new();
        app.insert_resource(State {
            enabled: true,
            supported: true,
            software: false,
            active: true,
            pathtracing: false,
        })
        .init_resource::<Cache>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_systems(Update, activate);
        let camera = app
            .world_mut()
            .spawn((
                super::super::FieldCamera,
                ModernView,
                SolariLighting::default(),
                Smaa {
                    preset: SmaaPreset::Ultra,
                },
                Tonemapping::ReinhardLuminance,
                ColorGrading {
                    global: ColorGradingGlobal {
                        exposure: 0.6,
                        post_saturation: 1.15,
                        ..default()
                    },
                    ..default()
                },
                CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
            ))
            .id();
        let light = app.world_mut().spawn(Lighting).id();
        let overlay = app
            .world_mut()
            .spawn((
                super::super::FieldOverlayCamera,
                Camera {
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    output_mode: CameraOutputMode::Write {
                        blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        clear_color: ClearColorConfig::None,
                    },
                    ..default()
                },
            ))
            .id();
        app.update();
        assert!(app.world().get::<SolariLighting>(camera).is_none());
        assert!(app.world().get::<Smaa>(camera).is_none());
        assert!(app.world().get::<Hdr>(camera).is_none());
        assert!(app.world().get::<DeferredPrepass>(camera).is_none());
        assert!(app.world().get::<DepthPrepass>(camera).is_none());
        assert!(app.world().get::<MotionVectorPrepass>(camera).is_none());
        assert_eq!(
            *app.world().get::<Tonemapping>(camera).unwrap(),
            Tonemapping::None
        );
        let grading = app.world().get::<ColorGrading>(camera).unwrap();
        assert_eq!(grading.global.exposure, 0.);
        assert_eq!(grading.global.post_saturation, 1.);
        assert!(app.world().get_entity(light).is_err());
        assert!(!app.world().resource::<State>().active);
        let camera = app.world().get::<Camera>(overlay).unwrap();
        assert!(matches!(camera.clear_color, ClearColorConfig::None));
        assert!(matches!(
            camera.output_mode,
            CameraOutputMode::Write {
                blend_state: None,
                ..
            }
        ));
    }
}
