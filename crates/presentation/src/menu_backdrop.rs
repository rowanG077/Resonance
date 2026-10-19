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

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "menu_backdrop.wgsl");
    app.add_plugins((
        Material2dPlugin::<Material>::default(),
        ExtractComponentPlugin::<Capture>::default(),
        ExtractComponentPlugin::<SceneCapture>::default(),
    ))
    .add_systems(
        PostUpdate,
        sync.before(bevy::transform::TransformSystems::Propagate),
    );
    app.sub_app_mut(RenderApp)
        .add_systems(Core3d, capture.before(Core3dSystems::Prepass))
        .add_systems(Core3d, capture_scene.after(upscaling))
        // Battle requests attach to the warmed HUD camera. Its upscaling pass
        // has just composed this visit's HUD into the retained source image.
        .add_systems(Core2d, capture.after(upscaling));
}

#[allow(clippy::too_many_arguments)] // Allocate once, then only update the held-frame request.
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
                Mesh2d(meshes.add(Rectangle::from_size(display.0.ui_size()))),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0., 0., 90.),
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
