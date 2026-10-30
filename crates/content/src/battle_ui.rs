//! Battle artwork and fixed 640×480 layouts, without presentation state.
use crate::font::{BitmapFont, UiTexture};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
pub mod document;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Art<Font = BitmapFont> {
    /// Atlas sampling is linear with clamped edges.
    pub font: Font,
    /// Character IDs 1–9; four vertically stacked 64×64 expressions each.
    /// Portrait sampling is linear with repeated edges.
    #[serde(default)]
    #[serde(with = "crate::ui_art::portraits")]
    pub portraits: [Option<UiTexture>; 9],
    pub meters: MeterColors,
    pub damage: DamageNumbers,
    /// HP and TP recovery gradients, ordered top and bottom.
    #[serde(with = "crate::ui_art::vitals")]
    pub recovery_colors: [[[u8; 4]; 2]; 2],
    /// Experience, other statistics, and notice headings; top and bottom colors.
    #[serde(with = "crate::ui_art::results")]
    pub result_colors: [[[u8; 4]; 2]; 3],
    /// Shared target frame followed by one pointer palette per controller.
    #[serde(with = "crate::ui_art::target_images")]
    pub targets: [UiTexture; 5],
    #[serde(with = "crate::ui_art::image_path")]
    pub stun: UiTexture,
    pub notices: NoticeArt,
    pub combo: ComboArt,
    #[serde(with = "crate::ui_art::vertical")]
    pub stat_colors: [[u8; 4]; 2],
    /// Two opposed blue/gold feedback strips; TL, TR, BL, BR corners.
    #[serde(with = "crate::ui_art::lines")]
    pub stat_backdrops: [[[u8; 4]; 4]; 2],
    /// Attack, defense, up arrow, magic attack, physical/magical barriers,
    /// accuracy/curse, reduced item effect, Holy Song, down arrow.
    #[serde(with = "crate::ui_art::status_images")]
    pub stat_icons: [UiTexture; 10],
    /// Source bank roles retained in the art document; runtime prompts use the shared input atlas.
    #[serde(with = "crate::ui_art::unison_images")]
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
    #[serde(with = "crate::ui_art::vitals")]
    pub scan_vitals: [[[u8; 4]; 2]; 2],
    /// Neutral and eight elements, followed by weakness/resistance headings.
    #[serde(with = "crate::ui_art::scan_images")]
    pub scan_icons: [UiTexture; 10],
    #[serde(with = "crate::ui_art::image_path")]
    pub unison_frame: UiTexture,
    pub unison_ready: String,
    pub unison_text: [[u8; 4]; 2],
    /// Frame, needle, and fill.
    #[serde(with = "crate::ui_art::escape_images")]
    pub escape: [UiTexture; 3],
    #[serde(with = "crate::ui_art::sides")]
    pub escape_positions: [[u16; 2]; 2],
}

/// Command sprites share one atlas so overlapping emblems use an ordinary sprite batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandArt {
    #[serde(with = "crate::ui_art::image_path")]
    pub texture: UiTexture,
    /// Tech, Unison, Strategy, Equipment, Item, Escape; Unison ends with three sparkle frames.
    #[serde(with = "crate::ui_art::commands")]
    pub icons: [Vec<[u16; 4]>; 6],
    #[serde(with = "crate::ui_art::characters")]
    pub faces: [[u16; 4]; 9],
    pub background: [u16; 4],
    pub shadow: [u16; 4],
    pub disabled: [u16; 4],
    pub plate: [u16; 4],
    pub cursor: [u16; 4],
    pub cursor_shadow: [u16; 4],
    #[serde(with = "crate::ui_art::commands")]
    pub names: [String; 6],
    pub cancel_escape: String,
    pub player_format: String,
    pub player_colors: [[u8; 4]; 2],
    pub text_color: [u8; 4],
    pub shadow_color: [u8; 4],
    pub pending_item_color: [u8; 4],
    #[serde(with = "crate::ui_art::selectors")]
    pub selector_labels: [String; 2],
    #[serde(with = "crate::ui_art::selectors")]
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
    #[serde(with = "crate::ui_art::image_path")]
    pub frame: UiTexture,
    #[serde(with = "crate::ui_art::image_path")]
    pub fill: UiTexture,
    /// Martial arte, spell, item, and outcome emblems.
    #[serde(with = "crate::ui_art::notice_images")]
    pub icons: [UiTexture; 4],
    #[serde(with = "crate::ui_art::notices")]
    pub icon_origins: [[u16; 2]; 4],
    pub defeated: String,
    #[serde(with = "crate::ui_art::face_images")]
    pub faces: [UiTexture; 9],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComboArt {
    #[serde(with = "crate::ui_art::damage_targets")]
    pub anchors: [u16; 2],
    #[serde(with = "crate::ui_art::vertical")]
    pub number_colors: [[u8; 4]; 2],
    #[serde(with = "crate::ui_art::damage_targets")]
    pub bar_colors: [[[u8; 4]; 4]; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DamageNumbers {
    /// Party and enemy damage gradients, ordered top and bottom.
    #[serde(with = "crate::ui_art::damage_targets")]
    pub colors: [[[u8; 4]; 2]; 2],
    /// Ordinary, guarded, and critical/weakness glyph dimensions.
    #[serde(with = "crate::ui_art::damage_sizes")]
    pub sizes: [[u16; 2]; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeterColors {
    /// HP, TP; each gradient is ordered top-left, top-right, bottom-left, bottom-right.
    #[serde(with = "crate::ui_art::vitals")]
    pub bars: [[[u8; 4]; 4]; 2],
    /// HP, TP; top and bottom text colors.
    #[serde(with = "crate::ui_art::vitals")]
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
    /// Decode required controls and independently admit optional feedback.
    pub fn load(files: &crate::prepared::Files) -> Result<Self> {
        document::read(&mut |path| files.read(path), files.diagnostics())
    }

    pub fn validate_required_dialogue(&self, font: &BitmapFont) -> Result<()> {
        font.validate_text(
            "0123456789 HP TP / CASTING Unknown PARTY ENEMY HITS DAMAGE STUN DEFEATED ESCAPED:",
        )?;
        font.validate_text(&self.notices.defeated)
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

    fn required_images_mut(&mut self) -> impl Iterator<Item = &mut UiTexture> {
        std::iter::once(&mut self.commands.texture)
            .chain(&mut self.targets)
            .chain([
                &mut self.stun,
                &mut self.notices.frame,
                &mut self.notices.fill,
            ])
            .chain(&mut self.notices.icons)
            .chain(&mut self.notices.faces)
            .chain(&mut self.stat_icons)
            .chain(&mut self.feedback.scan_icons)
            .chain([&mut self.feedback.unison_frame])
            .chain(&mut self.feedback.escape)
    }

    /// Required controls and shared geometry; optional feedback validates separately.
    pub fn validate_layout(&self) -> Result<()> {
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

impl<Font> Art<Font> {
    pub fn with_font<Other>(self, font: Other) -> Art<Other> {
        Art {
            font,
            portraits: self.portraits,
            meters: self.meters,
            damage: self.damage,
            recovery_colors: self.recovery_colors,
            result_colors: self.result_colors,
            targets: self.targets,
            stun: self.stun,
            notices: self.notices,
            combo: self.combo,
            stat_colors: self.stat_colors,
            stat_backdrops: self.stat_backdrops,
            stat_icons: self.stat_icons,
            unison_buttons: self.unison_buttons,
            commands: self.commands,
            feedback: self.feedback,
        }
    }
}

impl Art<String> {
    fn decode_fields(bytes: &[u8], diagnostics: &crate::diagnostics::Diagnostics) -> Result<Self> {
        let mut source: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(bytes)?;
        let portraits = source.remove("portraits").unwrap_or_default();
        let mut art: Self = serde_json::from_value(source.into())?;
        if let Some(values) = diagnostics
            .attempt(
                "battle portraits",
                serde_json::from_value::<
                    Option<std::collections::BTreeMap<String, serde_json::Value>>,
                >(portraits)
                .map_err(anyhow::Error::from),
            )?
            .flatten()
        {
            for (name, value) in values {
                let Some((index, portrait)) = diagnostics.attempt(
                    "battle portrait",
                    (|| {
                        let index = crate::ui_art::characters::NAMES
                            .iter()
                            .position(|role| *role == name)
                            .ok_or_else(|| anyhow::anyhow!("unknown portrait character {name}"))?;
                        let portrait: Option<String> = serde_json::from_value(value)?;
                        if let Some(path) = &portrait {
                            crate::validate_asset_path(path)?;
                        }
                        Ok((index, portrait.map(crate::ui_art::image)))
                    })(),
                )?
                else {
                    continue;
                };
                art.portraits[index] = portrait;
            }
        }
        Ok(art)
    }
}
