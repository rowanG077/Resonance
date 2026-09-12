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
/// Chosen at startup. Window geometry never changes the game render resolution.
pub(super) struct Display(pub Resolution);

/// Frame dumps precede display positioning, which only moves the final scanout.
/// Both stages retain the same resolution, color conversion and game state.
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
    use bevy::camera::CameraProjection;

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
