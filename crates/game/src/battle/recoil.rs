//! Resolve shared recoil parameters from the encounter snapshot.
use anyhow::{Context, Result};
use resonance_battle::{Character, RecoilProfile, VerticalRecoil};
use resonance_content::{battle_recoil, prepared::Files};

pub struct Parameters {
    source: battle_recoil::Table,
}

impl Parameters {
    pub fn load(files: &Files) -> Result<Self> {
        Ok(Self {
            source: resonance_content::battle_rules::recoil(files)?,
        })
    }

    pub(super) fn hit(
        &self,
        hit: &resonance_content::battle_action::ActionHit,
    ) -> Result<resonance_battle::RecoilRule> {
        Ok(resonance_battle::RecoilRule {
            impulse: *self
                .source
                .impulses
                .get(usize::from(hit.recoil))
                .context("missing action recoil impulse")?,
            delay: hit.recoil_delay,
            knock_down: hit.knock_down,
            launch: hit.launch,
            lift_guard: hit.lift_guard,
            guard_speed: self.source.guard_speed,
            suppression_distance: self.source.suppression_distance,
        })
    }

    /// Resolve the saved position choice, using the cooked bonus for its default.
    pub fn guard_recovery_bonus(&self, position: u8, character: Character) -> Result<u8> {
        if position == 0 {
            self.source
                .default_guard_recovery_bonuses
                .get(character as usize)
                .copied()
                .context("missing actor guard recovery bonus")
        } else {
            Ok(battle_recoil::strategy_guard_recovery(position))
        }
    }

    /// Translate actor weight and defensive traits into recoil behavior.
    pub fn profile(
        &self,
        weight: u8,
        traits: &resonance_content::battle_profile::ProfileTraits,
    ) -> RecoilProfile {
        RecoilProfile {
            vertical: match weight {
                1 => VerticalRecoil::Scale(self.source.light_vertical_scale),
                2 => VerticalRecoil::Scale(self.source.heavy_vertical_scale),
                3 => VerticalRecoil::Grounded,
                _ => VerticalRecoil::Unchanged,
            },
            can_knock_down: !traits.knockdown_immune,
            can_launch: !traits.knockdown_immune && !traits.launch_immune && weight != 3,
            clear_pending: traits.clear_pending_on_hit,
        }
    }
}
