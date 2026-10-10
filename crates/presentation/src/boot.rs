//! High-level startup logos, rendered from cooked textures.
use super::*;
use resonance_content::BootAssets;
use resonance_game::boot::Logos;
use std::path::Path;

#[derive(Resource, Default)]
pub(super) struct Playback {
    pub logos: Option<Logos>,
    pub asset: Option<BootAssets>,
    images: Vec<Handle<Image>>,
}

impl Playback {
    pub fn load(root: &Path, options: &RunOptions) -> Result<Self> {
        if options.skip_intro || matches!(options.capture_at, Some(CaptureAt::MovieFrame(_))) {
            return Ok(Self::default());
        }
        let asset: BootAssets =
            serde_json::from_slice(&fs::read(root.join("boot.json")).context(
                "missing startup logos; run resonance-import cook-all or use --skip-intro",
            )?)?;
        asset.validate()?;
        Ok(Self {
            logos: Some(Logos::default()),
            asset: Some(asset),
            images: Vec::new(),
        })
    }

    pub fn active(&self) -> bool {
        self.logos.as_ref().is_some_and(Logos::active)
    }

    pub fn ready(&self, server: &AssetServer) -> bool {
        self.asset.as_ref().is_none_or(|a| {
            self.images.len() == a.textures.len()
                && self
                    .images
                    .iter()
                    .all(|h| server.is_loaded_with_dependencies(h.id()))
        })
    }

    pub fn check_images(&mut self, server: &AssetServer) -> Result<()> {
        for image in &self.images {
            if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(image.id()) {
                // A broken logo must not hold the title behind an endless load.
                *self = Self::default();
                anyhow::bail!("startup logo image failed: {error}");
            }
        }
        Ok(())
    }
}

#[derive(Component)]
pub(super) struct BootCamera;
#[derive(Component)]
pub(super) struct LogoQuad;

pub(super) fn setup(
    commands: &mut Commands,
    server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<TitleText>,
    boot: &mut Playback,
    source: Handle<Image>,
) {
    let Some(asset) = &boot.asset else {
        return;
    };
    boot.images = asset
        .textures
        .iter()
        .map(|t| {
            server
                .load_builder()
                .with_settings(|s: &mut ImageLoaderSettings| s.is_srgb = false)
                .load(t.path.clone())
        })
        .collect();
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(1., 1.))),
        MeshMaterial2d(materials.add(TitleText {
            source: boot.images[3].clone(),
            opacity_pulse: Vec4::ZERO,
        })),
        LogoQuad,
        RenderLayers::layer(4),
    ));
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        BootCamera,
        RenderLayers::layer(4),
        Camera {
            order: -2,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        camera::overlay_alignment(),
        RenderTarget::Image(source.into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
}

pub(super) fn advance(
    mut boot: ResMut<Playback>,
    ready: Res<timing::Ready>,
    mut input: ResMut<PendingInput>,
    scenario: Option<ResMut<saves::ScenarioInput>>,
) {
    if !ready.0 {
        return;
    }
    if let Some(logos) = &mut boot.logos
        && logos.active()
    {
        if let Some(mut scenario) = scenario {
            scenario.acknowledge_input();
        }
        logos.step(std::mem::take(&mut input.pressed).accept);
    }
}

pub(super) fn update(
    boot: Res<Playback>,
    mut cameras: Query<&mut Camera, With<BootCamera>>,
    mut quads: Query<(&MeshMaterial2d<TitleText>, &mut Transform), With<LogoQuad>>,
    mut materials: ResMut<Assets<TitleText>>,
) {
    for mut camera in &mut cameras {
        camera.is_active = boot.active();
    }
    let Some(logos) = &boot.logos else {
        return;
    };
    let texture = &boot.asset.as_ref().unwrap().textures[logos.texture];
    let alpha = f32::from(logos.alpha) / 255.;
    for mut camera in &mut cameras {
        let rgb = texture.background.map(|v| f32::from(v) / 255. * alpha);
        camera.clear_color = ClearColorConfig::Custom(Color::linear_rgb(rgb[0], rgb[1], rgb[2]));
    }
    for (handle, mut transform) in &mut quads {
        let mut material = materials.get_mut(&handle.0).unwrap();
        let source = &boot.images[logos.texture];
        let opacity = Vec4::new(alpha, 0., 0., 0.);
        if material.source != *source || material.opacity_pulse != opacity {
            material.source = source.clone();
            material.opacity_pulse = opacity;
        }
        // Use symmetric integer half-extents: an odd width such as 379 draws as 378.
        transform.scale = Vec3::new(
            (texture.width & !1) as f32,
            (texture.height & !1) as f32,
            1.,
        );
    }
}
