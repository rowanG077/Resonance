//! Cooked, vector-derived Kenney CC0 input prompts (256px per glyph).
use anyhow::Context;
use bevy::prelude::*;
use resonance_content::input::{AxisDirection, Keymap, PhysicalInput, StandardAction};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PromptDevice {
    Keyboard,
    #[default]
    Controller,
}
pub(crate) use resonance_content::input_prompts::Atlas;
#[cfg(test)]
pub(crate) fn test_image() -> &'static [u8] {
    include_bytes!("../../../resources/input-prompts/atlas.ktx2")
}
#[cfg(test)]
pub(crate) fn default_atlas() -> &'static Atlas {
    static ATLAS: std::sync::OnceLock<Atlas> = std::sync::OnceLock::new();
    ATLAS.get_or_init(|| {
        serde_json::from_slice(include_bytes!(
            "../../../resources/input-prompts/atlas.json"
        ))
        .unwrap()
    })
}
pub(crate) fn install(app: &mut App) {
    app.init_resource::<DeviceTracker>();
}
#[derive(Clone, Debug)]
pub(crate) struct Prepared {
    pub atlas: std::sync::Arc<Atlas>,
    pub image: Handle<Image>,
}
pub(crate) fn prepare(
    read: &impl Fn(&str) -> anyhow::Result<Vec<u8>>,
    server: &AssetServer,
) -> anyhow::Result<Prepared> {
    use resonance_content::input_prompts as prompts;
    let atlas: Atlas = serde_json::from_slice(
        &read(prompts::MANIFEST_PATH).with_context(|| prompts::MANIFEST_PATH)?,
    )
    .with_context(|| prompts::MANIFEST_PATH)?;
    atlas.validate()?;
    let image = server
        .load_builder()
        .with_settings(|settings: &mut bevy::image::ImageLoaderSettings| {
            settings.is_srgb = false;
            settings.sampler = bevy::image::ImageSampler::linear();
        })
        .load(prompts::IMAGE_PATH);
    Ok(Prepared {
        atlas: std::sync::Arc::new(atlas),
        image,
    })
}

#[derive(Resource, Default)]
pub(crate) struct DeviceTracker {
    device: PromptDevice,
    axes: BTreeMap<Entity, [StickActivity; 4]>,
}
#[derive(Clone, Copy, Default)]
struct StickActivity {
    anchor: f32,
    direction: i8,
}
impl StickActivity {
    fn sample(&mut self, value: f32) -> bool {
        // Prompt switching needs a wider neutral band than movement itself.
        // A moderate walking tilt still counts, but noise around the gameplay
        // dead zone (0.2) must not repeatedly activate the controller.
        const ACTIVATE: f32 = 0.3;
        const RELEASE: f32 = 0.15;
        const MOTION: f32 = 0.1;
        if value.abs() <= RELEASE {
            *self = Self::default();
            return false;
        }
        let direction = if value < 0. { -1 } else { 1 };
        if value.abs() > ACTIVATE && direction != self.direction {
            self.anchor = value;
            self.direction = direction;
            return true;
        }
        if direction != self.direction {
            return false;
        }
        let outward = value.abs() - self.anchor.abs() >= MOTION;
        // Accumulate slow movement; inward movement only updates the anchor.
        if (value - self.anchor).abs() >= MOTION {
            self.anchor = value;
        }
        value.abs() > ACTIVATE && outward
    }
}
impl DeviceTracker {
    pub(crate) fn device(&self) -> PromptDevice {
        self.device
    }
    fn sample<'a>(
        &mut self,
        keys: &ButtonInput<KeyCode>,
        pads: impl IntoIterator<Item = (Entity, &'a Gamepad)>,
        configured: &BTreeSet<PhysicalInput>,
    ) {
        let overlay = keys.pressed(KeyCode::Tab)
            && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight));
        let keyboard = keys.get_just_pressed().any(|key| {
            !(overlay && matches!(key, KeyCode::Tab | KeyCode::ShiftLeft | KeyCode::ShiftRight))
                && configured.contains(&PhysicalInput::Key(format!("{key:?}")))
        });
        let mut controller = false;
        let mut connected = BTreeSet::new();
        for (entity, pad) in pads {
            connected.insert(entity);
            if pad
                .get_just_pressed()
                .any(|button| configured.contains(&PhysicalInput::Button(format!("{button:?}"))))
            {
                controller = true;
            }
            let left = pad.left_stick();
            let right = pad.right_stick();
            let values = [left.x, left.y, right.x, right.y];
            let previous = self.axes.entry(entity).or_default();
            use AxisDirection::*;
            for (index, [negative, positive]) in [
                [LeftLeft, LeftRight],
                [LeftDown, LeftUp],
                [RightLeft, RightRight],
                [RightDown, RightUp],
            ]
            .into_iter()
            .enumerate()
            {
                let value = values[index];
                let input = PhysicalInput::Axis(if value < 0. { negative } else { positive });
                if previous[index].sample(value) && configured.contains(&input) {
                    controller = true;
                }
            }
        }
        self.axes.retain(|entity, _| connected.contains(entity));
        // Bevy's keyboard and gamepad batches have no shared event timestamp.
        // An ambiguous frame retains the current device instead of giving
        // either device unconditional priority.
        if keyboard != controller {
            self.device = if keyboard {
                PromptDevice::Keyboard
            } else {
                PromptDevice::Controller
            };
        }
    }
}
fn configured_inputs<'a>(
    map: &Keymap,
    chords: impl IntoIterator<Item = &'a resonance_content::input::InputChord>,
) -> BTreeSet<PhysicalInput> {
    map.bindings
        .iter()
        .flatten()
        .cloned()
        .chain(
            chords
                .into_iter()
                .filter(|chord| chord.validate(map).is_ok())
                .flat_map(|chord| chord.0.iter().cloned()),
        )
        .collect()
}

pub(crate) fn track_device(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<(Entity, &Gamepad)>,
    session: Option<Res<crate::new_game::Session>>,
    title: Option<Res<crate::saves::title::TitleMenu>>,
    battle: Option<Res<crate::battle::Owner>>,
    preferences: Option<Res<crate::config::Preferences>>,
    mut tracker: ResMut<DeviceTracker>,
) {
    let party = battle
        .as_ref()
        .and_then(|owner| owner.prompt_party())
        .or_else(|| {
            session
                .as_ref()
                .and_then(|session| session.events().world.party.as_ref())
        });
    let menu = title
        .as_ref()
        .map(|title| &title.0)
        .or_else(|| session.as_ref().and_then(|session| session.menu()));
    let map = crate::input::active_keymap(menu, party, preferences.as_deref());
    let chords = party
        .into_iter()
        .flat_map(|party| &party.members)
        .flat_map(|member| {
            member
                .arte_bindings
                .iter()
                .filter(|binding| member.techniques.contains(&binding.technique))
                .map(|binding| &binding.chord)
        });
    let configured = configured_inputs(&map, chords);
    tracker.sample(&keys, pads.iter(), &configured);
}
pub(crate) fn action_input(
    map: &Keymap,
    action: StandardAction,
    device: PromptDevice,
) -> Option<&PhysicalInput> {
    map.get(action)
        .iter()
        .find(|i| i.keyboard() == (device == PromptDevice::Keyboard))
        .or_else(|| map.get(action).first())
}
pub(crate) fn action_rect_state(
    atlas: &Atlas,
    map: &Keymap,
    action: StandardAction,
    pressed: bool,
    device: PromptDevice,
) -> [u32; 4] {
    action_input(map, action, device)
        .map(|input| rect_state(atlas, input, pressed))
        .unwrap_or_else(|| named_state(atlas, "button", pressed))
}
pub(crate) fn named_state(atlas: &Atlas, name: &str, pressed: bool) -> [u32; 4] {
    if pressed && let Some(rect) = atlas.pressed.get(name) {
        return *rect;
    }
    atlas
        .icons
        .get(name)
        .copied()
        .unwrap_or(atlas.icons["button"])
}
#[cfg(test)]
pub(crate) fn rect(atlas: &Atlas, input: &PhysicalInput) -> [u32; 4] {
    rect_state(atlas, input, false)
}
pub(crate) fn rect_state(atlas: &Atlas, input: &PhysicalInput, pressed: bool) -> [u32; 4] {
    let name = name(input);
    named_state(
        atlas,
        if atlas.icons.contains_key(&name) {
            &name
        } else {
            "button"
        },
        pressed,
    )
}
pub(crate) fn short_label(input: &PhysicalInput) -> String {
    match input {
        PhysicalInput::Button(button) => button
            .strip_prefix("Other(")
            .and_then(|n| n.strip_suffix(')'))
            .map(|n| format!("B{n}"))
            .unwrap_or_else(|| button.clone()),
        _ => input.label(),
    }
}
pub(crate) fn name(input: &PhysicalInput) -> String {
    match input {
        PhysicalInput::Key(key) => {
            let name = match key.as_str() {
                "Enter" | "NumpadEnter" => "enter",
                "Escape" => "escape",
                "Space" => "space",
                "Tab" => "tab",
                "ArrowLeft" => "left",
                "ArrowRight" => "right",
                "ArrowUp" => "up",
                "ArrowDown" => "down",
                "Home" => "home",
                "PageUp" => "page_up",
                "PageDown" => "page_down",
                "ShiftLeft" | "ShiftRight" => "shift",
                "ControlLeft" | "ControlRight" => "control",
                "AltLeft" | "AltRight" => "alt",
                "Backspace" => "backspace",
                "Delete" => "delete",
                "Insert" => "insert",
                "BracketLeft" => "bracket_left",
                "BracketRight" => "bracket_right",
                "Minus" => "minus",
                "Equal" => "equal",
                "Comma" => "comma",
                "Period" => "period",
                "Semicolon" => "semicolon",
                "Slash" => "slash",
                "Backslash" => "backslash",
                "Quote" => "quote",
                "Backquote" => "backquote",
                "SuperLeft" | "SuperRight" => "win",
                "Fn" => "function",
                "NumpadAdd" => "plus",
                "NumpadSubtract" => "minus",
                "NumpadMultiply" => "asterisk",
                "NumpadDivide" => "slash",
                "NumpadDecimal" => "period",
                other => {
                    return format!(
                        "key_{}",
                        other
                            .strip_prefix("Key")
                            .or_else(|| other.strip_prefix("Digit"))
                            .or_else(|| other.strip_prefix("Numpad"))
                            .unwrap_or(other)
                            .to_lowercase()
                    );
                }
            };
            format!("key_{name}")
        }
        PhysicalInput::Button(button) => match button.as_str() {
            "South" => "pad_a",
            "East" => "pad_b",
            "West" => "pad_x",
            "North" => "pad_y",
            // Extra C/Z buttons use the same labeled blue/gold generic caps;
            // they must not fall back to an unlabeled or native controller picture.
            "C" => "key_c",
            "Z" => "key_z",
            "DPadLeft" => "pad_dpad_left",
            "DPadRight" => "pad_dpad_right",
            "DPadUp" => "pad_dpad_up",
            "DPadDown" => "pad_dpad_down",
            "LeftTrigger" => "pad_l1",
            "RightTrigger" => "pad_r1",
            "LeftTrigger2" => "pad_l2",
            "RightTrigger2" => "pad_r2",
            "LeftThumb" => "pad_l3",
            "RightThumb" => "pad_r3",
            "Start" => "pad_start",
            "Select" => "pad_back",
            _ => "button",
        }
        .into(),
        PhysicalInput::Axis(direction) => match direction {
            AxisDirection::LeftUp => "stick_l_up",
            AxisDirection::RightUp => "stick_r_up",
            AxisDirection::LeftDown => "stick_l_down",
            AxisDirection::RightDown => "stick_r_down",
            AxisDirection::LeftLeft => "stick_l_left",
            AxisDirection::RightLeft => "stick_r_left",
            AxisDirection::LeftRight => "stick_l_right",
            AxisDirection::RightRight => "stick_r_right",
        }
        .into(),
    }
}
pub(crate) fn has_specific_icon(atlas: &Atlas, input: &PhysicalInput) -> bool {
    atlas.icons.contains_key(&name(input))
        && !matches!(input, PhysicalInput::Button(_) if name(input) == "button")
}

pub(crate) struct PromptQuad {
    pub rect: [f32; 4],
    pub uv: [u32; 4],
    pub input: Option<PhysicalInput>,
    pub pressed: bool,
}
/// Resolve script glyphs at draw time, including remaps and the active device.
/// A keyboard direction group consists of individual configured keys, never a
/// baked controller picture or a grouped-arrow key graphic.
pub(crate) fn controller_quads(
    atlas: &Atlas,
    map: &Keymap,
    id: usize,
    [left, top, right, bottom]: [f32; 4],
    pressed_frame: bool,
    device: PromptDevice,
) -> anyhow::Result<Vec<PromptQuad>> {
    use resonance_content::input_prompts::ControllerGlyph as G;
    let glyph = resonance_content::input_prompts::controller_glyph(id)
        .ok_or_else(|| anyhow::anyhow!("unknown controller glyph {id}"))?;
    let rect = [left, top, right, bottom];
    let icon = |input: PhysicalInput, rect, pressed| PromptQuad {
        uv: rect_state(atlas, &input, pressed),
        rect,
        input: Some(input),
        pressed,
    };
    if let G::Action(action, pressed) = glyph {
        return Ok(action_input(map, action, device)
            .map(|input| icon(input.clone(), rect, pressed && pressed_frame))
            .into_iter()
            .collect());
    }
    let actions: &[StandardAction] = match glyph {
        G::Movement => &[
            StandardAction::MoveUp,
            StandardAction::MoveLeft,
            StandardAction::MoveDown,
            StandardAction::MoveRight,
        ],
        G::Camera => &[
            StandardAction::CameraUp,
            StandardAction::CameraLeft,
            StandardAction::CameraDown,
            StandardAction::CameraRight,
        ],
        G::Orders => &[
            StandardAction::OrderUp,
            StandardAction::OrderLeft,
            StandardAction::OrderCancel,
            StandardAction::OrderRight,
        ],
        G::Horizontal => &[StandardAction::MoveLeft, StandardAction::MoveRight],
        G::Panel => anyhow::bail!("window ornament is not an input prompt"),
        G::Action(..) => unreachable!(),
    };
    let inputs: Vec<_> = actions
        .iter()
        .filter_map(|&action| action_input(map, action, device))
        .collect();
    if !inputs.is_empty() && inputs.iter().all(|i| matches!(i, PhysicalInput::Axis(_))) {
        return Ok(vec![PromptQuad {
            rect,
            uv: named_state(
                atlas,
                if glyph == G::Horizontal {
                    "stick_horizontal"
                } else {
                    "stick"
                },
                glyph == G::Horizontal && pressed_frame,
            ),
            input: None,
            pressed: glyph == G::Horizontal && pressed_frame,
        }]);
    }
    let size = ((right - left) / if actions.len() == 2 { 2. } else { 3. }).min((bottom - top) / 2.);
    let x = left + ((right - left) - size * if actions.len() == 2 { 2. } else { 3. }) / 2.;
    let y = top + ((bottom - top) - size * 2.) / 2.;
    Ok(actions
        .iter()
        .enumerate()
        .filter_map(|(index, &action)| {
            let input = action_input(map, action, device)?;
            let [dx, dy] = if actions.len() == 2 {
                [index as f32, 0.5]
            } else {
                [[1., 0.], [0., 1.], [1., 1.], [2., 1.]][index]
            };
            let [x, y] = [x + dx * size, y + dy * size];
            Some(icon(input.clone(), [x, y, x + size, y + size], false))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_and_inline_glyphs_resolve_remapped_icons_and_pressed_frames() -> anyhow::Result<()> {
        use resonance_content::input_prompts::{ControllerGlyph, controller_glyph};
        let mut atlas = default_atlas().clone();
        let mut map = Keymap::default();
        // Deterministic keyboard prompts, independent of the active device.
        for row in &mut map.bindings {
            row.retain(PhysicalInput::keyboard);
        }
        for id in 0..39 {
            let glyph = controller_glyph(id).unwrap();
            if glyph == ControllerGlyph::Panel {
                continue;
            }
            let quads = controller_quads(
                &atlas,
                &map,
                id,
                [0., 0., 24., 24.],
                true,
                PromptDevice::Controller,
            )?;
            assert!(!quads.is_empty(), "missing glyph {id}");
            for quad in &quads {
                assert!(
                    quad.rect[0] >= 0.
                        && quad.rect[1] >= 0.
                        && quad.rect[2] <= 24.
                        && quad.rect[3] <= 24.
                );
            }
            if let ControllerGlyph::Action(action, pressed) = glyph {
                let input = &map.get(action)[0];
                assert_eq!(quads.len(), 1);
                assert_eq!(quads[0].uv, rect_state(&atlas, input, pressed));
                assert_eq!(
                    controller_quads(
                        &atlas,
                        &map,
                        id,
                        [0., 0., 24., 24.],
                        false,
                        PromptDevice::Controller
                    )?[0]
                        .uv,
                    rect_state(&atlas, input, false)
                );
            }
        }
        for (id, action) in [
            (28, StandardAction::OrderUp),
            (29, StandardAction::OrderCancel),
            (30, StandardAction::OrderRight),
            (31, StandardAction::OrderLeft),
        ] {
            assert_eq!(
                controller_glyph(id),
                Some(ControllerGlyph::Action(action, false))
            );
        }
        let key = PhysicalInput::Key("F11".into());
        map.assign(StandardAction::Confirm, key.clone());
        atlas.icons.insert("key_f11".into(), [12, 24, 256, 256]);
        atlas.pressed.insert("key_f11".into(), [300, 24, 256, 256]);
        assert_eq!(
            controller_quads(
                &atlas,
                &map,
                5,
                [0., 0., 24., 24.],
                true,
                PromptDevice::Controller
            )?[0]
                .uv,
            [12, 24, 256, 256]
        );
        assert_eq!(
            controller_quads(
                &atlas,
                &map,
                6,
                [0., 0., 24., 24.],
                true,
                PromptDevice::Controller
            )?[0]
                .uv,
            [300, 24, 256, 256]
        );
        assert!(
            controller_quads(
                &atlas,
                &map,
                39,
                [0., 0., 24., 24.],
                true,
                PromptDevice::Controller
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn cooked_atlas_covers_defaults_at_high_resolution() {
        let atlas = default_atlas();
        let map = Keymap::default();
        for input in map.bindings.iter().flatten() {
            assert!(
                atlas.icons.contains_key(&name(input)),
                "missing icon: {input:?}"
            );
            assert!(
                has_specific_icon(atlas, input),
                "unlabeled default input: {input:?}"
            );
            assert_eq!(rect(atlas, input)[2..], [256, 256]);
        }
        assert_eq!(&test_image()[..12], b"\xabKTX 20\xbb\r\n\x1a\n");
    }

    #[test]
    fn physical_buttons_use_canonical_labels_without_atlas_aliases() {
        for (button, icon) in [
            ("South", "pad_a"),
            ("East", "pad_b"),
            ("West", "pad_x"),
            ("North", "pad_y"),
            ("C", "key_c"),
            ("Z", "key_z"),
            ("LeftTrigger", "pad_l1"),
            ("RightTrigger", "pad_r1"),
            ("LeftTrigger2", "pad_l2"),
            ("RightTrigger2", "pad_r2"),
            ("LeftThumb", "pad_l3"),
            ("RightThumb", "pad_r3"),
            ("Start", "pad_start"),
            ("Select", "pad_back"),
        ] {
            let input = PhysicalInput::Button(button.into());
            assert_eq!(name(&input), icon);
            assert!(has_specific_icon(default_atlas(), &input));
        }
        let unknown = PhysicalInput::Button("Other(42)".into());
        assert!(!has_specific_icon(default_atlas(), &unknown));
        assert_eq!(short_label(&unknown), "B42");
    }
}

#[cfg(test)]
mod loading_tests {
    use super::*;
    use resonance_content::input_prompts as prompts;

    fn image_app(root: &std::path::Path) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
                ..default()
            },
        ))
        .init_asset::<Image>()
        .register_asset_loader(bevy::image::ImageLoader::new(
            bevy::image::CompressedImageFormats::NONE,
        ));
        app
    }

    #[test]
    fn missing_manifest_reports_the_path() {
        let app = image_app(std::path::Path::new("assets"));
        let error = prepare(
            &|_| anyhow::bail!("absent"),
            app.world().resource::<AssetServer>(),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains(prompts::MANIFEST_PATH));
    }

    #[test]
    fn cooked_manifest_changes_prompt_rectangles() -> anyhow::Result<()> {
        let app = image_app(std::path::Path::new("assets"));
        let server = app.world().resource::<AssetServer>();
        let mut modified = default_atlas().clone();
        modified.icons.insert("key_z".into(), [0, 0, 512, 512]);
        let loaded = prepare(
            &|path| match path {
                prompts::MANIFEST_PATH => Ok(serde_json::to_vec(&modified)?),
                _ => anyhow::bail!("presentation preparation must only read the layout"),
            },
            server,
        )?;
        assert_eq!(
            server.get_path(loaded.image.id()).unwrap().to_string(),
            prompts::IMAGE_PATH
        );
        let z = PhysicalInput::Key("KeyZ".into());
        assert_ne!(rect(default_atlas(), &z), rect(&loaded.atlas, &z));
        assert_eq!(rect(&loaded.atlas, &z), [0, 0, 512, 512]);
        Ok(())
    }

    #[test]
    fn the_normal_image_loader_decodes_textures_and_reports_missing_or_corrupt_files()
    -> anyhow::Result<()> {
        for (bytes, valid) in [
            (Some(test_image()), true),
            (Some(b"invalid KTX2".as_slice()), false),
            (None, false),
        ] {
            let root = tempfile::tempdir()?;
            let image_path = root.path().join(prompts::IMAGE_PATH);
            std::fs::create_dir_all(image_path.parent().unwrap())?;
            if let Some(bytes) = bytes {
                std::fs::write(&image_path, bytes)?;
            }
            std::fs::write(
                root.path().join(prompts::MANIFEST_PATH),
                serde_json::to_vec(default_atlas())?,
            )?;
            let mut app = image_app(root.path());
            // Preparation reads only the layout. The image loader owns the
            // texture read, decoding and any texture errors.
            let prepared = prepare(
                &|path| Ok(std::fs::read(root.path().join(path))?),
                app.world().resource::<AssetServer>(),
            )?;
            let mut finished = false;
            for _ in 0..1000 {
                app.update();
                if let Some(image) = app.world().resource::<Assets<Image>>().get(&prepared.image) {
                    assert!(valid);
                    assert_eq!([image.width(), image.height()], prepared.atlas.size);
                    finished = true;
                    break;
                }
                if matches!(
                    app.world()
                        .resource::<AssetServer>()
                        .get_load_state(prepared.image.id()),
                    Some(bevy::asset::LoadState::Failed(_))
                ) {
                    assert!(!valid);
                    finished = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(finished, "image loader did not finish");
        }
        Ok(())
    }
}

#[cfg(test)]
mod device_tests {
    use super::*;
    use resonance_content::input::InputChord;

    #[test]
    fn separate_apps_keep_independent_prompt_devices_and_rendered_icons() {
        let make_app = || {
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<DeviceTracker>()
                .add_systems(Update, track_device);
            app
        };
        let mut keyboard_app = make_app();
        let mut controller_app = make_app();
        keyboard_app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        keyboard_app.update();
        let entity = controller_app.world_mut().spawn(Gamepad::default()).id();
        controller_app
            .world_mut()
            .entity_mut(entity)
            .get_mut::<Gamepad>()
            .unwrap()
            .digital_mut()
            .press(GamepadButton::South);
        controller_app.update();
        let map = Keymap::default();
        let keyboard = keyboard_app.world().resource::<DeviceTracker>().device();
        let controller = controller_app.world().resource::<DeviceTracker>().device();
        assert_eq!(keyboard, PromptDevice::Keyboard);
        assert_eq!(controller, PromptDevice::Controller);
        assert_eq!(
            action_input(&map, StandardAction::Confirm, keyboard),
            Some(&PhysicalInput::Key("Enter".into()))
        );
        assert_eq!(
            action_input(&map, StandardAction::Confirm, controller),
            Some(&PhysicalInput::Button("South".into()))
        );
        assert_ne!(
            action_rect_state(
                default_atlas(),
                &map,
                StandardAction::Confirm,
                false,
                keyboard
            ),
            action_rect_state(
                default_atlas(),
                &map,
                StandardAction::Confirm,
                false,
                controller
            ),
        );
        keyboard_app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        keyboard_app.update();
        assert_eq!(
            controller_app.world().resource::<DeviceTracker>().device(),
            controller
        );
    }

    #[test]
    fn only_configured_presses_switch_device_and_holding_does_not_reclaim_it() {
        let map = Keymap::default();
        let configured = configured_inputs(&map, []);
        let mut tracker = DeviceTracker::default();
        let mut keys = ButtonInput::default();
        let mut pad = Gamepad::default();
        let entity = Entity::from_bits(1);
        assert!(
            tracker.device == PromptDevice::Controller,
            "controller is the default until a valid keyboard press"
        );
        keys.press(KeyCode::F24);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(tracker.device == PromptDevice::Controller);
        keys.clear();
        keys.press(KeyCode::Enter);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(tracker.device == PromptDevice::Keyboard);
        keys.clear();
        pad.digital_mut().press(GamepadButton::Other(27));
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(tracker.device == PromptDevice::Keyboard);
        pad.digital_mut().clear();
        pad.digital_mut().press(GamepadButton::South);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(tracker.device == PromptDevice::Controller);
        pad.digital_mut().clear();
        keys.release(KeyCode::F24);
        keys.press(KeyCode::F24);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(
            tracker.device == PromptDevice::Controller,
            "unassigned keys cannot take prompts from the controller"
        );
        keys.clear();
        keys.release(KeyCode::Enter);
        keys.press(KeyCode::Enter);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(
            tracker.device == PromptDevice::Keyboard,
            "a held controller button is not a new input"
        );
        keys.clear();
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(tracker.device == PromptDevice::Keyboard);
    }

    #[test]
    fn remaps_and_valid_arte_assignment_components_define_eligible_inputs() {
        let mut map = Keymap::default();
        map.assign(StandardAction::Confirm, PhysicalInput::Key("KeyJ".into()));
        let valid = InputChord(vec![
            PhysicalInput::Key("F11".into()),
            PhysicalInput::Key("KeyJ".into()),
        ]);
        let invalid = InputChord(vec![
            PhysicalInput::Key("F12".into()),
            PhysicalInput::Key("F12".into()),
        ]);
        let configured = configured_inputs(&map, [&valid, &invalid]);
        assert!(!configured.contains(&PhysicalInput::Key("Enter".into())));
        assert!(!configured.contains(&PhysicalInput::Key("Space".into())));
        assert!(configured.contains(&PhysicalInput::Key("F11".into())));
        assert!(!configured.contains(&PhysicalInput::Key("F12".into())));
        let mut tracker = DeviceTracker::default();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Enter);
        tracker.sample(&keys, [], &configured);
        assert!(tracker.device == PromptDevice::Controller);
        keys.clear();
        keys.press(KeyCode::F11);
        tracker.sample(&keys, [], &configured);
        assert!(tracker.device == PromptDevice::Keyboard);
    }

    #[test]
    fn moderate_configured_stick_motion_counts_but_holding_noise_and_return_do_not() {
        let configured = configured_inputs(&Keymap::default(), []);
        let mut tracker = DeviceTracker::default();
        let mut keys = ButtonInput::default();
        let mut pad = Gamepad::default();
        let entity = Entity::from_bits(1);
        keys.press(KeyCode::Enter);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(tracker.device == PromptDevice::Keyboard);
        keys.clear();
        pad.analog_mut().set(GamepadAxis::LeftStickX, 0.35);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(
            tracker.device == PromptDevice::Controller,
            "walking-strength tilt is a configured input"
        );
        keys.press(KeyCode::KeyA);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(tracker.device == PromptDevice::Keyboard);
        keys.clear();
        for value in [0.35, 0.37, 0.34, 0.22, 0.1, 0.] {
            pad.analog_mut().set(GamepadAxis::LeftStickX, value);
            tracker.sample(&keys, [(entity, &pad)], &configured);
            assert!(
                tracker.device == PromptDevice::Keyboard,
                "stick holding/noise/return at {value} must not reclaim prompts"
            );
        }
        pad.analog_mut().set(GamepadAxis::LeftStickX, 0.35);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(tracker.device == PromptDevice::Controller);
        tracker.sample(&keys, [], &configured);
        assert!(
            tracker.axes.is_empty(),
            "disconnected devices leave no stale anchors"
        );
    }

    #[test]
    fn only_the_configured_axis_direction_counts_and_slow_movement_accumulates() {
        let configured = [
            PhysicalInput::Key("Enter".into()),
            PhysicalInput::Axis(AxisDirection::LeftLeft),
        ]
        .into();
        let mut tracker = DeviceTracker::default();
        let mut keys = ButtonInput::default();
        let mut pad = Gamepad::default();
        let entity = Entity::from_bits(1);
        keys.press(KeyCode::Enter);
        pad.analog_mut().set(GamepadAxis::LeftStickX, 0.8);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(
            tracker.device == PromptDevice::Keyboard,
            "the unconfigured opposite direction must be ignored"
        );
        keys.clear();
        pad.analog_mut().set(GamepadAxis::LeftStickX, -0.35);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(tracker.device == PromptDevice::Controller);
        keys.release(KeyCode::Enter);
        keys.press(KeyCode::Enter);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(tracker.device == PromptDevice::Keyboard);
        keys.clear();
        for value in [-0.37, -0.40, -0.43] {
            pad.analog_mut().set(GamepadAxis::LeftStickX, value);
            tracker.sample(&keys, [(entity, &pad)], &configured);
            assert!(tracker.device == PromptDevice::Keyboard);
        }
        pad.analog_mut().set(GamepadAxis::LeftStickX, -0.47);
        tracker.sample(&keys, [(entity, &pad)], &configured);
        assert!(
            tracker.device == PromptDevice::Controller,
            "slow intentional movement must not disappear between frames"
        );
    }

    #[test]
    fn platform_overlay_shortcut_does_not_switch_the_prompt_device() {
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::Tab);
        let mut tracker = DeviceTracker::default();
        tracker.sample(&keys, [], &configured_inputs(&Keymap::default(), []));
        assert!(tracker.device == PromptDevice::Controller);
    }

    #[test]
    fn stick_dead_zone_chatter_does_not_switch_before_or_after_a_deliberate_tilt() {
        let configured = configured_inputs(&Keymap::default(), []);
        for axis in [
            GamepadAxis::LeftStickX,
            GamepadAxis::LeftStickY,
            GamepadAxis::RightStickX,
            GamepadAxis::RightStickY,
        ] {
            for sign in [-1., 1.] {
                let mut tracker = DeviceTracker::default();
                let mut keys = ButtonInput::default();
                let mut pad = Gamepad::default();
                let entity = Entity::from_bits(1);
                keys.press(KeyCode::Enter);
                tracker.sample(&keys, [(entity, &pad)], &configured);
                keys.clear();
                for value in [0.19, 0.21, 0.18, 0.22, 0.29] {
                    pad.analog_mut().set(axis, sign * value);
                    tracker.sample(&keys, [(entity, &pad)], &configured);
                    assert!(
                        tracker.device == PromptDevice::Keyboard,
                        "idle chatter on {axis:?}: {value}"
                    );
                }
                pad.analog_mut().set(axis, sign * 0.35);
                tracker.sample(&keys, [(entity, &pad)], &configured);
                assert!(
                    tracker.device == PromptDevice::Controller,
                    "walking-strength tilt on {axis:?}"
                );
                keys.release(KeyCode::Enter);
                keys.press(KeyCode::Enter);
                tracker.sample(&keys, [(entity, &pad)], &configured);
                assert!(tracker.device == PromptDevice::Keyboard);
                keys.clear();
                for value in [0.21, 0.19, 0.21, 0.18, 0.22, 0.29, 0., 0.21] {
                    pad.analog_mut().set(axis, sign * value);
                    tracker.sample(&keys, [(entity, &pad)], &configured);
                    assert!(
                        tracker.device == PromptDevice::Keyboard,
                        "return chatter on {axis:?}: {value}"
                    );
                }
                pad.analog_mut().set(axis, sign * 0.35);
                tracker.sample(&keys, [(entity, &pad)], &configured);
                assert!(
                    tracker.device == PromptDevice::Controller,
                    "fresh tilt on {axis:?}"
                );
            }
        }
    }

    #[test]
    fn simultaneous_device_activity_preserves_the_current_device() {
        let configured = configured_inputs(&Keymap::default(), []);
        for keyboard in [false, true] {
            for stick in [false, true] {
                let mut tracker = DeviceTracker {
                    device: if keyboard {
                        PromptDevice::Keyboard
                    } else {
                        PromptDevice::Controller
                    },
                    ..default()
                };
                let mut keys = ButtonInput::default();
                let mut pad = Gamepad::default();
                keys.press(KeyCode::Enter);
                if stick {
                    pad.analog_mut().set(GamepadAxis::LeftStickX, 0.5);
                } else {
                    pad.digital_mut().press(GamepadButton::South);
                }
                tracker.sample(&keys, [(Entity::from_bits(1), &pad)], &configured);
                assert_eq!(tracker.device == PromptDevice::Keyboard, keyboard);
                keys.clear();
                pad.digital_mut().clear();
                tracker.sample(&keys, [(Entity::from_bits(1), &pad)], &configured);
                assert_eq!(
                    tracker.device == PromptDevice::Keyboard,
                    keyboard,
                    "holding cannot break the tie"
                );
                keys.press(KeyCode::KeyA);
                tracker.sample(&keys, [(Entity::from_bits(1), &pad)], &configured);
                assert!(
                    tracker.device == PromptDevice::Keyboard,
                    "subsequent keyboard input wins"
                );
                keys.clear();
                pad.digital_mut().press(GamepadButton::East);
                tracker.sample(&keys, [(Entity::from_bits(1), &pad)], &configured);
                assert!(
                    tracker.device == PromptDevice::Controller,
                    "subsequent controller input wins"
                );
            }
        }
    }

    #[test]
    fn title_draft_and_fixed_editor_controls_drive_navigation_and_prompt_switching() {
        use resonance_game::menu::{Menu, Page, customize::Focus};
        for (focus, key, accepted) in [
            (Focus::Options, KeyCode::KeyJ, true),
            (Focus::Options, KeyCode::Enter, false),
            (Focus::Controls, KeyCode::KeyJ, false),
            (Focus::Controls, KeyCode::Enter, true),
        ] {
            // Saved preferences still use Enter; the title draft has remapped it.
            let mut menu = Menu::new(Page::Customize, None, false);
            menu.customize.focus = focus;
            menu.customize
                .draft
                .keymap
                .assign(StandardAction::Confirm, PhysicalInput::Key("KeyJ".into()));
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<DeviceTracker>()
                .init_resource::<crate::field_view::Controls>()
                .init_resource::<crate::config::Preferences>()
                .insert_resource(crate::saves::title::TitleMenu(menu))
                .add_systems(
                    Update,
                    (track_device, crate::field_view::gather_controls).chain(),
                );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            assert_eq!(
                app.world().resource::<DeviceTracker>().device == PromptDevice::Keyboard,
                accepted,
                "prompt device for {focus:?} / {key:?}"
            );
            assert_eq!(
                app.world_mut()
                    .resource_mut::<crate::field_view::Controls>()
                    .consume()
                    .pressed(resonance_events::input::Button::Accept),
                accepted,
                "navigation for {focus:?} / {key:?}"
            );
        }
    }

    #[test]
    fn one_icon_uses_the_preferred_device_with_a_fallback_for_single_device_actions() {
        let mut map = Keymap::default();
        map.assign(StandardAction::Confirm, PhysicalInput::Key("F11".into()));
        map.assign(
            StandardAction::Confirm,
            PhysicalInput::Button("RightTrigger2".into()),
        );
        assert_eq!(
            action_input(&map, StandardAction::Confirm, PromptDevice::Keyboard),
            Some(&PhysicalInput::Key("F11".into()))
        );
        assert_eq!(
            action_input(&map, StandardAction::Confirm, PromptDevice::Controller),
            Some(&PhysicalInput::Button("RightTrigger2".into()))
        );
        map.bindings[StandardAction::Confirm as usize].retain(PhysicalInput::keyboard);
        assert_eq!(
            action_input(&map, StandardAction::Confirm, PromptDevice::Controller),
            Some(&PhysicalInput::Key("F11".into()))
        );
        map.bindings[StandardAction::Confirm as usize] =
            vec![PhysicalInput::Button("South".into())];
        assert_eq!(
            action_input(&map, StandardAction::Confirm, PromptDevice::Keyboard),
            Some(&PhysicalInput::Button("South".into()))
        );
    }
}
