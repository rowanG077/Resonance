#[path = "common/action.rs"]
mod action;
use action::Action::{self, *};
use resonance_content::{menu_data::*, options};
use resonance_game::menu::{Menu, customize::Focus};

fn data() -> CustomizeData {
    let defaults = CustomizeSettings::default();
    let options: Vec<_> = options::OPTIONS
        .iter()
        .map(|name| CustomizeOption {
            name: (*name).into(),
            description: String::new(),
        })
        .collect();
    CustomizeData {
        options,
        difficulties: ["Normal", "Hard", "Mania"].map(String::from),
        actions: options::ACTIONS.map(String::from),
        color_groups: options::COLOR_GROUPS.map(String::from),
        volume_channels: options::VOLUME_CHANNELS.map(String::from),
        themes: std::array::from_fn(|_| defaults.colors.clone()),
        defaults,
        control_buttons: options::control_buttons(),
        labels: [
            "cancel",
            "default",
            "cancel_help",
            "default_help",
            "on",
            "off",
            "stereo",
            "mono",
            "color",
            "volume",
        ]
        .map(|key| (key.into(), key.into()))
        .into(),
        confirmations: options::CONFIRMATIONS
            .map(|key| (key.into(), key.into()))
            .into(),
    }
}
fn press(menu: &mut Menu, action: Action) {
    menu.step(action.input());
    for _ in 0..40 {
        menu.step(Default::default());
    }
}
fn keep_display(menu: &mut Menu) {
    assert!(matches!(
        menu.customize.focus,
        Focus::DisplayConfirmation { keep: true, .. }
    ));
    press(menu, Accept);
    assert_eq!(menu.customize.focus, Focus::Options);
}

fn select_resolution(menu: &mut Menu, value: [u32; 2]) {
    press(menu, Accept);
    for _ in 0..menu.customize.resolution_choices().len() {
        if menu.customize.focus == (Focus::Resolution { choice: value }) {
            break;
        }
        press(menu, Down);
    }
    assert_eq!(menu.customize.focus, Focus::Resolution { choice: value });
    press(menu, Accept);
}

#[test]
fn desktop_menu_edits_restore_reset_and_exit_with_committed_choices() -> anyhow::Result<()> {
    let mut data = data();
    data.defaults.display_mode = DisplayMode::Fullscreen;
    data.defaults.resolution = [1920, 1080];
    let original = CustomizeSettings {
        resolution: [1279, 719],
        ..Default::default()
    };
    let mut menu = Menu::title_customize(original.clone(), data.clone())?;
    press(&mut menu, Idle);
    menu.customize.row = DISPLAY_MODE_OPTION;
    menu.customize.first = DISPLAY_MODE_OPTION - 8;
    for expected in [
        DisplayMode::Borderless,
        DisplayMode::Fullscreen,
        DisplayMode::Windowed,
    ] {
        press(&mut menu, Right);
        assert_eq!(menu.customize.draft.display_mode, expected);
        keep_display(&mut menu);
        assert_eq!(menu.customize.focus, Focus::Options);
    }
    press(&mut menu, Left);
    assert_eq!(menu.customize.draft.display_mode, DisplayMode::Fullscreen);
    keep_display(&mut menu);
    menu.customize.row = ANTI_ALIASING_OPTION;
    menu.customize.first = ANTI_ALIASING_OPTION - 8;
    menu.customize.draft.anti_aliasing = AntiAliasing::SmaaS2x;
    for expected in AntiAliasing::ALL {
        press(&mut menu, Accept);
        press(&mut menu, Down);
        press(&mut menu, Accept);
        assert_eq!(menu.customize.draft.anti_aliasing, expected);
    }
    press(&mut menu, Accept);
    press(&mut menu, Up);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.anti_aliasing, AntiAliasing::Msaa8);
    menu.customize.row = RESOLUTION_OPTION;
    menu.customize.first = RESOLUTION_OPTION - 8;
    select_resolution(&mut menu, [1280, 720]);
    assert_eq!(menu.customize.draft.resolution, [1280, 720]);
    keep_display(&mut menu);
    select_resolution(&mut menu, [1024, 768]);
    assert_eq!(menu.customize.draft.resolution, [1024, 768]);
    keep_display(&mut menu);
    menu.customize.draft.resolution = DISPLAY_RESOLUTIONS[0];
    press(&mut menu, Accept);
    press(&mut menu, Up);
    press(&mut menu, Accept);
    assert_eq!(
        menu.customize.draft.resolution,
        *DISPLAY_RESOLUTIONS.last().unwrap()
    );
    keep_display(&mut menu);
    press(&mut menu, Accept);
    press(&mut menu, Down);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.resolution, DISPLAY_RESOLUTIONS[0]);
    keep_display(&mut menu);
    // Navigation reaches the existing Cancel/Default row after the final option.
    for _ in RESOLUTION_OPTION..CUSTOMIZE_OPTIONS {
        press(&mut menu, Down);
    }
    assert_eq!(menu.customize.row, CUSTOMIZE_OPTIONS);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft, original);
    keep_display(&mut menu);
    press(&mut menu, Right);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft, data.defaults);
    keep_display(&mut menu);
    press(&mut menu, Cancel);
    assert!(menu.closed);
    assert_eq!(menu.customize.draft.display_mode, DisplayMode::Fullscreen);
    assert_eq!(menu.customize.draft.resolution, [1920, 1080]);
    Ok(())
}

#[test]
fn accepting_the_current_custom_resolution_does_not_resize_or_open_confirmation()
-> anyhow::Result<()> {
    let settings = CustomizeSettings {
        resolution: [1279, 719],
        ..Default::default()
    };
    let mut menu = Menu::title_customize(settings, data())?;
    press(&mut menu, Idle);
    menu.customize.row = RESOLUTION_OPTION;
    menu.customize.first = RESOLUTION_OPTION - 8;
    press(&mut menu, Accept);
    assert_eq!(
        menu.customize.focus,
        Focus::Resolution {
            choice: [1279, 719]
        }
    );
    assert!(menu.customize.resolution_choices().contains(&[1279, 719]));
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.resolution, [1279, 719]);
    assert_eq!(menu.customize.focus, Focus::Options);
    // Choosing a different size still starts the normal safety confirmation.
    press(&mut menu, Accept);
    press(&mut menu, Down);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.resolution, [1280, 720]);
    assert!(matches!(
        menu.customize.focus,
        Focus::DisplayConfirmation {
            previous_resolution: [1279, 719],
            ..
        }
    ));
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.resolution, [1279, 719]);
    press(&mut menu, Accept);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.focus, Focus::Options);
    assert_eq!(menu.customize.draft.resolution, [1279, 719]);
    Ok(())
}

#[test]
fn resolution_changes_require_confirmation_and_revert_to_the_last_kept_size() -> anyhow::Result<()>
{
    let mut menu = Menu::title_customize(CustomizeSettings::default(), data())?;
    press(&mut menu, Idle);
    menu.customize.row = RESOLUTION_OPTION;
    menu.customize.first = RESOLUTION_OPTION - 8;
    for key in [Left, Right] {
        press(&mut menu, key);
        assert_eq!(menu.customize.draft.resolution, [640, 480]);
        assert_eq!(menu.customize.focus, Focus::Options);
    }
    press(&mut menu, Accept);
    press(&mut menu, Down);
    assert_eq!(
        menu.customize.focus,
        Focus::Resolution { choice: [800, 600] }
    );
    assert_eq!(
        menu.customize.draft.resolution,
        [640, 480],
        "highlighting must not apply"
    );
    for key in [Left, Right] {
        press(&mut menu, key);
        assert_eq!(
            menu.customize.focus,
            Focus::Resolution { choice: [800, 600] }
        );
    }
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.focus, Focus::Options);
    assert_eq!(menu.customize.draft.resolution, [640, 480]);
    select_resolution(&mut menu, [800, 600]);
    assert!(matches!(
        menu.customize.focus,
        Focus::DisplayConfirmation {
            previous_resolution: [640, 480],
            remaining_secs: 15,
            keep: true,
            ..
        }
    ));
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.resolution, [640, 480]);
    assert!(!menu.closed);
    select_resolution(&mut menu, [800, 600]);
    press(&mut menu, Down);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.resolution, [640, 480]);
    select_resolution(&mut menu, [800, 600]);
    keep_display(&mut menu);
    select_resolution(&mut menu, [960, 720]);
    assert!(matches!(
        menu.customize.focus,
        Focus::DisplayConfirmation {
            previous_resolution: [800, 600],
            ..
        }
    ));
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.resolution, [800, 600]);
    select_resolution(&mut menu, [800, 600]);
    assert_eq!(
        menu.customize.focus,
        Focus::Options,
        "accepting the current size needs no popup"
    );
    press(&mut menu, Cancel);
    assert!(menu.closed);
    Ok(())
}

#[test]
fn native_and_monitor_resolutions_join_the_presets_and_keep_confirmation() -> anyhow::Result<()> {
    let original = CustomizeSettings {
        display_mode: DisplayMode::Fullscreen,
        resolution: [2001, 1301],
        ..Default::default()
    };
    let mut menu = Menu::title_customize(original.clone(), data())?;
    press(&mut menu, Idle);
    let native = [3024, 1964];
    let mut choices = DISPLAY_RESOLUTIONS.to_vec();
    choices.extend([native, [1512, 982], native, [1920, 1080]]);
    menu.customize.set_resolution_choices(choices, Some(native));
    assert_eq!(menu.customize.native_resolution(), Some(native));
    assert_eq!(
        menu.customize.draft, original,
        "discovery must not alter settings"
    );
    let choices = menu.customize.resolution_choices().to_vec();
    assert_eq!(choices.len(), DISPLAY_RESOLUTIONS.len() + 2);
    assert!(choices.windows(2).all(|pair| pair[0] < pair[1]));
    menu.customize.row = RESOLUTION_OPTION;
    menu.customize.first = RESOLUTION_OPTION - 8;
    menu.customize.draft.resolution = [2560, 1600];
    select_resolution(&mut menu, native);
    assert_eq!(menu.customize.draft.resolution, native);
    assert!(matches!(
        menu.customize.focus,
        Focus::DisplayConfirmation {
            previous_mode: DisplayMode::Fullscreen,
            previous_resolution: [2560, 1600],
            ..
        }
    ));
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.resolution, [2560, 1600]);
    select_resolution(&mut menu, native);
    keep_display(&mut menu);
    select_resolution(&mut menu, [3440, 1440]);
    assert_eq!(menu.customize.draft.resolution, [3440, 1440]);
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.resolution, native);
    // Changing monitors does not resize or silently replace a kept selection.
    press(&mut menu, Accept);
    menu.customize
        .set_resolution_choices(DISPLAY_RESOLUTIONS.to_vec(), Some([1920, 1080]));
    assert_eq!(menu.customize.draft.resolution, native);
    assert_eq!(menu.customize.native_resolution(), Some([1920, 1080]));
    assert_eq!(
        menu.customize.focus,
        Focus::Resolution {
            choice: [3440, 1440]
        }
    );
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.resolution, native);
    Ok(())
}

#[test]
fn held_directions_cannot_restart_a_display_preview_until_released() -> anyhow::Result<()> {
    for direction in [Left, Right] {
        for close in ["timeout", "cancel", "revert", "keep"] {
            let mut menu = Menu::title_customize(CustomizeSettings::default(), data())?;
            press(&mut menu, Idle);
            menu.customize.row = DISPLAY_MODE_OPTION;
            menu.customize.first = DISPLAY_MODE_OPTION - 8;
            let display = |menu: &Menu| {
                (
                    menu.customize.draft.display_mode,
                    menu.customize.draft.resolution,
                )
            };
            let previous = display(&menu);
            for _ in 0..60 {
                menu.step(direction.input());
            }
            let candidate = display(&menu);
            assert_ne!(candidate, previous);
            if close == "timeout" {
                // The presentation host calls this on wall-clock expiry.
                menu.customize.revert_display_change();
            } else {
                if close == "revert" {
                    if let Focus::DisplayConfirmation { keep, .. } = &mut menu.customize.focus {
                        *keep = false;
                    }
                }
                let mut input = if close == "cancel" { Cancel } else { Accept }.input();
                input.direction = direction.input().direction;
                menu.step(input);
            }
            let expected = if close == "keep" { candidate } else { previous };
            for _ in 0..60 {
                menu.step(direction.input());
                assert_eq!(display(&menu), expected, "close {close}");
                assert_eq!(menu.customize.focus, Focus::Options);
            }
            menu.step(Idle.input());
            menu.step(direction.input());
            assert_ne!(display(&menu), expected, "a fresh press must work");
            assert!(matches!(
                menu.customize.focus,
                Focus::DisplayConfirmation { .. }
            ));
            // If neutral was already observed when the dialog closed, the
            // next fresh press works without requiring another idle tick.
            menu.step(Idle.input());
            menu.customize.revert_display_change();
            let restored = display(&menu);
            menu.step(direction.input());
            assert_ne!(display(&menu), restored);
            assert!(matches!(
                menu.customize.focus,
                Focus::DisplayConfirmation { .. }
            ));
        }
    }
    Ok(())
}

#[test]
fn display_defaults_validate_before_the_menu_can_open() -> anyhow::Result<()> {
    let doc = options::Document::from_data(&data())?;
    for resolution in [[0, 480], [640, 0], [8193, 480]] {
        let mut invalid = doc.clone();
        invalid.defaults.resolution = resolution;
        assert!(invalid.prepare().is_err());
    }
    let mut missing = doc;
    missing.options.remove("resolution");
    assert!(
        missing.prepare().is_err(),
        "every engine option must be authored"
    );
    Ok(())
}

#[test]
fn aa_dropdown_filters_choices_and_cancel_keeps_the_draft() -> anyhow::Result<()> {
    let mut menu = Menu::title_customize(CustomizeSettings::default(), data())?;
    press(&mut menu, Idle);
    menu.customize.row = ANTI_ALIASING_OPTION;
    menu.customize.first = ANTI_ALIASING_OPTION - 8;
    menu.customize.set_anti_aliasing_choices(&[
        AntiAliasing::Off,
        AntiAliasing::SmaaUltra,
        AntiAliasing::Msaa4,
    ]);
    press(&mut menu, Accept);
    assert_eq!(
        menu.customize.focus,
        Focus::AntiAliasing {
            choice: AntiAliasing::Msaa4
        }
    );
    press(&mut menu, Down);
    assert_eq!(
        menu.customize.focus,
        Focus::AntiAliasing {
            choice: AntiAliasing::Off
        }
    );
    press(&mut menu, Down);
    assert_eq!(menu.customize.draft.anti_aliasing, AntiAliasing::Msaa4);
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.focus, Focus::Options);
    assert_eq!(menu.customize.draft.anti_aliasing, AntiAliasing::Msaa4);
    press(&mut menu, Accept);
    press(&mut menu, Up);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.focus, Focus::Options);
    assert_eq!(menu.customize.draft.anti_aliasing, AntiAliasing::SmaaUltra);
    for key in [Left, Right] {
        press(&mut menu, key);
        assert_eq!(menu.customize.draft.anti_aliasing, AntiAliasing::SmaaUltra);
        assert_eq!(menu.customize.focus, Focus::Options);
    }
    Ok(())
}

#[test]
fn unavailable_aa_is_normalized_on_open_reset_and_cancel_without_another_frame()
-> anyhow::Result<()> {
    let mut data = data();
    data.defaults.anti_aliasing = AntiAliasing::SmaaS2x;
    let mut menu = Menu::title_customize(
        CustomizeSettings {
            anti_aliasing: AntiAliasing::Msaa8,
            ..Default::default()
        },
        data,
    )?;
    menu.customize.set_anti_aliasing_choices(&[
        AntiAliasing::Off,
        AntiAliasing::SmaaUltra,
        AntiAliasing::Msaa4,
    ]);
    assert_eq!(menu.customize.draft.anti_aliasing, AntiAliasing::Msaa4);
    press(&mut menu, Idle);
    menu.customize.row = CUSTOMIZE_OPTIONS;
    menu.customize.defaults = true;
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.anti_aliasing, AntiAliasing::SmaaUltra);
    menu.customize.defaults = false;
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.anti_aliasing, AntiAliasing::Msaa4);
    Ok(())
}

#[test]
fn backend_dropdown_selects_without_a_countdown_and_applies_on_restart() -> anyhow::Result<()> {
    use GraphicsBackend::*;
    let mut menu = Menu::title_customize(CustomizeSettings::default(), data())?;
    press(&mut menu, Idle);
    menu.customize
        .set_backend_choices(&[Automatic, Dx12, Vulkan], Dx12, false);
    menu.customize.row = GRAPHICS_BACKEND_OPTION;
    menu.customize.first = GRAPHICS_BACKEND_OPTION - 8;
    press(&mut menu, Accept);
    press(&mut menu, Down);
    assert_eq!(
        menu.customize.focus,
        Focus::GraphicsBackend { choice: Dx12 }
    );
    assert_eq!(menu.customize.draft.graphics_backend, Automatic);
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.graphics_backend, Automatic);
    press(&mut menu, Accept);
    press(&mut menu, Down);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.focus, Focus::Options);
    assert_eq!(menu.customize.draft.graphics_backend, Dx12);
    press(&mut menu, Accept);
    press(&mut menu, Down);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.focus, Focus::Options);
    assert_eq!(menu.customize.draft.graphics_backend, Vulkan);
    assert_eq!(menu.customize.active_backend, Some(Dx12));
    for key in [Left, Right] {
        press(&mut menu, key);
        assert_eq!(menu.customize.draft.graphics_backend, Vulkan);
        assert_eq!(menu.customize.focus, Focus::Options);
    }
    // Display previews still need confirmation and never undo the next-launch backend.
    menu.customize.row = RESOLUTION_OPTION;
    select_resolution(&mut menu, [800, 600]);
    assert!(matches!(
        menu.customize.focus,
        Focus::DisplayConfirmation { .. }
    ));
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.resolution, [640, 480]);
    assert_eq!(menu.customize.draft.graphics_backend, Vulkan);
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.graphics_backend, Vulkan);
    Ok(())
}

#[test]
fn old_options_and_settings_gain_the_backend_selector_without_recooking() -> anyhow::Result<()> {
    let mut doc = serde_json::to_value(options::Document::from_data(&data())?)?;
    doc["options"]
        .as_object_mut()
        .unwrap()
        .remove("graphics_backend");
    doc["defaults"]
        .as_object_mut()
        .unwrap()
        .remove("graphics_backend");
    let doc: options::Document = serde_json::from_value(doc)?;
    let migrated = doc.prepare()?;
    assert_eq!(migrated.options.len(), CUSTOMIZE_OPTIONS);
    assert_eq!(
        migrated.option_name(GRAPHICS_BACKEND_OPTION),
        "Graphics Backend"
    );
    assert_eq!(
        migrated.defaults.graphics_backend,
        GraphicsBackend::Automatic
    );
    Ok(())
}

#[test]
fn backend_choices_handle_moved_configs_and_default_and_cancel_actions() -> anyhow::Result<()> {
    use GraphicsBackend::*;
    let mut data = data();
    data.defaults.graphics_backend = Metal;
    let mut menu = Menu::title_customize(
        CustomizeSettings {
            graphics_backend: Dx12,
            ..Default::default()
        },
        data,
    )?;
    press(&mut menu, Idle);
    menu.customize
        .set_backend_choices(&[Automatic, Vulkan], Vulkan, true);
    assert_eq!(menu.customize.draft.graphics_backend, Dx12);
    assert!(menu.customize.backend_override);
    menu.customize.row = GRAPHICS_BACKEND_OPTION;
    menu.customize.first = GRAPHICS_BACKEND_OPTION - 8;
    press(&mut menu, Accept);
    assert_eq!(
        menu.customize.focus,
        Focus::GraphicsBackend { choice: Automatic }
    );
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.graphics_backend, Dx12);
    press(&mut menu, Accept);
    press(&mut menu, Down);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.focus, Focus::Options);
    // Reset only uses modes this host can support, even with modded defaults.
    menu.customize.row = CUSTOMIZE_OPTIONS;
    menu.customize.defaults = true;
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.graphics_backend, Automatic);
    assert_eq!(menu.customize.focus, Focus::Options);
    menu.customize.row = GRAPHICS_BACKEND_OPTION;
    press(&mut menu, Accept);
    press(&mut menu, Down);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.graphics_backend, Vulkan);
    // The top Cancel action restores the editor's original settings.
    menu.customize.row = CUSTOMIZE_OPTIONS;
    menu.customize.defaults = false;
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.graphics_backend, Dx12);
    assert_eq!(menu.customize.focus, Focus::Options);
    Ok(())
}
