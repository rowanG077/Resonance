use super::{DESTINATIONS, Menu, State};
use bevy::{
    camera::visibility::RenderLayers,
    core_pipeline::tonemapping::Tonemapping,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::PrimaryWindow,
};

const WIDTH: u32 = 584;
const ROW_TOP: u32 = 90;
const ROW_HEIGHT: u32 = 25;
const FOOTER_TOP: u32 = ROW_TOP + ROW_HEIGHT * DESTINATIONS.len() as u32 + 10;
const HEIGHT: u32 = FOOTER_TOP + 90;
const LAYER: usize = 28;

#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(super) struct OverlayCamera;
#[derive(Resource)]
pub(super) struct Artwork {
    image: Handle<Image>,
}

pub(super) fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new(
        Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels(&Menu::default()),
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    image.sampler = ImageSampler::nearest();
    let image = images.add(image);
    commands.spawn((
        Sprite::from_image(image.clone()),
        Panel,
        RenderLayers::layer(LAYER),
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 110,
            clear_color: ClearColorConfig::None,
            is_active: false,
            ..default()
        },
        Tonemapping::None,
        Msaa::Off,
        OverlayCamera,
        RenderLayers::layer(LAYER),
    ));
    commands.insert_resource(Artwork { image });
}

fn scale(size: Vec2) -> f32 {
    ((size.x - 16.) / WIDTH as f32)
        .min((size.y - 16.) / HEIGHT as f32)
        .clamp(0.1, 2.)
}

pub(super) fn row_at(cursor: Vec2, size: Vec2) -> Option<usize> {
    let point = (cursor - size / 2.) / scale(size) + Vec2::new(WIDTH as f32, HEIGHT as f32) / 2.;
    if !(16. ..WIDTH as f32 - 16.).contains(&point.x) || point.y < ROW_TOP as f32 {
        return None;
    }
    let row = ((point.y - ROW_TOP as f32) / ROW_HEIGHT as f32) as usize;
    (row < DESTINATIONS.len()).then_some(row)
}

pub(super) fn update(
    menu: Res<Menu>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut camera: Single<&mut Camera, With<OverlayCamera>>,
    mut panel: Single<&mut Transform, With<Panel>>,
    art: Res<Artwork>,
    mut images: ResMut<Assets<Image>>,
) {
    camera.is_active = menu.open();
    if !menu.open() {
        return;
    }
    if let Some(window) = window {
        panel.scale = Vec3::splat(scale(Vec2::new(window.width(), window.height())));
    }
    if menu.is_changed()
        && let Some(mut image) = images.get_mut(&art.image)
    {
        image.data = Some(pixels(&menu));
    }
}

fn pixels(menu: &Menu) -> Vec<u8> {
    let mut rgba = [13, 20, 32, 250].repeat((WIDTH * HEIGHT) as usize);
    let white = [226, 235, 246, 255];
    let muted = [154, 174, 194, 255];
    let accent = [104, 223, 200, 255];
    text(&mut rgba, 24, 20, "DUNGEONS AND SCENES", accent);
    text(
        &mut rgba,
        24,
        45,
        "FRESH PUZZLES - RING READY - BATTLES SKIPPED",
        white,
    );
    text(
        &mut rgba,
        24,
        66,
        "REPLACES CURRENT RUN. DISK SAVES UNCHANGED.",
        muted,
    );
    for (index, destination) in DESTINATIONS.iter().enumerate() {
        let top = ROW_TOP + index as u32 * ROW_HEIGHT;
        if index == menu.selected {
            for y in top..top + ROW_HEIGHT - 2 {
                for x in 16..WIDTH - 16 {
                    put(&mut rgba, x, y, [32, 69, 80, 255]);
                }
            }
        }
        text(
            &mut rgba,
            24,
            top + 4,
            &format!(
                "{} {}  {}",
                if index == menu.selected { ">" } else { " " },
                if index < 10 {
                    ((index + 1) % 10).to_string()
                } else {
                    " ".into()
                },
                destination.name
            ),
            white,
        );
    }
    if let State::Failed(error) = &menu.state {
        text(
            &mut rgba,
            24,
            FOOTER_TOP,
            "LOAD FAILED - CHOOSE AGAIN OR CLOSE",
            [255, 151, 136, 255],
        );
        text(
            &mut rgba,
            24,
            FOOTER_TOP + 21,
            &error.to_uppercase().chars().take(44).collect::<String>(),
            muted,
        );
    } else if matches!(menu.state, State::Loading(_)) {
        text(&mut rgba, 24, FOOTER_TOP, "LOADING...", accent);
        text(
            &mut rgba,
            24,
            FOOTER_TOP + 21,
            "SHIFT TAB OR ESC CANCELS",
            muted,
        );
    } else {
        text(
            &mut rgba,
            24,
            FOOTER_TOP,
            &format!("MAP {}", DESTINATIONS[menu.selected].map),
            accent,
        );
        text(
            &mut rgba,
            24,
            FOOTER_TOP + 21,
            "ARROWS CHOOSE - ENTER GO - OR CLICK A ROW",
            white,
        );
    }
    text(
        &mut rgba,
        24,
        FOOTER_TOP + 56,
        "1-0 JUMP   SHIFT TAB OR ESC CLOSE",
        muted,
    );
    rgba
}

fn put(rgba: &mut [u8], x: u32, y: u32, color: [u8; 4]) {
    if x < WIDTH && y < HEIGHT {
        let at = ((y * WIDTH + x) * 4) as usize;
        rgba[at..at + 4].copy_from_slice(&color);
    }
}

fn text(rgba: &mut [u8], x: u32, y: u32, text: &str, color: [u8; 4]) {
    for (index, character) in text.chars().enumerate() {
        for (column, bits) in crate::debug_font::glyph(character).into_iter().enumerate() {
            for row in 0..7 {
                if bits & (1 << row) == 0 {
                    continue;
                }
                for dy in 0..2 {
                    for dx in 0..2 {
                        put(
                            rgba,
                            x + index as u32 * 12 + column as u32 * 2 + dx,
                            y + row * 2 + dy,
                            color,
                        );
                    }
                }
            }
        }
    }
}
