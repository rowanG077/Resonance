//! Physical render sizes and a centered, authored UI coordinate system.
use anyhow::{Result, ensure};
use bevy::{
    camera::{RenderTarget, ScalingMode},
    prelude::*,
    window::{EnabledButtons, PresentMode, WindowResizeConstraints, WindowResolution},
};

use resonance_content::{HEIGHT, SCENE_HEIGHT, WIDTH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
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
}

#[derive(Resource, Clone, Copy, Debug, Default)]
/// Chosen at startup. Window geometry never changes the game render resolution.
pub(super) struct Display(pub Resolution);

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

#[derive(Resource)]
pub(super) struct Targets {
    pub source: Handle<Image>,
    pub output: Option<Handle<Image>>,
}
#[derive(Component)]
pub(super) struct OutputQuad;
#[derive(Component)]
pub(super) struct OutputCamera;

/// Runs once after scene setup. Only the final window camera follows the
/// compositor's surface size; every game camera and render target stays fixed.
pub(super) fn initialize(
    display: Res<Display>,
    mut cameras: Query<(
        &bevy::camera::RenderTarget,
        &mut Projection,
        Has<OutputCamera>,
    )>,
    mut output: Query<&mut Transform, With<OutputQuad>>,
) {
    let desired = display.0;
    let size = desired.ui_size();
    for (target, mut projection, output_camera) in &mut cameras {
        if let Projection::Custom(p) = &mut *projection
            && let Some(p) = p.get_mut::<super::camera::TitleProjection>()
        {
            p.0.aspect_ratio = desired.aspect();
        }
        if (output_camera || matches!(target, RenderTarget::Image(_)))
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
    use bevy::{
        camera::CameraProjection, render::render_resource::TextureFormat, window::PrimaryWindow,
    };

    #[test]
    fn forced_window_sizes_preserve_startup_targets_and_fit_the_image() {
        for size in ["640x480", "1920x1080", "3440x1440"].map(|s| s.parse::<Resolution>().unwrap())
        {
            let mut app = App::new();
            app.insert_resource(Display(size))
                .add_systems(Startup, initialize);
            let window = window(size);
            assert!(!window.resizable && !window.enabled_buttons.maximize);
            assert!(window.enabled_buttons.minimize && window.enabled_buttons.close);
            assert_eq!(
                window.resize_constraints.min_width,
                window.resize_constraints.max_width
            );
            assert_eq!(
                window.resize_constraints.min_height,
                window.resize_constraints.max_height
            );
            let window = app.world_mut().spawn((window, PrimaryWindow)).id();
            let mut images = Assets::<Image>::default();
            let source = images.add(Image::new_target_texture(
                size.width,
                size.scene_height(),
                TextureFormat::Bgra8Unorm,
                None,
            ));
            let output = images.add(Image::new_target_texture(
                size.width,
                size.height,
                TextureFormat::Bgra8UnormSrgb,
                None,
            ));
            app.insert_resource(images).insert_resource(Targets {
                source: source.clone(),
                output: Some(output.clone()),
            });
            let scene = app
                .world_mut()
                .spawn((
                    RenderTarget::Image(source.clone().into()),
                    Projection::custom(super::super::camera::TitleProjection(
                        PerspectiveProjection::default(),
                    )),
                ))
                .id();
            let ui = app
                .world_mut()
                .spawn((
                    RenderTarget::Image(source.clone().into()),
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
            let game_projection = app
                .world()
                .get::<Projection>(scene)
                .unwrap()
                .get_clip_from_view();
            let quad_scale = app.world().get::<Transform>(quad).unwrap().scale;
            let bounds = size.ui_size();
            assert_eq!(quad_scale, Vec3::new(bounds.x / 640., bounds.y / 480., 1.));
            for (width, height) in [
                (900, 900),
                (1200, 500),
                (400, 900),
                (0, 0),
                (size.width, size.height),
            ] {
                {
                    let mut forced = app.world_mut().get_mut::<Window>(window).unwrap();
                    forced.resolution.set_physical_resolution(width, height);
                    forced.resolution.set_scale_factor(2.);
                }
                app.update();
                assert_eq!(app.world().resource::<Display>().0, size);
                let targets = app.world().resource::<Targets>();
                assert_eq!(targets.source, source);
                assert_eq!(targets.output.as_ref(), Some(&output));
                let images = app.world().resource::<Assets<Image>>();
                assert_eq!(images.len(), 2);
                assert_eq!(
                    images.get(&source).unwrap().size(),
                    UVec2::new(size.width, size.scene_height())
                );
                assert_eq!(
                    images.get(&output).unwrap().size(),
                    UVec2::new(size.width, size.height)
                );
                assert_eq!(
                    app.world()
                        .get::<Projection>(scene)
                        .unwrap()
                        .get_clip_from_view(),
                    game_projection
                );
                assert_eq!(
                    app.world().get::<Transform>(quad).unwrap().scale,
                    quad_scale
                );
                let Projection::Orthographic(ui) = app.world().get::<Projection>(ui).unwrap()
                else {
                    panic!("UI projection changed")
                };
                assert!(
                    matches!(ui.scaling_mode, ScalingMode::Fixed { width, height }
                    if width == bounds.x && height == bounds.y)
                );
                if width == 0 || height == 0 {
                    continue;
                } // Bevy skips minimized surfaces.
                let mut p = app.world_mut().get_mut::<Projection>(present).unwrap();
                let Projection::Orthographic(p) = &mut *p else {
                    panic!("output projection changed")
                };
                p.update(width as f32, height as f32);
                let view = p.area.size();
                let pixels = bounds / view * Vec2::new(width as f32, height as f32);
                assert!((pixels.x / pixels.y - size.aspect()).abs() < 0.00001);
                assert!(pixels.x <= width as f32 + 0.01 && pixels.y <= height as f32 + 0.01);
                assert!(
                    (pixels.x - width as f32).abs() < 0.01
                        || (pixels.y - height as f32).abs() < 0.01
                );
                assert!(p.area.center().length() < 0.001);
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
