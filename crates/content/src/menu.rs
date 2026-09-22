//! Prepared menu artwork; slots themselves belong to the persistence service.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SHOP_LABELS: [&str; 25] = [
    "shop_buy",
    "shop_sell",
    "shop_equip",
    "shop_exit",
    "shop_status",
    "shop_empty",
    "shop_confirm",
    "shop_yes",
    "shop_no",
    "shop_total",
    "shop_gald",
    "shop_select",
    "shop_add",
    "shop_reduce",
    "shop_ok",
    "shop_info",
    "shop_slash",
    "shop_thrust",
    "shop_defense",
    "shop_accuracy",
    "shop_evasion",
    "shop_intelligence",
    "shop_luck",
    "shop_attack",
    "shop_cannot_equip",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuArt {
    pub version: u32,
    pub textures: Vec<MenuTexture>,
    pub windows: [WindowArt; 3],
    pub fill: [u8; 4],
    pub popup_fill: [u8; 4],
    pub shade: [[u8; 4]; 2],
    pub palette: Vec<[u8; 4]>,
    pub labels: BTreeMap<String, String>,
    pub sprites: MenuSprites,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowArt {
    pub patterns: [usize; 6],
    pub heading: Option<usize>,
    pub cursor: Option<usize>,
    pub cursor_motion: [f32; 2],
    /// A plain beveled window has no decorative slices.
    pub slices: Option<[usize; 12]>,
    pub outset: u8,
    pub flourish_outset: [u8; 2],
    pub foot_outset: u8,
    pub left_joins: [u8; 2],
    pub left_strip: [u8; 2],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuSprites {
    pub buttons: Vec<[u32; 4]>,
    pub item_images: Vec<[u32; 4]>,
    pub recipes: [[u32; 4]; 24],
    pub cooking_stars: [[u32; 4]; 2],
    pub item_tabs: Vec<[u32; 4]>,
    pub tech_ranks: Vec<[u32; 4]>,
    pub elements: Vec<[u32; 4]>,
    pub items: Vec<[u32; 4]>,
    pub portraits: [[u32; 4]; 9],
    pub petrified_portraits: [[u32; 4]; 9],
    pub condition_icons: [[u32; 4]; 14],
    /// Three better/worse frames, an equal marker, and the equipped glyph.
    pub equipment_markers: [[u32; 4]; 8],
    pub strategy_characters: [[u32; 4]; 9],
    pub technique: [[u32; 4]; 13],
    pub numbers: [u32; 4],
    pub leader: [u32; 4],
    pub number_colors: [[[u8; 4]; 2]; 18],
    pub bar_colors: [[[u8; 4]; 4]; 3],
    pub names: [String; 9],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuTexture {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub repeat: bool,
    /// Window patterns take opacity from their vertex color, not the image.
    pub opaque: bool,
}
impl MenuArt {
    pub const VERSION: u32 = 15;

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == Self::VERSION
                && (30..=80).contains(&self.textures.len())
                && self.palette.len() == 11
                && self.sprites.item_images.len() == 528
                && self.sprites.item_tabs.len() == 9
                && self.sprites.buttons.len() == 32
                && self.sprites.tech_ranks.len() == 2
                && self.sprites.elements.len() == 8,
            "unsupported menu artwork; recook the field"
        );
        for window in &self.windows {
            ensure!(
                window
                    .patterns
                    .iter()
                    .chain(window.slices.iter().flatten())
                    .chain(&window.heading)
                    .chain(&window.cursor)
                    .all(|&i| i < self.textures.len())
                    && window
                        .cursor_motion
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=16.0).contains(v))
                    && window.outset <= 8
                    && window.flourish_outset.iter().all(|&v| v <= 32),
                "invalid menu window artwork"
            );
        }
        for texture in &self.textures {
            crate::validate_asset_path(&texture.path)?;
            ensure!(
                (1..=1024).contains(&texture.width) && (1..=1024).contains(&texture.height),
                "invalid menu texture"
            );
        }
        let atlas = &self.textures[18];
        for &[x, y, w, h] in self
            .sprites
            .portraits
            .iter()
            .chain(&self.sprites.petrified_portraits)
            .chain(&self.sprites.condition_icons)
            .chain(&self.sprites.equipment_markers)
            .chain(&self.sprites.strategy_characters)
            .chain(&self.sprites.items)
            .chain(&self.sprites.item_images)
            .chain(&self.sprites.recipes)
            .chain(&self.sprites.cooking_stars)
            .chain(&self.sprites.item_tabs)
            .chain(&self.sprites.buttons)
            .chain(&self.sprites.tech_ranks)
            .chain(&self.sprites.elements)
            .chain(&self.sprites.technique)
            .chain([&self.sprites.numbers, &self.sprites.leader])
        {
            ensure!(
                w > 0
                    && h > 0
                    && x.checked_add(w).is_some_and(|v| v <= atlas.width)
                    && y.checked_add(h).is_some_and(|v| v <= atlas.height),
                "menu sprite exceeds atlas"
            );
        }
        ensure!(
            self.sprites.names.iter().all(|name| !name.is_empty()
                && name.len() <= 32
                && name.chars().all(|c| c.is_ascii_graphic() || c == ' ')),
            "invalid party name"
        );
        for key in [
            "tech",
            "unison",
            "strategy",
            "status",
            "synopsis",
            "items",
            "ex_skill",
            "equip",
            "cooking",
            "system",
            "save",
            "go_in",
            "talk",
            "shop",
            "examine",
            "go_out",
            "load",
            "customize",
            "empty",
            "time",
            "encounter",
            "combo",
            "next",
            "gald",
            "play_time",
            "encounters",
            "max_combo",
            "yes",
            "no",
            "confirm_save_a",
            "confirm_save_b",
            "confirm_load_a",
            "confirm_load_b",
            "confirm_overwrite_a",
            "confirm_overwrite_b",
        ]
        .into_iter()
        .chain(SHOP_LABELS)
        {
            ensure!(
                self.labels.get(key).is_some_and(|s| !s.is_empty()
                    && s.len() <= 128
                    && !s
                        .chars()
                        .any(|c| c.is_control() && !(key.starts_with("confirm_") && c == '\n'))),
                "missing menu label {key}"
            );
        }
        Ok(())
    }
}
