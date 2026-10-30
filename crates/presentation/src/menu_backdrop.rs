//! Retain the last field image on the GPU while a menu owns the screen.
use super::{field_view::State, materials::TitleOutput};
use bevy::{
    core_pipeline::{
        Core3dSystems,
        schedule::{Core2d, Core3d},
        upscaling::upscaling,
    },
    image::ImageSampler,
    prelude::*,
    render::{
        RenderApp,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        render_resource::*,
        renderer::{RenderContext, ViewQuery},
        texture::GpuImage,
        view::ViewTarget,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct Material {
    #[texture(0)]
    #[sampler(1)]
    image: Handle<Image>,
    #[uniform(2)]
    enabled: u32,
}
impl Material {
    pub(super) fn visible(image: Handle<Image>) -> Self {
        Self { image, enabled: 1 }
    }
}
impl Material2d for Material {
    fn fragment_shader() -> ShaderRef {
        "embedded://resonance_presentation/menu_backdrop.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _: &bevy::mesh::MeshVertexBufferLayoutRef,
        _: bevy::sprite_render::Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.label = Some("resonance/menu-backdrop".into());
        Ok(())
    }
}

#[derive(Component)]
pub(super) struct Quad;

#[derive(Component, Clone, ExtractComponent)]
pub(super) struct Capture {
    source: Handle<Image>,
    target: Handle<Image>,
    generation: u64,
    completed: Arc<AtomicBool>,
}

/// Retain the current scene before dialogue and field fades are composited.
#[derive(Component, Clone, ExtractComponent)]
pub(super) struct SceneCapture(pub Capture);

impl SceneCapture {
    pub(super) fn copy(
        &self,
        source: &Texture,
        images: &RenderAssets<GpuImage>,
        context: &mut RenderContext,
    ) {
        if !self.0.ready() {
            copy(&self.0, source, images, &mut None, context);
        }
    }
}

impl Capture {
    pub(super) fn once(source: Handle<Image>, target: Handle<Image>) -> Self {
        Self {
            source,
            target,
            generation: 0,
            completed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn ready(&self) -> bool {
        self.completed.load(Ordering::Acquire)
    }
}
struct Backdrop {
    entity: Entity,
    capture: Capture,
    material: Handle<Material>,
    held: bool,
    field: Option<(u32, u32)>,
}

/// One prepared battle menu capture, using the same GPU copy and material as
/// field menus. Its quad participates in the encounter's ordinary warmup.
pub(super) struct BattleBackdrop {
    entity: Entity,
    capture: Capture,
    material: Handle<Material>,
    pending_draw: bool,
}
impl BattleBackdrop {
    pub fn new(world: &mut World) -> anyhow::Result<Self> {
        use anyhow::Context;
        let source = world
            .resource::<Assets<TitleOutput>>()
            .iter()
            .next()
            .context("battle menu lost its composed output")?
            .1
            .source
            .clone();
        Self::from_source(world, source)
    }

    pub fn from_source(world: &mut World, source: Handle<Image>) -> anyhow::Result<Self> {
        use anyhow::Context;
        let size = world
            .resource::<Assets<Image>>()
            .get(&source)
            .context("battle composed output image is absent")?
            .texture_descriptor
            .size;
        let mut image =
            Image::new_target_texture(size.width, size.height, TextureFormat::Bgra8Unorm, None);
        image.sampler = ImageSampler::linear();
        let target = world.resource_mut::<Assets<Image>>().add(image);
        let material = world.resource_mut::<Assets<Material>>().add(Material {
            image: target.clone(),
            enabled: 0,
        });
        let size = world.resource::<super::display::Display>().0.ui_size();
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Rectangle::from_size(size));
        let entity = world
            .spawn((
                Mesh2d(mesh),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0., 0., 90.),
                bevy::camera::visibility::RenderLayers::layer(super::battle_view::WARM_LAYER),
                Visibility::Hidden,
            ))
            .id();
        Ok(Self {
            entity,
            capture: Capture::once(source, target),
            material,
            pending_draw: false,
        })
    }

    pub fn entity(&self) -> Entity {
        self.entity
    }

    /// Capture after the final command strip and portrait cursor draw. Retain that frame
    /// until PostUpdate even when several fixed updates precede rendering.
    pub fn request_capture(&mut self) {
        if !self.pending_draw {
            self.capture.generation = self.capture.generation.wrapping_add(1);
            self.pending_draw = true;
        }
    }

    pub fn awaiting_draw(&self) -> bool {
        self.pending_draw
    }

    pub fn show(
        &mut self,
        held: bool,
        camera: Option<Entity>,
        commands: &mut Commands,
        materials: &mut Assets<Material>,
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        let capture = self.pending_draw;
        // PostUpdate is the publication boundary. Commands are applied before
        // extraction, and the next extracted frame follows this ordered copy.
        // A recoverable missing dependency must not leave simulation held.
        self.pending_draw = false;
        let result = (|| -> anyhow::Result<()> {
            let camera = camera.context("battle menu HUD camera is absent")?;
            materials
                .get_mut(&self.material)
                .context("battle menu capture material was removed")?
                .enabled = u32::from(held);
            commands.entity(self.entity).insert(if held {
                Visibility::Visible
            } else {
                Visibility::Hidden
            });
            if capture {
                commands.entity(camera).insert(self.capture.clone());
            } else {
                commands.entity(camera).remove::<Capture>();
            }
            Ok(())
        })();
        if result.is_err() {
            commands.entity(self.entity).insert(Visibility::Hidden);
            if let Some(camera) = camera {
                commands.entity(camera).remove::<Capture>();
            }
        }
        result
    }
}

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "menu_backdrop.wgsl");
    app.add_plugins((
        Material2dPlugin::<Material>::default(),
        ExtractComponentPlugin::<Capture>::default(),
        ExtractComponentPlugin::<SceneCapture>::default(),
    ))
    .add_systems(
        PostUpdate,
        sync.before(bevy::transform::TransformSystems::Propagate)
            .run_if(super::battle::field_presenting),
    );
    app.sub_app_mut(RenderApp)
        .add_systems(Core3d, capture.before(Core3dSystems::Prepass))
        .add_systems(Core3d, capture_scene.after(upscaling))
        // Battle requests attach to the warmed HUD camera. Its upscaling pass
        // has just composed this visit's HUD into the retained source image.
        .add_systems(Core2d, capture.after(upscaling));
}

#[allow(clippy::too_many_arguments)] // Retained images, drawing resources and field ownership.
fn sync(
    mut commands: Commands,
    state: State,
    outputs: Res<Assets<TitleOutput>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<Material>>,
    views: Query<Entity, With<super::FieldCamera>>,
    resident: Option<Res<super::loading::Resident>>,
    display: Res<super::display::Display>,
    mut backdrop: Local<Option<Backdrop>>,
) {
    let Some((_, output)) = outputs.iter().next() else {
        return;
    };
    let backdrop = backdrop.get_or_insert_with(|| {
        let source = images.get(&output.source).expect("retained field output");
        let size = source.texture_descriptor.size;
        let mut target_image =
            Image::new_target_texture(size.width, size.height, TextureFormat::Bgra8Unorm, None);
        target_image.sampler = ImageSampler::linear();
        let target = images.add(target_image);
        let material = materials.add(Material {
            image: target.clone(),
            enabled: 0,
        });
        let entity = commands
            .spawn((
                Quad,
                Mesh2d(meshes.add(Rectangle::new(1., 1.))),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0., 0., 90.).with_scale(display.0.ui_size().extend(1.)),
            ))
            .id();
        Backdrop {
            entity,
            capture: Capture::once(output.source.clone(), target),
            material,
            held: false,
            field: None,
        }
    });
    if display.is_changed() {
        commands
            .entity(backdrop.entity)
            .insert(Transform::from_xyz(0., 0., 90.).with_scale(display.0.ui_size().extend(1.)));
    }
    // Keep the frozen background while a menu is open. Before the next capture,
    // its destination must match the resized scene texture exactly.
    if !backdrop.held {
        let size = images
            .get(&backdrop.capture.source)
            .expect("retained field output")
            .texture_descriptor
            .size;
        let mut target = images
            .get_mut(&backdrop.capture.target)
            .expect("retained menu image");
        if target.texture_descriptor.size != size {
            target.resize(size);
            materials.get_mut(&backdrop.material).unwrap().image = backdrop.capture.target.clone();
        }
    }
    let field = state
        .live
        .as_ref()
        .filter(|s| s.is_field())
        .map(|s| s.field());
    let identity = field.map(|f| (f.map_id, f.events.tick()));
    // Closing has one fully transparent pose before field simulation resumes.
    let held = field.is_some_and(|f| f.menu_is_open())
        || backdrop.held && identity.is_some() && identity == backdrop.field;
    // Submit the transparent quad during preparation, then only draw it for menus.
    let warming = field.is_some() && resident.is_some_and(|r| !r.active.load(Ordering::Acquire));
    commands.entity(backdrop.entity).insert(if held || warming {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if held != backdrop.held {
        materials.get_mut(&backdrop.material).unwrap().enabled = u32::from(held);
        if held {
            backdrop.capture.generation += 1;
        }
    }
    backdrop.held = held;
    backdrop.field = identity;
    for view in &views {
        if held {
            commands.entity(view).insert(backdrop.capture.clone());
        } else {
            commands.entity(view).remove::<Capture>();
        }
    }
}

fn capture(
    view: ViewQuery<&Capture>,
    images: Res<RenderAssets<GpuImage>>,
    mut copied: Local<Option<(AssetId<Image>, u64)>>,
    mut context: RenderContext,
) {
    let request = view.into_inner();
    if let Some(source) = images.get(&request.source) {
        copy(request, &source.texture, &images, &mut copied, &mut context);
    }
}

fn capture_scene(
    view: ViewQuery<(&SceneCapture, &ViewTarget)>,
    images: Res<RenderAssets<GpuImage>>,
    mut context: RenderContext,
) {
    let (request, target) = view.into_inner();
    // The field camera skips output: the UI camera publishes their shared
    // texture afterward. Copy the scene before that composition adds fades.
    request.copy(target.main_texture(), &images, &mut context);
}

fn copy(
    request: &Capture,
    source: &Texture,
    images: &RenderAssets<GpuImage>,
    copied: &mut Option<(AssetId<Image>, u64)>,
    context: &mut RenderContext,
) {
    let identity = (request.target.id(), request.generation);
    if *copied == Some(identity) {
        return;
    }
    let Some(target) = images.get(&request.target) else {
        return;
    };
    context.command_encoder().copy_texture_to_texture(
        source.as_image_copy(),
        target.texture.as_image_copy(),
        source.size(),
    );
    *copied = Some(identity);
    request.completed.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared() -> (World, BattleBackdrop, Entity, Assets<Material>) {
        let mut world = World::new();
        world.init_resource::<crate::display::Display>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Material>>();
        let source = world.resource_mut::<Assets<Image>>().add(Image::default());
        let backdrop = BattleBackdrop::from_source(&mut world, source).unwrap();
        let camera = world.spawn_empty().id();
        let materials = world.remove_resource::<Assets<Material>>().unwrap();
        (world, backdrop, camera, materials)
    }

    fn publish(
        world: &mut World,
        backdrop: &mut BattleBackdrop,
        held: bool,
        camera: Option<Entity>,
        materials: &mut Assets<Material>,
    ) -> anyhow::Result<()> {
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let result = backdrop.show(
            held,
            camera,
            &mut Commands::new(&mut queue, world),
            materials,
        );
        queue.apply(world);
        result
    }

    #[test]
    fn field_backdrop_follows_render_size_changes_before_the_next_capture() {
        let mut app = App::new();
        app.init_resource::<super::super::display::Display>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Material>>()
            .init_resource::<Assets<TitleOutput>>()
            .add_systems(Update, sync);
        let source =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    640,
                    448,
                    TextureFormat::Bgra8Unorm,
                    None,
                ));
        app.world_mut()
            .resource_mut::<Assets<TitleOutput>>()
            .add(TitleOutput {
                source: source.clone(),
                brightness: Vec4::ONE,
            });
        app.update();
        for resolution in ["1920x1080", "640x480"] {
            let size: crate::Resolution = resolution.parse().unwrap();
            app.world_mut()
                .resource_mut::<super::super::display::Display>()
                .0 = size;
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .get_mut(&source)
                .unwrap()
                .resize(Extent3d {
                    width: size.width,
                    height: size.scene_height(),
                    depth_or_array_layers: 1,
                });
            app.update();
            let world = app.world_mut();
            let mut quads =
                world.query_filtered::<(&MeshMaterial2d<Material>, &Transform), With<Quad>>();
            let (material, transform) = quads.single(world).unwrap();
            let material = world
                .resource::<Assets<Material>>()
                .get(&material.0)
                .unwrap();
            let target = world
                .resource::<Assets<Image>>()
                .get(&material.image)
                .unwrap();
            assert_eq!(
                target.size(),
                UVec2::new(size.width, size.scene_height()),
                "retained menu texture must fit the next GPU copy"
            );
            assert_eq!(transform.scale.truncate(), size.ui_size());
        }
    }

    #[test]
    fn battle_capture_holds_the_frame_until_hud_publication() {
        let (mut world, mut backdrop, camera, mut materials) = prepared();
        backdrop.request_capture();
        // No intervening fixed visit can replace the outgoing strip. Repeating
        // the same pending request also cannot manufacture another copy.
        for _ in 0..4 {
            assert!(backdrop.awaiting_draw());
            backdrop.request_capture();
        }
        assert_eq!(backdrop.capture.generation, 1);
        publish(
            &mut world,
            &mut backdrop,
            false,
            Some(camera),
            &mut materials,
        )
        .unwrap();
        assert!(!backdrop.awaiting_draw());
        assert_eq!(world.get::<Capture>(camera).unwrap().generation, 1);
        assert_eq!(
            world.get::<Visibility>(backdrop.entity()),
            Some(&Visibility::Hidden)
        );

        // The next extracted frame can draw the inventory. It consumes the
        // completed preceding HUD copy and must not recapture inventory layers.
        publish(
            &mut world,
            &mut backdrop,
            true,
            Some(camera),
            &mut materials,
        )
        .unwrap();
        assert!(world.get::<Capture>(camera).is_none());
        assert_eq!(
            world.get::<Visibility>(backdrop.entity()),
            Some(&Visibility::Visible)
        );
        assert_eq!(materials.get(&backdrop.material).unwrap().enabled, 1);

        backdrop.request_capture();
        publish(
            &mut world,
            &mut backdrop,
            false,
            Some(camera),
            &mut materials,
        )
        .unwrap();
        assert_eq!(world.get::<Capture>(camera).unwrap().generation, 2);
        assert_eq!(materials.get(&backdrop.material).unwrap().enabled, 0);
    }

    #[test]
    fn a_missing_battle_backdrop_releases_the_draw_hold_and_obeys_diagnostics() {
        for paranoid in [false, true] {
            let (mut world, mut backdrop, camera, mut materials) = prepared();
            let diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
            materials.remove(backdrop.material.id());
            backdrop.request_capture();
            let result = diagnostics.attempt(
                "battle menu backdrop",
                publish(
                    &mut world,
                    &mut backdrop,
                    false,
                    Some(camera),
                    &mut materials,
                ),
            );
            assert_eq!(result.is_err(), paranoid);
            assert!(diagnostics.has_errors());
            assert!(!backdrop.awaiting_draw());
            assert!(world.get::<Capture>(camera).is_none());
            assert_eq!(
                world.get::<Visibility>(backdrop.entity()),
                Some(&Visibility::Hidden)
            );
        }
    }
}
