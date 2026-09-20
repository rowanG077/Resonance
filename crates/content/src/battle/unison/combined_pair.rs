use super::*;
use crate::battle::{
    BattleCatalog,
    actions::{HitRule, TechniquePhase},
    effects::{EffectBank, EffectId},
    visual::PartyVisuals,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombinedPair {
    Stardust,
    Mjollnir,
    Prism,
    Punishment,
    ArchWind,
    Tempest,
    Blast,
    Plasma,
}

impl CombinedPair {
    pub const ALL: [Self; 8] = [
        Self::Stardust,
        Self::Mjollnir,
        Self::Prism,
        Self::Punishment,
        Self::ArchWind,
        Self::Tempest,
        Self::Blast,
        Self::Plasma,
    ];
    pub const fn native(self) -> u16 {
        match self {
            Self::Stardust => 305,
            Self::Mjollnir => 310,
            Self::Prism => 311,
            Self::Punishment => 307,
            Self::ArchWind => 308,
            Self::Tempest => 309,
            Self::Blast => 316,
            Self::Plasma => 318,
        }
    }
    pub const fn combination(self) -> u8 {
        match self {
            Self::Stardust => 6,
            Self::Mjollnir => 11,
            Self::Prism => 12,
            Self::Punishment => 8,
            Self::ArchWind => 9,
            Self::Tempest => 10,
            Self::Blast => 17,
            Self::Plasma => 19,
        }
    }
    pub fn from_native(native: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.native() == native)
    }
    pub fn phase(self, role: usize, character: u8) -> Option<u8> {
        match (self, role, character) {
            (Self::Stardust | Self::Mjollnir, 0, 2) | (Self::Prism, 0, 3) => Some(0),
            (Self::Stardust, 1, 1) | (Self::Mjollnir, 1, 3 | 6 | 9) | (Self::Prism, 1, 4) => {
                Some(1)
            }
            (Self::Punishment, 0, 3) | (Self::ArchWind, 0, 6) => Some(0),
            (Self::Punishment, 0, 6) | (Self::ArchWind, 0, 9) => Some(1),
            (Self::Punishment, 0, 9) | (Self::ArchWind, 1, 7) => Some(2),
            (Self::Punishment, 1, 7) => Some(3),
            (Self::Tempest, 0, 1) | (Self::Blast, 0, 2) | (Self::Plasma, 0, 6) => Some(0),
            (Self::Plasma, 0, 9) => Some(1),
            (Self::Plasma, 1, 4) => Some(2),
            (Self::Tempest | Self::Blast, 1, 4) => Some(1),
            _ => None,
        }
    }
    pub fn selected(self, party: &[u8]) -> bool {
        (0..2).all(|role| party.iter().any(|&c| self.phase(role, c).is_some()))
    }
    pub fn effect(self, id: u8) -> EffectId {
        EffectId {
            bank: EffectBank::Magic(self.native() - 200),
            id,
        }
    }
    pub fn programs(self) -> std::ops::Range<u8> {
        1..match self {
            Self::Stardust => 11,
            Self::Mjollnir => 5,
            Self::Prism | Self::Plasma => 3,
            _ => 2,
        }
    }
    pub const fn presea(self) -> bool {
        matches!(self, Self::Punishment | Self::ArchWind)
    }
    pub const fn phase_count(self) -> u8 {
        match self {
            Self::Punishment => 4,
            Self::ArchWind | Self::Plasma => 3,
            _ => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PairEffect {
    World { role: u8, position: [f32; 3] },
    ActorAim { role: u8 },
    FixedRoot { role: u8 },
}
impl Default for PairEffect {
    fn default() -> Self {
        Self::ActorAim { role: 0 }
    }
}
impl PairEffect {
    pub fn role(self) -> usize {
        usize::from(match self {
            Self::ActorAim { role } | Self::FixedRoot { role } | Self::World { role, .. } => role,
        })
    }
}

/// A scalar retains compatibility with the earlier fixed-voice recipes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PairVoice {
    Fixed(u16),
    ByPrimary(#[serde(deserialize_with = "PairVoice::choices")] BTreeMap<u8, u16>),
}
impl PairVoice {
    fn choices<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<BTreeMap<u8, u16>, D::Error> {
        // Untagged variants buffer JSON keys as strings before selecting a variant.
        BTreeMap::<String, u16>::deserialize(d)?
            .into_iter()
            .map(|(key, voice)| {
                key.parse()
                    .map(|key| (key, voice))
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
    pub fn for_primary(&self, character: u8) -> Option<u16> {
        match self {
            Self::Fixed(voice) => Some(*voice),
            Self::ByPrimary(voices) => voices.get(&character).copied(),
        }
    }
    pub fn ids(&self) -> impl Iterator<Item = u16> + '_ {
        let (fixed, choices) = match self {
            Self::Fixed(v) => (Some(*v), None),
            Self::ByPrimary(v) => (None, Some(v)),
        };
        fixed
            .into_iter()
            .chain(choices.into_iter().flat_map(|v| v.values().copied()))
    }
}

#[test]
fn pair_voices_read_fixed_and_character_keyed_json() {
    for voice in [
        PairVoice::Fixed(0x87b1),
        PairVoice::ByPrimary(BTreeMap::from([(6, 0x87c1), (9, 0x87c2)])),
    ] {
        let json = serde_json::to_string(&voice).unwrap();
        assert_eq!(serde_json::from_str::<PairVoice>(&json).unwrap(), voice);
    }
    assert!(serde_json::from_str::<PairVoice>(r#"{"Kratos":34754}"#).is_err());
}

/// Two visible participants retain independent action and impact clocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinedPairProgram {
    pub phases: Vec<TechniquePhase>,
    pub directions: [[f32; 3]; 2],
    pub distances: [f32; 2],
    pub color: [u8; 4],
    pub effect_scale: f32,
    pub feedback_tick: u16,
    pub feedback_ticks: u16,
    #[serde(default = "feedback_limit")]
    pub feedback_limit: u8,
    /// Participant order follows the original combination recipe.
    pub projectiles: [Option<CombinedPairProjectile>; 2],
    #[serde(default)]
    pub initial_effect: PairEffect,
    pub voice: PairVoice,
    #[serde(default = "secondary_role")]
    pub voice_role: u8,
    #[serde(default = "before_effect")]
    pub voice_before_effect: bool,
    /// Per role: hide both original carried slots during the combination.
    #[serde(default)]
    pub hide_attachments: [bool; 2],
}
fn secondary_role() -> u8 {
    1
}
fn feedback_limit() -> u8 {
    8
}
fn before_effect() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairOrigin {
    #[default]
    TargetAim,
    ActorRoot,
    TargetRoot,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PairProjectilePattern {
    #[default]
    Fixed,
    Stardust {
        offset: [f32; 3],
        spread: [f32; 3],
        modulus: u16,
        scale: f32,
        variants: u8,
    },
    Prism {
        angle_choices: u16,
        angle_offset: u16,
        angle_step: u16,
        radians_per_degree: f32,
        distance: f32,
        contact_interval: u16,
        short_contact_duration: u16,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CombinedPairProjectile {
    pub tick: u16,
    pub id: u8,
    pub rule: HitRule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_tick: Option<u16>,
    #[serde(default)]
    pub origin: PairOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<u8>,
    #[serde(default)]
    pub pattern: PairProjectilePattern,
}
impl CombinedPairProjectile {
    pub fn ids(self) -> std::ops::Range<u8> {
        self.id
            ..self.id
                + match self.pattern {
                    PairProjectilePattern::Stardust { variants, .. } => variants,
                    _ => 1,
                }
    }
}

impl CombinedPairProgram {
    pub fn validate(&self, kind: CombinedPair) -> Result<()> {
        ensure!(
            self.directions
                .iter()
                .all(|v| v.iter().all(|x| x.is_finite())
                    && v[1] == 0.
                    && (v[0] * v[0] + v[2] * v[2] - 1.).abs() < 0.0001)
                && self.distances.iter().all(|v| v.is_finite() && *v > 0.)
                && self.effect_scale.is_finite()
                && self.effect_scale > 0.,
            "invalid paired combination placement"
        );
        let (feedback_tick, feedback_ticks) = match kind {
            CombinedPair::Stardust => (40, 140),
            CombinedPair::Mjollnir => (90, 30),
            CombinedPair::Prism => (30, 140),
            CombinedPair::Punishment | CombinedPair::ArchWind => (30, 130),
            CombinedPair::Tempest => (55, 40),
            _ => (70, 40),
        };
        let primary_voice = matches!(
            kind,
            CombinedPair::Stardust | CombinedPair::Mjollnir | CombinedPair::Prism
        );
        ensure!(
            (self.feedback_tick, self.feedback_ticks) == (feedback_tick, feedback_ticks)
                && self.feedback_limit == if kind == CombinedPair::Prism { 12 } else { 8 }
                && self.voice_role == if primary_voice { 0 } else { 1 }
                && self.voice_before_effect == (kind != CombinedPair::Mjollnir)
                && self.hide_attachments == [kind == CombinedPair::Stardust, false]
                && self.initial_effect
                    == match kind {
                        CombinedPair::Stardust | CombinedPair::Prism => PairEffect::World {
                            role: 0,
                            position: [0.; 3]
                        },
                        CombinedPair::Punishment | CombinedPair::ArchWind =>
                            PairEffect::FixedRoot { role: 1 },
                        _ => PairEffect::default(),
                    }
                && self.voice
                    == match kind {
                        CombinedPair::Stardust => PairVoice::Fixed(0x87bb),
                        CombinedPair::Mjollnir => PairVoice::Fixed(0x87ba),
                        CombinedPair::Prism => PairVoice::Fixed(0x87c3),
                        CombinedPair::Punishment => PairVoice::Fixed(0x87b1),
                        CombinedPair::ArchWind =>
                            PairVoice::ByPrimary(BTreeMap::from([(6, 0x87c1), (9, 0x87c2)])),
                        CombinedPair::Tempest => PairVoice::Fixed(0x879a),
                        _ => PairVoice::Fixed(0x87b3),
                    },
            "invalid paired combination feedback"
        );
        ensure!(
            self.phases.len() == usize::from(kind.phase_count()),
            "invalid paired combination phases"
        );
        for (index, phase) in self.phases.iter().enumerate() {
            let sword = kind == CombinedPair::Plasma && index < 2;
            ensure!(
                usize::from(phase.variant) == index
                    && phase.action.duration
                        == if sword {
                            95
                        } else if kind.presea()
                            || matches!(kind, CombinedPair::Stardust | CombinedPair::Prism)
                        {
                            180
                        } else {
                            160
                        }
                    && phase.buffer_until == if sword { 65 } else { 0 }
                    && phase.combo_at == if sword { 60 } else { 0 }
                    && phase.recovery_ticks == if sword { 10 } else { 20 }
                    && phase.action.tp == 0
                    && phase.callback.is_none()
                    && phase.effect.is_none(),
                "invalid paired combination phase"
            );
            phase.action.validate(0)?;
        }
        ensure!(
            if kind.presea()
                || matches!(
                    kind,
                    CombinedPair::Stardust | CombinedPair::Mjollnir | CombinedPair::Prism
                )
            {
                self.phases.iter().all(|p| p.action.hits.is_empty())
            } else {
                self.phases
                    .last()
                    .is_some_and(|p| p.action.commands.is_empty() && p.action.hits.is_empty())
            },
            "invalid paired combination spell track"
        );
        let expected = match kind {
            CombinedPair::Stardust | CombinedPair::Prism => [Some((30, 1)), None],
            CombinedPair::Mjollnir => [Some((42, 1)), Some((86, 2))],
            CombinedPair::Punishment | CombinedPair::ArchWind => [None, Some((30, 1))],
            CombinedPair::Tempest => [None, Some((60, 2))],
            CombinedPair::Blast => [None, Some((88, 2))],
            CombinedPair::Plasma => [Some((78, 3)), Some((70, 2))],
        };
        for (role, (projectile, expected)) in self.projectiles.iter().zip(expected).enumerate() {
            ensure!(
                projectile.as_ref().map(|p| (p.tick, p.id)) == expected,
                "invalid paired combination projectile callback"
            );
            if let Some(p) = projectile {
                let (end, origin, effect) = match kind {
                    CombinedPair::Stardust => (Some(180), PairOrigin::ActorRoot, None),
                    CombinedPair::Prism => (Some(150), PairOrigin::TargetRoot, None),
                    CombinedPair::Mjollnir if role == 0 => (None, PairOrigin::ActorRoot, None),
                    CombinedPair::Mjollnir => (None, PairOrigin::TargetRoot, Some(4)),
                    _ => (None, PairOrigin::TargetAim, None),
                };
                ensure!(
                    p.end_tick == end
                        && p.origin == origin
                        && p.effect == effect
                        && match (kind, p.pattern) {
                            (
                                CombinedPair::Stardust,
                                PairProjectilePattern::Stardust {
                                    offset,
                                    spread,
                                    modulus,
                                    scale,
                                    variants,
                                },
                            ) =>
                                offset == [0., 600., 0.]
                                    && spread == [60., 20., 60.]
                                    && modulus == 100
                                    && scale == 0.1
                                    && variants == 3,
                            (
                                CombinedPair::Prism,
                                PairProjectilePattern::Prism {
                                    angle_choices,
                                    angle_offset,
                                    angle_step,
                                    radians_per_degree,
                                    distance,
                                    contact_interval,
                                    short_contact_duration,
                                },
                            ) =>
                                (angle_choices, angle_offset, angle_step) == (30, 5, 12)
                                    && radians_per_degree == 0.017_453_289
                                    && distance == -800.
                                    && contact_interval == 4
                                    && short_contact_duration == 1,
                            (CombinedPair::Stardust | CombinedPair::Prism, _) => false,
                            (_, PairProjectilePattern::Fixed) => true,
                            _ => false,
                        },
                    "invalid paired combination emission pattern"
                );
                p.rule
                    .impact_program_from(EffectBank::Techniques, Some(kind.native() - 200))?;
            }
        }
        Ok(())
    }
}

impl UnisonData {
    pub fn pair_program(&self, kind: CombinedPair) -> Option<&CombinedPairProgram> {
        self.pairs.get(&kind).or_else(|| {
            (kind == CombinedPair::Plasma)
                .then_some(self.plasma_blade.as_ref())
                .flatten()
        })
    }
    pub(crate) fn validate_combined_pair_motions(
        &self,
        kind: CombinedPair,
        models: &BTreeMap<u8, PartyVisuals>,
    ) -> Result<()> {
        let program = self
            .pair_program(kind)
            .context("missing paired combination program")?;
        program.validate(kind)?;
        for (&character, variants) in models {
            if let Some(phase) = (0..2).find_map(|role| kind.phase(role, character)) {
                validate_phase_motions(variants, &program.phases[usize::from(phase)].action)
                    .with_context(|| format!("paired combination actor {character}"))?;
            }
        }
        Ok(())
    }
}

impl BattleCatalog {
    /// Also retained at the native launch boundary for older, partial bundles.
    pub fn validate_combined_pair(&self, kind: CombinedPair) -> Result<()> {
        self.unison
            .validate_combined_pair_motions(kind, &self.visuals.party)?;
        let program = self
            .unison
            .pair_program(kind)
            .context("missing paired combination program")?;
        for id in kind.programs() {
            self.effect_programs
                .program(kind.effect(id))
                .context("missing paired combination effect")?;
        }
        for projectile in program.projectiles.iter().flatten() {
            for id in projectile.ids() {
                self.effects
                    .projectile(kind.effect(id))
                    .context("missing paired combination projectile")?;
            }
        }
        let mut sounds = BTreeSet::new();
        let mut voices = program.voice.ids().collect::<BTreeSet<_>>();
        for phase in &program.phases {
            let (phase_sounds, phase_voices) = phase.action.audio_ids();
            sounds.extend(phase_sounds);
            voices.extend(phase_voices);
        }
        for rule in program
            .phases
            .iter()
            .flat_map(|phase| &phase.action.hits)
            .map(|hit| hit.rule)
            .chain(program.projectiles.iter().flatten().map(|p| p.rule))
        {
            if rule.sound != 0 {
                sounds.insert(rule.sound);
            }
            if let Some(effect) =
                rule.impact_program_from(EffectBank::Techniques, Some(kind.native() - 200))?
            {
                self.effect_programs
                    .program(effect)
                    .context("missing paired combination impact effect")?;
            }
        }
        for sound in sounds {
            ensure!(
                i16::try_from(sound).is_ok_and(|id| self.audio.audio.sounds.contains_key(&id)),
                "missing paired combination sound {sound}"
            );
        }
        for voice in voices.into_iter().filter(|&voice| voice != 0) {
            ensure!(
                self.audio.voice_cues.contains_key(&voice),
                "missing paired combination voice {voice}"
            );
        }
        Ok(())
    }
}
