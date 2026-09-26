//! Original battle artwork and fixed 640×480 layouts, without presentation state.
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
    /// The two-texture text pass has different punctuation coordinates.
    pub overlay_punctuation: [[u16; 2]; 15],
    /// Character IDs 1–9; four vertically stacked 64×64 expressions each.
    /// Portrait sampling is linear with repeated edges.
    pub portraits: [UiTexture; 9],
    /// Original degree-indexed sine samples 0..450 from BTLusual member 5.
    /// Native cosine lookup uses this same table at index 90 + angle.
    pub sine: Vec<f32>,
    pub party: PartyPanel,
    pub results: ResultsPanel,
    pub overlays: Overlays,
    pub markers: Markers,
    pub combat_number_colors: [[[u8; 4]; 2]; 3],
    pub combat_number_sizes: [[i16; 2]; 3],
    pub recovery: RecoveryNumbers,
    pub intro: Intro,
    pub orders: Orders,
    pub radar: Radar,
    pub combo: Combo,
    pub commands: CommandArt,
}

/// Battle command strip, sourced from REL 63F8 and bank 12/member 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandArt {
    pub background: UiTexture,
    pub icons: [Vec<UiTexture>; 6],
    pub disabled: UiTexture,
    pub plate: UiTexture,
    pub shadow: UiTexture,
    pub cursor: UiTexture,
    pub cursor_shadow: UiTexture,
    pub names: [String; 6],
    pub player_format: String,
    pub player_colors: [[u8; 4]; 2],
    pub text_color: [u8; 4],
    pub shadow_color: [u8; 4],
    pub motion: CommandMotion,
    pub cursor_amplitude: f32,
}

/// Original command motion operands; all values are imported from REL rodata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandMotion {
    pub selected_shade_center: f32,
    pub selected_shade_amplitude: f32,
    pub bob_amplitude: f32,
    pub y_rotation: f32,
    pub small_bob_amplitude: f32,
    pub strategy_rotation_amplitude: f32,
    pub lift_amplitude: f32,
    pub label_bob_amplitude: f32,
    pub item_rotation_amplitude: f32,
    pub item_sway_amplitude: f32,
    pub escape_rotation_amplitude: f32,
}

impl CommandMotion {
    fn valid(&self) -> bool {
        [
            self.selected_shade_center,
            self.selected_shade_amplitude,
            self.bob_amplitude,
            self.y_rotation,
            self.small_bob_amplitude,
            self.strategy_rotation_amplitude,
            self.lift_amplitude,
            self.label_bob_amplitude,
            self.item_rotation_amplitude,
            self.item_sway_amplitude,
            self.escape_rotation_amplitude,
        ]
        .into_iter()
        .all(f32::is_finite)
    }
}

impl CommandArt {
    pub fn textures(&self) -> impl Iterator<Item = &UiTexture> {
        [
            &self.background,
            &self.disabled,
            &self.plate,
            &self.shadow,
            &self.cursor,
            &self.cursor_shadow,
        ]
        .into_iter()
        .chain(self.icons.iter().flatten())
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.icons.iter().map(Vec::len).eq([3, 3, 4, 2, 3, 2]),
            "invalid battle command layer counts"
        );
        for texture in self.textures() {
            texture.validate()?;
            ensure!(
                [texture.width, texture.height] == [512, 512],
                "invalid battle command atlas size"
            );
        }
        ensure!(
            self.motion.valid() && self.cursor_amplitude.is_finite(),
            "invalid command animation operands"
        );
        ensure!(
            self.names
                .iter()
                .chain([&self.player_format])
                .all(|text| !text.is_empty()
                    && text.len() <= 128
                    && !text.chars().any(char::is_control)),
            "invalid battle command text"
        );
        ensure!(
            self.player_format.matches("%d").count() == 1,
            "invalid battle command player format"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intro {
    pub panel_colors: [[u8; 4]; 4],
    pub text_color: [u8; 4],
    pub hidden_name_symbol: String,
    pub group_count_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Orders {
    pub cancel_text: String,
    pub name_format: String,
    pub panel_colors: [[u8; 4]; 2],
    pub shadow: [u8; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Radar {
    pub number_color: [u8; 4],
    pub initial_ordinals: [u8; 4],
    pub panel_colors: [[u8; 4]; 4],
    pub shadow: [u8; 4],
    /// Additively blended halo while an enemy is chanting.
    pub effect: Sprite,
    pub pulse_amplitude: f32,
    pub shade_center: f32,
    pub target_size_amplitude: f32,
    pub target_size_center: f64,
    pub effect_size_amplitude: f32,
    pub effect_size_center: f32,
    pub selection_color_amplitude: f32,
    pub selection_blue_center: f32,
    pub radians_per_degree: f32,
    pub depth: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Combo {
    /// Party and enemy-side positions, from distinct draw and movement tables.
    pub anchors: [i16; 2],
    pub destinations: [i16; 2],
    pub colors: [[u8; 4]; 8],
    pub panel_colors: [[u8; 4]; 8],
    pub hits: String,
    pub count_format: String,
    pub damage_format: String,
    pub damage_suffix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryNumbers {
    /// Right edge of the first party member's HP row.
    pub origin: [i16; 2],
    pub party_spacing: u16,
    pub row_spacing: u16,
    /// Digit zero; following digits advance by its source width.
    pub rect: [u16; 4],
    pub number: Number,
    pub colors: [[[u8; 4]; 2]; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartyPanel {
    pub origin: [i16; 2],
    pub spacing: u16,
    pub portrait_offset: [i16; 2],
    pub portrait_size: [u16; 2],
    pub portrait_inset: u16,
    pub hp: Gauge,
    pub tp: Gauge,
    pub number: Number,
    pub number_shadow_offset: [i16; 2],
    pub bar_shadow_offset: [i16; 2],
    /// Original GX colors; the battle UI texture pass doubles RGB modulation.
    pub shadow: [u8; 4],
    pub lost_value_color: [u8; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gauge {
    pub number_offset: [i16; 2],
    pub bar_offset: [i16; 2],
    pub bar_size: [u16; 2],
    pub skew: i16,
    /// Top left, top right, bottom left, bottom right.
    pub colors: [[u8; 4]; 4],
    /// Initial source gradient; HP display overrides this for health/conditions.
    pub number_colors: [[u8; 4]; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Number {
    pub glyph_size: [u16; 2],
    pub advance: u16,
    pub skew: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultsPanel {
    /// EXP, bonus, max combo, Gald, time, grade.
    pub positions: [[i16; 2]; 6],
    pub strips: [[i16; 4]; 6],
    pub window_y_scale: f32,
    pub character_icons: [Sprite; 9],
    /// Technique acquisition, title acquisition, Compound EX discovery.
    pub notice_formats: [String; 3],
    /// Original printf formats, including separate positive/negative grade rows.
    pub formats: [String; 7],
    /// Item(s) found, cooking, EX skill effect, information.
    pub headings: [String; 4],
    pub colors: [[[u8; 4]; 2]; 6],
    pub heading_colors: [[u8; 4]; 2],
    pub shadows: [[u8; 4]; 3],
    pub strip_color: [u8; 4],
    pub item_colors: [[u8; 4]; 5],
    pub number: Number,
    pub next_button: [Sprite; 2],
    pub cook_button: [Sprite; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Markers {
    /// Original sine samples at [90 + angle, angle], for angle = 24 * vertex.
    /// The shadow consumer negates the second component for world Z.
    pub shadow_circle: [[f32; 2]; 15],
    pub target_background: Sprite,
    pub target_foregrounds: [UiTexture; 4],
    pub target_frames: [[u32; 4]; 3],
    pub stun: UiTexture,
    pub stun_frames: [[u32; 4]; 2],
    pub stun_period_ticks: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlays {
    pub notice_texts: [String; 2],
    pub notice_background: UiTexture,
    pub notice_shadow: UiTexture,
    pub notice_symbols: [Sprite; 4],
    pub notice_text_color: [u8; 4],
    /// Level Up, New EX Skill, Critical Damage; original kinds7,13,1.
    pub actor_lines: [[String; 2]; 3],
    pub actor_colors: [[u8; 4]; 2],
    pub actor_bars: [[u8; 4]; 8],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sprite {
    pub texture: UiTexture,
    pub rect: [u32; 4],
}

impl Sprite {
    pub fn validate(&self) -> Result<()> {
        self.texture.validate()?;
        let [x, y, w, h] = self.rect;
        ensure!(
            w > 0
                && h > 0
                && x.checked_add(w).is_some_and(|v| v <= self.texture.width)
                && y.checked_add(h).is_some_and(|v| v <= self.texture.height),
            "battle sprite exceeds texture"
        );
        Ok(())
    }
}

impl Art {
    pub const VERSION: u32 = 5;

    pub fn files(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.font.texture.as_str())
            .chain(self.commands.textures().map(|image| image.path.as_str()))
            .chain(self.portraits.iter().map(|image| image.path.as_str()))
            .chain(
                self.results
                    .next_button
                    .iter()
                    .chain(&self.results.cook_button)
                    .chain(&self.results.character_icons)
                    .map(|sprite| sprite.texture.path.as_str()),
            )
            .chain([
                self.overlays.notice_background.path.as_str(),
                self.overlays.notice_shadow.path.as_str(),
                self.radar.effect.texture.path.as_str(),
                self.markers.target_background.texture.path.as_str(),
                self.markers.stun.path.as_str(),
            ])
            .chain(
                self.overlays
                    .notice_symbols
                    .iter()
                    .map(|sprite| sprite.texture.path.as_str()),
            )
            .chain(
                self.markers
                    .target_foregrounds
                    .iter()
                    .map(|texture| texture.path.as_str()),
            )
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == Self::VERSION,
            "unsupported battle UI version"
        );
        self.font.validate()?;
        self.commands.validate()?;
        ensure!(
            self.sine.len() == 450
                && self
                    .sine
                    .iter()
                    .all(|v| v.is_finite() && (-1. ..=1.).contains(v)),
            "invalid battle degree lookup table"
        );
        self.markers.target_background.validate()?;
        ensure!(
            self.markers
                .shadow_circle
                .iter()
                .flatten()
                .all(|v| v.is_finite()),
            "invalid battle shadow directions"
        );
        self.markers.stun.validate()?;
        for texture in &self.markers.target_foregrounds {
            texture.validate()?;
        }
        ensure!(
            self.markers.stun_period_ticks != 0,
            "invalid stun caption period"
        );
        self.overlays.notice_background.validate()?;
        self.overlays.notice_shadow.validate()?;
        for sprite in &self.overlays.notice_symbols {
            sprite.validate()?;
        }
        self.radar.effect.validate()?;
        ensure!(
            [
                self.radar.pulse_amplitude,
                self.radar.shade_center,
                self.radar.target_size_amplitude,
                self.radar.effect_size_amplitude,
                self.radar.effect_size_center,
                self.radar.selection_color_amplitude,
                self.radar.selection_blue_center,
                self.radar.radians_per_degree,
                self.radar.depth,
            ]
            .into_iter()
            .all(f32::is_finite)
                && self.radar.target_size_center.is_finite(),
            "invalid battle radar operands"
        );
        ensure!(
            [
                &self.intro.hidden_name_symbol,
                &self.intro.group_count_format,
                &self.orders.cancel_text,
                &self.orders.name_format,
                &self.combo.hits,
                &self.combo.count_format,
                &self.combo.damage_format,
                &self.combo.damage_suffix,
            ]
            .into_iter()
            .all(|text| !text.is_empty()
                && text.len() <= 128
                && !text.chars().any(char::is_control)),
            "invalid battle HUD text"
        );
        ensure!(
            self.results.window_y_scale.is_finite() && self.results.window_y_scale > 0.,
            "invalid result window scale"
        );
        for portrait in &self.portraits {
            portrait.validate()?;
            ensure!(
                [portrait.width, portrait.height] == [64, 256],
                "invalid battle portrait expressions"
            );
        }
        ensure!(
            self.overlay_punctuation.iter().all(|&[x, y]| {
                u32::from(x) + 16 <= self.font.width && u32::from(y) + 23 <= self.font.height
            }),
            "battle punctuation exceeds font"
        );
        for gauge in [&self.party.hp, &self.party.tp] {
            ensure!(
                gauge.bar_size.iter().all(|&v| (1..=640).contains(&v))
                    && (-64..=64).contains(&gauge.skew),
                "invalid battle gauge size"
            );
        }
        ensure!(
            (1..=640).contains(&self.party.spacing)
                && self
                    .party
                    .portrait_size
                    .iter()
                    .all(|&v| (1..=256).contains(&v))
                && self.party.portrait_inset < 32,
            "invalid battle party layout"
        );
        for number in [
            &self.party.number,
            &self.results.number,
            &self.recovery.number,
        ] {
            ensure!(
                number.glyph_size.iter().all(|&v| (1..=64).contains(&v))
                    && (1..=64).contains(&number.advance)
                    && (-64..=64).contains(&number.skew),
                "invalid battle number layout"
            );
        }
        let [x, y, width, height] = self.recovery.rect.map(u32::from);
        ensure!(
            width > 0
                && height > 0
                && x + width * 10 <= self.font.width
                && y + height <= self.font.height
                && (1..=640).contains(&self.recovery.party_spacing)
                && (1..=480).contains(&self.recovery.row_spacing)
                && self
                    .combat_number_sizes
                    .iter()
                    .flatten()
                    .all(|&v| (1..=64).contains(&v)),
            "invalid battle feedback number layout"
        );
        for sprite in self
            .results
            .next_button
            .iter()
            .chain(&self.results.cook_button)
            .chain(&self.results.character_icons)
        {
            sprite.validate()?;
        }
        ensure!(
            self.results
                .formats
                .iter()
                .chain(&self.results.headings)
                .chain(&self.results.notice_formats)
                .all(|s| {
                    !s.is_empty()
                        && s.len() <= 128
                        && s.chars().all(|c| c.is_ascii_graphic() || c == ' ')
                }),
            "invalid battle result text"
        );
        Ok(())
    }
}
