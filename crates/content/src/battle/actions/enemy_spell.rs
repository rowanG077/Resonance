//! Chanting and action commands retain separate native admission rules.
use super::{TechniqueAction, TechniqueProgram};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum DirectSpell {
    AquaEdge = 200,
    FireBall = 204,
    WindBlade = 208,
    Lightning = 216,
    GravityWell = 228,
}

impl DirectSpell {
    pub const fn from_native(native: u16) -> Option<Self> {
        Some(match native {
            200 => Self::AquaEdge,
            204 => Self::FireBall,
            208 => Self::WindBlade,
            216 => Self::Lightning,
            228 => Self::GravityWell,
            _ => return None,
        })
    }

    fn matches(self, program: &TechniqueProgram) -> bool {
        match (self, program) {
            (Self::AquaEdge, TechniqueProgram::AquaEdge { .. })
            | (Self::FireBall, TechniqueProgram::FireBall { .. })
            | (Self::WindBlade, TechniqueProgram::WindBlade { .. }) => true,
            (Self::Lightning, TechniqueProgram::Lightning { recipe, .. }) => {
                recipe.kind.native() == 216 && recipe.stored.is_none()
            }
            (Self::GravityWell, TechniqueProgram::GroundPulse { recipe, .. }) => {
                recipe.kind == super::ground_pulse::GroundPulseSpell::GravityWell
            }
            _ => false,
        }
    }
}

/// Computed source dependencies retain entry ownership through import and preparation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnemySpellBinding {
    Chant(u16),
    Direct(u16),
}

impl EnemySpellBinding {
    pub const fn native_id(self) -> u16 {
        match self {
            Self::Chant(native) | Self::Direct(native) => native,
        }
    }

    pub const fn supported(self) -> bool {
        match self {
            Self::Chant(native) => EnemySpell::from_native(native).is_some(),
            Self::Direct(native) => DirectSpell::from_native(native).is_some(),
        }
    }

    pub fn matches(self, definition: &TechniqueAction) -> bool {
        self.native_id() == definition.native_id
            && match self {
                Self::Chant(_) => definition.enemy_spell().is_some(),
                Self::Direct(_) => definition.direct_spell().is_some(),
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum EnemySpell {
    AquaEdge = 200,
    Spread = 201,
    AquaLaser = 203,
    Eruption = 205,
    Explosion = 206,
    FlameLance = 207,
    WindBlade = 208,
    AirThrust = 209,
    Cyclone = 210,
    AirBlade = 211,
    StoneBlast = 212,
    Stalagmite = 213,
    GroundDasher = 214,
    Grave = 215,
    Lightning = 216,
    SparkWave = 217,
    Indignation = 218,
    ThunderBlade = 219,
    Icicle = 220,
    IceTornado = 221,
    FreezeLancer = 222,
    RagingMist = 223,
    DreadedWave = 224,
    SpiralFlare = 226,
    ThunderArrow = 227,
    GravityWell = 228,
    Atlas = 229,
    Absolute = 230,
    EarthBite = 231,
    MeteorStorm = 233,
    PrismSword = 232,
    Nurse = 237,
    Photon = 251,
    Ray = 252,
    HolyLance = 253,
    BloodyLance = 283,
    AcidRain = 265,
    DarkSphere = 278,
}

impl EnemySpell {
    pub const fn from_native(native: u16) -> Option<Self> {
        Some(match native {
            200 => Self::AquaEdge,
            201 => Self::Spread,
            203 => Self::AquaLaser,
            205 => Self::Eruption,
            206 => Self::Explosion,
            207 => Self::FlameLance,
            208 => Self::WindBlade,
            209 => Self::AirThrust,
            210 => Self::Cyclone,
            211 => Self::AirBlade,
            212 => Self::StoneBlast,
            213 => Self::Stalagmite,
            214 => Self::GroundDasher,
            215 => Self::Grave,
            216 => Self::Lightning,
            217 => Self::SparkWave,
            218 => Self::Indignation,
            219 => Self::ThunderBlade,
            220 => Self::Icicle,
            221 => Self::IceTornado,
            222 => Self::FreezeLancer,
            223 => Self::RagingMist,
            224 => Self::DreadedWave,
            226 => Self::SpiralFlare,
            227 => Self::ThunderArrow,
            228 => Self::GravityWell,
            229 => Self::Atlas,
            230 => Self::Absolute,
            231 => Self::EarthBite,
            233 => Self::MeteorStorm,
            232 => Self::PrismSword,
            237 => Self::Nurse,
            251 => Self::Photon,
            252 => Self::Ray,
            253 => Self::HolyLance,
            283 => Self::BloodyLance,
            265 => Self::AcidRain,
            278 => Self::DarkSphere,
            _ => return None,
        })
    }

    pub const fn stored(self) -> bool {
        !matches!(
            self,
            Self::AquaEdge | Self::WindBlade | Self::StoneBlast | Self::Lightning | Self::Icicle
        )
    }

    fn matches(self, program: &TechniqueProgram) -> bool {
        match (self, program) {
            (Self::PrismSword, TechniqueProgram::PrismSword { .. })
            | (Self::Absolute, TechniqueProgram::Absolute { .. })
            | (Self::EarthBite, TechniqueProgram::EarthBite { .. })
            | (Self::MeteorStorm, TechniqueProgram::MeteorStorm { .. })
            | (Self::Ray, TechniqueProgram::Ray { .. })
            | (Self::AcidRain, TechniqueProgram::AcidRain { .. })
            | (Self::Nurse, TechniqueProgram::Nurse { .. })
            | (Self::AquaEdge, TechniqueProgram::AquaEdge { .. })
            | (Self::Spread, TechniqueProgram::Spread { .. })
            | (Self::Icicle, TechniqueProgram::Icicle { .. })
            | (Self::IceTornado, TechniqueProgram::IceTornado { .. })
            | (Self::FreezeLancer, TechniqueProgram::FreezeLancer { .. })
            | (Self::SpiralFlare, TechniqueProgram::SpiralFlare { .. })
            | (Self::ThunderArrow, TechniqueProgram::ThunderArrow { .. })
            | (Self::StoneBlast, TechniqueProgram::StoneBlast { .. })
            | (Self::WindBlade, TechniqueProgram::WindBlade { .. })
            | (Self::AirThrust, TechniqueProgram::AirThrust { .. }) => true,
            (
                Self::RagingMist | Self::DreadedWave | Self::GravityWell | Self::Atlas,
                TechniqueProgram::GroundPulse { recipe, .. },
            ) => recipe.kind as u16 == self as u16,
            (Self::HolyLance | Self::BloodyLance, TechniqueProgram::Lance { recipe, .. }) => {
                recipe.kind as u16 == self as u16
            }
            (Self::Photon | Self::DarkSphere, TechniqueProgram::Orb { recipe, .. }) => {
                recipe.kind as u16 == self as u16
            }
            (
                Self::Eruption | Self::Explosion | Self::FlameLance,
                TechniqueProgram::FireField { recipe, .. },
            ) => recipe.native_id == self as u16,
            (Self::Cyclone | Self::AirBlade, TechniqueProgram::WindField { recipe, .. }) => {
                recipe.kind as u16 == self as u16
            }
            (
                Self::Stalagmite | Self::GroundDasher | Self::Grave,
                TechniqueProgram::EarthField { recipe, .. },
            ) => recipe.kind as u16 == self as u16,
            (Self::AquaLaser, TechniqueProgram::Water { recipe, .. }) => {
                recipe.kind as u16 == self as u16
            }
            (_, TechniqueProgram::Lightning { recipe, .. }) => {
                recipe.kind.native() == self as u16 && recipe.stored.is_some() == self.stored()
            }
            _ => false,
        }
    }
}

impl TechniqueAction {
    pub fn direct_spell(&self) -> Option<DirectSpell> {
        DirectSpell::from_native(self.native_id).filter(|spell| spell.matches(&self.program))
    }

    /// Native identity and cooked program must agree before an enemy may bind them.
    pub fn enemy_spell(&self) -> Option<EnemySpell> {
        EnemySpell::from_native(self.native_id).filter(|spell| spell.matches(&self.program))
    }
}
