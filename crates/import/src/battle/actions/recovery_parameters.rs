//! Shared caster motion and healing callbacks recovered during physical cooking.
use super::*;
use resonance_content::battle::{
    actions::nurse::NurseRecipe,
    effects::{EffectBank, EffectId},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub stored_release_rate: f32,
    pub recovery_pose: recovery_pose::Parameters,
    pub first_aid: RecoveryPulse,
    pub heal: StoredHealing,
    pub cure: StoredHealing,
    pub nurse: NurseRecipe,
}

#[derive(Serialize, Deserialize)]
pub(super) struct StoredHealing {
    pub lifetime: u16,
    pub percent: u16,
    pub presentation: StoredSpellPresentation,
}

impl Parameters {
    pub fn read(rel: &Rel) -> Result<Self> {
        for (native, initializer) in [(236, 0x60ab4), (238, 0x74378), (257, 0x7e99c)] {
            let dispatch = rel.pointer(DATA, 0x1238 + (native - 200) * 4)?;
            ensure!(
                rel.pointer(dispatch.0, dispatch.1)? == (1, initializer)
                    && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37e48),
                "unexpected targeted recovery controller {native}"
            );
            if native != 236 {
                ensure!(
                    rel.pointer(dispatch.0, dispatch.1 + 8)? == (1, 0x37dd8),
                    "unexpected stored recovery cleanup {native}"
                );
            }
        }
        ensure!(
            rel.local_targets().contains(&(1, 0x6096c)),
            "missing First Aid callback"
        );
        let stored_release_rate = float(rel.at((4, 0x1c84))?, 0)?;
        ensure!(
            stored_release_rate.is_finite() && stored_release_rate > 0.,
            "invalid stored release rate"
        );
        // First Aid heals at age 10. Heal and Cure heal immediately, then retain
        // their presentation for 165 ticks before releasing the stored slot.
        let stored = |offset, percent| -> Result<StoredHealing> {
            Ok(StoredHealing {
                lifetime: 165,
                percent,
                presentation: stored_parameters::presentation(rel.at((4, offset))?)?,
            })
        };
        Ok(Self {
            stored_release_rate,
            recovery_pose: recovery_pose::Parameters::read(rel)?,
            first_aid: RecoveryPulse {
                tick: 10,
                percent: 30,
                effect: EffectId {
                    bank: EffectBank::Techniques,
                    id: 20,
                },
            },
            heal: stored(0x4f20, 60)?,
            cure: stored(0x6270, 100)?,
            nurse: nurse::read_parameters(rel)?,
        })
    }
}

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((module, _)) = crate::battle::embedded::Layout::identify(file) else {
        return Ok(None);
    };
    if module != "US_r_Top2Btl.rel" {
        return Ok(None);
    }
    crate::battle::embedded::write(
        file,
        output,
        "battle-recovery-parameters",
        &Parameters::read(&Rel::read(file)?)?,
        serde_json::json!({"dispatch":{"section":5,"offset":0x1238},"parameters_section":4}),
    )
    .map(Some)
}
