//! Pause feedback uses the prepared game font and advances independently of game time.
use super::bitmap_text::{Spacing, draw as draw_text, width as text_width};
use super::*;
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    render::render_resource::Extent3d,
};
use resonance_content::input::{Keymap, StandardAction};

// Separate from field warm-up (30) and performance diagnostics (31).
const LAYER: usize = 32;

#[derive(Component)]
pub(crate) struct PauseCamera;

#[derive(Resource)]
pub(crate) struct Target(Handle<Image>);

#[derive(Resource)]
pub(crate) struct Panel {
    font: BitmapFont,
    input_atlas: std::sync::Arc<crate::input_icons::Atlas>,
    layers: [Layer; 3],
}

pub(crate) fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    display: Res<crate::display::Display>,
    adapter: Res<bevy::render::renderer::RenderAdapterInfo>,
) {
    let mut image = Image::new_target_texture(
        display.0.width,
        display.0.height,
        crate::renderer::scene_color_format(Some(&adapter)),
        None,
    );
    image.sampler = ImageSampler::linear();
    let target = images.add(image);
    commands.insert_resource(Target(target.clone()));
    commands.spawn((
        Camera2d,
        Camera {
            order: -2,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            output_mode: crate::renderer::COMPOSITED_OUTPUT,
            is_active: false,
            ..default()
        },
        Tonemapping::Linear,
        Msaa::Off,
        PauseCamera,
        crate::display::UiCamera,
        RenderLayers::layer(LAYER),
        RenderTarget::Image(target.into()),
        Projection::Orthographic(OrthographicProjection::default_2d()),
    ));
}

/// Borrow the font and materials already admitted by shared UI preparation.
pub(crate) fn prepare(world: &mut World, field: &super::Artwork) {
    let resident = world.resource::<Resident>().clone();
    let white = resident.images(&mut world.resource_mut::<Assets<Image>>())[0].clone();
    let background = world.resource_mut::<Assets<Surface>>().add(Surface {
        source: white.clone(),
        sampling: white.clone(),
        frame_mask: white.clone(),
        color_mask: white,
        coverage: Coverage::default(),
        interpolation: UiInterpolation::Center,
        opaque: false,
        additive: false,
        texture_effect: Default::default(),
    });
    let layers = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
        let mut index = 0;
        [
            background,
            field.menu.input_material(),
            field.surfaces[9].clone(),
        ]
        .map(|material| {
            let layer = Layer::new(material, index as f32, &mut world.commands(), &mut meshes);
            index += 1;
            world
                .commands()
                .entity(layer.entity)
                .insert(RenderLayers::layer(LAYER));
            layer
        })
    });
    world.insert_resource(Panel {
        font: field.font.clone(),
        input_atlas: field.menu.prepared_inputs().atlas,
        layers,
    });
}

#[allow(clippy::too_many_arguments)] // Shared UI, active bindings and a separate real-time camera.
pub(crate) fn update(
    controls: Res<crate::testing::Controls>,
    time: Res<Time<Real>>,
    session: Option<Res<crate::new_game::Session>>,
    preferences: Option<Res<crate::config::Preferences>>,
    device: Res<crate::input_icons::DeviceTracker>,
    mut panel: Option<ResMut<Panel>>,
    mut camera: Single<&mut Camera, With<PauseCamera>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut elapsed: Local<f32>,
) {
    camera.is_active = controls.paused && panel.is_some();
    if !controls.paused {
        *elapsed = 0.;
        return;
    }
    let Some(panel) = panel.as_deref_mut() else {
        return;
    };
    *elapsed += time.delta_secs();
    let settings = crate::input::settings(session.as_deref(), preferences.as_deref());
    match draw(
        &panel.font,
        &panel.input_atlas,
        &settings.keymap,
        device.device(),
        *elapsed,
    ) {
        Ok(batches) => {
            let sizes = [
                [1, 1],
                panel.input_atlas.size,
                [panel.font.width, panel.font.height],
            ];
            for ((layer, batch), size) in panel.layers.iter_mut().zip(batches).zip(sizes) {
                if let Err(error) = layer.update_mesh(batch, size, &mut meshes) {
                    error!("Pause overlay: {error:#}");
                    return;
                }
                layer.show(true, &mut commands);
            }
        }
        Err(error) => error!("Pause overlay: {error:#}"),
    }
}

/// Resize during display preparation, before Bevy caches the camera's target size.
pub(crate) fn resize(
    target: Res<Target>,
    display: Res<crate::display::Display>,
    mut images: ResMut<Assets<Image>>,
    mut outputs: ResMut<Assets<crate::materials::TitleOutput>>,
) {
    let size = UVec2::new(display.0.width, display.0.height);
    if images.get(&target.0).unwrap().size() == size {
        return;
    }
    images.get_mut(&target.0).unwrap().resize(Extent3d {
        width: size.x,
        height: size.y,
        depth_or_array_layers: 1,
    });
    // Resizing replaces the GPU view. Refresh materials already sampling it.
    let refresh: Vec<_> = outputs
        .iter()
        .filter_map(|(id, output)| (output.overlay.as_ref() == Some(&target.0)).then_some(id))
        .collect();
    for id in refresh {
        outputs.get_mut(id).unwrap().overlay = Some(target.0.clone());
    }
}

/// Composite the UI after scene fades, in the same encoded color space.
pub(crate) fn composite(
    target: Res<Target>,
    camera: Single<&Camera, With<PauseCamera>>,
    output: Single<
        &MeshMaterial2d<crate::materials::TitleOutput>,
        With<crate::display::OutputQuad>,
    >,
    mut outputs: ResMut<Assets<crate::materials::TitleOutput>>,
) {
    let overlay = camera.is_active.then(|| target.0.clone());
    if outputs.get(&output.0).unwrap().overlay != overlay {
        outputs.get_mut(&output.0).unwrap().overlay = overlay;
    }
}

fn draw(
    font: &BitmapFont,
    atlas: &crate::input_icons::Atlas,
    map: &Keymap,
    device: crate::input_icons::PromptDevice,
    elapsed: f32,
) -> Result<[Batch; 3]> {
    let mut background = Batch::default();
    background.quad([152., 176., 488., 300.], [0.5; 4], [0.03, 0.05, 0.09, 0.92]);
    let mut caption = Batch::default();
    let mut icons = Batch::default();
    centered(&mut caption, font, "Pause", 194., 32.)?;
    if let Some(input) = crate::input_icons::action_input(map, StandardAction::Pause, device) {
        let prefix = text_width(font, "Press", 18., Spacing::Font)?;
        let suffix = text_width(font, "to continue", 18., Spacing::Font)?;
        let x = 320. - (prefix + 8. + 28. + 8. + suffix) * 0.5;
        text(&mut caption, font, "Press", [x, 254.], [18.; 2], [1.; 4])?;
        let x = x + prefix + 8.;
        // Use the same two-state icon and cadence as the menu's input prompts.
        let pressed = (elapsed * 60.) as u32 % 40 < 20;
        let [u, v, w, h] = crate::input_icons::rect_state(atlas, input, pressed).map(|v| v as f32);
        icons.quad([x, 249., x + 28., 277.], [u, v, u + w, v + h], [1.; 4]);
        if !crate::input_icons::has_specific_icon(atlas, input) {
            let label = crate::input_icons::short_label(input);
            let width = (28. / label.len().max(1) as f32).min(12.);
            let rgb = if pressed { 0. } else { 1. };
            text(
                &mut caption,
                font,
                &label,
                [x + 3., 249. + 28. * 0.3],
                [width, 28. * 0.4],
                [rgb, rgb, rgb, 1.],
            )?;
        }
        text(
            &mut caption,
            font,
            "to continue",
            [x + 28. + 8., 254.],
            [18.; 2],
            [1.; 4],
        )?;
    } else {
        centered(&mut caption, font, "Pause control is unbound", 254., 18.)?;
    }
    Ok([background, icons, caption])
}

fn centered(batch: &mut Batch, font: &BitmapFont, label: &str, y: f32, size: f32) -> Result<()> {
    let width = text_width(font, label, size, Spacing::Font)?;
    text(
        batch,
        font,
        label,
        [320. - width * 0.5, y],
        [size; 2],
        [1.; 4],
    )
}

fn text(
    batch: &mut Batch,
    font: &BitmapFont,
    label: &str,
    position: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
) -> Result<()> {
    draw_text(font, label, position, size, Spacing::Font, |rect, uv| {
        batch.quad(rect, uv, color)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_icons::PromptDevice;
    use resonance_content::{font::Glyph, input::PhysicalInput};

    #[test]
    fn pause_camera_and_target_resize_together_before_the_first_paused_frame() {
        use bevy::camera::ScalingMode;
        use bevy::render::renderer::RenderAdapterInfo;
        let mut app = App::new();
        app.init_resource::<crate::display::Display>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<crate::materials::TitleOutput>>()
            .init_resource::<bevy::render::texture::ManualTextureViews>()
            .add_message::<bevy::window::WindowResized>()
            .add_message::<bevy::window::WindowCreated>()
            .add_message::<bevy::window::WindowScaleFactorChanged>()
            .add_message::<AssetEvent<Image>>()
            .insert_resource(crate::display::Targets {
                source: Handle::default(),
                output: None,
            })
            .insert_resource(RenderAdapterInfo::new(wgpu::AdapterInfo {
                name: "Pause framing test".into(),
                vendor: 0,
                device: 0,
                device_type: wgpu::DeviceType::Cpu,
                device_pci_bus_id: String::new(),
                driver: String::new(),
                driver_info: String::new(),
                backend: wgpu::Backend::Gl,
                subgroup_min_size: 0,
                subgroup_max_size: 0,
                transient_saves_memory: None,
                limit_bucket: None,
            }))
            .add_systems(Startup, (setup, crate::display::initialize).chain())
            .add_systems(PostUpdate, bevy::render::camera::camera_system)
            .add_systems(Last, composite);
        crate::display::install(&mut app);
        let output = app
            .world_mut()
            .resource_mut::<Assets<crate::materials::TitleOutput>>()
            .add(crate::materials::TitleOutput {
                source: Handle::default(),
                overlay: None,
                brightness: Vec4::X,
            });
        app.world_mut().spawn((
            MeshMaterial2d(output.clone()),
            crate::display::OutputQuad,
            Transform::default(),
        ));
        app.update();
        let camera = app
            .world_mut()
            .query_filtered::<Entity, With<PauseCamera>>()
            .single(app.world())
            .unwrap();
        let target = app.world().resource::<Target>().0.clone();
        let assert_size = |app: &App| {
            let display = app.world().resource::<crate::display::Display>().0;
            let size = UVec2::new(display.width, display.height);
            assert_eq!(
                app.world()
                    .resource::<Assets<Image>>()
                    .get(&target)
                    .unwrap()
                    .size(),
                size
            );
            assert_eq!(
                app.world()
                    .get::<Camera>(camera)
                    .unwrap()
                    .physical_target_size(),
                Some(size)
            );
        };
        for [width, height] in [[1920, 1080], [480, 640]] {
            app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = false;
            let resolution = crate::Resolution { width, height };
            app.world_mut().resource_mut::<crate::display::Display>().0 = resolution;
            app.update();
            assert_size(&app);
            app.update(); // The display change is no longer new when pause begins.
            assert!(!app.world().get::<Camera>(camera).unwrap().is_active);
            app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = true;
            app.update();
            assert_size(&app);
            let Projection::Orthographic(projection) =
                app.world().get::<Projection>(camera).unwrap()
            else {
                panic!("pause camera lost its UI projection")
            };
            let size = resolution.ui_size();
            assert!(
                matches!(projection.scaling_mode, ScalingMode::Fixed { width, height }
                if width == size.x && height == size.y)
            );
            assert_eq!(
                app.world()
                    .resource::<Assets<crate::materials::TitleOutput>>()
                    .get(&output)
                    .unwrap()
                    .overlay
                    .as_ref(),
                Some(&target)
            );

            // A resize during pause must also reach the camera in this frame.
            app.world_mut().resource_mut::<crate::display::Display>().0 = Default::default();
            app.update();
            assert_size(&app);
        }
    }

    #[test]
    fn remapped_keyboard_and_controller_icons_flip_while_text_stays_steady() -> Result<()> {
        let font = BitmapFont {
            texture: String::new(),
            width: 24,
            height: 24,
            line_height: 24,
            glyphs: "PausePressto continue"
                .chars()
                .map(|c| {
                    (
                        c,
                        Glyph {
                            rect: [0, 0, 24, 24],
                            advance: 12,
                        },
                    )
                })
                .collect(),
        };
        let mut map = Keymap::default();
        map.assign(StandardAction::Pause, PhysicalInput::Key("F2".into()));
        map.assign(StandardAction::Pause, PhysicalInput::Button("North".into()));
        let atlas = crate::input_icons::default_atlas();
        for device in [PromptDevice::Keyboard, PromptDevice::Controller] {
            let pressed = draw(&font, atlas, &map, device, 0.2)?;
            let released = draw(&font, atlas, &map, device, 0.5)?;
            assert!(pressed[0] == released[0], "background must stay steady");
            assert!(pressed[2] == released[2], "text must stay steady");
            assert_eq!(pressed[1].positions, released[1].positions);
            assert_ne!(
                pressed[1].uv, released[1].uv,
                "configured icon must flip states"
            );
            let expected = crate::input_icons::action_rect_state(
                atlas,
                &map,
                StandardAction::Pause,
                true,
                device,
            );
            assert_eq!(pressed[1].uv[0], [expected[0] as f32, expected[1] as f32]);
        }
        Ok(())
    }
}
