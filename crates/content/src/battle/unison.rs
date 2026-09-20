//! Unison selections and combination recipes, expressed in gameplay identities.
use super::actions::{Action, AnimationCommand, Recovery};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod combined_pair;
mod strike;
pub use combined_pair::{
    CombinedPair, CombinedPairProgram, CombinedPairProjectile, PairEffect, PairOrigin,
    PairProjectilePattern, PairVoice,
};
pub use strike::{Strike, StrikeProgram};

pub const COMBINATION_COUNT: usize = 19;
pub const UNLOCK_STORY: i32 = 1_403_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnisonData {
    pub artes: BTreeMap<u16, Arte>,
    /// Source priority order matters when several combinations match.
    pub combinations: Vec<Combination>,
    pub party: [PartyTraits; 9],
    pub opener: [Opener; 9],
    pub pow: BTreeMap<PowWeapon, PowProgram>,
    pub thrusts: BTreeMap<Thrust, ThrustProgram>,
    pub strikes: BTreeMap<Strike, StrikeProgram>,
    /// Read compatibility for early Plasma-only bundles; new cooks use pairs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plasma_blade: Option<CombinedPairProgram>,
    #[serde(
        default,
        alias = "photon_pairs",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub pairs: BTreeMap<CombinedPair, CombinedPairProgram>,
    pub windup: AnimationCommand,
    pub windup_color: [u8; 4],
    pub combined_prelude: CombinedPrelude,
    pub placement: [[f32; 3]; 4],
    /// Nonfloating actors return to this height after staged attacks.
    pub restore_ground_height: f32,
    pub short_weapon_penalty: f32,
    pub minimum_distance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinedPrelude {
    pub hidden_position: [f32; 3],
    pub first_x: f32,
    pub spacing: f32,
    pub camera_eye: [f32; 3],
    pub camera_target: [f32; 3],
    pub color: [u8; 4],
    /// Zero is an authored silent request, including its voice-selection RNG draw.
    pub voices: [u16; 9],
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Thrust {
    Cross,
    Mirage,
    Dark,
}

impl Thrust {
    pub const ALL: [Self; 3] = [Self::Cross, Self::Mirage, Self::Dark];
    pub const fn native(self) -> u16 {
        match self {
            Self::Cross => 302,
            Self::Mirage => 303,
            Self::Dark => 304,
        }
    }
    pub fn selected(self, party: &[u8]) -> bool {
        let swords = party.iter().filter(|&&id| matches!(id, 1 | 6 | 9)).count();
        match self {
            Self::Cross => swords >= 2,
            _ => swords != 0 && party.contains(&5),
        }
    }
    pub fn phase(self, role: usize, character: u8) -> Option<u8> {
        let sword = match character {
            1 => Some(0),
            6 => Some(1),
            9 => Some(2),
            _ => None,
        };
        match (self, role) {
            (Self::Cross, 0..=1) => sword.map(|_| role as u8),
            (_, 0) => sword,
            (_, 1) if character == 5 => Some(3),
            _ => None,
        }
    }
    pub fn voices(self) -> &'static [u16] {
        match self {
            Self::Cross => &[0x87a8, 0x87a9, 0x87aa],
            Self::Mirage => &[0x879e, 0x879f],
            Self::Dark => &[0x87a4],
        }
    }
}

/// Shared rush tracks with character-specific phase identities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrustProgram {
    pub phases: Vec<super::actions::TechniquePhase>,
    pub directions: [[f32; 3]; 2],
    pub distance: f32,
    pub color: [u8; 4],
    pub feedback_at: u16,
}

impl ThrustProgram {
    fn validate(&self, kind: Thrust) -> Result<()> {
        ensure!(
            self.phases.len() == (if kind == Thrust::Cross { 2 } else { 4 })
                && self.directions.iter().flatten().all(|v| v.is_finite())
                && self.distance.is_finite()
                && self.distance > 0.
                && self.feedback_at > 0,
            "invalid combined thrust program"
        );
        for (index, phase) in self.phases.iter().enumerate() {
            ensure!(
                usize::from(phase.variant) == index
                    && phase.action.duration > 0
                    && phase.action.tp == 0
                    && phase.buffer_until == 0
                    && phase.combo_at == 0
                    && phase.callback.is_none(),
                "invalid combined thrust phase"
            );
            phase.action.validate(0)?;
        }
        Ok(())
    }
}

/// Temporary carried weapons share a visible primary and a hidden Colette phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowProgram {
    pub phases: Vec<super::actions::TechniquePhase>,
    pub direction: [f32; 3],
    pub distance: f32,
    pub color: [u8; 4],
    pub end_bones: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opener {
    pub action: Action,
    /// Shared timer writes that are inactive when the carried slot is unoccupied.
    pub inactive_trail_slots: BTreeSet<u16>,
    /// A shared opener may reference an empty carried attack group.
    pub optional_hit_groups: BTreeSet<u8>,
    /// Opening strikes hold their action beyond the descriptor's ordinary duration.
    pub minimum_active_ticks: u16,
    pub recovery: Recovery,
    pub reach: u16,
    pub texture_layers: [u8; 4],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PartyTraits {
    pub opener_contact_ticks: u16,
    pub overlimit_rate: u8,
    pub overlimit_voice: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Arte {
    pub native_id: u16,
    pub usable: bool,
    pub duration: u16,
    pub distance: u16,
    pub altitude: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Combination {
    pub id: u8,
    pub name: String,
    pub native_id: u16,
    pub participants: u8,
    /// Ingredients are native identities: Kratos and Zelos share some artes.
    /// An empty list means the authored combination cannot be selected.
    pub recipes: Vec<Vec<u16>>,
    pub duration: u16,
    pub camera_pitch: f32,
}

impl UnisonData {
    /// The same source-only admission runs before cooking and during catalog validation.
    pub fn validate_party_motions(
        &self,
        models: &BTreeMap<u8, super::visual::PartyVisuals>,
    ) -> Result<()> {
        self.validate()?;
        let party = models.keys().copied().collect::<Vec<_>>();
        for (&character, variants) in models {
            let opener = character
                .checked_sub(1)
                .and_then(|index| self.opener.get(usize::from(index)))
                .with_context(|| format!("invalid Unison party identity {character}"))?;
            for (costume, model) in variants.concrete() {
                model
                    .validate_animation(self.windup)
                    .with_context(|| format!("Unison windup {character}:{costume:?}"))?;
                for command in opener.action.animations.commands() {
                    model
                        .validate_animation(command)
                        .with_context(|| format!("Unison opener {character}:{costume:?}"))?;
                }
                if let Some(clip) = opener.recovery.animation {
                    model
                        .visual
                        .motion_slot(u16::from(clip))
                        .with_context(|| format!("Unison recovery {character}:{costume:?}"))?;
                }
            }
        }
        for (kind, program) in &self.pow {
            if !kind.selected(&party) {
                continue;
            }
            for (&character, phase) in kind.characters().iter().zip(&program.phases) {
                if let Some(models) = models.get(&character) {
                    validate_phase_motions(models, &phase.action)
                        .with_context(|| format!("Pow {kind:?} actor {character}"))?;
                }
            }
        }
        for (kind, program) in &self.thrusts {
            if !kind.selected(&party) {
                continue;
            }
            for &character in &party {
                for role in 0..2 {
                    if let Some(phase) = kind.phase(role, character) {
                        validate_phase_motions(
                            &models[&character],
                            &program.phases[usize::from(phase)].action,
                        )
                        .with_context(|| {
                            format!("combined thrust {kind:?} actor {character} role {role}")
                        })?;
                    }
                }
            }
        }
        for kind in Strike::ALL {
            if kind.selected(&party) {
                self.validate_strike_motions(kind, models)?;
            }
        }
        for kind in CombinedPair::ALL {
            if kind.selected(&party) && self.pair_program(kind).is_some() {
                self.validate_combined_pair_motions(kind, models)?;
            }
        }
        Ok(())
    }

    pub(super) fn validate_strike_motions(
        &self,
        kind: Strike,
        models: &BTreeMap<u8, super::visual::PartyVisuals>,
    ) -> Result<()> {
        let program = self
            .strikes
            .get(&kind)
            .context("missing combined strike program")?;
        for (&character, variants) in models {
            if let Some(role) = kind.phase(usize::from(character != 1), character) {
                validate_phase_motions(variants, &program.phases[usize::from(role)].action)
                    .with_context(|| format!("combined strike {kind:?} actor {character}"))?;
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.plasma_blade.is_none() || !self.pairs.contains_key(&CombinedPair::Plasma),
            "duplicate Plasma combination program"
        );
        for kind in CombinedPair::ALL {
            if let Some(program) = self.pair_program(kind) {
                program.validate(kind)?;
            }
        }
        ensure!(
            self.pow.len() == PowWeapon::ALL.len(),
            "incomplete Pow programs"
        );
        for kind in PowWeapon::ALL {
            self.pow
                .get(&kind)
                .ok_or_else(|| anyhow::anyhow!("missing Pow program"))?
                .validate(kind)?;
        }
        ensure!(
            self.thrusts.len() == Thrust::ALL.len(),
            "incomplete combined thrust programs"
        );
        for kind in Thrust::ALL {
            self.thrusts
                .get(&kind)
                .ok_or_else(|| anyhow::anyhow!("missing combined thrust program"))?
                .validate(kind)?;
        }
        ensure!(
            self.strikes.len() == Strike::ALL.len(),
            "incomplete combined strike programs"
        );
        for kind in Strike::ALL {
            self.strikes
                .get(&kind)
                .ok_or_else(|| anyhow::anyhow!("missing combined strike program"))?
                .validate(kind)?;
        }
        let prelude = &self.combined_prelude;
        ensure!(
            prelude
                .hidden_position
                .iter()
                .chain(&prelude.camera_eye)
                .chain(&prelude.camera_target)
                .chain([&prelude.first_x, &prelude.spacing])
                .all(|value| value.is_finite())
                && prelude.spacing > 0.
                && prelude.camera_eye != prelude.camera_target
                && !prelude.title.is_empty(),
            "invalid combined Unison prelude"
        );
        ensure!(
            self.combinations.len() == COMBINATION_COUNT,
            "incomplete Unison combinations"
        );
        ensure!(
            self.artes.len() == crate::menu_data::TECHNIQUE_COUNT,
            "incomplete Unison arte bindings"
        );
        for (index, combination) in self.combinations.iter().enumerate() {
            ensure!(
                usize::from(combination.id) == index + 1
                    && (2..=4).contains(&combination.participants)
                    && combination.native_id >= 300
                    && combination.duration > 0
                    && combination.camera_pitch.is_finite(),
                "invalid Unison combination"
            );
            ensure!(
                combination.recipes.len() <= 6
                    && combination
                        .recipes
                        .iter()
                        .all(|r| r.len() == usize::from(combination.participants)
                            && r.iter().all(|&id| id != 0)),
                "invalid Unison recipe"
            );
        }
        ensure!(
            self.placement.iter().flatten().all(|v| v.is_finite())
                && self.restore_ground_height.is_finite()
                && self.restore_ground_height >= 0.
                && self.short_weapon_penalty.is_finite()
                && self.short_weapon_penalty >= 0.
                && self.minimum_distance.is_finite()
                && self.minimum_distance > 0.,
            "invalid Unison placement"
        );
        ensure!(
            matches!(self.windup, AnimationCommand::Play { rate, .. } if rate.is_finite() && rate > 0.),
            "invalid Unison windup animation"
        );
        for (index, opener) in self.opener.iter().enumerate() {
            ensure!(
                opener.action.duration > 0
                    && opener.minimum_active_ticks >= opener.action.duration
                    && opener.action.tp == 0
                    && opener.recovery.rate.is_finite()
                    && opener.recovery.rate > 0.
                    && opener.reach > 0,
                "invalid Unison opener"
            );
            ensure!(
                opener.inactive_trail_slots.is_empty()
                    || (matches!(index, 2 | 3 | 5 | 8)
                        && opener.inactive_trail_slots.iter().all(|&slot| slot == 1)
                        && opener.action.commands.iter().any(|step| matches!(
                            step.command,
                            super::actions::ActionCommand::AttachmentTrail { slot: 1, .. }
                        ))),
                "invalid Unison inactive trail slot"
            );
            ensure!(
                opener.optional_hit_groups.is_empty()
                    || (matches!(index, 3 | 5 | 8)
                        && opener.optional_hit_groups.iter().all(|&group| group == 1)
                        && opener.action.hits.iter().any(|hit| matches!(&hit.emission,
                        super::actions::HitEmission::Contact {
                            attachment: super::actions::HitAttachment::Groups(groups), ..
                        } if groups.contains(&1)))),
                "invalid Unison optional attack group"
            );
        }
        Ok(())
    }
}

fn validate_phase_motions(models: &super::visual::PartyVisuals, action: &Action) -> Result<()> {
    for (costume, model) in models.concrete() {
        ensure!(
            model.visual.rig.motions.contains_key(&0),
            "missing combined recovery motion {costume:?}"
        );
        for command in action.animations.commands() {
            model
                .validate_animation(command)
                .with_context(|| format!("combined phase costume {costume:?}"))?;
        }
    }
    Ok(())
}

impl PowProgram {
    pub fn validate(&self, kind: PowWeapon) -> Result<()> {
        ensure!(
            self.phases.len() == kind.characters().len(),
            "invalid Pow phase count"
        );
        ensure!(
            self.direction.iter().all(|v| v.is_finite())
                && self.distance.is_finite()
                && self.distance > 0.,
            "invalid Pow placement"
        );
        ensure!(
            self.end_bones.len() == (if kind == PowWeapon::Blade { 2 } else { 1 })
                && self.end_bones.iter().all(|bone| !bone.is_empty())
                && self.end_bones.iter().collect::<BTreeSet<_>>().len() == self.end_bones.len(),
            "invalid Pow end attachments"
        );
        for (index, phase) in self.phases.iter().enumerate() {
            ensure!(
                usize::from(phase.variant) == index
                    && phase.action.duration != 0
                    && phase.action.tp == 0
                    && phase.buffer_until == 0
                    && phase.combo_at == 0
                    && phase.callback.is_none(),
                "invalid Pow phase"
            );
            phase.action.validate(0)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowWeapon {
    Blade,
    Devastation,
    Spear,
}

impl PowWeapon {
    pub const ALL: [Self; 3] = [Self::Blade, Self::Devastation, Self::Spear];
    pub const fn native(self) -> u16 {
        match self {
            Self::Blade => 301,
            Self::Devastation => 312,
            Self::Spear => 314,
        }
    }
    pub const fn combination(self) -> u8 {
        match self {
            Self::Blade => 1,
            Self::Devastation => 13,
            Self::Spear => 15,
        }
    }
    pub fn characters(self) -> &'static [u8] {
        match self {
            Self::Blade => &[1, 2],
            Self::Devastation => &[7, 2],
            Self::Spear => &[6, 9, 2],
        }
    }
    pub fn phase(self, role: usize, character: u8) -> Option<usize> {
        let phase = self.characters().iter().position(|&id| id == character)?;
        ((role == 0 && character != 2) || (role == 1 && character == 2)).then_some(phase)
    }
    pub fn selected(self, party: &[u8]) -> bool {
        party.contains(&2)
            && self
                .characters()
                .iter()
                .any(|id| *id != 2 && party.contains(id))
    }
    pub const fn model(self, equipment: u16) -> super::visual::WeaponModel {
        match self {
            Self::Blade => super::visual::WeaponModel::PowBlade,
            Self::Devastation => super::visual::WeaponModel::PowDevastation(equipment),
            Self::Spear => super::visual::WeaponModel::PowSpear,
        }
    }
    pub const fn startup(self) -> u8 {
        if matches!(self, Self::Spear) { 5 } else { 2 }
    }
    pub const fn release(self) -> u8 {
        if matches!(self, Self::Spear) { 4 } else { 5 }
    }
    pub fn effects(self) -> &'static [u8] {
        match self {
            Self::Blade => &[0, 1, 2, 3, 4, 5],
            Self::Devastation => &[2, 3, 4, 5],
            Self::Spear => &[2, 3, 4, 5],
        }
    }
}

#[cfg(test)]
mod pow_roles {
    use super::PowWeapon;
    #[test]
    fn recipe_roles_keep_sword_character_phases_and_colettes_hidden_slot() {
        for (kind, expected) in [
            (PowWeapon::Blade, vec![(0, 1, 0), (1, 2, 1)]),
            (PowWeapon::Devastation, vec![(0, 7, 0), (1, 2, 1)]),
            (PowWeapon::Spear, vec![(0, 6, 0), (0, 9, 1), (1, 2, 2)]),
        ] {
            for role in 0..3 {
                for character in 1..=9 {
                    assert_eq!(
                        kind.phase(role, character),
                        expected
                            .iter()
                            .find(|&&(r, c, _)| r == role && c == character)
                            .map(|&(_, _, phase)| phase)
                    );
                }
            }
        }
    }
}
