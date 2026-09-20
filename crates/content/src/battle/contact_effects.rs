//! Shared accepted-contact feedback from the native battle dispatcher.
use super::effects::{EffectBank, EffectId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactEffects {
    /// Neutral then the eight elemental events. Zero entries emit no elemental burst.
    pub elemental: [u8; 9],
    pub flash: [[u8; 3]; 9],
}

impl ContactEffects {
    pub fn programs(&self) -> BTreeSet<EffectId> {
        self.elemental
            .into_iter()
            .filter(|&id| id != 0)
            .chain([0, 1, 2, 11, 12, 16, 47])
            .map(|id| EffectId {
                bank: EffectBank::Common,
                id,
            })
            .collect()
    }
}
