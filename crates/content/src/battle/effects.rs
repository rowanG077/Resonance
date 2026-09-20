//! Projectile motion and visual effect recipes, resolved from battle resources.
use super::actions::HitShape;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum EffectBank {
    Common,
    Techniques,
    Enemy(u8),
    /// Authored spell package (native technique ID minus 200), independent of cast slots.
    Magic(u16),
    Skill(u16),
    Arena(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EffectId {
    pub bank: EffectBank,
    pub id: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleEffects {
    pub projectiles: Vec<ProjectileRecipe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectileRecipe {
    /// None identifies a native contact volume without an authored recipe row.
    pub id: Option<EffectId>,
    /// Zero disables age expiry; contacts, height and range may still end the flight.
    pub lifetime: u16,
    pub movement: ProjectileMovement,
    #[serde(default)]
    pub behavior: ProjectileBehavior,
    pub velocity_jitter: [f32; 3],
    pub spawn_offset: [f32; 3],
    pub hit_offset: [f32; 3],
    pub shape: HitShape,
    pub knockback: KnockbackDirection,
    /// Inclusive contact interval; None remains active throughout its lifetime.
    pub active: Option<[u16; 2]>,
    pub persist_after_hit: bool,
    pub clashable: bool,
    /// Retain a resolvable bank for later overrides; None means an unbound, disabled sequence.
    pub birth_bank: Option<EffectBank>,
    pub spawn_effect: Option<EffectId>,
    pub trail_effect: Option<EffectId>,
    pub trail_interval: u16,
    pub ground_effect: Option<EffectId>,
    pub shadow: Option<ProjectileShadow>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectileMovement {
    Ballistic {
        velocity: [f32; 3],
        acceleration: [f32; 3],
        #[serde(default)]
        steering: Option<ProjectileSteering>,
    },
    Directed {
        direction: [f32; 3],
        speed: f32,
    },
    Homing {
        direction: [f32; 3],
        speed: f32,
        blend: f32,
        start: u8,
        end: Option<u8>,
        #[serde(default)]
        horizontal: bool,
    },
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectileBehavior {
    pub aim: ProjectileAim,
    pub spawn_at_target: bool,
    /// Tracks directions with a positive X or Z component; other headings remain held.
    pub face_velocity: bool,
    pub clamp_ground: bool,
    /// Stop translation on floor contact without clearing acceleration or attached model angles.
    pub stop_on_ground: bool,
    /// Scatter and tumble on every floor contact, independently of ordinary bouncing.
    pub scatter_on_ground: bool,
    /// Reflects vertical velocity after acceleration and steering on ground contact.
    pub bounce_restitution: Option<f32>,
    /// Contact debris enables floor reflection; ordinary flight keeps its authored motion.
    pub bounce_after_contact: bool,
    pub contact_response: ProjectileContactResponse,
    pub unlimited_range: bool,
    pub repeat_limit: u8,
    pub hit_growth: [f32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity_reset: Option<ProjectileVelocityReset>,
}

/// Changes flight motion only after the complete contact batch has resolved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectileContactResponse {
    #[default]
    Ordinary,
    /// Rebound away from the hit, tumble, and remain visible without further hits.
    BounceAway,
    /// Keep flying through ordinary hits; rebound after a block or opposing attack.
    BounceAwayOnBlock,
    /// Stop submitting hits while retaining the original motion and model orientation.
    Disarm,
    /// Ordinary hits remain live; a block or clash disables further contacts.
    DisarmOnBlock,
}

impl ProjectileContactResponse {
    pub fn rebounds(self) -> bool {
        matches!(self, Self::BounceAway | Self::BounceAwayOnBlock)
    }
}

/// After this age's movement, replace velocity with scaled acceleration.
/// Acceleration, steering, contact windows, and expiry continue independently.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProjectileVelocityReset {
    pub age: std::num::NonZeroU8,
    pub acceleration_scale: f32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectileAim {
    #[default]
    Heading,
    World,
    Target,
    /// Initial world direction from one owner body bone toward another.
    Bones {
        toward: u8,
        from: u8,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProjectileSteering {
    /// Unclamped interpolation weight; values above one extrapolate.
    pub blend: f32,
    pub start: u8,
    pub end: Option<u8>,
    pub horizontal: bool,
}

impl ProjectileSteering {
    fn validate(self) -> Result<()> {
        ensure!(
            self.blend.is_finite() && self.end.is_none_or(|end| end > self.start),
            "invalid projectile steering"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnockbackDirection {
    Attacker,
    Velocity,
    AwayFromProjectile,
    TowardProjectile,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectileShadow {
    pub color: [u8; 4],
    pub radius: f32,
    pub additive: bool,
}

impl BattleEffects {
    pub fn projectile(&self, id: EffectId) -> Option<&ProjectileRecipe> {
        self.projectiles.iter().find(|p| p.id == Some(id))
    }
    pub fn validate(&self) -> Result<()> {
        for (i, recipe) in self.projectiles.iter().enumerate() {
            ensure!(
                recipe.id.is_some(),
                "catalogue projectile has no source identity"
            );
            ensure!(
                self.projectiles[..i].iter().all(|p| p.id != recipe.id),
                "duplicate projectile recipe"
            );
            recipe
                .validate()
                .with_context(|| format!("projectile {:?}", recipe.id.unwrap()))?;
        }
        Ok(())
    }
}

impl ProjectileRecipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.spawn_effect.is_none() || self.birth_bank.is_some(),
            "projectile birth effect has no resolved bank"
        );
        ensure!(
            self.lifetime <= i16::MAX as u16
                && self.trail_interval > 0
                && self.active.is_none_or(|[start, end]| start <= end)
                && self
                    .spawn_offset
                    .iter()
                    .chain(&self.hit_offset)
                    .chain(&self.velocity_jitter)
                    .chain(&self.behavior.hit_growth)
                    .all(|v| v.is_finite()),
            "invalid projectile lifetime or transform"
        );
        ensure!(
            self.behavior.bounce_restitution.is_none_or(f32::is_finite),
            "invalid projectile bounce restitution"
        );
        ensure!(
            [
                self.shape.radius,
                self.shape.height,
                self.shape.inner_radius
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.),
            "invalid projectile hit shape"
        );
        if let Some(reset) = self.behavior.velocity_reset {
            ensure!(
                reset.acceleration_scale.is_finite(),
                "invalid projectile velocity reset"
            );
            ensure!(
                matches!(self.movement, ProjectileMovement::Ballistic { .. }),
                "directed projectile reset requires retained acceleration"
            );
        }
        ensure!(
            !(self.behavior.contact_response.rebounds() || self.behavior.scatter_on_ground)
                || matches!(self.movement, ProjectileMovement::Ballistic { .. }),
            "projectile scattering requires ballistic motion"
        );
        ensure!(
            !self.behavior.contact_response.rebounds()
                || self.behavior.bounce_restitution.is_some(),
            "contact debris requires authored bounce restitution"
        );
        ensure!(
            !self.behavior.bounce_after_contact
                || self.behavior.contact_response != ProjectileContactResponse::Ordinary,
            "deferred ground bounce requires contact debris"
        );
        match self.movement {
            ProjectileMovement::Ballistic {
                velocity,
                acceleration,
                steering,
            } => {
                ensure!(
                    velocity.iter().chain(&acceleration).all(|v| v.is_finite()),
                    "invalid projectile velocity"
                );
                if let Some(steering) = steering {
                    steering.validate()?;
                }
            }
            ProjectileMovement::Directed { direction, speed } => ensure!(
                direction.iter().all(|v| v.is_finite()) && speed.is_finite(),
                "invalid directed projectile velocity"
            ),
            ProjectileMovement::Homing {
                direction,
                speed,
                blend,
                start,
                end,
                horizontal,
            } => {
                ensure!(
                    direction.iter().all(|v| v.is_finite()) && speed.is_finite(),
                    "invalid projectile homing"
                );
                ProjectileSteering {
                    blend,
                    start,
                    end,
                    horizontal,
                }
                .validate()?;
            }
        }
        ensure!(
            self.shadow
                .is_none_or(|shadow| shadow.radius.is_finite() && shadow.radius >= 0.),
            "invalid projectile shadow"
        );
        Ok(())
    }
}
