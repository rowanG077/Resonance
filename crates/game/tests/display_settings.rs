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
        press(&mut menu, Right);
        assert_eq!(menu.customize.draft.anti_aliasing, expected);
    }
    press(&mut menu, Left);
    assert_eq!(menu.customize.draft.anti_aliasing, AntiAliasing::Msaa8);
    menu.customize.row = RESOLUTION_OPTION;
    menu.customize.first = RESOLUTION_OPTION - 8;
    press(&mut menu, Right);
    assert_eq!(menu.customize.draft.resolution, [1280, 720]);
    keep_display(&mut menu);
    press(&mut menu, Left);
    assert_eq!(menu.customize.draft.resolution, [1024, 768]);
    keep_display(&mut menu);
    menu.customize.draft.resolution = DISPLAY_RESOLUTIONS[0];
    press(&mut menu, Left);
    assert_eq!(
        menu.customize.draft.resolution,
        *DISPLAY_RESOLUTIONS.last().unwrap()
    );
    keep_display(&mut menu);
    press(&mut menu, Right);
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
fn resolution_changes_require_confirmation_and_revert_to_the_last_kept_size() -> anyhow::Result<()>
{
    let mut menu = Menu::title_customize(CustomizeSettings::default(), data())?;
    press(&mut menu, Idle);
    menu.customize.row = RESOLUTION_OPTION;
    menu.customize.first = RESOLUTION_OPTION - 8;
    press(&mut menu, Right);
    assert_eq!(menu.customize.draft.resolution, [800, 600]);
    assert!(matches!(
        menu.customize.focus,
        Focus::DisplayConfirmation {
            previous_resolution: [640, 480],
            remaining_secs: 15,
            keep: true,
            ..
        }
    ));
    press(&mut menu, Right); // Holding the opener cannot edit another resolution.
    assert_eq!(menu.customize.draft.resolution, [800, 600]);
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.resolution, [640, 480]);
    assert!(!menu.closed);
    press(&mut menu, Right);
    press(&mut menu, Down);
    press(&mut menu, Accept);
    assert_eq!(menu.customize.draft.resolution, [640, 480]);
    press(&mut menu, Right);
    keep_display(&mut menu);
    press(&mut menu, Right);
    assert_eq!(menu.customize.draft.resolution, [960, 720]);
    assert!(matches!(
        menu.customize.focus,
        Focus::DisplayConfirmation {
            previous_resolution: [800, 600],
            ..
        }
    ));
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.resolution, [800, 600]);
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
    press(&mut menu, Right);
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
    press(&mut menu, Right);
    keep_display(&mut menu);
    press(&mut menu, Right);
    assert_eq!(menu.customize.draft.resolution, [3440, 1440]);
    press(&mut menu, Cancel);
    assert_eq!(menu.customize.draft.resolution, native);
    // Changing monitors does not resize or silently replace a kept selection.
    menu.customize
        .set_resolution_choices(DISPLAY_RESOLUTIONS.to_vec(), Some([1920, 1080]));
    assert_eq!(menu.customize.draft.resolution, native);
    assert_eq!(menu.customize.native_resolution(), Some([1920, 1080]));
    Ok(())
}

#[test]
fn held_directions_cannot_restart_a_display_preview_until_released() -> anyhow::Result<()> {
    for row in [DISPLAY_MODE_OPTION, RESOLUTION_OPTION] {
        for direction in [Left, Right] {
            for close in ["timeout", "cancel", "revert", "keep"] {
                let mut menu = Menu::title_customize(CustomizeSettings::default(), data())?;
                press(&mut menu, Idle);
                menu.customize.row = row;
                menu.customize.first = row - 8;
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
                    assert_eq!(display(&menu), expected, "row {row}, close {close}");
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
    press(&mut menu, Right);
    assert_eq!(menu.customize.draft.anti_aliasing, AntiAliasing::Msaa4);
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
