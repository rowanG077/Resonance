//! Battle artwork and fixed 640×480 layouts, without presentation state.
use crate::font::{BitmapFont, UiTexture};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "battle/ui.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Art {
    pub version: u32,
    /// Atlas sampling is linear with clamped edges.
    pub font: BitmapFont,
    /// Character IDs 1–9; four vertically stacked 64×64 expressions each.
    /// Portrait sampling is linear with repeated edges.
    #[serde(default)]
    pub portraits: [Option<UiTexture>; 9],
    pub meters: MeterColors,
    pub damage: DamageNumbers,
    /// HP and TP recovery gradients, ordered top and bottom.
    pub recovery_colors: [[[u8; 4]; 2]; 2],
    /// Experience, other statistics, and notice headings; top and bottom colors.
    pub result_colors: [[[u8; 4]; 2]; 3],
    /// Shared target frame followed by one pointer palette per controller.
    pub targets: [UiTexture; 5],
    pub stun: UiTexture,
    pub notices: NoticeArt,
    pub combo: ComboArt,
    pub stat_colors: [[u8; 4]; 2],
    /// Two opposed blue/gold feedback strips; TL, TR, BL, BR corners.
    pub stat_backdrops: [[[u8; 4]; 4]; 2],
    /// Attack, defense, increase arrow, magic attack, physical protection, magical protection.
    pub stat_icons: [UiTexture; 6],
    /// Physical Unison buttons: A/B palette, then X/Y palette.
    pub unison_buttons: [UiTexture; 2],
    pub commands: CommandArt,
    pub feedback: FeedbackArt,
}

/// Timers, enemy inspection, and battle gauges use the shared battle atlas.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackArt {
    pub item_label: String,
    pub item_bar: [[u8; 4]; 4],
    pub item_text: [[u8; 4]; 2],
    pub scan_vitals: [[[u8; 4]; 2]; 2],
    /// Neutral and eight elements, followed by weakness/resistance headings.
    pub scan_icons: [UiTexture; 10],
    pub unison_frame: UiTexture,
    pub unison_ready: String,
    pub unison_text: [[u8; 4]; 2],
    /// Frame, needle, and fill.
    pub escape: [UiTexture; 3],
    pub escape_positions: [[u16; 2]; 2],
}

/// Command sprites share one atlas so overlapping emblems use an ordinary sprite batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandArt {
    pub texture: UiTexture,
    /// Tech, Unison, Strategy, Equipment, Item, Escape; Unison ends with three sparkle frames.
    pub icons: [Vec<[u16; 4]>; 6],
    pub faces: [[u16; 4]; 9],
    pub background: [u16; 4],
    pub shadow: [u16; 4],
    pub disabled: [u16; 4],
    pub plate: [u16; 4],
    pub cursor: [u16; 4],
    pub cursor_shadow: [u16; 4],
    pub names: [String; 6],
    pub cancel_escape: String,
    pub player_format: String,
    pub player_colors: [[u8; 4]; 2],
    pub text_color: [u8; 4],
    pub shadow_color: [u8; 4],
    pub pending_item_color: [u8; 4],
    pub selector_labels: [String; 2],
    pub selector_colors: [[[u8; 4]; 2]; 2],
    pub selector_plate_colors: [[u8; 4]; 4],
    pub motion: CommandMotion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandMotion {
    pub selected_shade: [f32; 2],
    pub bob: f32,
    pub tilt: f32,
    pub small_bob: f32,
    pub strategy_rotation: f32,
    pub lift: f32,
    pub label_bob: f32,
    pub item_sway: f32,
    pub escape_rotation: f32,
    pub cursor_bob: f32,
    pub selector_bob: f32,
}

impl CommandArt {
    fn validate(&self) -> Result<()> {
        self.texture.validate()?;
        let motion = &self.motion;
        ensure!(
            motion
                .selected_shade
                .into_iter()
                .chain([
                    motion.bob,
                    motion.tilt,
                    motion.small_bob,
                    motion.strategy_rotation,
                    motion.lift,
                    motion.label_bob,
                    motion.item_sway,
                    motion.escape_rotation,
                    motion.cursor_bob,
                    motion.selector_bob,
                ])
                .all(f32::is_finite),
            "invalid battle command motion"
        );
        ensure!(
            self.names.iter().all(|name| !name.is_empty())
                && self.player_format.matches("%d").count() == 1,
            "invalid battle command labels"
        );
        for (icons, count) in self.icons.iter().zip([3, 5, 4, 2, 3, 1]) {
            ensure!(icons.len() == count, "incomplete battle command emblem");
        }
        for &[x, y, width, height] in self.icons.iter().flatten().chain(&self.faces).chain([
            &self.background,
            &self.shadow,
            &self.disabled,
            &self.plate,
            &self.cursor,
            &self.cursor_shadow,
        ]) {
            ensure!(
                width > 0
                    && height > 0
                    && u32::from(x) + u32::from(width) <= self.texture.width
                    && u32::from(y) + u32::from(height) <= self.texture.height,
                "battle command sprite exceeds its atlas"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoticeArt {
    pub frame: UiTexture,
    pub fill: UiTexture,
    /// Martial arte, spell, and item emblems.
    pub icons: [UiTexture; 3],
    pub icon_origins: [[u16; 2]; 3],
    pub faces: [UiTexture; 9],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComboArt {
    pub anchors: [u16; 2],
    pub number_colors: [[u8; 4]; 2],
    pub bar_colors: [[[u8; 4]; 4]; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DamageNumbers {
    /// Party and enemy damage gradients, ordered top and bottom.
    pub colors: [[[u8; 4]; 2]; 2],
    /// Ordinary, guarded, and critical/weakness glyph dimensions.
    pub sizes: [[u16; 2]; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeterColors {
    /// HP, TP; each gradient is ordered top-left, top-right, bottom-left, bottom-right.
    pub bars: [[[u8; 4]; 4]; 2],
    /// HP, TP; top and bottom text colors.
    pub numbers: [[[u8; 4]; 2]; 2],
    pub trail: [u8; 4],
    pub shadow: [u8; 4],
}

fn validate_portrait(image: &UiTexture) -> Result<()> {
    image.validate()?;
    ensure!(
        [image.width, image.height] == [64, 256],
        "invalid battle portrait expressions"
    );
    Ok(())
}

impl Art {
    pub const VERSION: u32 = 43;

    /// Decode required controls and independently admit optional feedback.
    pub fn load(files: &crate::prepared::Files) -> Result<Self> {
        let mut art = Self::decode(&files.read(PATH)?, files.diagnostics())?;
        for portrait in &mut art.portraits {
            if let Some(image) = portrait
                && files
                    .diagnostics()
                    .attempt("battle portrait image", files.read(&image.path))?
                    .is_none()
            {
                *portrait = None;
            }
        }
        Ok(art)
    }

    pub fn decode(bytes: &[u8], diagnostics: &crate::diagnostics::Diagnostics) -> Result<Self> {
        #[derive(Deserialize)]
        struct Source {
            #[serde(flatten)]
            art: Art,
            #[serde(default)]
            portraits: serde_json::Value,
        }
        let mut source: Source = serde_json::from_slice(bytes)?;
        if let Some(values) = diagnostics
            .attempt(
                "battle portraits",
                serde_json::from_value::<Option<[serde_json::Value; 9]>>(source.portraits)
                    .map_err(anyhow::Error::from),
            )?
            .flatten()
        {
            for (portrait, value) in source.art.portraits.iter_mut().zip(values) {
                *portrait = diagnostics
                    .attempt(
                        "battle portrait",
                        (|| {
                            let portrait: Option<UiTexture> = serde_json::from_value(value)?;
                            if let Some(image) = &portrait {
                                validate_portrait(image)?;
                            }
                            Ok(portrait)
                        })(),
                    )?
                    .flatten();
            }
        }
        source.art.validate_layout()?;
        Ok(source.art)
    }

    pub fn validate_required_dialogue(&self, font: &BitmapFont) -> Result<()> {
        font.validate_text(
            "0123456789 HP TP / CASTING Unknown PARTY ENEMY HITS DAMAGE STUN DEFEATED ESCAPED:",
        )
    }

    pub fn files(&self) -> impl Iterator<Item = &str> {
        [
            self.font.texture.as_str(),
            self.commands.texture.path.as_str(),
        ]
        .into_iter()
        .chain(
            self.portraits
                .iter()
                .flatten()
                .map(|image| image.path.as_str()),
        )
        .chain(
            self.targets
                .iter()
                .chain([&self.stun])
                .chain([&self.notices.frame, &self.notices.fill])
                .chain(&self.notices.icons)
                .chain(&self.notices.faces)
                .chain(&self.stat_icons)
                .chain(&self.unison_buttons)
                .chain(&self.feedback.scan_icons)
                .chain([&self.feedback.unison_frame])
                .chain(&self.feedback.escape)
                .map(|image| image.path.as_str()),
        )
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_layout()?;
        for portrait in self.portraits.iter().flatten() {
            validate_portrait(portrait)?;
        }
        Ok(())
    }

    /// Required controls and shared geometry; optional feedback validates separately.
    pub fn validate_layout(&self) -> Result<()> {
        ensure!(
            self.version == Self::VERSION,
            "unsupported battle UI version"
        );
        self.font.validate()?;
        self.commands.validate()?;
        self.font.validate_text(&self.feedback.item_label)?;
        self.font.validate_text(&self.feedback.unison_ready)?;
        ensure!(
            self.damage
                .sizes
                .iter()
                .flatten()
                .all(|&size| (1..=128).contains(&size)),
            "invalid damage numeral dimensions"
        );
        for image in self
            .targets
            .iter()
            .chain([&self.stun, &self.notices.frame, &self.notices.fill])
            .chain(&self.notices.icons)
            .chain(&self.notices.faces)
            .chain(&self.stat_icons)
            .chain(&self.unison_buttons)
            .chain(&self.feedback.scan_icons)
            .chain([&self.feedback.unison_frame])
            .chain(&self.feedback.escape)
        {
            image.validate()?;
            ensure!(
                [image.width, image.height] == [512, 512],
                "invalid battle marker atlas"
            );
        }
        Ok(())
    }
}
