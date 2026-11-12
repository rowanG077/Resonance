//! A complete asset-authored spell release. The technique entry keeps the
//! ordinary casting rules: TP, cast time, element, targeting and recovery.
//! This document describes what happens once the chant completes.
use crate::{
    battle_action::ActionHit, battle_conditions::Condition, battle_projectile::Projectile,
    battle_unison::Placement, items::BattleCure,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredSpell {
    /// Gameplay updates the spell stays active after release, including
    /// artwork that outlasts its last hit or pulse.
    pub duration: u16,
    /// Where projectiles and scene effects are placed.
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub origin: Origin,
    /// Effect bank for this spell's programs; absent selects the shared
    /// technique bank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects: Option<String>,
    /// An original spell whose chant and release voice lines the caster reuses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<u16>,
    /// Sound cue played at release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<u16>,
    /// Effect program started at the origin on release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visuals: Vec<Visual>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pulses: Vec<Pulse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projectiles: Vec<Shot>,
    /// Placement and identity for Unison Attacks; required for an offensive spell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unison: Option<Placement>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Origin {
    /// The caster's body center, sampled as each projectile launches.
    #[default]
    Caster,
    /// The target's body center, chosen on the update after release.
    Target,
    /// The ground under the target, chosen on the update after release.
    TargetGround,
    /// The ground under the target at release, moved toward the caster.
    Ground {
        #[serde(default)]
        toward_caster: f32,
    },
}

/// A timed effect program without gameplay of its own.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visual {
    pub at: u16,
    pub effect: u16,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub placement: VisualPlacement,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VisualPlacement {
    /// Offset from the origin, rotated by the heading captured at release.
    Origin {
        #[serde(default)]
        offset: [f32; 3],
    },
    #[default]
    CasterGround,
    CasterCenter,
    /// One instance on every living enemy.
    EnemyCenters,
}

/// A benefit or penalty applied to recipients at one moment of the release.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pulse {
    pub at: u16,
    /// Consecutive updates during which recipients are reconsidered.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub duration: u16,
    pub benefit: Benefit,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub targets: Targets,
    /// Effect program shown on each recipient.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<u16>,
}

fn one() -> u16 {
    1
}
fn is_one(value: &u16) -> bool {
    *value == 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Benefit {
    /// Percent of the recipient's maximum HP.
    Hp {
        percent: u8,
    },
    /// TP points.
    Tp {
        amount: u16,
    },
    Cure {
        ailments: BattleCure,
    },
    /// Revive a fallen ally with this percent of maximum HP.
    Resurrect {
        percent: u8,
    },
    /// Speed up the recipient's actions, casting and movement by `percent`.
    Haste {
        percent: u8,
        /// Gameplay updates; 60 per second.
        duration: u16,
    },
    /// A timed statistic change: `attack_up` or `defense_up`.
    Stat {
        condition: Condition,
        percent: u8,
        /// Gameplay updates; 60 per second.
        duration: u16,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Targets {
    /// The actor selected when the spell was cast.
    #[default]
    Target,
    Caster,
    LivingAllies,
    LivingOpponents,
    /// Allies of the caster within this distance of the origin.
    Area {
        radius: f32,
    },
}

/// One projectile launched during the release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shot {
    pub at: u16,
    pub projectile: ShotProjectile,
    pub hit: ActionHit,
    /// Replaces the projectile's own launch velocity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<[f32; 3]>,
    /// Offset from the origin, rotated by the heading captured at release.
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub offset: [f32; 3],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub launch: Launch,
}

/// A member of the shared `battle/projectiles` records, or a complete
/// definition in the same format. A definition's effects name bank 0 for the
/// common bank, 1 for the shared technique bank and 2 for this spell's own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShotProjectile {
    Record(u8),
    Definition(Box<Projectile>),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Launch {
    /// Aim at the target's current position when the shot is born.
    #[default]
    Live,
    /// Start above the caster: body height times `scale`, plus `offset`.
    CasterAbove { scale: f32, offset: f32 },
    /// Aim at the origin's ground position and this absolute height.
    AtHeight { height: f32 },
}

impl AuthoredSpell {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.duration > 0, "a spell needs a positive duration");
        ensure!(
            !self.pulses.is_empty() || !self.projectiles.is_empty(),
            "a spell needs at least one pulse or projectile"
        );
        let within = |at: u16| at <= self.duration;
        ensure!(
            self.visuals.iter().all(|visual| within(visual.at))
                && self.pulses.iter().all(|pulse| within(pulse.at))
                && self.projectiles.iter().all(|shot| within(shot.at)),
            "spell events must start within its duration"
        );
        for pulse in &self.pulses {
            ensure!(pulse.duration > 0, "a pulse lasts at least one update");
            match pulse.benefit {
                Benefit::Hp { percent } | Benefit::Resurrect { percent } => {
                    ensure!((1..=100).contains(&percent), "pulse percent is 1-100")
                }
                Benefit::Tp { amount } => ensure!(amount > 0, "a TP pulse restores TP"),
                Benefit::Stat {
                    condition,
                    percent,
                    duration,
                } => ensure!(
                    matches!(condition, Condition::AttackUp | Condition::DefenseUp)
                        && percent > 0
                        && duration > 0,
                    "a stat pulse raises attack_up or defense_up by a percent for a duration"
                ),
                Benefit::Haste { percent, duration } => ensure!(
                    (1..=100).contains(&percent) && duration > 0,
                    "a haste pulse needs a percent of 1-100 and a duration"
                ),
                Benefit::Cure { .. } => {}
            }
            if let Targets::Area { radius } = pulse.targets {
                ensure!(radius.is_finite() && radius > 0., "invalid pulse radius");
            }
        }
        for shot in &self.projectiles {
            ensure!(
                shot.velocity
                    .is_none_or(|velocity| velocity.iter().all(|value| value.is_finite()))
                    && shot.offset.iter().all(|value| value.is_finite()),
                "invalid projectile launch"
            );
        }
        if let Some(effects) = &self.effects {
            crate::library::validate_path(effects)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_heal_and_a_projectile_spell_use_the_shared_vocabulary() -> Result<()> {
        let heal: AuthoredSpell = serde_json::from_value(json!({
            "duration": 90,
            "origin": {"kind": "target"},
            "voice": 98,
            "pulses": [
                {"at": 11, "benefit": {"kind": "hp", "percent": 45}, "effect": 20},
                {"at": 11, "benefit": {"kind": "stat", "condition": "defense_up",
                    "percent": 10, "duration": 1800},
                 "targets": {"kind": "living_allies"}}
            ]
        }))?;
        heal.validate()?;
        assert_eq!(heal.pulses[0].duration, 1);
        assert_eq!(heal.pulses[1].targets, Targets::LivingAllies);

        let bolt: AuthoredSpell = serde_json::from_value(json!({
            "duration": 40,
            "startup": 23,
            "sound": 81,
            "projectiles": [{
                "at": 4,
                "projectile": 3,
                "hit": {"kind": "magic", "power": {"percent": 120}, "element": {"element": "fire"}},
                "launch": {"kind": "caster_above", "scale": 1.0, "offset": 20.0}
            }],
            "unison": {"native_id": 1001, "altitude": 0, "distance": 300, "duration": 40}
        }))?;
        bolt.validate()?;
        assert_eq!(bolt.projectiles[0].projectile, ShotProjectile::Record(3));
        Ok(())
    }

    #[test]
    fn empty_misspelled_and_out_of_range_spells_are_rejected() {
        for broken in [
            json!({"duration": 30}),
            json!({"duration": 0, "pulses": [{"at": 0, "benefit": {"kind": "hp", "percent": 10}}]}),
            json!({"duration": 30, "pulses": [{"at": 31, "benefit": {"kind": "hp", "percent": 10}}]}),
            json!({"duration": 30, "pulses": [{"at": 0, "benefit": {"kind": "hp", "percent": 0}}]}),
            json!({"duration": 30, "pulses": [{"at": 0, "benifit": {"kind": "hp", "percent": 10}}]}),
            json!({"duration": 30, "pulses": [{"at": 0, "benefit": {"kind": "hp", "percent": 10},
                "targets": {"kind": "area", "radius": -1.0}}]}),
        ] {
            let result = serde_json::from_value::<AuthoredSpell>(broken.clone())
                .map_err(anyhow::Error::from)
                .and_then(|spell| spell.validate());
            assert!(result.is_err(), "{broken}");
        }
    }
}
