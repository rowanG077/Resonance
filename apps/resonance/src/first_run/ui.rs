use super::*;
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType},
    input_focus::{
        InputFocus,
        tab_navigation::{TabGroup, TabIndex},
    },
    picking::{Pickable, hover::Hovered},
    text::{FontSize, LineHeight},
    ui::{InteractionDisabled, Pressed, widget::NodeImageMode},
    ui_widgets::{Activate, Button},
};

const CREAM: Color = Color::srgb(0.965, 0.953, 0.922);
const GOLD: Color = Color::srgb(0.929, 0.875, 0.678);
const MUTED: Color = Color::srgb(0.769, 0.8, 0.863);
const BLUE: Color = Color::srgba(0.161, 0.216, 0.306, 0.98);

#[derive(Resource)]
pub(super) struct Art {
    frame: Handle<Image>,
    pattern: Handle<Image>,
    logo: Handle<Image>,
    handwriting: Handle<Font>,
    body: Handle<Font>,
    mono: Handle<Font>,
}
#[derive(Component)]
pub(super) struct Root;
#[derive(Component)]
pub(super) struct PanelBackground;
#[derive(Component)]
pub(super) struct ProgressFill;
#[derive(Component)]
pub(super) struct StatusText;
#[derive(Component)]
pub(super) struct PercentText;
#[derive(Component)]
pub(super) struct LogText;
#[derive(Component)]
pub(super) struct LogHint;
#[derive(Component)]
pub(super) struct LogViewport;
#[derive(Component, Clone, Copy)]
pub(super) enum Action {
    Disc(usize),
    ChooseDiscs,
    Start,
    Details,
    Hide,
    Reveal,
    Stop,
    Retry,
    Play,
    Quit,
    Follow,
}

pub(super) fn load_art(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    let mut image = |bytes: &[u8]| {
        images.add(
            Image::from_buffer(
                bytes,
                ImageType::Extension("png"),
                bevy::image::CompressedImageFormats::NONE,
                true,
                ImageSampler::linear(),
                RenderAssetUsages::default(),
            )
            .expect("bundled setup image"),
        )
    };
    let frame = image(include_bytes!("../../assets/setup/window-frame.png"));
    let pattern = image(include_bytes!("../../assets/setup/menu-pattern.png"));
    let logo = image(include_bytes!("../../assets/setup/logo.png"));
    images.get_mut(&pattern).unwrap().sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    commands.insert_resource(Art {
        frame,
        pattern,
        logo,
        body: fonts.add(Font::from_bytes(
            include_bytes!("../../assets/setup/DejaVuSans.ttf").to_vec(),
        )),
        mono: fonts.add(Font::from_bytes(
            include_bytes!("../../assets/setup/DejaVuSansMono.ttf").to_vec(),
        )),
        handwriting: fonts.add(Font::from_bytes(
            include_bytes!("../../assets/setup/kalam.ttf").to_vec(),
        )),
    });
}

fn label(art: &Art, text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font: art.body.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn heading(text: &str, art: &Art) -> impl Bundle {
    (
        Text::new(text),
        TextColor(CREAM),
        TextFont {
            font: art.handwriting.clone().into(),
            font_size: FontSize::Px(34.),
            ..default()
        },
        TextShadow::default(),
    )
}

fn frame(
    parent: &mut ChildSpawnerCommands,
    art: &Art,
    node: Node,
    content: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent.spawn(node).with_children(|parent| {
        // The frame has transparent outer corners. Keep both the fill and
        // texture inside its inner outline instead of backing the whole box.
        parent.spawn((
            PanelBackground,
            ImageNode {
                image: art.pattern.clone(),
                // Repeat through the sampler: Bevy's sliced/tiled image
                // pipeline does not clip textures to the node's corner radii.
                image_mode: NodeImageMode::Stretch,
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                left: px(4),
                right: px(4),
                top: px(4),
                bottom: px(4),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            BackgroundColor(BLUE),
            Pickable::IGNORE,
        ));
        parent.spawn((
            ImageNode {
                image: art.frame.clone(),
                image_mode: NodeImageMode::Sliced(TextureSlicer {
                    border: BorderRect::all(24.),
                    center_scale_mode: SliceScaleMode::Stretch,
                    sides_scale_mode: SliceScaleMode::Stretch,
                    max_corner_scale: 1.,
                }),
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
        ));
        content(parent);
    });
}

fn disc_name(name: &str) -> String {
    let letters: Vec<_> = name.chars().collect();
    if letters.len() <= 44 {
        return name.to_owned();
    }
    format!(
        "{}…{}",
        letters[..20].iter().collect::<String>(),
        letters[letters.len() - 20..].iter().collect::<String>()
    )
}

fn button(
    art: &Art,
    parent: &mut ChildSpawnerCommands,
    text: impl Into<String>,
    action: Action,
    enabled: bool,
    first: &mut Option<Entity>,
) {
    let mut button = parent.spawn((
        Button,
        action,
        Hovered::default(),
        TabIndex(0),
        Node {
            min_height: px(42),
            padding: UiRect::axes(px(18), px(10)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(4)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_shrink: 0.,
            ..default()
        },
        BackgroundColor(Color::srgb(0.23, 0.30, 0.41)),
        BorderColor::all(MUTED),
    ));
    if !enabled {
        button.insert(InteractionDisabled);
    }
    if enabled && (first.is_none() || matches!(action, Action::Start)) {
        *first = Some(button.id());
    }
    button.with_child((
        label(
            &art,
            text,
            17.,
            if enabled {
                CREAM
            } else {
                Color::srgb(0.5, 0.55, 0.62)
            },
        ),
        Pickable::IGNORE,
    ));
}

fn row(parent: &mut ChildSpawnerCommands, content: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(12),
            justify_content: JustifyContent::Center,
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(8),
            ..default()
        })
        .with_children(content);
}

fn card() -> Node {
    Node {
        width: px(610),
        max_width: percent(94),
        padding: UiRect::all(px(30)),
        flex_direction: FlexDirection::Column,
        row_gap: px(16),
        ..default()
    }
}

pub(super) fn rebuild(
    mut commands: Commands,
    mut state: ResMut<Setup>,
    art: Res<Art>,
    roots: Query<Entity, With<Root>>,
    mut focus: ResMut<InputFocus>,
) {
    if !state.rebuild {
        return;
    }
    state.rebuild = false;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let mut first = None;
    commands.spawn((Root, TabGroup::modal(), Node {
        width: percent(100), height: percent(100), padding: UiRect::all(px(24)),
        align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default()
    })).with_children(|root| {
        // Modal screens replace the controls beneath them: neither pointer nor
        // keyboard navigation can activate an obscured import/play button.
        match state.panel {
            Panel::Warning => {
                frame(root, &art, card(), |panel| {
                    panel.spawn(label(&art, "BEFORE YOU LOOK", 13., GOLD));
                    panel.spawn(heading("A word of warning", &art));
                    panel.spawn(label(&art, "The importer’s output includes character, location, and story asset names. Reading it may spoil events in Tales of Symphonia.", 18., CREAM));
                    panel.spawn(label(&art, "You can leave it hidden and enjoy the animation. Importing will continue either way.", 16., MUTED));
                    row(panel, |row| {
                        button(&art, row, "Keep hidden", Action::Hide, true, &mut first);
                        button(&art, row, "Show output · spoilers", Action::Reveal, true, &mut first);
                    });
                });
            }
            Panel::Output => {
                frame(root, &art, Node { width: percent(100), height: percent(92), max_width: px(1000), ..card() }, |panel| {
                    panel.spawn(heading("Importer output", &art));
                    panel.spawn((label(&art, "", 14., GOLD), StatusText));
                    panel.spawn(Node { flex_grow: 1., width: percent(100), min_height: px(0), overflow: Overflow::clip(),
                        padding: UiRect::all(px(12)), ..default() }).insert(LogViewport)
                        .insert(BackgroundColor(Color::srgb(0.08, 0.12, 0.18)))
                        .with_child(((Text::new(""), TextFont { font: art.mono.clone().into(), font_size: FontSize::Px(13.), ..default() }, TextColor(CREAM)), LineHeight::Px(18.), TextLayout::default().with_no_wrap(), LogText));
                    panel.spawn((label(&art, "", 12., MUTED), LogHint));
                    row(panel, |row| {
                        button(&art, row, "Back to the tree", Action::Hide, true, &mut first);
                        button(&art, row, "Follow latest", Action::Follow, true, &mut first);
                    });
                });
            }
            Panel::Hidden => match state.phase {
                Phase::Select => {
                    frame(root, &art, card(), |panel| {
                        panel.spawn((ImageNode::new(art.logo.clone()), Node { width: px(if state.reimport { 190 } else { 230 }), align_self: AlignSelf::Center, ..default() }));
                        panel.spawn(heading(if state.reimport { "Your world needs refreshing" } else { "Every journey begins with a seed" }, &art));
                        if state.extracted.is_some() {
                            if state.reimport {
                                panel.spawn(label(&art, "Your imported assets need an update for this version of Resonance. Re-import them to continue your journey.", 18., CREAM));
                            }
                            panel.spawn(label(&art, "Both extracted discs were found in ./local/extracted-discs. They will be used automatically.", 16., CREAM));
                            button(&art, panel, "Choose disc images instead", Action::ChooseDiscs, true, &mut first);
                        } else {
                            panel.spawn(label(&art, if state.reimport { "Your imported assets need an update for this version of Resonance. Choose both disc images to re-import them." } else { "Choose both Tales of Symphonia disc images to prepare your world." }, 18., CREAM));
                            for index in 0..2 {
                                let name = state.discs[index].as_ref().and_then(|p| p.file_name()).map(|n| disc_name(&n.to_string_lossy()));
                                button(&art, panel, format!("Disc {}   ·   {}", index + 1, name.as_deref().unwrap_or("Choose disc image…")), Action::Disc(index), state.picker.is_none(), &mut first);
                            }
                            panel.spawn(label(&art, "North American GameCube release · GQSEAF, revision 0\nRVZ, ISO, GCM, GCZ or WIA · You can also drop both images here.", 13., MUTED));
                        }
                        if !state.notice.is_empty() { panel.spawn(label(&art, &state.notice, 14., GOLD)); }
                        row(panel, |row| {
                            button(&art, row, if state.reimport { "◆  Re-import assets" } else { "◆  Begin your journey" }, Action::Start,
                                state.source().is_some() && state.picker.is_none(), &mut first);
                            if state.reimport { button(&art, row, "Quit", Action::Quit, true, &mut first); }
                        });
                    });
                }
                _ => {
                    let mut node = card();
                    node.width = px(860); node.position_type = PositionType::Absolute;
                    node.bottom = px(16); node.row_gap = px(8);
                    node.padding = UiRect::axes(px(24), px(12));
                    frame(root, &art, node, |panel| {
                        panel.spawn(Node { width: percent(100), justify_content: JustifyContent::SpaceBetween, column_gap: px(16), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                            row.spawn((label(&art, "", 16., CREAM), StatusText));
                            row.spawn((label(&art, "", 14., GOLD), PercentText));
                        });
                        panel.spawn(Node { width: percent(100), height: px(10), padding: UiRect::all(px(2)), ..default() })
                            .insert(BackgroundColor(Color::srgb(0.08, 0.12, 0.18)))
                            .with_child((ProgressFill, Node { height: percent(100), width: percent(0), ..default() }, BackgroundColor(GOLD)));
                        row(panel, |row| {
                            match state.phase {
                                Phase::Complete => button(&art, row, "◆  Play Resonance", Action::Play, true, &mut first),
                                Phase::Failed => button(&art, row, "Try again", Action::Retry, true, &mut first),
                                _ => {}
                            }
                            button(&art, row, "View import details", Action::Details, true, &mut first);
                            if state.phase == Phase::Importing { button(&art, row, "Stop import", Action::Stop, true, &mut first); }
                        });
                    });
                }
            }
        }
    });
    if let Some(first) = first {
        focus.set(first, bevy::input_focus::FocusCause::Navigated);
    } else {
        focus.clear();
    }
}

pub(super) fn refresh(
    state: Res<Setup>,
    mut nodes: Query<&mut Node, With<ProgressFill>>,
    mut backgrounds: Query<(&ComputedNode, &mut ImageNode), With<PanelBackground>>,
    viewport: Query<&ComputedNode, With<LogViewport>>,
    mut text: Query<(
        &mut Text,
        Has<StatusText>,
        Has<PercentText>,
        Has<LogText>,
        Has<LogHint>,
    )>,
) {
    for (node, mut image) in &mut backgrounds {
        let size = node.size() * node.inverse_scale_factor();
        if size.cmpgt(Vec2::ZERO).all() {
            let rect = Some(Rect::from_corners(Vec2::ZERO, size));
            if image.rect != rect {
                image.rect = rect;
            }
        }
    }
    for mut node in &mut nodes {
        node.width = percent(state.pacing.shown as f32 * 100.);
    }
    for (mut text, status, percentage, log, hint) in &mut text {
        let value = if status {
            state.stage.clone()
        } else if percentage {
            format!(
                "{}%{}",
                (state.pacing.shown * 100.).floor() as u32,
                if state.phase == Phase::Importing {
                    " estimated"
                } else {
                    ""
                }
            )
        } else if log && state.panel == Panel::Output && state.spoiler_consent {
            let end = state.log.len().saturating_sub(state.log_offset);
            let lines = viewport
                .iter()
                .next()
                .map(|n| ((n.size().y * n.inverse_scale_factor() - 24.) / 18.).max(1.) as usize)
                .unwrap_or(20);
            state
                .log
                .iter()
                .take(end)
                .skip(end.saturating_sub(lines))
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        } else if hint {
            format!(
                "{} · Scroll or Page Up / Page Down · End follows the latest output · showing up to 2,000 recent lines",
                if state.log_offset == 0 {
                    "Following latest"
                } else {
                    "Scroll paused"
                }
            )
        } else {
            continue;
        };
        if **text != value {
            **text = value;
        }
    }
}

pub(super) fn activate(
    event: On<Activate>,
    actions: Query<&Action>,
    mut state: ResMut<Setup>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    match *action {
        Action::Disc(index) => {
            state.select(index, Vec::new());
        }
        Action::ChooseDiscs => {
            state.extracted = None;
            state.notice.clear();
        }
        Action::Start => {
            let Some(source) = state.source() else {
                return;
            };
            match process::Job::start(&source, &state.assets, &state.mods, state.workers) {
                Ok(job) => {
                    state.job = Some(job);
                    state.phase = Phase::Importing;
                    state.pacing = pacing::Pacing::new(state.workers);
                    state.scene_seconds = 0.;
                    state.visit = 0;
                    state.stage = if state.extracted.is_some() {
                        "Preparing your extracted discs…"
                    } else {
                        "Opening your disc images…"
                    }
                    .into();
                    state.log.clear();
                    state.log_offset = 0;
                }
                Err(error) => state.failed(format!("{error:#}")),
            }
        }
        Action::Details => {
            state.panel = if state.spoiler_consent {
                Panel::Output
            } else {
                Panel::Warning
            }
        }
        Action::Hide => state.panel = Panel::Hidden,
        Action::Reveal => {
            state.spoiler_consent = true;
            state.panel = Panel::Output;
        }
        Action::Stop => {
            state.job.take();
            state.phase = Phase::Select;
            state.panel = Panel::Hidden;
            state.notice =
                "The import stopped. Begin again to replace the unfinished assets.".into();
        }
        Action::Retry => {
            state.phase = Phase::Select;
            state.panel = Panel::Hidden;
            state.notice.clear();
        }
        Action::Follow => state.log_offset = 0,
        Action::Play => {
            state.play.store(true, Ordering::Relaxed);
            exit.write(AppExit::Success);
        }
        Action::Quit => {
            exit.write(AppExit::Success);
        }
    }
    state.rebuild = true;
}

pub(super) fn button_appearance(
    focus: Res<InputFocus>,
    mut buttons: Query<
        (
            Entity,
            &Hovered,
            Has<Pressed>,
            Has<InteractionDisabled>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Button>,
    >,
) {
    for (entity, hovered, pressed, disabled, mut background, mut border) in &mut buttons {
        let highlighted = !disabled && (hovered.get() || focus.get() == Some(entity));
        background.0 = if pressed {
            Color::srgb(0.34, 0.39, 0.48)
        } else if highlighted {
            Color::srgb(0.32, 0.39, 0.50)
        } else {
            Color::srgb(0.23, 0.30, 0.41)
        };
        *border = BorderColor::all(if highlighted { GOLD } else { MUTED });
    }
}
