//! Resolve shared recoil parameters from the encounter snapshot.
use anyhow::{Context, Result};
use resonance_battle::{RecoilProfile, VerticalRecoil};
use resonance_content::{battle_recoil, prepared::Files};

pub struct Parameters {
    light_vertical_scale: f32,
    heavy_vertical_scale: f32,
    default_guard_recovery_bonuses: Vec<u8>,
}

impl Parameters {
    pub fn load(files: &Files) -> Result<Self> {
        let source: battle_recoil::Table = files.json(battle_recoil::PATH)?;
        Ok(Self {
            light_vertical_scale: source.light_vertical_scale,
            heavy_vertical_scale: source.heavy_vertical_scale,
            default_guard_recovery_bonuses: source.default_guard_recovery_bonuses,
        })
    }

    /// Resolve the saved position choice, using the cooked bonus for its default.
    pub fn guard_recovery_bonus(&self, position: u8, actor_kind: u8) -> Result<u8> {
        if position == 0 {
            self.default_guard_recovery_bonuses
                .get(usize::from(actor_kind))
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
                1 => VerticalRecoil::Scale(self.light_vertical_scale),
                2 => VerticalRecoil::Scale(self.heavy_vertical_scale),
                3 => VerticalRecoil::Grounded,
                _ => VerticalRecoil::Unchanged,
            },
            can_knock_down: !traits.knockdown_immune,
            can_launch: !traits.knockdown_immune && !traits.launch_immune && weight != 3,
            clear_pending: traits.clear_pending_on_hit,
        }
    }
}
