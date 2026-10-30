//! Prepared menu artwork; slots themselves belong to the persistence service.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod document;
mod sprites;

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

/// Optional label catalogues retain healthy entries and diagnose rejected text.
pub(crate) fn decode_labels(
    value: serde_json::Value,
    diagnostics: &crate::diagnostics::Diagnostics,
    scope: &str,
) -> Result<BTreeMap<String, String>> {
    let rows = diagnostics
        .attempt(
            scope,
            serde_json::from_value::<BTreeMap<String, serde_json::Value>>(value)
                .context("decode labels"),
        )?
        .unwrap_or_default();
    let mut labels = BTreeMap::new();
    for (key, value) in rows {
        if let Some(text) = diagnostics.attempt(
            &format!("{scope} {key}"),
            serde_json::from_value::<String>(value).context("decode label text"),
        )? {
            labels.insert(key, text);
        }
    }
    Ok(labels)
}

#[derive(Debug, Clone)]
pub struct MenuArt {
    pub textures: BTreeMap<usize, MenuTexture>,
    pub windows: BTreeMap<usize, WindowArt>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sprite {
    Buttons,
    ItemImages,
    Recipes,
    CookingStars,
    ItemTabs,
    TechRanks,
    Elements,
    Items,
    Portraits,
    PetrifiedPortraits,
    ConditionIcons,
    EquipmentMarkers,
    StrategyCharacters,
    Technique,
    Numbers,
    Leader,
    WorldHud,
}
#[derive(Debug, Clone, Default)]
pub struct MenuSprites {
    pub rects: BTreeMap<Sprite, Vec<[u32; 4]>>,
    pub number_colors: Vec<[[u8; 4]; 2]>,
    pub bar_colors: Vec<[[u8; 4]; 4]>,
    pub names: Vec<String>,
    /// Named item/recipe artwork resolves through their existing catalogues.
    pub named_rects: BTreeMap<Sprite, BTreeMap<String, [u32; 4]>>,
    pub name_overrides: BTreeMap<String, String>,
}
impl MenuSprites {
    pub fn group(&self, group: Sprite) -> Result<&[[u32; 4]]> {
        self.rects
            .get(&group)
            .map(Vec::as_slice)
            .with_context(|| format!("menu sprites {group:?} were not prepared"))
    }
}

fn decode_art_group<T: serde::de::DeserializeOwned + Default>(
    value: serde_json::Value,
    diagnostics: &crate::diagnostics::Diagnostics,
    scope: &str,
) -> Result<T> {
    if value.is_null() {
        return Ok(T::default());
    }
    Ok(diagnostics
        .attempt(
            scope,
            serde_json::from_value(value).map_err(anyhow::Error::from),
        )?
        .unwrap_or_default())
}
fn decode_art_rows<K: serde::de::DeserializeOwned + Ord, V: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    diagnostics: &crate::diagnostics::Diagnostics,
    scope: &str,
) -> Result<BTreeMap<K, V>> {
    let rows: BTreeMap<String, serde_json::Value> = decode_art_group(value, diagnostics, scope)?;
    let mut accepted = BTreeMap::new();
    for (key, value) in rows {
        let row = serde_json::Value::Object([(key.clone(), value)].into_iter().collect());
        if let Some(row) = diagnostics.attempt(
            &format!("{scope} {key}"),
            serde_json::from_value::<BTreeMap<K, V>>(row).map_err(anyhow::Error::from),
        )? {
            accepted.extend(row);
        }
    }
    Ok(accepted)
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
    pub const ATLAS_TEXTURE: usize = 18;
    pub const PORTRAIT_TEXTURES: std::ops::Range<usize> = 19..28;
    pub const WORLD_MAP_TEXTURES: std::ops::Range<usize> = 28..30;

    pub fn validate(&self, item_count: usize) -> Result<()> {
        self.validate_layout(item_count)?;
        ensure!(
            self.labels.values().all(|text| !text.is_empty()
                && text.len() <= 4096
                && text.chars().all(|c| !c.is_control() || c == '\n')),
            "invalid menu artwork label"
        );
        Ok(())
    }

    /// Shared drawing invariants; page-specific resources are checked when selected.
    pub fn validate_structure(&self) -> Result<()> {
        ensure!(self.palette.len() == 11, "unsupported menu artwork");
        Ok(())
    }

    fn validate_window(&self, index: usize, background: usize) -> Result<()> {
        let window = self
            .windows
            .get(&index)
            .context("missing menu window style")?;
        let pattern = window
            .patterns
            .get(background)
            .context("invalid menu background selection")?;
        for &index in std::iter::once(pattern)
            .chain(window.slices.iter().flatten())
            .chain(&window.heading)
            .chain(&window.cursor)
        {
            self.texture(index)?;
        }
        Ok(())
    }

    pub fn texture(&self, index: usize) -> Result<&MenuTexture> {
        let texture = self
            .textures
            .get(&index)
            .context("missing selected menu texture")?;
        crate::validate_asset_path(&texture.path)?;
        ensure!(
            texture.width > 0 && texture.height > 0,
            "unavailable menu image dimensions: {}",
            texture.path
        );
        Ok(texture)
    }

    pub fn sprite(&self, group: Sprite, index: usize) -> Result<[u32; 4]> {
        let rect = self
            .sprites
            .group(group)?
            .get(index)
            .copied()
            .with_context(|| format!("menu sprite {group:?}[{index}] was not prepared"))?;
        let [x, y, width, height] = rect;
        let atlas = self.texture(Self::ATLAS_TEXTURE)?;
        ensure!(
            width > 0
                && height > 0
                && x.checked_add(width)
                    .is_some_and(|right| right <= atlas.width)
                && y.checked_add(height)
                    .is_some_and(|bottom| bottom <= atlas.height),
            "invalid menu sprite {group:?}[{index}]"
        );
        Ok(rect)
    }

    pub fn validate_layout(&self, item_count: usize) -> Result<()> {
        self.validate_structure()?;
        for (&index, window) in &self.windows {
            for background in 0..window.patterns.len() {
                self.validate_window(index, background)?;
            }
        }
        for &index in self.textures.keys() {
            self.texture(index)?;
        }
        for index in Self::PORTRAIT_TEXTURES.chain(Self::WORLD_MAP_TEXTURES) {
            self.texture(index)?;
        }
        for (group, count) in [
            (Sprite::ItemImages, item_count),
            (Sprite::ItemTabs, 9),
            (Sprite::Buttons, 32),
            (Sprite::TechRanks, 2),
            (Sprite::Elements, 8),
            (Sprite::Recipes, 24),
            (Sprite::CookingStars, 2),
            (Sprite::Portraits, 9),
            (Sprite::PetrifiedPortraits, 9),
            (Sprite::ConditionIcons, 14),
            (Sprite::EquipmentMarkers, 8),
            (Sprite::StrategyCharacters, 9),
            (Sprite::Technique, 13),
            (Sprite::Numbers, 1),
            (Sprite::Leader, 1),
            (Sprite::WorldHud, 7),
        ] {
            ensure!(
                self.sprites.group(group)?.len() == count,
                "invalid menu sprite group {group:?}"
            );
        }
        for (&group, rects) in &self.sprites.rects {
            for index in 0..rects.len() {
                self.sprite(group, index)?;
            }
        }
        ensure!(
            self.sprites.number_colors.len() == 18
                && self.sprites.bar_colors.len() == 3
                && self.sprites.names.len() == 9
                && self.sprites.names.iter().all(|name| !name.is_empty()),
            "incomplete menu glyph styles"
        );
        Ok(())
    }
}
