//! Target-ground summons share staging while retaining their own contacts and blessings.
use super::{
    HitElement, StoredSpellPresentation, earth_field::EarthFieldPulse, lightning::GroundSpellOrigin,
};
use crate::{
    battle::effects::{EffectBank, EffectId},
    menu_data::Element,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u16)]
pub enum GroundSummonKind {
    Efreet = 284,
    Undine = 285,
    Sylph = 286,
    Gnome = 287,
    Celsius = 288,
    Volt = 289,
    Shadow = 291,
    Origin = 293,
}

impl GroundSummonKind {
    pub const fn native(self) -> u16 {
        self as u16
    }
    pub const fn menu(self) -> u16 {
        match self {
            Self::Efreet => 235,
            Self::Undine => 236,
            Self::Sylph => 237,
            Self::Gnome => 239,
            Self::Celsius => 240,
            Self::Volt => 241,
            Self::Shadow => 242,
            Self::Origin => 243,
        }
    }
    pub const fn element(self) -> Option<Element> {
        match self {
            Self::Efreet => Some(Element::Fire),
            Self::Undine => Some(Element::Water),
            Self::Sylph => Some(Element::Wind),
            Self::Gnome => Some(Element::Earth),
            Self::Celsius => Some(Element::Ice),
            Self::Volt => Some(Element::Lightning),
            Self::Shadow => Some(Element::Darkness),
            Self::Origin => None,
        }
    }
    pub const fn effect(self, id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(self.native() - 200),
            id,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "settings", rename_all = "snake_case")]
pub enum GroundSummonOrigin {
    World,
    TargetGround(GroundSpellOrigin),
    /// Set target-root Y, then add the captured caster action direction times distance.
    TargetDirection {
        height: f32,
        distance: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "amount", rename_all = "snake_case")]
pub enum SummonBlessing {
    Attack(i16),
    Defense(i16),
    Magic(i16),
    AttackDefense(i16),
    Heal(u16),
    PhysicalImmunity,
    MagicalImmunity,
    Accuracy(i16),
    Speed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummonLanes {
    pub visual_tick: u16,
    /// Radian offsets from the captured heading, shared by visuals and contacts.
    pub heading_offsets: [f32; 4],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SummonVoice {
    pub tick: u16,
    pub voice: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SummonSound {
    pub id: u16,
    pub first_tick: u16,
    pub interval: u16,
    pub count: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummonWaves {
    /// Rotate each offset by captured heading, then add the captured ground point.
    pub offsets: [[f32; 3]; 4],
    pub first_tick: u16,
    pub interval: u16,
    pub projectile_delay: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundSummonRecipe {
    pub kind: GroundSummonKind,
    pub lifetime: u16,
    pub origin: GroundSummonOrigin,
    pub pulses: Vec<EarthFieldPulse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waves: Option<SummonWaves>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lanes: Option<SummonLanes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<SummonSound>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_voices: Vec<SummonVoice>,
    pub presentation: StoredSpellPresentation,
    pub effect_scale: f32,
    pub title: String,
    pub focus_distance: f32,
    pub focus_ticks: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<u16>,
    pub voice_tick: u16,
    pub blessing: SummonBlessing,
    /// Horizontal radius around world origin, independent of the captured contact point.
    pub blessing_radius: f32,
}

impl GroundSummonRecipe {
    pub const BLESSING_START: u16 = 30;
    pub const BLESSING_END: u16 = 80;

    pub fn voices(&self) -> impl Iterator<Item = (u16, u16)> + '_ {
        self.voice
            .map(|voice| (self.voice_tick, voice))
            .into_iter()
            .chain(
                self.extra_voices
                    .iter()
                    .map(|voice| (voice.tick, voice.voice)),
            )
    }

    pub fn validate(&self) -> Result<()> {
        let (blessing, title, contacts) = match self.kind {
            GroundSummonKind::Efreet => (SummonBlessing::Attack(15), "-Efreet-", &[2][..]),
            GroundSummonKind::Gnome => (SummonBlessing::Defense(15), "-Gnome-", &[1, 2][..]),
            GroundSummonKind::Origin => (SummonBlessing::AttackDefense(10), "-Origin-", &[1][..]),
            GroundSummonKind::Undine => (SummonBlessing::Heal(50), "-Undine-", &[1, 1, 1, 1][..]),
            GroundSummonKind::Volt => (SummonBlessing::PhysicalImmunity, "-Volt-", &[1][..]),
            GroundSummonKind::Shadow => (SummonBlessing::MagicalImmunity, "-Shadow-", &[1][..]),
            GroundSummonKind::Celsius => (SummonBlessing::Accuracy(15), "-Celsius-", &[1, 2][..]),
            GroundSummonKind::Sylph => {
                (SummonBlessing::Speed, "-Sylph-", &[1, 2, 2, 2, 2, 2, 3][..])
            }
        };
        ensure!(
            self.lifetime > Self::BLESSING_END
                && self.blessing == blessing
                && self.title == title
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.
                && self.focus_distance.is_finite()
                && self.focus_distance > 0.
                && self.focus_ticks > 0
                && self.focus_ticks < self.lifetime
                && match self.voice {
                    None => self.kind == GroundSummonKind::Volt && self.voice_tick == 0,
                    Some(voice) => self.kind != GroundSummonKind::Volt && voice & 0x8000 != 0,
                }
                && self.voice_tick < self.lifetime
                && self
                    .voices()
                    .all(|(tick, voice)| tick < self.lifetime && voice & 0x8000 != 0)
                && if self.kind == GroundSummonKind::Sylph {
                    self.extra_voices.len() == 2
                        && self.extra_voices[0].tick > self.voice_tick
                        && self.extra_voices[1].tick > self.extra_voices[0].tick
                } else {
                    self.extra_voices.is_empty()
                }
                && self.blessing_radius.is_finite()
                && self.blessing_radius > 0.
                && self.presentation.color[3] == 255
                && self.presentation.camera_distance.is_finite()
                && self.presentation.camera_distance >= 0.
                && (0. ..90.).contains(&self.presentation.camera_elevation),
            "invalid ground summon presentation or blessing"
        );
        ensure!(
            match (self.kind, self.origin) {
                (GroundSummonKind::Celsius, GroundSummonOrigin::World) => true,
                (
                    GroundSummonKind::Efreet
                    | GroundSummonKind::Origin
                    | GroundSummonKind::Shadow
                    | GroundSummonKind::Sylph,
                    GroundSummonOrigin::TargetDirection { height, distance },
                ) => height.is_finite() && distance.is_finite(),
                (
                    GroundSummonKind::Gnome | GroundSummonKind::Undine | GroundSummonKind::Volt,
                    GroundSummonOrigin::TargetGround(origin),
                ) =>
                    origin.height.is_finite()
                        && origin.nudge.is_finite()
                        && origin.nudge >= 0.
                        && origin.direction_threshold.is_finite()
                        && origin.direction_threshold > 0.,
                _ => false,
            },
            "invalid ground summon origin binding"
        );
        ensure!(
            self.pulses.len() == contacts.len()
                && self.pulses.windows(2).all(|p| p[0].tick < p[1].tick),
            "invalid ground summon contact schedule"
        );
        match (self.kind, &self.waves) {
            (GroundSummonKind::Undine, Some(waves)) => {
                ensure!(
                    waves.interval > 0
                        && waves.projectile_delay > 0
                        && waves.offsets.iter().flatten().all(|v| v.is_finite())
                        && self
                            .pulses
                            .iter()
                            .enumerate()
                            .all(|(i, pulse)| u32::from(pulse.tick)
                                == u32::from(waves.first_tick)
                                    + i as u32 * u32::from(waves.interval)
                                    + u32::from(waves.projectile_delay)),
                    "invalid summon wave schedule"
                );
            }
            (GroundSummonKind::Undine, None) | (_, Some(_)) => {
                anyhow::bail!("invalid summon wave binding")
            }
            (_, None) => {}
        }
        match (self.kind, &self.lanes, &self.sound) {
            (GroundSummonKind::Celsius, Some(lanes), Some(sound)) => ensure!(
                lanes.visual_tick < self.pulses[0].tick
                    && lanes.heading_offsets.iter().all(|angle| angle.is_finite())
                    && sound.id != 0
                    && sound.interval > 0
                    && sound.count > 0
                    && u32::from(sound.first_tick)
                        + u32::from(sound.interval) * u32::from(sound.count - 1)
                        < u32::from(self.lifetime),
                "invalid summon lane or sound schedule"
            ),
            (GroundSummonKind::Celsius, _, _) => anyhow::bail!("missing summon lanes or sounds"),
            (_, None, None) => {}
            _ => anyhow::bail!("unexpected summon lanes or sounds"),
        }
        let element = self
            .kind
            .element()
            .map_or(HitElement::Neutral, HitElement::Element);
        for (pulse, &id) in self.pulses.iter().zip(contacts) {
            ensure!(
                pulse.tick < self.lifetime
                    && pulse.projectile == self.kind.effect(id)
                    && pulse.rule.element
                        == if self.kind == GroundSummonKind::Sylph && id == 3 {
                            HitElement::Neutral
                        } else {
                            element
                        },
                "invalid ground summon contact"
            );
            pulse
                .rule
                .impact_program_from(self.kind.effect(1).bank, Some(self.kind.native() - 200))?;
        }
        Ok(())
    }
}
