//! Retain the last field image on the GPU while a menu owns the screen.
use super::{display::Targets, field_view::State};
use crate::native::MeshMaterial2d;
use bevy::{
    image::ImageSampler,
    prelude::*,
    render::{
        extract_component::ExtractComponent, render_asset::RenderAssets, render_resource::*,
        texture::GpuImage,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Asset, TypePath, Debug, Clone)]
pub(crate) struct Material {
    image: Handle<Image>,
    enabled: u32,
    cover: Vec4,
}
impl Material {
    pub(super) fn visible(image: Handle<Image>) -> Self {
        Self {
            image,
            enabled: 1,
            cover: Vec4::ZERO,
        }
    }
    pub(super) fn with_cover(mut self, cover: Vec4) -> Self {
        self.cover = cover;
        self
    }
    /// The image and cover color the native frame draws, or nothing while the
    /// backdrop is disabled: its zero alpha leaves the target unchanged.
    pub(crate) fn native(&self) -> Option<(AssetId<Image>, Vec4)> {
        (self.enabled != 0).then(|| (self.image.id(), self.cover))
    }
}

#[derive(Component)]
pub(super) struct Quad;

#[derive(Component, Clone, ExtractComponent)]
#[extract_app(bevy::render::RenderApp)]
pub(crate) struct Capture {
    target: Handle<Image>,
    generation: u64,
    resized_to: Option<Extent3d>,
    /// The generation last copied, so that a menu opening copies once.
    copied: Arc<AtomicU64>,
}

/// Retain the current scene before dialogue and field fades are composited.
#[derive(Component, Clone, ExtractComponent)]
#[extract_app(bevy::render::RenderApp)]
pub(super) struct SceneCapture(pub Capture);

impl Capture {
    pub(super) fn once(target: Handle<Image>) -> Self {
        Self {
            target,
            generation: 0,
            resized_to: None,
            copied: Arc::new(AtomicU64::new(u64::MAX)),
        }
    }

    pub(super) fn fit_source(
        &mut self,
        source: &Handle<Image>,
        images: &mut Assets<Image>,
    ) -> bool {
        let size = images
            .get(source)
            .expect("retained scene image")
            .texture_descriptor
            .size;
        let target = images.get(&self.target).expect("retained capture image");
        if target.texture_descriptor.size == size {
            return false;
        }
        images.get_mut(&self.target).unwrap().resize(size);
        // An in-flight copy at the old extent cannot complete this generation.
        self.generation += 1;
        self.resized_to = Some(size);
        true
    }

    pub(super) fn ready(&self) -> bool {
        self.copied.load(Ordering::Acquire) == self.generation
    }

    pub(crate) fn copy_native(
        &self,
        source: &Texture,
        images: &RenderAssets<GpuImage>,
        encoder: &mut CommandEncoder,
    ) {
        if self.ready() {
            return;
        }
        let Some(target) = images.get(&self.target) else {
            return;
        };
        if target.texture.size() != source.size()
            || self.resized_to.is_some_and(|size| size != source.size())
        {
            return;
        }
        encoder.copy_texture_to_texture(
            source.as_image_copy(),
            target.texture.as_image_copy(),
            source.size(),
        );
        self.copied.store(self.generation, Ordering::Release);
    }
}

struct Backdrop {
    entity: Entity,
    capture: Capture,
    material: Handle<Material>,
    held: bool,
    field: Option<(u32, u32)>,
}

pub(super) fn install(app: &mut App) {
    crate::native::init_material::<Material>(app);
    app.add_systems(
        PostUpdate,
        sync.before(bevy::transform::TransformSystems::Propagate)
            .run_if(super::battle::field_presenting),
    );
}

#[allow(clippy::too_many_arguments)] // Retained images, drawing resources and field ownership.
fn sync(
    mut commands: Commands,
    state: State,
    targets: Option<Res<Targets>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<Material>>,
    views: Query<Entity, With<super::FieldCamera>>,
    resident: Option<Res<super::loading::Resident>>,
    display: Res<super::display::Display>,
    mut backdrop: Local<Option<Backdrop>>,
) {
    let Some(targets) = targets else {
        return;
    };
    let backdrop = backdrop.get_or_insert_with(|| {
        let source = images.get(&targets.source).expect("retained field output");
        let size = source.texture_descriptor.size;
        let mut target_image =
            Image::new_target_texture(size.width, size.height, TextureFormat::Bgra8Unorm, None);
        target_image.sampler = ImageSampler::linear();
        let target = images.add(target_image);
        let material = materials.add(Material {
            image: target.clone(),
            enabled: 0,
            cover: Vec4::ZERO,
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
            capture: Capture::once(target),
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
    if !backdrop.held || !backdrop.capture.ready() {
        backdrop.capture.fit_source(&targets.source, &mut images);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_backdrop_follows_render_size_changes_before_the_next_capture() {
        let mut app = App::new();
        app.init_resource::<super::super::display::Display>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Material>>()
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
        app.insert_resource(Targets {
            source: source.clone(),
            output: None,
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
}
