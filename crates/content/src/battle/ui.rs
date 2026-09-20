//! Original battle artwork with ordinary bitmap text and screen-space layouts.
use crate::font::{BitmapFont, UiRegion, UiTexture};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleUi {
    pub font: BitmapFont,
    /// Character IDs 1–9; four vertically stacked expressions per portrait.
    pub portraits: BTreeMap<u8, UiTexture>,
    pub icons: BTreeMap<u8, UiRegion>,
    pub enemy_icons: BTreeMap<u8, UiTexture>,
    pub sprites: BTreeMap<UiSprite, UiRegion>,
    pub party: PartyPanel,
    pub damage: DamageLayout,
    pub combo: ComboLayout,
    pub notices: NoticeLayout,
    pub steal: StealText,
    pub target: TargetLayout,
    pub scan: ScanLayout,
    pub gauge_bonus_colors: [[[u8; 4]; 2]; 2],
    pub result_labels: BTreeMap<ResultLabel, String>,
    pub result_messages: BTreeMap<ResultMessage, String>,
    pub results: ResultsLayout,
    pub defeat: DefeatLayout,
    pub escape_banner: String,
    /// Older bundles may omit these images until an ailment needs them.
    #[serde(default)]
    pub status: Option<StatusArt>,
    #[serde(default)]
    pub unison: Option<UnisonArt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StealText {
    pub prefix: String,
    pub suffix: String,
    /// Appended after an item name when Rover also steals Gald.
    pub with_gald: String,
    pub gald_only: String,
    pub rover_notice: [String; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnisonArt {
    /// Physical A, B, X, Y; each has two alternating frames.
    pub buttons: [[UiRegion; 2]; 4],
}
impl UnisonArt {
    pub fn textures(&self) -> impl Iterator<Item = &UiRegion> {
        self.buttons.iter().flatten()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusArt {
    pub curse: UiRegion,
    pub curse_cross: UiRegion,
    pub hp_shimmer: UiRegion,
    #[serde(default)]
    pub item_recovery_down: Option<UiRegion>,
    #[serde(default)]
    pub down_arrow: Option<UiRegion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_arrow: Option<UiRegion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack: Option<UiRegion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defense: Option<UiRegion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magic: Option<UiRegion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_ailment_immunity: Option<UiRegion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magical_ailment_immunity: Option<UiRegion>,
    pub badges: BTreeMap<StatusBadge, StatusBadgeArt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum StatusBadge {
    // The draw consumer proves this bit and its palette cycle; its gameplay name is unresolved.
    AnimatedCondition = 6,
    HolySong = 35,
}
impl StatusBadge {
    pub const fn bit(self) -> u64 {
        1 << self as u8
    }
    pub const fn frames(self) -> usize {
        match self {
            Self::AnimatedCondition => 3,
            Self::HolySong => 1,
        }
    }
    pub fn frame(self, tick: u32) -> usize {
        (tick as usize >> 3) % self.frames()
    }
}

/// Regions reference shared palette atlases, without separately cropped texture files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusBadgeArt {
    pub frames: Vec<UiRegion>,
}

impl StatusArt {
    pub fn textures(&self) -> impl Iterator<Item = &UiRegion> {
        [&self.curse, &self.curse_cross, &self.hp_shimmer]
            .into_iter()
            .chain(self.item_recovery_down.iter())
            .chain(self.down_arrow.iter())
            .chain(self.up_arrow.iter())
            .chain(self.attack.iter())
            .chain(self.defense.iter())
            .chain(self.magic.iter())
            .chain(self.physical_ailment_immunity.iter())
            .chain(self.magical_ailment_immunity.iter())
            .chain(self.badges.values().flat_map(|badge| &badge.frames))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum DamageKind {
    Party,
    Enemy,
    Recovery,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum DamageEmphasis {
    #[default]
    Normal,
    Reduced,
    Enhanced,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DamageLayout {
    /// Palettes follow DamageKind; damage emphasis never changes their colors.
    pub palettes: [[[u8; 4]; 2]; 3],
    /// Normal, reduced, and enhanced digit dimensions, independently of the recipient.
    pub sizes: [[u16; 2]; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiSprite {
    TargetArrow,
    TargetPlayerOne,
    TargetPlayerTwo,
    TargetPlayerThree,
    TargetPlayerFour,
    StrategyUp,
    StrategyRight,
    TargetLeft,
    TargetRight,
    CastingGlow,
    EscapeFill,
    EscapeFrame,
    EscapeNeedle,
    CommandFrameShadow,
    CommandFrame,
    CommandTechA,
    CommandTechB,
    CommandTechC,
    CommandUnisonA,
    CommandUnisonB,
    CommandUnisonC,
    CommandStrategyA,
    CommandStrategyB,
    CommandStrategyC,
    CommandStrategyD,
    CommandEquipA,
    CommandEquipB,
    CommandItemA,
    CommandItemB,
    CommandItemC,
    CommandEscapeLeg,
    CommandDisabled,
    CommandCursor,
    CommandLabelShadow,
    CommandLabel,
    UnisonPanel,
    TechniqueGlowMiddle,
    TechniqueGlowEdge,
    TechniqueGlowRight,
    TechniqueCapLeft,
    TechniqueMiddle,
    TechniqueCapRight,
    BannerIcon,
    BannerTech,
    BannerSpell,
    BannerItem,
    BannerSystem,
    ResultNextButton,
    ResultNextButtonPressed,
    CookButton,
    CookButtonPressed,
}
impl UiSprite {
    pub const ALL: [Self; 51] = [
        Self::TargetArrow,
        Self::TargetPlayerOne,
        Self::TargetPlayerTwo,
        Self::TargetPlayerThree,
        Self::TargetPlayerFour,
        Self::StrategyUp,
        Self::StrategyRight,
        Self::TargetLeft,
        Self::TargetRight,
        Self::CastingGlow,
        Self::EscapeFill,
        Self::EscapeFrame,
        Self::EscapeNeedle,
        Self::CommandFrameShadow,
        Self::CommandFrame,
        Self::CommandTechA,
        Self::CommandTechB,
        Self::CommandTechC,
        Self::CommandUnisonA,
        Self::CommandUnisonB,
        Self::CommandUnisonC,
        Self::CommandStrategyA,
        Self::CommandStrategyB,
        Self::CommandStrategyC,
        Self::CommandStrategyD,
        Self::CommandEquipA,
        Self::CommandEquipB,
        Self::CommandItemA,
        Self::CommandItemB,
        Self::CommandItemC,
        Self::CommandEscapeLeg,
        Self::CommandDisabled,
        Self::CommandCursor,
        Self::CommandLabelShadow,
        Self::CommandLabel,
        Self::UnisonPanel,
        Self::TechniqueGlowMiddle,
        Self::TechniqueGlowEdge,
        Self::TechniqueGlowRight,
        Self::TechniqueCapLeft,
        Self::TechniqueMiddle,
        Self::TechniqueCapRight,
        Self::BannerIcon,
        Self::BannerTech,
        Self::BannerSpell,
        Self::BannerItem,
        Self::BannerSystem,
        Self::ResultNextButton,
        Self::ResultNextButtonPressed,
        Self::CookButton,
        Self::CookButtonPressed,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultLabel {
    Experience,
    Bonus,
    MaxCombo,
    Gald,
    Time,
    Grade,
    ItemsFound,
    Cook,
    ExSkillEffect,
    Info,
}
impl ResultLabel {
    pub const ALL: [Self; 10] = [
        Self::Experience,
        Self::Bonus,
        Self::MaxCombo,
        Self::Gald,
        Self::Time,
        Self::Grade,
        Self::ItemsFound,
        Self::Cook,
        Self::ExSkillEffect,
        Self::Info,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultMessage {
    LifeUp,
    LifeUpSibilant,
    SpiritUp,
    SpiritUpSibilant,
    HappinessExperience,
    HappinessGald,
    LearnedTechnique,
    LearnedTitle,
    CookingSuccess,
    CookingFailure,
    CompoundSkill,
}
impl ResultMessage {
    pub const ALL: [Self; 11] = [
        Self::LifeUp,
        Self::LifeUpSibilant,
        Self::SpiritUp,
        Self::SpiritUpSibilant,
        Self::HappinessExperience,
        Self::HappinessGald,
        Self::LearnedTechnique,
        Self::LearnedTitle,
        Self::CookingSuccess,
        Self::CookingFailure,
        Self::CompoundSkill,
    ];

    pub const fn arguments(self) -> usize {
        match self {
            Self::HappinessGald => 0,
            Self::LearnedTechnique => 2,
            _ => 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyPanel {
    pub origin: [i16; 2],
    pub spacing: u16,
    pub portrait_offset: [i16; 2],
    pub portrait_size: [u16; 2],
    /// Inset removes the one-pixel padding around each expression.
    pub portrait_inset: u16,
    pub hp: GaugeStyle,
    pub tp: GaugeStyle,
    pub number: NumberStyle,
    pub bar_shadow: [u8; 4],
    pub lost_value_color: [u8; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaugeStyle {
    pub number_offset: [i16; 2],
    pub bar_offset: [i16; 2],
    pub bar_size: [u16; 2],
    /// Top left, top right, bottom left, bottom right.
    pub colors: [[u8; 4]; 4],
    pub number_colors: [[u8; 4]; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumberStyle {
    pub glyph_size: [u16; 2],
    pub advance: u16,
    /// Horizontal offset of the glyph's top edge, producing the authored slant.
    pub skew: i16,
    pub colors: [[u8; 4]; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComboLayout {
    /// Victim side: party, then enemy.
    pub anchors: [i16; 2],
    /// Foreground top/bottom, then shadow top/bottom.
    pub colors: [[u8; 4]; 4],
    pub panels: [[[u8; 4]; 4]; 2],
    pub hits: String,
    pub damage: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    CriticalDamage,
    GuardBreak,
    StatusEffect,
    GotItem,
    HealHp,
    HealTp,
    LevelUp,
    ItemEffect,
    ExSkillEffect,
    WeaponBreak,
    StatusUp,
    StatusDown,
    NewExSkill,
    MagicEffect,
    Overlimit,
    DefenseDown,
    AccuracyDown,
    EvasionDown,
    StatusCancel,
}
impl NoticeKind {
    pub const ALL: [Self; 19] = [
        Self::CriticalDamage,
        Self::GuardBreak,
        Self::StatusEffect,
        Self::GotItem,
        Self::HealHp,
        Self::HealTp,
        Self::LevelUp,
        Self::ItemEffect,
        Self::ExSkillEffect,
        Self::WeaponBreak,
        Self::StatusUp,
        Self::StatusDown,
        Self::NewExSkill,
        Self::MagicEffect,
        Self::Overlimit,
        Self::DefenseDown,
        Self::AccuracyDown,
        Self::EvasionDown,
        Self::StatusCancel,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoticeLayout {
    pub labels: BTreeMap<NoticeKind, [String; 2]>,
    pub colors: [[u8; 4]; 2],
    pub panels: [[[u8; 4]; 4]; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetLayout {
    pub entry_colors: [[u8; 4]; 4],
    pub entry_text: [u8; 4],
    pub cancel_orders: String,
    pub strategy_colors: [[u8; 4]; 2],
    pub strategy_shadow: [u8; 4],
    pub marker_half_size: [f32; 2],
    pub marker_raise: f32,
    pub marker_follow_speed: f32,
    pub marker_trail_step: f32,
    pub marker_trail_shrink: f32,
    pub marker_color: [u8; 4],
    pub leader: [f32; 4],
    pub colors: [[u8; 4]; 5],
    pub roster_colors: [[u8; 4]; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanLayout {
    /// Physical damage, followed by the eight elemental affinities.
    pub affinities: [UiRegion; 9],
    pub weakness: UiRegion,
    pub resistance: UiRegion,
    pub colors: [[[u8; 4]; 2]; 2],
    /// Sixteenth-pixel reductions from 24-pixel spacing for five to nine icons.
    pub spacing_reduction: [u8; 5],
}
impl ScanLayout {
    pub fn textures(&self) -> impl Iterator<Item = &UiRegion> {
        self.affinities
            .iter()
            .chain([&self.weakness, &self.resistance])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultsLayout {
    pub positions: [[i16; 2]; 6],
    pub colors: [[[u8; 4]; 2]; 6],
    pub numbers: NumberStyle,
    pub strip_color: [u8; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefeatLayout {
    pub banner: String,
    pub background: UiTexture,
    pub caption: String,
    pub choices: [String; 2],
}

impl BattleUi {
    pub fn regions(&self) -> impl Iterator<Item = &UiRegion> {
        self.icons
            .values()
            .chain(self.sprites.values())
            .chain(self.scan.textures())
            .chain(self.status.iter().flat_map(StatusArt::textures))
            .chain(self.unison.iter().flat_map(UnisonArt::textures))
    }

    pub fn assets(&self) -> impl Iterator<Item = &str> {
        [
            self.font.texture.as_str(),
            self.defeat.background.path.as_str(),
        ]
        .into_iter()
        .chain(
            self.portraits
                .values()
                .chain(self.enemy_icons.values())
                .chain(self.regions().map(|region| &region.texture))
                .map(|t| t.path.as_str()),
        )
    }

    pub fn validate(&self) -> Result<()> {
        self.font.validate()?;
        if let Some(unison) = &self.unison {
            ensure!(
                unison.textures().all(|image| image.size() == [32, 32]),
                "invalid Unison button artwork"
            );
        }
        ensure!(
            self.scan.textures().all(|image| image.size() == [24, 24]),
            "invalid battle scan artwork"
        );
        if let Some(status) = &self.status {
            ensure!(
                [StatusBadge::AnimatedCondition, StatusBadge::HolySong]
                    .iter()
                    .all(|badge| status.badges.contains_key(badge)),
                "incomplete battle status badge artwork"
            );
            for (&badge, art) in &status.badges {
                ensure!(
                    art.frames.len() == badge.frames()
                        && art.frames.iter().all(|image| image.size() == [24, 24]),
                    "invalid battle badge region or animation"
                );
            }
            ensure!(
                status
                    .textures()
                    .zip([[24, 24], [24, 24], [64, 32]])
                    .all(|(image, size)| image.size() == size),
                "invalid battle status artwork"
            );
            ensure!(
                ('0'..='9').enumerate().all(|(index, digit)| self
                    .font
                    .glyphs
                    .get(&digit)
                    .is_some_and(|glyph| glyph.rect == [index as u32 * 16, 0, 16, 23])),
                "unsupported battle status digit layout"
            );
        }
        ensure!(
            self.target.leader.iter().all(|v| v.is_finite())
                && self
                    .target
                    .marker_half_size
                    .iter()
                    .all(|v| v.is_finite() && *v > 0.)
                && [
                    self.target.marker_raise,
                    self.target.marker_follow_speed,
                    self.target.marker_trail_step,
                    self.target.marker_trail_shrink
                ]
                .iter()
                .all(|v| v.is_finite())
                && self.target.marker_follow_speed > 0.
                && !self.target.cancel_orders.is_empty()
                && [
                    UiSprite::TargetPlayerOne,
                    UiSprite::TargetPlayerTwo,
                    UiSprite::TargetPlayerThree,
                    UiSprite::TargetPlayerFour
                ]
                .iter()
                .all(|key| self
                    .sprites
                    .get(key)
                    .is_some_and(|image| image.size() == [144, 64])),
            "invalid target leader"
        );
        ensure!(
            self.notices.labels.len() == NoticeKind::ALL.len()
                && NoticeKind::ALL
                    .iter()
                    .all(|kind| self.notices.labels.get(kind).is_some_and(|lines| {
                        lines.iter().all(|text| {
                            !text.is_empty()
                                && text.len() < 12
                                && text
                                    .chars()
                                    .all(|ch| ch == ' ' || self.font.glyphs.contains_key(&ch))
                        })
                    })),
            "invalid battle notice labels"
        );
        ensure!(
            (!self.steal.prefix.is_empty() || !self.steal.suffix.is_empty())
                && [
                    &self.steal.prefix,
                    &self.steal.suffix,
                    &self.steal.with_gald,
                    &self.steal.gald_only
                ]
                .into_iter()
                .all(|text| text.len() <= 64 && !text.chars().any(char::is_control))
                && !self.steal.with_gald.is_empty()
                && !self.steal.gald_only.is_empty()
                && self.steal.rover_notice.iter().all(|text| !text.is_empty()
                    && text.len() < 12
                    && text
                        .chars()
                        .all(|ch| ch == ' ' || self.font.glyphs.contains_key(&ch))),
            "invalid battle steal text"
        );
        ensure!(
            self.combo.anchors.iter().all(|x| (0..640).contains(x))
                && [&self.combo.hits, &self.combo.damage]
                    .into_iter()
                    .all(|text| {
                        !text.is_empty()
                            && text.chars().all(|ch| self.font.glyphs.contains_key(&ch))
                    }),
            "invalid battle combo layout"
        );
        crate::validate_asset_path(&self.defeat.background.path)?;
        ensure!(
            self.defeat.background.width == 640 && self.defeat.background.height == 480,
            "invalid defeat background"
        );
        ensure!(
            [&self.defeat.caption, &self.defeat.banner]
                .into_iter()
                .chain(&self.defeat.choices)
                .all(|text| !text.is_empty() && text.len() <= 128),
            "invalid defeat text"
        );
        ensure!(
            self.portraits.keys().copied().eq(1..=9),
            "missing battle portraits"
        );
        ensure!(
            self.icons.keys().copied().eq(1..=9),
            "missing battle party icons"
        );
        ensure!(
            self.enemy_icons
                .values()
                .all(|image| image.width == 32 && image.height == 32),
            "invalid battle enemy icon"
        );
        ensure!(
            self.sprites.len() == UiSprite::ALL.len()
                && UiSprite::ALL.iter().all(|s| self.sprites.contains_key(s)),
            "missing battle UI artwork"
        );
        ensure!(
            !self.escape_banner.is_empty() && self.escape_banner.len() <= 64,
            "invalid escape banner"
        );
        ensure!(
            self.result_labels.len() == ResultLabel::ALL.len()
                && ResultLabel::ALL.iter().all(|s| self
                    .result_labels
                    .get(s)
                    .is_some_and(|s| !s.is_empty() && s.len() < 64 && !s.contains('%'))),
            "invalid battle result labels"
        );
        ensure!(
            self.result_messages.len() == ResultMessage::ALL.len()
                && ResultMessage::ALL.iter().all(|kind| self
                    .result_messages
                    .get(kind)
                    .is_some_and(|text| !text.is_empty()
                        && text.len() <= 128
                        && text.matches("%s").count() == kind.arguments()
                        && text.split("%s").all(
                            |part| !part.contains('%') && !part.chars().any(char::is_control)
                        ))),
            "invalid battle result messages"
        );
        for region in self.regions() {
            region.validate()?;
        }
        for texture in self
            .portraits
            .values()
            .chain(self.enemy_icons.values())
            .chain(self.regions().map(|region| &region.texture))
        {
            crate::validate_asset_path(&texture.path)?;
            ensure!(
                (1..=512).contains(&texture.width) && (1..=512).contains(&texture.height),
                "invalid battle UI texture size"
            );
        }
        ensure!(
            self.portraits
                .values()
                .all(|t| t.width == 64 && t.height == 256),
            "invalid battle portrait expression atlas"
        );
        let p = &self.party;
        ensure!(
            (1..=160).contains(&p.spacing) && p.portrait_size == [64, 64] && p.portrait_inset <= 4,
            "invalid battle party layout"
        );
        for gauge in [&p.hp, &p.tp] {
            ensure!(
                gauge.bar_size.iter().all(|v| (1..=128).contains(v)),
                "invalid battle gauge size"
            );
        }
        ensure!(
            self.damage
                .sizes
                .iter()
                .flatten()
                .all(|v| (1..=64).contains(v)),
            "invalid battle damage sizes"
        );
        for style in [&p.number, &self.results.numbers] {
            ensure!(
                style.glyph_size.iter().all(|v| (1..=64).contains(v))
                    && (1..=64).contains(&style.advance)
                    && style.skew.abs() <= 32,
                "invalid battle number style"
            );
        }
        Ok(())
    }
}
