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
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin},
};
use std::sync::atomic::Ordering;

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
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Rectangle::new(640., 480.));
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
            capture: Capture {
                source,
                target,
                generation: 0,
            },
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
    ))
    .add_systems(
        PostUpdate,
        sync.before(bevy::transform::TransformSystems::Propagate)
            .run_if(super::battle::field_presenting),
    );
    app.sub_app_mut(RenderApp)
        .add_systems(Core3d, capture.before(Core3dSystems::Prepass))
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
    let field = state.live.as_ref().map(|s| &s.field);
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
    let identity = (request.target.id(), request.generation);
    if *copied == Some(identity) {
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
    *copied = Some(identity);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared() -> (World, BattleBackdrop, Entity, Assets<Material>) {
        let mut world = World::new();
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
