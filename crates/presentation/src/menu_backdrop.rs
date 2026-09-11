//! Retain the last field image on the GPU while a menu owns the screen.
use super::{field_view::State, materials::TitleOutput};
use bevy::{
    core_pipeline::{Core3dSystems, schedule::Core3d},
    image::ImageSampler,
    prelude::*,
    render::{
        RenderApp,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        render_resource::*,
        renderer::{RenderContext, ViewQuery},
        texture::GpuImage,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin},
};
use std::sync::atomic::Ordering;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct Material {
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
struct Capture {
    source: Handle<Image>,
    target: Handle<Image>,
    generation: u64,
}
struct Backdrop {
    entity: Entity,
    capture: Capture,
    material: Handle<Material>,
    held: bool,
    field: Option<(u32, u32)>,
}

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "menu_backdrop.wgsl");
    app.add_plugins((
        Material2dPlugin::<Material>::default(),
        ExtractComponentPlugin::<Capture>::default(),
    ))
    .add_systems(
        PostUpdate,
        sync.before(bevy::transform::TransformSystems::Propagate),
    );
    app.sub_app_mut(RenderApp)
        .add_systems(Core3d, capture.before(Core3dSystems::Prepass));
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
                Mesh2d(meshes.add(Rectangle::new(640., 480.))),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0., 0., 90.),
            ))
            .id();
        Backdrop {
            entity,
            capture: Capture {
                source: output.source.clone(),
                target,
                generation: 0,
            },
            material,
            held: false,
            field: None,
        }
    });
    let field = state
        .checkpoint
        .as_ref()
        .map(|s| &s.0)
        .or_else(|| state.live.as_ref().map(|s| &s.field));
    let identity = field.map(|f| (f.map_id, f.events.tick()));
    // Closing has one fully transparent pose before field simulation resumes.
    let held = field.is_some_and(|f| f.menu.is_some())
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
    mut copied: Local<u64>,
    mut context: RenderContext,
) {
    let request = view.into_inner();
    if *copied == request.generation {
        return;
    }
    let (Some(source), Some(target)) = (images.get(&request.source), images.get(&request.target))
    else {
        return;
    };
    context.command_encoder().copy_texture_to_texture(
        source.texture.as_image_copy(),
        target.texture.as_image_copy(),
        source.texture_descriptor.size,
    );
    *copied = request.generation;
}
