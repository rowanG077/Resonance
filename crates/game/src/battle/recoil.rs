//! Resolve shared recoil operands from the encounter's verified source snapshot.
use anyhow::{Context, Result};
use resonance_battle::{ReactionRule, RecoilDirection, RecoilProfile, RecoilRule, VerticalRecoil};
use resonance_content::{battle_action, battle_recoil, prepared::Files};

pub struct Parameters {
    impulses: Vec<[f32; 2]>,
    pub suppression_distance: f32,
    light_vertical_scale: f32,
    heavy_vertical_scale: f32,
    guard_speed: f32,
    default_guard_preferences: Vec<u8>,
}

impl Parameters {
    pub fn load(files: &Files) -> Result<Self> {
        let source: battle_recoil::Table = files.json(battle_recoil::PATH)?;
        Ok(Self {
            impulses: source
                .impulses
                .into_iter()
                .map(|[x, y]| Ok([x.finite()?, y.finite()?]))
                .collect::<Result<_>>()?,
            suppression_distance: source.suppression_distance.finite()?,
            light_vertical_scale: source.light_vertical_scale.finite()?,
            heavy_vertical_scale: source.heavy_vertical_scale.finite()?,
            guard_speed: source.guard_speed.finite()?,
            default_guard_preferences: source.default_guard_preferences,
        })
    }

    /// 1A9AC chooses a nonzero explicit preference before consulting actor-kind
    /// defaults. Resolve this once into the guard state; combat has no table indices.
    pub fn guard_recovery_bonus(&self, preference: u8, actor_kind: u8) -> Result<u8> {
        let preference = if preference == 0 {
            *self
                .default_guard_preferences
                .get(usize::from(actor_kind))
                .context("missing actor guard preference")?
        } else {
            preference
        };
        Ok(preference.wrapping_sub(1).min(2) * 5 + if preference == 3 { 15 } else { 0 })
    }

    /// The selector belongs to the original contact shape, independently of the
    /// common action hit-rule index. This does not admit the complete hit yet.
    pub fn rule(&self, hit: &battle_action::HitRule, selector: u8) -> Result<RecoilRule> {
        Ok(RecoilRule {
            impulse: *self
                .impulses
                .get(usize::from(selector))
                .context("missing recoil impulse")?,
            delay: hit.knockback_delay,
            knock_down: hit.flags & 4 != 0,
            launch: hit.flags & 8 != 0,
            lift_guard: hit.flags & 0x100 != 0,
            guard_speed: self.guard_speed,
        })
    }

    pub fn reaction(
        &self,
        hit: &battle_action::HitRule,
        selector: u8,
        direction: u8,
    ) -> Result<ReactionRule> {
        Ok(ReactionRule {
            recoil: self.rule(hit, selector)?,
            direction: match direction {
                0 => RecoilDirection::Travel,
                1 => RecoilDirection::AwayFromOwner,
                2 => RecoilDirection::AwayFromContact,
                3 => RecoilDirection::TowardContact,
                _ => RecoilDirection::None,
            },
            hitstun: hit.hitstun,
            alternate_motion: hit.flags & 2 != 0,
            armor_damage: hit.armor_damage,
            stun_chance: hit.stun_chance,
            stagger: hit.stagger,
            hits_down: hit.flags & 0x40 != 0,
        })
    }

    /// Original actor-profile fields, resolved here so gameplay has no packed
    /// offsets or masks. Unknown weight values take the original default branch.
    pub fn profile(&self, weight: u8, flags: u32) -> RecoilProfile {
        RecoilProfile {
            vertical: match weight {
                1 => VerticalRecoil::Scale(self.light_vertical_scale),
                2 => VerticalRecoil::Scale(self.heavy_vertical_scale),
                3 => VerticalRecoil::Grounded,
                _ => VerticalRecoil::Unchanged,
            },
            can_knock_down: flags & 0x200 == 0,
            can_launch: flags & 0x200 == 0 && flags & 0x02000000 == 0 && weight != 3,
            clear_pending: flags & 0x400 != 0,
        }
    }
}
