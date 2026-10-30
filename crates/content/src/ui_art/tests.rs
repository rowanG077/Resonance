use super::*;
use crate::{battle_ui, diagnostics::Diagnostics, font::MovieSubtitles, menu::MenuArt};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

fn root() -> Result<PathBuf> {
    Ok(std::env::var_os("RESONANCE_TEST_ASSETS")
        .context("set RESONANCE_TEST_ASSETS")?
        .into())
}
fn load(path: impl AsRef<std::path::Path>) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn canonical(value: &impl Serialize) -> Result<Value> {
    Ok(serde_json::from_slice(&serde_json::to_vec(value)?)?)
}

#[test]
#[ignore = "requires current library and F02 metadata originals; no media output"]
fn named_ui_preserves_all_menu_selectors_battle_roles_and_screen_timing() -> Result<()> {
    let root = root()?;
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/structure-review-20261027/ui-ledger");
    let before = evidence.join("before");
    let menu_json = load(root.join("ui/menu.json"))?;
    let mut menu: MenuArt = serde_json::from_value(menu_json.clone())?;
    menu.prepare_resources(|p| Ok(fs::read(root.join(p))?));
    menu.validate_layout(528)?;
    let original = load(before.join("ui/menu.json"))?;
    for (value, key) in [
        (json!(menu.fill), "fill"),
        (json!(menu.popup_fill), "popup_fill"),
        (json!(menu.shade), "shade"),
        (json!(menu.palette), "palette"),
        (json!(menu.labels), "labels"),
    ] {
        assert_eq!(value, original[key], "{key}");
    }
    for (value, key) in [
        (json!(menu.sprites.rects), "rects"),
        (json!(menu.sprites.names), "names"),
        (json!(menu.sprites.number_colors), "number_colors"),
        (json!(menu.sprites.bar_colors), "bar_colors"),
    ] {
        assert_eq!(value, original["sprites"][key], "{key}");
    }
    assert!(
        canonical(&menu)? == menu_json,
        "menu publication differs from the installed document"
    );

    let mut old = load(before.join(battle_ui::document::PATH))?;
    let migration = load(evidence.join("migration.json"))?;
    let moves: BTreeMap<_, _> = migration["moves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["from"].as_str().unwrap().to_owned(),
                r["to"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    fn rename(value: &mut Value, moves: &BTreeMap<String, String>) {
        match value {
            Value::String(s) => {
                if let Some(to) = moves.get(s) {
                    *s = to.clone();
                }
            }
            Value::Array(values) => {
                for v in values {
                    rename(v, moves);
                }
            }
            Value::Object(values) => {
                for v in values.values_mut() {
                    rename(v, moves);
                }
            }
            _ => {}
        }
    }
    rename(&mut old, &moves);
    let art = battle_ui::document::read(
        &mut |p| Ok(fs::read(root.join(p))?.into()),
        &Diagnostics::new(true),
    )?;
    art.validate()?;
    // Compare actual prepared arrays to their original slots, independently of
    // their new serialized names. UiTexture serialization here includes sizes.
    for (value, path) in [
        (json!(art.portraits), "/portraits"),
        (json!(art.targets), "/targets"),
        (json!(art.stun), "/stun"),
        (json!(art.recovery_colors), "/recovery_colors"),
        (json!(art.result_colors), "/result_colors"),
        (json!(art.stat_colors), "/stat_colors"),
        (json!(art.stat_backdrops), "/stat_backdrops"),
        (json!(art.stat_icons), "/stat_icons"),
        (json!(art.feedback.scan_vitals), "/feedback/scan_vitals"),
        (json!(art.feedback.scan_icons), "/feedback/scan_icons"),
        (json!(art.feedback.unison_frame), "/feedback/unison_frame"),
        (json!(art.feedback.escape), "/feedback/escape"),
        (
            json!(art.feedback.escape_positions),
            "/feedback/escape_positions",
        ),
        (json!(art.commands.texture), "/commands/texture"),
        (json!(art.commands.icons), "/commands/icons"),
        (json!(art.commands.faces), "/commands/faces"),
        (json!(art.commands.names), "/commands/names"),
        (
            json!(art.commands.selector_labels),
            "/commands/selector_labels",
        ),
        (
            json!(art.commands.selector_colors),
            "/commands/selector_colors",
        ),
        (json!(art.notices.icons), "/notices/icons"),
        (json!(art.notices.icon_origins), "/notices/icon_origins"),
        (json!(art.notices.faces), "/notices/faces"),
        (json!(art.notices.frame), "/notices/frame"),
        (json!(art.notices.fill), "/notices/fill"),
        (json!(art.combo.anchors), "/combo/anchors"),
        (json!(art.combo.number_colors), "/combo/number_colors"),
        (json!(art.combo.bar_colors), "/combo/bar_colors"),
        (json!(art.damage.colors), "/damage/colors"),
        (json!(art.damage.sizes), "/damage/sizes"),
        (json!(art.meters.bars), "/meters/bars"),
        (json!(art.meters.numbers), "/meters/numbers"),
    ] {
        assert_eq!(value, *old.pointer(path).unwrap(), "{path}");
    }
    assert_eq!(
        canonical(&art.with_font(battle_ui::document::FONT.to_owned()))?,
        load(root.join(battle_ui::document::PATH))?
    );
    let game_over: crate::game_over::Art =
        serde_json::from_slice(&fs::read(root.join(crate::game_over::document::PATH))?)?;
    let old_game_over = load(before.join(crate::game_over::document::PATH))?;
    let game_over = game_over.prepare(|p| Ok(fs::read(root.join(p))?), &Diagnostics::new(true))?;
    assert_eq!(json!(game_over.background), old_game_over["background"]);
    assert_eq!(json!(game_over.font), old_game_over["font"]);
    assert_eq!(json!(game_over.caption), old_game_over["caption"]);
    assert_eq!(json!(game_over.choices), old_game_over["choices"]);
    let subtitles: MovieSubtitles =
        serde_json::from_slice(&fs::read(root.join("ui/story-subtitles.json"))?)?;
    subtitles.validate()?;
    let old = load(before.join("ui/story-subtitles.json"))?;
    assert_eq!(subtitles.cues.len(), old["cues"].as_array().unwrap().len());
    for (cue, old) in subtitles.cues.iter().zip(old["cues"].as_array().unwrap()) {
        assert_eq!(json!(cue.frame), old["frame"]);
        assert_eq!(
            json!(cue.lines),
            Value::Array(
                old["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|v| v["text"] != "")
                    .cloned()
                    .collect()
            )
        );
    }
    let controls =
        crate::effect::OverlayArt::load(crate::input_prompts::FIELD_BANK_PATH, |path| {
            Ok(fs::read(root.join(path))?)
        })?;
    assert_eq!(controls.textures.len(), crate::input_prompts::FIELD_GLYPHS);
    eprintln!(
        "Named UI roles retain their layout; controller prompts resolve through the shared input atlas"
    );
    Ok(())
}

#[test]
#[ignore = "requires current library; modded named UI and missing-portrait policy, no media output"]
fn named_battle_roles_mod_independently_and_bad_portraits_stay_optional() -> Result<()> {
    let root = root()?;
    let original = load(root.join(battle_ui::document::PATH))?;
    let mut edited = original.clone();
    edited["commands"]["names"]["items"] = "Inventory".into();
    edited["commands"]["faces"]["colette"] = json!([0, 0, 16, 16]);
    edited["meters"]["numbers"]["tp"] = json!([[1, 2, 3, 255], [4, 5, 6, 255]]);
    edited["portraits"]["lloyd"] = "ui/dialogue.json".into();
    let bytes = serde_json::to_vec(&edited)?;
    let mut read = |p: &str| {
        Ok(if p == battle_ui::document::PATH {
            bytes.clone().into()
        } else {
            fs::read(root.join(p))?.into()
        })
    };
    assert!(battle_ui::document::read(&mut read, &Diagnostics::new(true)).is_err());
    let diagnostics = Diagnostics::new(false);
    let art = battle_ui::document::read(&mut read, &diagnostics)?;
    assert!(art.portraits[0].is_none());
    assert!(art.portraits[1..].iter().all(Option::is_some));
    assert_eq!(art.commands.names[4], "Inventory");
    assert_eq!(art.commands.faces[1], [0, 0, 16, 16]);
    assert_eq!(art.meters.numbers[1], [[1, 2, 3, 255], [4, 5, 6, 255]]);
    assert_eq!(
        art.commands.names[0],
        original["commands"]["names"]["techniques"]
    );
    assert!(diagnostics.has_errors());
    let mut invalid = original.clone();
    invalid["version"] = 44.into();
    assert!(serde_json::from_value::<battle_ui::Art<String>>(invalid).is_err());
    let mut invalid = original;
    invalid["commands"]["names"]["invented"] = "bad".into();
    assert!(serde_json::from_value::<battle_ui::Art<String>>(invalid).is_err());
    Ok(())
}
