//! Physical render sizes and a centered, authored UI coordinate system.
use anyhow::{Result, ensure};
use bevy::{
    camera::{RenderTarget, ScalingMode},
    prelude::*,
    window::{
        EnabledButtons, Monitor, MonitorSelection, OnMonitor, PresentMode, PrimaryWindow,
        RawHandleWrapper, VideoModeSelection, WindowMode, WindowResizeConstraints,
        WindowResolution,
    },
};

use raw_window_handle::RawDisplayHandle;
use resonance_content::{HEIGHT, SCENE_HEIGHT, WIDTH};

#[cfg(test)]
#[path = "display_gpu_tests.rs"]
mod gpu_tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}
impl Default for Resolution {
    fn default() -> Self {
        Self {
            width: WIDTH,
            height: HEIGHT,
        }
    }
}
impl std::str::FromStr for Resolution {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (width, height) = value.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
        let size = Self {
            width: width.parse().map_err(|_| "invalid resolution width")?,
            height: height.parse().map_err(|_| "invalid resolution height")?,
        };
        if size.width == 0 || size.height == 0 {
            return Err("resolution must be nonzero".into());
        }
        Ok(size)
    }
}
impl Resolution {
    pub fn validate(self, limit: u32) -> Result<()> {
        ensure!(
            self.width > 0 && self.height > 0 && self.width <= limit && self.height <= limit,
            "resolution {}x{} exceeds GPU target limits (1..={limit})",
            self.width,
            self.height
        );
        Ok(())
    }
    pub fn aspect(self) -> f32 {
        self.width as f32 / self.height as f32
    }
    pub fn scene_height(self) -> u32 {
        ((u64::from(self.height) * u64::from(SCENE_HEIGHT) + u64::from(HEIGHT / 2))
            / u64::from(HEIGHT))
        .max(1) as u32
    }
    pub fn ui_size(self) -> Vec2 {
        let aspect = self.aspect();
        let (width, height) = (WIDTH as f32, HEIGHT as f32);
        Vec2::new((height * aspect).max(width), (width / aspect).max(height))
    }
    /// Visible bounds around the centered, authored UI canvas.
    pub(super) fn ui_rect(self) -> [f32; 4] {
        let center = Vec2::new(WIDTH as f32, HEIGHT as f32) * 0.5;
        let half = self.ui_size() * 0.5;
        [
            center.x - half.x,
            center.y - half.y,
            center.x + half.x,
            center.y + half.y,
        ]
    }
}

#[derive(Resource, Clone, Copy, Debug, Default)]
/// Render resolution. Compositor-imposed window geometry does not change it.
pub(super) struct Display(pub Resolution);

/// Distinguishes offscreen captures from interactive output.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OutputStage {
    Framebuffer,
    Scanout,
}

pub(super) fn window(size: Resolution) -> Window {
    Window {
        title: "Resonance".into(),
        resolution: WindowResolution::new(size.width, size.height).with_scale_factor_override(1.),
        resizable: false,
        resize_constraints: WindowResizeConstraints {
            min_width: size.width as f32,
            max_width: size.width as f32,
            min_height: size.height as f32,
            max_height: size.height as f32,
        },
        enabled_buttons: EnabledButtons {
            maximize: false,
            ..default()
        },
        present_mode: PresentMode::AutoNoVsync,
        ..default()
    }
}

/// Only interactive player sessions follow config.toml. Replays and captures keep
/// their explicitly requested render size and never change the host display.
#[derive(Resource, Default)]
pub(super) struct InteractiveDisplay {
    applied: Option<(resonance_content::menu_data::DisplayMode, [u32; 2])>,
    monitor: Option<Entity>,
    preview: Option<(resonance_content::menu_data::DisplayMode, [u32; 2])>,
    confirmation: Option<(MenuOwner, u64, std::time::Instant)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuOwner {
    Title,
    Session,
}

fn monitor_resolutions(monitor: Option<&Monitor>, limit: u32) -> (Vec<[u32; 2]>, Option<[u32; 2]>) {
    let native = monitor.map(|monitor| monitor.physical_size().to_array());
    let mut choices = resonance_content::menu_data::DISPLAY_RESOLUTIONS.to_vec();
    choices.extend(native);
    if let Some(monitor) = monitor {
        choices.extend(
            monitor
                .video_modes
                .iter()
                .map(|mode| mode.physical_size.to_array()),
        );
    }
    // Match both the renderer's allocation limit and the saved-settings limit.
    choices.retain(|size| {
        Resolution {
            width: size[0],
            height: size[1],
        }
        .validate(limit.min(8192))
        .is_ok()
    });
    (choices, native)
}

/// Preview only display fields. The saved preferences remain untouched until
/// Customize exits, and an abandoned menu always falls back to those settings.
/// Instant keeps the timeout independent of gameplay speed, pause and fixed ticks.
pub(super) fn preview_preferences(
    mut display: ResMut<InteractiveDisplay>,
    mut session: Option<ResMut<crate::new_game::Session>>,
    mut title: Option<ResMut<crate::saves::title::TitleMenu>>,
    device: Option<Res<bevy::render::renderer::RenderDevice>>,
    windows: Query<Option<&OnMonitor>, With<PrimaryWindow>>,
    monitors: Query<&Monitor>,
) {
    use resonance_game::menu::{Menu, Page, customize::Focus};
    let active = |menu: &&mut Menu| menu.page == Page::Customize && !menu.closed;
    let menu = session
        .as_mut()
        .and_then(|session| session.menu_mut())
        .filter(active)
        .map(|menu| (MenuOwner::Session, menu))
        .or_else(|| {
            title
                .as_mut()
                .map(|title| &mut title.0)
                .filter(active)
                .map(|menu| (MenuOwner::Title, menu))
        });
    let Some((owner, menu)) = menu else {
        display.preview = None;
        display.confirmation = None;
        return;
    };
    let monitor = windows
        .single()
        .ok()
        .flatten()
        .and_then(|on_monitor| monitors.get(on_monitor.0).ok());
    let limit = device
        .as_ref()
        .map_or(8192, |device| device.limits().max_texture_dimension_2d);
    let (choices, native) = monitor_resolutions(monitor, limit);
    menu.customize.set_resolution_choices(choices, native);
    if let Focus::DisplayConfirmation {
        revision,
        remaining_secs,
        ..
    } = menu.customize.focus
    {
        let [width, height] = menu.customize.draft.resolution;
        let valid = Resolution { width, height }.validate(limit);
        if display
            .confirmation
            .as_ref()
            .is_none_or(|(source, id, _)| *source != owner || *id != revision)
        {
            display.confirmation = Some((owner, revision, std::time::Instant::now()));
        }
        let remaining = std::time::Duration::from_secs(u64::from(
            resonance_game::menu::customize::DISPLAY_CONFIRMATION_SECONDS,
        ))
        .saturating_sub(display.confirmation.as_ref().unwrap().2.elapsed());
        if valid.is_err() || remaining.is_zero() {
            menu.customize.revert_display_change();
            if valid.is_err() {
                menu.notice = Some("This resolution is not supported by the GPU.".into());
            }
            display.confirmation = None;
        } else {
            let seconds = remaining.as_secs_f64().ceil() as u8;
            if seconds != remaining_secs {
                if let Focus::DisplayConfirmation { remaining_secs, .. } = &mut menu.customize.focus
                {
                    *remaining_secs = seconds;
                }
            }
        }
    } else {
        display.confirmation = None;
    }
    display.preview = Some((
        menu.customize.draft.display_mode,
        menu.customize.draft.resolution,
    ));
}

fn mode(
    selected: resonance_content::menu_data::DisplayMode,
    size: [u32; 2],
    monitor: Option<(Entity, &Monitor)>,
    wayland: bool,
) -> Option<WindowMode> {
    use resonance_content::menu_data::DisplayMode;
    Some(match selected {
        DisplayMode::Windowed => WindowMode::Windowed,
        DisplayMode::Fullscreen if !wayland => {
            let (entity, monitor) = monitor?;
            let video = monitor
                .video_modes
                .iter()
                .filter(|video| video.physical_size == UVec2::from_array(size))
                .max_by_key(|video| (video.refresh_rate_millihertz, video.bit_depth));
            WindowMode::Fullscreen(
                MonitorSelection::Entity(entity),
                video
                    .copied()
                    .map_or(VideoModeSelection::Current, VideoModeSelection::Specific),
            )
        }
        // Winit ignores exclusive fullscreen on Wayland. Desktop fullscreen
        // still fills this monitor while the render target retains its own size.
        DisplayMode::Borderless | DisplayMode::Fullscreen => {
            WindowMode::BorderlessFullscreen(MonitorSelection::Entity(monitor?.0))
        }
    })
}

/// Apply the current display preview or committed settings. Keeping handles stable
/// lets every scene, movie and UI camera retain its target across a size change.
pub(super) fn apply_preferences(
    preferences: Res<crate::config::Preferences>,
    mut applied: ResMut<InteractiveDisplay>,
    mut aa: ResMut<crate::anti_aliasing::AntiAliasingSettings>,
    mut display: ResMut<Display>,
    targets: Res<Targets>,
    mut images: ResMut<Assets<Image>>,
    mut outputs: ResMut<Assets<crate::materials::TitleOutput>>,
    mut windows: Query<
        (&mut Window, Option<&OnMonitor>, Option<&RawHandleWrapper>),
        With<PrimaryWindow>,
    >,
    monitors: Query<&Monitor>,
    device: Option<Res<bevy::render::renderer::RenderDevice>>,
) {
    let Ok((mut window, on_monitor, handle)) = windows.single_mut() else {
        return;
    };
    let monitor_entity = on_monitor.map(|m| m.0);
    let settings = &preferences.settings;
    aa.set_if_neq(crate::anti_aliasing::AntiAliasingSettings(
        settings.anti_aliasing,
    ));
    let desired = applied
        .preview
        .unwrap_or((settings.display_mode, settings.resolution));
    // Monitor information can arrive after the first window frame. Re-select
    // the exclusive video mode when it becomes available or the window moves.
    if applied.applied == Some(desired) && applied.monitor == monitor_entity {
        return;
    }
    let size = Resolution {
        width: desired.1[0],
        height: desired.1[1],
    };
    let limit = device
        .as_ref()
        .map_or(8192, |device| device.limits().max_texture_dimension_2d);
    if let Err(error) = size.validate(limit) {
        error!("Display settings could not be applied: {error:#}");
        applied.applied = Some(desired);
        applied.monitor = monitor_entity;
        return;
    }
    let monitor = monitor_entity
        .and_then(|entity| monitors.get(entity).ok().map(|monitor| (entity, monitor)));
    // Wayland assigns the output after window creation. Winit panics if exclusive
    // fullscreen is requested before then. Leave the request pending and retry;
    // an explicit monitor also avoids resolving Current again in Winit's system.
    let wayland = handle
        .is_some_and(|handle| matches!(handle.get_display_handle(), RawDisplayHandle::Wayland(_)));
    let Some(window_mode) = mode(desired.0, desired.1, monitor, wayland) else {
        // Bevy refreshes OnMonitor only for changed windows. Keep polling even
        // when the compositor assigns the output without a resize/focus event.
        window.set_changed();
        return;
    };
    // Update constraints before the physical size, including when leaving fullscreen.
    window.resize_constraints = WindowResizeConstraints {
        min_width: size.width as f32,
        max_width: size.width as f32,
        min_height: size.height as f32,
        max_height: size.height as f32,
    };
    window
        .resolution
        .set_physical_resolution(size.width, size.height);
    window.mode = window_mode;
    if display.0 != size {
        images
            .get_mut(&targets.source)
            .expect("scene render target")
            .resize(bevy::render::render_resource::Extent3d {
                width: size.width,
                height: size.scene_height(),
                depth_or_array_layers: 1,
            });
        // Resizing replaces the GPU texture view. Re-extract materials that
        // sample it so their bind groups do not keep the previous image.
        let refresh: Vec<_> = outputs
            .iter()
            .filter_map(|(id, output)| (output.source == targets.source).then_some(id))
            .collect();
        for id in refresh {
            outputs.get_mut(id).unwrap().source = targets.source.clone();
        }
        display.0 = size;
    }
    applied.applied = Some(desired);
    applied.monitor = monitor_entity;
}

#[derive(Resource)]
pub(super) struct Targets {
    pub source: Handle<Image>,
    pub output: Option<Handle<Image>>,
}
#[derive(Component)]
pub(super) struct OutputQuad;
#[derive(Component)]
pub(super) struct OutputCamera;

/// Reframe on startup and after a render-size change. Only the final window
/// camera fits the compositor surface; previews retain their authored framing.
pub(super) fn initialize(
    display: Res<Display>,
    targets: Option<Res<Targets>>,
    mut cameras: Query<(
        &bevy::camera::RenderTarget,
        &mut Projection,
        Has<OutputCamera>,
        Has<super::FieldCamera>,
    )>,
    mut output: Query<&mut Transform, With<OutputQuad>>,
) {
    let desired = display.0;
    let size = desired.ui_size();
    for (target, mut projection, output_camera, field_camera) in &mut cameras {
        // Offscreen previews have their own authored framing.
        if field_camera
            && let Projection::Custom(p) = &mut *projection
            && let Some(p) = p.get_mut::<super::camera::TitleProjection>()
        {
            p.0.aspect_ratio = desired.aspect();
        }
        let game_target = match target {
            RenderTarget::Image(image) => targets.as_ref().is_none_or(|t| image.handle == t.source),
            _ => false,
        };
        if (output_camera || game_target)
            && let Projection::Orthographic(p) = &mut *projection
        {
            p.scaling_mode = if output_camera && matches!(target, RenderTarget::Window(_)) {
                // Tiling compositors may override our fixed-size request.
                // Fit the retained image, leaving the camera's black clear
                // visible outside the quad. This adds no intermediate copy.
                ScalingMode::AutoMin {
                    min_width: size.x,
                    min_height: size.y,
                }
            } else {
                ScalingMode::Fixed {
                    width: size.x,
                    height: size.y,
                }
            };
        }
    }
    for mut transform in &mut output {
        transform.scale = Vec3::new(size.x / WIDTH as f32, size.y / HEIGHT as f32, 1.);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::CameraProjection;
    use bevy::window::PrimaryMonitor;
    use resonance_content::menu_data::DisplayMode;
    use resonance_game::menu::{Menu, Page, customize::Focus};

    fn preview_app() -> (App, Handle<Image>) {
        let mut app = App::new();
        app.init_resource::<crate::config::Preferences>()
            .init_resource::<InteractiveDisplay>()
            .init_resource::<crate::anti_aliasing::AntiAliasingSettings>()
            .init_resource::<Display>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<crate::materials::TitleOutput>>()
            .add_systems(
                Update,
                (preview_preferences, apply_preferences, initialize).chain(),
            );
        let source =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    640,
                    448,
                    bevy::render::render_resource::TextureFormat::Bgra8Unorm,
                    None,
                ));
        app.insert_resource(Targets {
            source: source.clone(),
            output: None,
        });
        app.world_mut()
            .spawn((window(Resolution::default()), PrimaryWindow));
        app.insert_resource(crate::saves::title::TitleMenu(Menu::new(
            Page::Customize,
            None,
            false,
        )));
        app.update();
        (app, source)
    }

    fn preview(app: &mut App, mode: DisplayMode, size: [u32; 2], revision: u64) {
        let mut title = app
            .world_mut()
            .resource_mut::<crate::saves::title::TitleMenu>();
        let state = &mut title.0.customize;
        state.focus = Focus::DisplayConfirmation {
            previous_mode: state.draft.display_mode,
            previous_resolution: state.draft.resolution,
            remaining_secs: 15,
            keep: true,
            revision,
        };
        state.draft.display_mode = mode;
        state.draft.resolution = size;
    }

    fn age_confirmation(app: &mut App, seconds: u64) {
        app.world_mut()
            .resource_mut::<InteractiveDisplay>()
            .confirmation
            .as_mut()
            .unwrap()
            .2 = std::time::Instant::now() - std::time::Duration::from_secs(seconds);
    }

    fn monitor(size: [u32; 2], video_sizes: &[[u32; 2]]) -> Monitor {
        Monitor {
            name: None,
            physical_width: size[0],
            physical_height: size[1],
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: Some(60000),
            scale_factor: 2.,
            video_modes: video_sizes
                .iter()
                .map(|size| bevy::window::VideoMode {
                    physical_size: UVec2::from_array(*size),
                    bit_depth: 24,
                    refresh_rate_millihertz: 60000,
                })
                .collect(),
        }
    }

    #[test]
    fn monitor_sizes_include_physical_native_and_modes_within_gpu_limits() {
        let native = [3024, 1964];
        let monitor = monitor(
            native,
            &[native, native, [1512, 982], [9000, 4000], [0, 600]],
        );
        let (choices, reported_native) = monitor_resolutions(Some(&monitor), 4096);
        let mut customize = resonance_game::menu::customize::Customize::default();
        customize.set_resolution_choices(choices, reported_native);
        assert_eq!(customize.native_resolution(), Some(native));
        let choices = customize.resolution_choices();
        assert!(choices.contains(&native));
        assert!(choices.contains(&[1512, 982]));
        assert!(choices.contains(&[1920, 1080]));
        assert!(choices.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            choices
                .iter()
                .all(|size| size.iter().all(|value| (1..=4096).contains(value)))
        );

        let (choices, native) = monitor_resolutions(Some(&monitor), 2048);
        customize.set_resolution_choices(choices, native);
        assert_eq!(customize.native_resolution(), None);
        assert!(!customize.resolution_choices().contains(&[3024, 1964]));
    }

    #[test]
    fn unknown_window_monitor_uses_presets_without_guessing_a_native_size() {
        let (mut app, _) = preview_app();
        let primary = app
            .world_mut()
            .spawn((monitor([3024, 1964], &[[1512, 982]]), PrimaryMonitor))
            .id();
        let other = app.world_mut().spawn(monitor([2561, 1441], &[])).id();
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        let assert_presets = |app: &App| {
            let state = &app
                .world()
                .resource::<crate::saves::title::TitleMenu>()
                .0
                .customize;
            assert_eq!(state.native_resolution(), None);
            assert_eq!(
                state.resolution_choices(),
                &resonance_content::menu_data::DISPLAY_RESOLUTIONS
            );
            assert_eq!(state.draft.resolution, [640, 480]);
        };
        app.update();
        assert_presets(&app);

        // An arbitrary monitor is no safer a guess than the primary monitor.
        app.world_mut()
            .entity_mut(primary)
            .remove::<PrimaryMonitor>();
        app.update();
        assert_presets(&app);

        let unavailable = app.world_mut().spawn_empty().id();
        app.world_mut()
            .entity_mut(window)
            .insert(OnMonitor(unavailable));
        app.update();
        assert_presets(&app);

        app.world_mut().entity_mut(window).insert(OnMonitor(other));
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::saves::title::TitleMenu>()
                .0
                .customize
                .native_resolution(),
            Some([2561, 1441])
        );
        app.world_mut().entity_mut(window).remove::<OnMonitor>();
        app.update();
        assert_presets(&app);
    }

    #[test]
    fn resolution_choices_follow_the_current_monitor_without_changing_the_draft() {
        let (mut app, _) = preview_app();
        let primary = app
            .world_mut()
            .spawn((
                monitor([3024, 1964], &[[3024, 1964], [1512, 982]]),
                PrimaryMonitor,
            ))
            .id();
        let other = app.world_mut().spawn(monitor([2561, 1441], &[])).id();
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .entity_mut(window)
            .insert(OnMonitor(primary));
        app.update();
        let customize = &app
            .world()
            .resource::<crate::saves::title::TitleMenu>()
            .0
            .customize;
        assert_eq!(customize.native_resolution(), Some([3024, 1964]));
        assert!(customize.resolution_choices().contains(&[1512, 982]));
        app.world_mut().entity_mut(window).insert(OnMonitor(other));
        app.update();
        let customize = &app
            .world()
            .resource::<crate::saves::title::TitleMenu>()
            .0
            .customize;
        assert_eq!(customize.native_resolution(), Some([2561, 1441]));
        assert_eq!(customize.draft.resolution, [640, 480]);
        assert_eq!(app.world().resource::<Display>().0, Resolution::default());
        assert!(!customize.resolution_choices().contains(&[3024, 1964]));

        // Odd-sized fullscreen selects its exact advertised mode and still rolls back.
        app.world_mut()
            .entity_mut(window)
            .insert(OnMonitor(primary));
        preview(&mut app, DisplayMode::Fullscreen, [3024, 1964], 1);
        app.update();
        assert_eq!(
            app.world().resource::<Display>().0,
            Resolution {
                width: 3024,
                height: 1964
            }
        );
        assert!(matches!(app.world().get::<Window>(window).unwrap().mode,
            WindowMode::Fullscreen(_, VideoModeSelection::Specific(video))
            if video.physical_size == UVec2::new(3024, 1964)));
        age_confirmation(&mut app, 15);
        app.update();
        assert_eq!(app.world().resource::<Display>().0, Resolution::default());
    }

    #[test]
    fn wayland_fullscreen_uses_the_known_monitor_without_an_exclusive_mode_request() {
        let output = Entity::PLACEHOLDER;
        let monitor = monitor([3024, 1964], &[[1920, 1080]]);
        assert_eq!(
            mode(DisplayMode::Fullscreen, [1920, 1080], None, true),
            None
        );
        assert_eq!(
            mode(
                DisplayMode::Fullscreen,
                [1920, 1080],
                Some((output, &monitor)),
                true
            ),
            Some(WindowMode::BorderlessFullscreen(MonitorSelection::Entity(
                output
            )))
        );
        assert!(
            matches!(mode(DisplayMode::Fullscreen, [1920, 1080], Some((output, &monitor)), false),
            Some(WindowMode::Fullscreen(MonitorSelection::Entity(entity), VideoModeSelection::Specific(video)))
            if entity == output && video.physical_size == UVec2::new(1920, 1080))
        );
    }

    #[test]
    fn fullscreen_startup_waits_for_a_resolved_window_monitor() {
        for selected in [DisplayMode::Fullscreen, DisplayMode::Borderless] {
            let (mut app, _) = preview_app();
            app.world_mut()
                .remove_resource::<crate::saves::title::TitleMenu>();
            {
                let mut preferences = app.world_mut().resource_mut::<crate::config::Preferences>();
                preferences.settings.display_mode = selected;
                preferences.settings.resolution = [1920, 1080];
            }
            let window = app
                .world_mut()
                .query_filtered::<Entity, With<PrimaryWindow>>()
                .single(app.world())
                .unwrap();
            // Saved fullscreen settings must not reach Winit's Current selector
            // while Wayland is still assigning the newly created window an output.
            for _ in 0..3 {
                app.update();
                assert_eq!(
                    app.world().get::<Window>(window).unwrap().mode,
                    WindowMode::Windowed
                );
                assert_ne!(
                    app.world().resource::<InteractiveDisplay>().applied,
                    Some((selected, [1920, 1080]))
                );
            }
            // A relationship alone is insufficient until its Monitor exists.
            let output = app.world_mut().spawn_empty().id();
            app.world_mut().entity_mut(window).insert(OnMonitor(output));
            app.update();
            assert_eq!(
                app.world().get::<Window>(window).unwrap().mode,
                WindowMode::Windowed
            );
            app.world_mut()
                .entity_mut(output)
                .insert(monitor([1920, 1080], &[[1920, 1080]]));
            app.update();
            let expected = match selected {
                DisplayMode::Fullscreen => WindowMode::Fullscreen(
                    MonitorSelection::Entity(output),
                    VideoModeSelection::Specific(bevy::window::VideoMode {
                        physical_size: UVec2::new(1920, 1080),
                        bit_depth: 24,
                        refresh_rate_millihertz: 60000,
                    }),
                ),
                DisplayMode::Borderless => {
                    WindowMode::BorderlessFullscreen(MonitorSelection::Entity(output))
                }
                DisplayMode::Windowed => unreachable!(),
            };
            assert_eq!(app.world().get::<Window>(window).unwrap().mode, expected);
            assert_eq!(
                app.world().resource::<Display>().0,
                Resolution {
                    width: 1920,
                    height: 1080
                }
            );
        }
    }

    #[test]
    fn fullscreen_startup_keeps_polling_for_a_late_monitor_assignment() {
        let (mut app, _) = preview_app();
        app.world_mut()
            .remove_resource::<crate::saves::title::TitleMenu>();
        app.world_mut()
            .resource_mut::<crate::config::Preferences>()
            .settings
            .display_mode = DisplayMode::Fullscreen;
        let output = app.world_mut().spawn(monitor([1920, 1080], &[])).id();
        // Model Winit's Changed<Window> polling with a delayed output assignment.
        app.add_systems(
            Last,
            move |mut commands: Commands,
                  windows: Query<Entity, (With<PrimaryWindow>, Changed<Window>)>,
                  mut polls: Local<u8>| {
                for window in &windows {
                    *polls += 1;
                    if *polls == 3 {
                        commands.entity(window).insert(OnMonitor(output));
                    }
                }
            },
        );
        for _ in 0..4 {
            app.update();
        }
        let mut windows = app
            .world_mut()
            .query_filtered::<&Window, With<PrimaryWindow>>();
        assert_eq!(
            windows.single(app.world()).unwrap().mode,
            WindowMode::Fullscreen(
                MonitorSelection::Entity(output),
                VideoModeSelection::Current
            )
        );
    }

    #[test]
    fn resolution_preview_resizes_immediately_and_times_out_without_saving() {
        let (mut app, source) = preview_app();
        app.insert_resource(Time::<Virtual>::default());
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        preview(&mut app, DisplayMode::Windowed, [800, 600], 1);
        app.update();
        assert_eq!(
            app.world().resource::<Display>().0,
            Resolution {
                width: 800,
                height: 600
            }
        );
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&source)
                .unwrap()
                .size(),
            UVec2::new(800, 560)
        );
        assert_eq!(
            app.world()
                .resource::<crate::config::Preferences>()
                .settings
                .resolution,
            [640, 480]
        );
        age_confirmation(&mut app, 14);
        app.update();
        assert!(matches!(
            app.world()
                .resource::<crate::saves::title::TitleMenu>()
                .0
                .customize
                .focus,
            Focus::DisplayConfirmation {
                remaining_secs: 1,
                ..
            }
        ));
        age_confirmation(&mut app, 15);
        app.update();
        assert_eq!(app.world().resource::<Display>().0, Resolution::default());
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&source)
                .unwrap()
                .size(),
            UVec2::new(640, 448)
        );
        assert_eq!(
            app.world()
                .resource::<crate::saves::title::TitleMenu>()
                .0
                .customize
                .focus,
            Focus::Options
        );
        assert_eq!(
            app.world()
                .resource::<crate::config::Preferences>()
                .settings
                .resolution,
            [640, 480]
        );
    }

    #[test]
    fn another_preview_reverts_to_the_last_kept_settings_and_abandoned_menus_recover() {
        let (mut app, _) = preview_app();
        preview(&mut app, DisplayMode::Windowed, [800, 600], 1);
        app.update();
        app.world_mut()
            .resource_mut::<crate::saves::title::TitleMenu>()
            .0
            .customize
            .focus = Focus::Options;
        app.update();
        assert!(
            app.world()
                .resource::<InteractiveDisplay>()
                .confirmation
                .is_none()
        );
        preview(&mut app, DisplayMode::Fullscreen, [1024, 768], 2);
        app.update();
        assert!(matches!(
            app.world()
                .resource::<crate::saves::title::TitleMenu>()
                .0
                .customize
                .focus,
            Focus::DisplayConfirmation {
                remaining_secs: 15,
                ..
            }
        ));
        age_confirmation(&mut app, 15);
        app.update();
        let state = &app
            .world()
            .resource::<crate::saves::title::TitleMenu>()
            .0
            .customize;
        assert_eq!(state.draft.resolution, [800, 600]);
        assert_eq!(state.draft.display_mode, DisplayMode::Windowed);
        assert_eq!(
            app.world().resource::<Display>().0,
            Resolution {
                width: 800,
                height: 600
            }
        );
        app.world_mut()
            .remove_resource::<crate::saves::title::TitleMenu>();
        app.update();
        assert_eq!(app.world().resource::<Display>().0, Resolution::default());
    }

    #[test]
    fn invalid_display_preview_is_rejected_before_resizing() {
        let (mut app, _) = preview_app();
        preview(&mut app, DisplayMode::Windowed, [8193, 480], 1);
        app.update();
        let menu = &app.world().resource::<crate::saves::title::TitleMenu>().0;
        assert_eq!(menu.customize.draft.resolution, [640, 480]);
        assert!(menu.notice.is_some());
        assert_eq!(app.world().resource::<Display>().0, Resolution::default());
    }

    #[test]
    fn committed_display_settings_resize_targets_and_switch_window_modes() {
        use bevy::render::render_resource::TextureFormat;
        use bevy::window::VideoMode;
        use resonance_content::menu_data::DisplayMode;
        let mut app = App::new();
        app.init_resource::<crate::config::Preferences>()
            .init_resource::<InteractiveDisplay>()
            .init_resource::<crate::anti_aliasing::AntiAliasingSettings>()
            .init_resource::<Display>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<crate::materials::TitleOutput>>()
            .add_systems(
                Update,
                (
                    apply_preferences,
                    initialize.run_if(resource_changed::<Display>),
                )
                    .chain(),
            );
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
        let monitor = app
            .world_mut()
            .spawn(Monitor {
                name: None,
                physical_width: 1920,
                physical_height: 1080,
                physical_position: IVec2::ZERO,
                refresh_rate_millihertz: Some(60000),
                scale_factor: 1.,
                video_modes: [60000, 144000]
                    .map(|refresh_rate_millihertz| VideoMode {
                        physical_size: UVec2::new(1920, 1080),
                        bit_depth: 24,
                        refresh_rate_millihertz,
                    })
                    .into(),
            })
            .id();
        let window_id = app
            .world_mut()
            .spawn((
                window(Resolution::default()),
                PrimaryWindow,
                OnMonitor(monitor),
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                RenderTarget::Image(source.clone().into()),
                Projection::Orthographic(OrthographicProjection::default_2d()),
            ))
            .id();
        let quad = app
            .world_mut()
            .spawn((OutputQuad, Transform::default()))
            .id();
        app.update();
        for selected in [
            DisplayMode::Borderless,
            DisplayMode::Fullscreen,
            DisplayMode::Windowed,
        ] {
            let mut preferences = app.world_mut().resource_mut::<crate::config::Preferences>();
            preferences.settings.display_mode = selected;
            preferences.settings.resolution = [1920, 1080];
            app.update();
            let window = app.world().get::<Window>(window_id).unwrap();
            match selected {
                DisplayMode::Borderless => assert_eq!(
                    window.mode,
                    WindowMode::BorderlessFullscreen(MonitorSelection::Entity(monitor))
                ),
                DisplayMode::Fullscreen => assert!(matches!(window.mode,
                    WindowMode::Fullscreen(_, VideoModeSelection::Specific(video)) if video.refresh_rate_millihertz == 144000)),
                DisplayMode::Windowed => assert_eq!(window.mode, WindowMode::Windowed),
            }
            assert_eq!(window.resolution.physical_size(), UVec2::new(1920, 1080));
            assert_eq!(window.resize_constraints.max_width, 1920.);
            let image = app
                .world()
                .resource::<Assets<Image>>()
                .get(&source)
                .unwrap();
            assert_eq!(image.size(), UVec2::new(1920, 1008));
            let Projection::Orthographic(projection) =
                app.world().get::<Projection>(camera).unwrap()
            else {
                panic!()
            };
            assert!(
                matches!(projection.scaling_mode, ScalingMode::Fixed { width, height }
                if (width - 480. * 1920. / 1080.).abs() < 0.001 && height == 480.)
            );
            assert!(
                (app.world().get::<Transform>(quad).unwrap().scale.x - 4. / 3.).abs() < 0.00001
            );
        }
        // A monitor-dependent request stays pending while its monitor is unknown.
        app.world_mut().entity_mut(window_id).remove::<OnMonitor>();
        app.world_mut()
            .resource_mut::<crate::config::Preferences>()
            .settings
            .display_mode = DisplayMode::Fullscreen;
        app.update();
        assert_eq!(
            app.world().get::<Window>(window_id).unwrap().mode,
            WindowMode::Windowed
        );
        app.world_mut()
            .entity_mut(window_id)
            .insert(OnMonitor(monitor));
        app.update();
        assert!(matches!(app.world().get::<Window>(window_id).unwrap().mode,
            WindowMode::Fullscreen(_, VideoModeSelection::Specific(video)) if video.refresh_rate_millihertz == 144000));
        // An unsupported exclusive mode retains the desktop mode but still renders
        // at the selected size. Returning to windowed restores its requested size.
        app.world_mut()
            .resource_mut::<crate::config::Preferences>()
            .settings
            .resolution = [1280, 960];
        app.world_mut()
            .resource_mut::<crate::config::Preferences>()
            .settings
            .display_mode = DisplayMode::Fullscreen;
        app.update();
        assert_eq!(
            app.world().get::<Window>(window_id).unwrap().mode,
            WindowMode::Fullscreen(
                MonitorSelection::Entity(monitor),
                VideoModeSelection::Current
            )
        );
        app.world_mut()
            .resource_mut::<crate::config::Preferences>()
            .settings
            .display_mode = DisplayMode::Windowed;
        app.update();
        assert_eq!(
            app.world()
                .get::<Window>(window_id)
                .unwrap()
                .resolution
                .physical_size(),
            UVec2::new(1280, 960)
        );
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&source)
                .unwrap()
                .size(),
            UVec2::new(1280, 896)
        );
    }

    #[test]
    fn display_frames_game_cameras_and_preserves_preview_framing() {
        for size in ["640x480", "1920x1080", "3440x1440"].map(|s| s.parse::<Resolution>().unwrap())
        {
            let mut app = App::new();
            app.insert_resource(Display(size))
                .add_systems(Startup, initialize);
            let window = window(size);
            assert!(!window.resizable && !window.enabled_buttons.maximize);
            assert!(window.enabled_buttons.minimize && window.enabled_buttons.close);
            assert_eq!(window.resize_constraints.min_width, size.width as f32);
            assert_eq!(window.resize_constraints.max_width, size.width as f32);
            assert_eq!(window.resize_constraints.min_height, size.height as f32);
            assert_eq!(window.resize_constraints.max_height, size.height as f32);
            let target = RenderTarget::Image(Handle::<Image>::default().into());
            let scene = app
                .world_mut()
                .spawn((
                    super::super::FieldCamera,
                    target.clone(),
                    Projection::custom(super::super::camera::TitleProjection(
                        PerspectiveProjection::default(),
                    )),
                ))
                .id();
            let preview_projection = Projection::custom(super::super::camera::TitleProjection(
                PerspectiveProjection {
                    aspect_ratio: 10. / 7.,
                    ..default()
                },
            ));
            let preview_clip = preview_projection.get_clip_from_view();
            let preview = app
                .world_mut()
                .spawn((target.clone(), preview_projection))
                .id();
            let ui = app
                .world_mut()
                .spawn((
                    target,
                    Projection::Orthographic(OrthographicProjection::default_2d()),
                ))
                .id();
            let present = app
                .world_mut()
                .spawn((
                    OutputCamera,
                    RenderTarget::default(),
                    Projection::Orthographic(OrthographicProjection::default_2d()),
                ))
                .id();
            let quad = app
                .world_mut()
                .spawn((OutputQuad, Transform::default()))
                .id();
            app.update();
            let projection = |entity| app.world().get::<Projection>(entity).unwrap();
            assert_eq!(
                projection(preview).get_clip_from_view(),
                preview_clip,
                "display initialization changed the preview's authored framing"
            );
            let scene = projection(scene).get_clip_from_view();
            assert!((scene.y_axis.y / scene.x_axis.x - size.aspect()).abs() < 0.00001);
            let bounds = size.ui_size();
            assert_eq!(
                app.world().get::<Transform>(quad).unwrap().scale,
                Vec3::new(bounds.x / WIDTH as f32, bounds.y / HEIGHT as f32, 1.)
            );
            let Projection::Orthographic(ui) = projection(ui) else {
                panic!("UI projection changed")
            };
            assert!(
                matches!(ui.scaling_mode, ScalingMode::Fixed { width, height }
                if width == bounds.x && height == bounds.y)
            );
            let Projection::Orthographic(mut present) = projection(present).clone() else {
                panic!("output projection changed")
            };
            for (width, height) in [
                (900, 900),
                (1200, 500),
                (400, 900),
                (size.width, size.height),
            ] {
                // Only the final camera fits compositor-imposed surface dimensions.
                present.update(width as f32, height as f32);
                let pixels = bounds / present.area.size() * Vec2::new(width as f32, height as f32);
                assert!((pixels.x / pixels.y - size.aspect()).abs() < 0.00001);
                assert!(pixels.x <= width as f32 + 0.01 && pixels.y <= height as f32 + 0.01);
                assert!(
                    (pixels.x - width as f32).abs() < 0.01
                        || (pixels.y - height as f32).abs() < 0.01
                );
                assert!(present.area.center().length() < 0.001);
            }
        }
    }

    #[test]
    fn render_sizes_preserve_native_overscan_and_fit_authored_ui() {
        for (width, height) in [
            (640, 480),
            (1280, 960),
            (1920, 1080),
            (2560, 1440),
            (3440, 1440),
            (1279, 719),
            (480, 800),
        ] {
            let size = Resolution { width, height };
            size.validate(8192).unwrap();
            let ui = size.ui_size();
            assert!(ui.x >= 640. && ui.y >= 480.);
            assert!((ui.x / ui.y - size.aspect()).abs() < 0.00001);
        }
        assert_eq!(Resolution::default().scene_height(), 448);
        assert_eq!(
            "1920x1080".parse::<Resolution>().unwrap().scene_height(),
            1008
        );
        for invalid in ["0x480", "640x0", "1920", "-1x480", "1x2x3"] {
            assert!(invalid.parse::<Resolution>().is_err());
        }
        assert!(
            Resolution {
                width: 9000,
                height: 480
            }
            .validate(8192)
            .is_err()
        );
    }
}
