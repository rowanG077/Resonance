//! High-level startup logos, rendered from cooked textures.
use super::*;
use resonance_content::BootAssets;
use resonance_game::boot::Logos;

#[derive(Resource, Default)]
pub(super) struct Playback {
    pub logos: Option<Logos>,
    pub asset: Option<BootAssets>,
    images: Vec<Handle<Image>>,
}

impl Playback {
    pub fn from_assets(asset: Option<BootAssets>) -> Self {
        Self {
            logos: asset.as_ref().map(|_| Logos::default()),
            asset,
            images: Vec::new(),
        }
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
