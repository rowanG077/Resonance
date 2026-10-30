//! Required cooked input icon atlas and its editable layout.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const IMAGE_PATH: &str = "ui/controls/input-prompts.ktx2";
pub const MANIFEST_PATH: &str = "ui/controls/input-prompts.json";
pub const LICENSE_PATH: &str = "ui/controls/input-prompts-license.txt";
#[cfg(test)]
const DEFAULT_KTX2: &[u8] = include_bytes!("../../../resources/input-prompts/atlas.ktx2");
#[cfg(test)]
const DEFAULT_JSON: &[u8] = include_bytes!("../../../resources/input-prompts/atlas.json");

/// Script-authored controller-help bank. The indices are part of the field
/// script contract; button pixels come exclusively from the configurable atlas.
pub const FIELD_BANK_PATH: &str = "ui/controls/gamecube.json";
pub const FIELD_GLYPHS: usize = 32;
pub const PANEL_GLYPHS: std::ops::RangeInclusive<usize> = 19..=21;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerGlyph {
    Action(crate::input::StandardAction, bool),
    Movement,
    Camera,
    Orders,
    Horizontal,
    Panel,
}
/// Shared by inline menu controls and script-created field sprites. Extended
/// menu glyphs 32–38 retain the same configured direction and pressed meaning.
pub fn controller_glyph(id: usize) -> Option<ControllerGlyph> {
    use crate::input::StandardAction as A;
    use ControllerGlyph::*;
    Some(match id {
        0 => Movement,
        1 | 33 => Action(A::MoveUp, id == 33),
        2 | 34 => Action(A::MoveDown, id == 34),
        3 | 36 => Action(A::MoveRight, id == 36),
        4 | 35 => Action(A::MoveLeft, id == 35),
        5 | 6 => Action(A::Confirm, id == 6),
        7 | 8 => Action(A::Cancel, id == 8),
        9 | 10 => Action(A::Alternate, id == 10),
        11 | 12 => Action(A::Menu, id == 12),
        13 | 14 => Action(A::Previous, id == 14),
        15 | 16 => Action(A::Next, id == 16),
        17 | 18 => Action(A::Start, id == 18),
        19..=21 => Panel,
        22 | 23 => Action(A::Skit, id == 23),
        24 => Camera,
        25 | 37 => Action(A::CameraUp, id == 37),
        26 | 38 => Action(A::CameraDown, id == 38),
        27 => Orders,
        28 => Action(A::OrderUp, false),
        29 => Action(A::OrderCancel, false),
        30 => Action(A::OrderRight, false),
        31 => Action(A::OrderLeft, false),
        32 => Horizontal,
        _ => return None,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Atlas {
    pub size: [u32; 2],
    pub icons: BTreeMap<String, [u32; 4]>,
    #[serde(default)]
    pub pressed: BTreeMap<String, [u32; 4]>,
}
#[cfg(test)]
impl Default for Atlas {
    fn default() -> Self {
        serde_json::from_slice(DEFAULT_JSON).expect("embedded input atlas manifest")
    }
}
impl Atlas {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.size.into_iter().all(|v| (1..=16384).contains(&v)),
            "invalid input atlas dimensions"
        );
        ensure!(
            self.icons.contains_key("button"),
            "input atlas needs a fallback button icon"
        );
        for (name, &[x, y, w, h]) in self.icons.iter().chain(&self.pressed) {
            ensure!(
                w > 0
                    && h > 0
                    && x.checked_add(w).is_some_and(|right| right <= self.size[0])
                    && y.checked_add(h)
                        .is_some_and(|bottom| bottom <= self.size[1]),
                "input icon {name} is outside its atlas"
            );
        }
        Ok(())
    }
}

/// Capture required files through the same layered library as other interface art.
pub fn capture(snapshot: &mut crate::library::Snapshot) -> Result<Atlas> {
    let atlas: Atlas = snapshot.json(MANIFEST_PATH, crate::preparation::JSON_LIMIT)?;
    atlas.validate()?;
    snapshot.read(IMAGE_PATH, crate::preparation::ASSET_LIMIT)?;
    Ok(atlas)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_capture_requires_both_assets_and_keeps_mod_overrides() -> Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join("content.json"),
            br#"{"version":94,"game":"tales-of-symphonia"}"#,
        )?;
        let snapshot =
            || Ok::<_, anyhow::Error>(crate::library::Library::open(root.path(), &[])?.snapshot());
        assert!(format!("{:#}", capture(&mut snapshot()?).unwrap_err()).contains(MANIFEST_PATH));
        std::fs::create_dir_all(root.path().join("ui/controls"))?;
        std::fs::write(root.path().join(MANIFEST_PATH), DEFAULT_JSON)?;
        assert!(format!("{:#}", capture(&mut snapshot()?).unwrap_err()).contains(IMAGE_PATH));
        std::fs::write(root.path().join(IMAGE_PATH), DEFAULT_KTX2)?;
        capture(&mut snapshot()?)?;
        let modification = tempfile::tempdir()?;
        std::fs::create_dir_all(modification.path().join("ui/controls"))?;
        let mut atlas = Atlas::default();
        atlas.icons.insert("key_z".into(), [0, 0, 512, 512]);
        std::fs::write(
            modification.path().join(MANIFEST_PATH),
            serde_json::to_vec(&atlas)?,
        )?;
        let library =
            crate::library::Library::open_paths(root.path(), &[modification.path().to_owned()])?;
        assert_eq!(
            capture(&mut library.snapshot())?.icons["key_z"],
            [0, 0, 512, 512]
        );
        Ok(())
    }
    #[test]
    fn every_icon_has_a_visible_distinct_pressed_frame() {
        let atlas = Atlas::default();
        assert_eq!(
            atlas.icons.keys().collect::<Vec<_>>(),
            atlas.pressed.keys().collect::<Vec<_>>()
        );
        let levels = resonance_asset_writer::texture::ktx2(DEFAULT_KTX2).unwrap();
        let image = &levels[0];
        let tile = |[x, y, w, h]: [u32; 4]| {
            let mut pixels = Vec::new();
            for row in y..y + h {
                let start = ((row * image.width + x) * 4) as usize;
                pixels.extend_from_slice(&image.rgba[start..start + (w * 4) as usize]);
            }
            assert!(pixels.chunks_exact(4).any(|rgba| rgba[3] > 0));
            pixels
        };
        for (name, &normal) in &atlas.icons {
            let pressed = atlas.pressed[name];
            assert_eq!(normal[2..], pressed[2..]);
            let normal_pixels = tile(normal);
            let pressed_pixels = tile(pressed);
            assert_ne!(
                normal_pixels, pressed_pixels,
                "missing pressed artwork for {name}"
            );
        }
    }
    #[test]
    fn multipart_controls_keep_unactuated_parts_unchanged() {
        let atlas = Atlas::default();
        let levels = resonance_asset_writer::texture::ktx2(DEFAULT_KTX2).unwrap();
        let image = &levels[0];
        // Coordinates are in the source artwork's 64px design grid. Compare
        // actual exported pixels, including transparency, rather than SVG code.
        let region = |tile: [u32; 4], [x, y, w, h]: [u32; 4]| {
            let mut pixels = Vec::new();
            for row in y * 4..(y + h) * 4 {
                let start = (((tile[1] + row) * image.width + tile[0] + x * 4) * 4) as usize;
                pixels.extend_from_slice(&image.rgba[start..start + (w * 16) as usize]);
            }
            pixels
        };
        for (name, fixed) in [
            ("pad_dpad_up", [42, 23, 14, 18]),
            ("pad_dpad_left", [42, 23, 14, 18]),
            ("stick_l_up", [15, 55, 34, 3]),
            ("button", [15, 54, 34, 4]),
        ] {
            let normal = region(atlas.icons[name], fixed);
            assert!(
                normal.chunks_exact(4).any(|p| p[3] > 0),
                "empty fixed part: {name}"
            );
            assert!(
                normal == region(atlas.pressed[name], fixed),
                "unactuated part changed: {name}"
            );
        }
    }
    #[test]
    fn every_letter_and_labeled_controller_button_has_distinct_rendered_pixels() {
        let atlas = Atlas::default();
        let levels = resonance_asset_writer::texture::ktx2(DEFAULT_KTX2).unwrap();
        let pixels = &levels[0];
        let crop = |name: &str| {
            let [x, y, w, h] = atlas.icons[name];
            let mut tile = Vec::with_capacity((w * h * 4) as usize);
            for row in y..y + h {
                let start = ((row * pixels.width + x) * 4) as usize;
                tile.extend_from_slice(&pixels.rgba[start..start + (w * 4) as usize]);
            }
            assert!(
                tile.chunks_exact(4).any(|pixel| pixel[3] > 0),
                "empty icon {name}"
            );
            tile
        };
        let alphabet: std::collections::BTreeSet<_> = ('a'..='z')
            .map(|letter| crop(&format!("key_{letter}")))
            .collect();
        assert_eq!(
            alphabet.len(),
            26,
            "keyboard letter glyphs must be distinct"
        );
        let controller: std::collections::BTreeSet<_> = ["a", "b", "x", "y"]
            .into_iter()
            .map(|letter| crop(&format!("pad_{letter}")))
            .collect();
        assert_eq!(controller.len(), 4);

        let unique: std::collections::BTreeSet<_> =
            atlas.icons.keys().map(|name| crop(name)).collect();
        assert_eq!(
            unique.len(),
            atlas.icons.len(),
            "duplicate normal tiles in the atlas"
        );
        for side in ["l", "r"] {
            for number in 1..=5 {
                let name = format!("pad_{side}{number}");
                assert!(atlas.icons.contains_key(&name), "missing {name}");
                assert!(atlas.pressed.contains_key(&name), "missing pressed {name}");
            }
        }
        assert!(
            atlas
                .icons
                .keys()
                .all(|name| !name.starts_with("mouse") && !name.starts_with("key_arrows"))
        );
        for removed in [
            "keyboard",
            "pad_select",
            "pad_south",
            "pad_east",
            "pad_west",
            "pad_north",
            "pad_left_trigger",
            "pad_right_trigger",
            "pad_left_shoulder",
            "pad_right_shoulder",
            "button_trigger_a",
            "button_trigger_b",
            "button_trigger_c",
        ] {
            assert!(
                !atlas.icons.contains_key(removed),
                "uncurated icon {removed}"
            );
        }
    }
    #[test]
    fn cooked_defaults_and_modded_layouts_validate() {
        let mut atlas = Atlas::default();
        atlas.validate().unwrap();
        atlas.icons.insert("key_z".into(), [0, 0, 512, 512]);
        atlas.validate().unwrap();
        atlas.icons.insert("button".into(), [u32::MAX, 0, 256, 256]);
        assert!(atlas.validate().is_err());
    }

    #[test]
    fn keyboard_and_controller_faces_use_the_selected_color_families() {
        let atlas = Atlas::default();
        let levels = resonance_asset_writer::texture::ktx2(DEFAULT_KTX2).unwrap();
        let image = &levels[0];
        let sample = |rect: [u32; 4], x: u32, y: u32| {
            let offset = (((rect[1] + y * 4) * image.width + rect[0] + x * 4) * 4) as usize;
            let p = &image.rgba[offset..offset + 4];
            assert_eq!(p[3], 255);
            [p[0], p[1], p[2]]
        };
        for name in ["key_a", "pad_l1", "pad_l3", "pad_r5"] {
            let [r, _, b] = sample(atlas.icons[name], 16, 16);
            assert!(b > r, "normal {name} should have a blue face");
            let [r, _, b] = sample(atlas.pressed[name], 16, 16);
            assert!(r > b, "pressed {name} should have a gold face");
        }
        let [r, g, b] = sample(atlas.icons["pad_a"], 32, 16);
        assert!(g > r && g > b);
        let [r, g, b] = sample(atlas.icons["pad_b"], 32, 16);
        assert!(r > g && r > b);
        let [r, _, b] = sample(atlas.icons["pad_x"], 32, 16);
        assert!(b > r);
        let [r, g, b] = sample(atlas.icons["pad_y"], 32, 16);
        assert!(r > b && g > b);
    }

    #[test]
    fn direction_accents_cover_the_arrow_without_recoloring_its_surroundings() {
        let atlas = Atlas::default();
        let levels = resonance_asset_writer::texture::ktx2(DEFAULT_KTX2).unwrap();
        let image = &levels[0];
        let pixel = |tile: [u32; 4], x: u32, y: u32| {
            let offset = (((tile[1] + y) * image.width + tile[0] + x) * 4) as usize;
            &image.rgba[offset..offset + 4]
        };
        for direction in ["up", "down", "left", "right"] {
            let name = format!("pad_dpad_{direction}");
            for y in 0..256 {
                for x in 0..256 {
                    let p = pixel(atlas.pressed[&name], x, y);
                    assert!(p[3] == 0 || p[0] >= p[2], "blue leaked into pressed {name}");
                }
            }
            for side in ["l", "r"] {
                let name = format!("stick_{side}_{direction}");
                for y in 0..256 {
                    for x in 0..256 {
                        // Separately colored native subpaths can round edge
                        // alpha differently, but cannot paint outside the source.
                        assert!(
                            pixel(atlas.pressed[&name], x, y)[3] == 0
                                || pixel(atlas.icons[&name], x, y)[3] > 0,
                            "accent extends outside the silhouette of {name}"
                        );
                    }
                }
            }
        }
        for name in ["stick_l_down", "stick_r_down"] {
            let arrow = pixel(atlas.pressed[name], 32 * 4, 44 * 4);
            assert!(
                arrow[3] == 255 && arrow[0] > arrow[2],
                "down arrow is not gold: {name}"
            );
            let base = pixel(atlas.pressed[name], 32 * 4, 54 * 4);
            assert!(
                base[3] == 255 && base[2] > base[0],
                "down accent leaked onto base: {name}"
            );
        }
        let click = atlas.pressed["stick"];
        let arrow = pixel(click, 32 * 4, 7 * 4);
        assert!(arrow[3] == 255 && arrow[0] > arrow[2]);
        // This nearby rim pixel was incorrectly gold under the rectangular mask.
        let rim = pixel(click, 25 * 4, 12 * 4);
        assert!(
            rim[3] > 200 && rim[2] > rim[0],
            "click arrow bleeds onto the rim"
        );
    }
}
