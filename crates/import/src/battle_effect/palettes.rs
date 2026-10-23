//! Resolve effect palette strides against the resident texture artwork.
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{
    SourceBank, declaration::Declaration, visual::ParticleTexture,
};
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) struct SharedPalettes {
    banks: Vec<SourceBank>,
    bindings: BTreeMap<(u8, u8), u8>,
}

impl SharedPalettes {
    pub(crate) fn read(usual: &[u8], output: &Path) -> Result<Self> {
        let identity = crate::digest(usual);
        let primary = output.join("battle/effects/common.json");
        let common: SourceBank = serde_json::from_slice(
            &fs::read(&primary)
                .context("prepare resident effect artwork before dependent effects")?,
        )?;
        let prefix = if common
            .art
            .as_ref()
            .is_some_and(|art| art.source_sha256 == identity)
        {
            "battle".to_owned()
        } else {
            format!("battle/variants/{identity}")
        };
        let mut banks = Vec::new();
        let mut bindings = BTreeMap::new();
        for (member, name) in [(2, "common"), (3, "techniques")] {
            let path = output.join(format!("{prefix}/effects/{name}.json"));
            let bank: SourceBank = serde_json::from_slice(&fs::read(&path)?)?;
            let (source, strides) =
                super::read_with_palettes(crate::source_assets::section(usual, member)?)?;
            ensure!(
                source.source_sha256 == bank.source_sha256
                    && source.actors.len() == bank.actors.len(),
                "resident palette declarations differ from source"
            );
            let art = bank
                .art
                .as_ref()
                .context("resident effect artwork is missing")?;
            ensure!(
                art.source_sha256 == identity,
                "resident effect artwork differs from source"
            );
            for ((authored, prepared), stride) in
                source.actors.iter().zip(&bank.actors).zip(strides)
            {
                let (
                    Declaration::Particle { visual, .. },
                    Declaration::Particle { visual: cooked, .. },
                ) = (authored, prepared)
                else {
                    continue;
                };
                let Some(slot) = visual.texture.slot() else {
                    continue;
                };
                ensure!(
                    cooked.texture.slot() == Some(slot)
                        && cooked.texture.dual() == visual.texture.dual(),
                    "resident texture binding differs from source"
                );
                let set = cooked.texture.palette_set();
                ensure!(
                    bindings
                        .insert((slot, stride), set)
                        .is_none_or(|previous| previous == set),
                    "conflicting resident palette bindings"
                );
            }
            banks.push(bank);
        }
        ensure!(
            serde_json::to_value(&banks[0].art)? == serde_json::to_value(&banks[1].art)?,
            "resident banks have different shared artwork"
        );
        Ok(Self { banks, bindings })
    }

    pub(crate) fn set(&mut self, slot: u8, stride: u8) -> Result<u8> {
        if let Some(&set) = self.bindings.get(&(slot, stride)) {
            return Ok(set);
        }
        let (&(_, prepared_stride), &prepared_set) = self
            .bindings
            .iter()
            .find(|((prepared_slot, step), _)| {
                *prepared_slot == slot && *step != 0 && stride != 0 && stride.is_multiple_of(*step)
            })
            .with_context(|| {
                format!("no prepared effect palette stride for slot {slot}, stride {stride}")
            })?;
        let textures = self.banks[0]
            .art
            .as_ref()
            .unwrap()
            .textures
            .get(&slot)
            .context("resident effect texture slot is missing")?;
        ensure!(
            !textures.is_empty(),
            "resident effect texture slot is empty"
        );
        let maps = textures
            .iter()
            .map(|texture| {
                let selectors = texture
                    .palette_sets
                    .get(usize::from(prepared_set))
                    .context("resident effect palette table is missing")?;
                Ok(resample(selectors, stride / prepared_stride))
            })
            .collect::<Result<Vec<_>>>()?;
        let set = (0..textures[0].palette_sets.len())
            .find(|&set| {
                textures
                    .iter()
                    .zip(&maps)
                    .all(|(texture, map)| texture.palette_sets.get(set) == Some(map))
            })
            .context("shared effect palette table was not prepared before publication")?;
        let set = u8::try_from(set).context("too many shared effect palette tables")?;
        self.bindings.insert((slot, stride), set);
        Ok(set)
    }

    pub(crate) fn bind(&mut self, bank: &mut SourceBank, strides: &[u8]) -> Result<()> {
        ensure!(
            bank.actors.len() == strides.len(),
            "effect palette binding count changed"
        );
        for (actor, &stride) in bank.actors.iter_mut().zip(strides) {
            if let Declaration::Particle { visual, .. } = actor
                && let ParticleTexture::Atlas {
                    slot, palette_set, ..
                } = &mut visual.texture
                && self.banks[0]
                    .art
                    .as_ref()
                    .unwrap()
                    .textures
                    .contains_key(slot)
            {
                *palette_set = self.set(*slot, stride)?;
            }
        }
        Ok(())
    }
}

pub(super) fn resample(selectors: &[Option<usize>], factor: u8) -> Vec<Option<usize>> {
    (0..=u8::MAX)
        .map(|selector| {
            selectors
                .get(usize::from(selector) * usize::from(factor))
                .copied()
                .flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_palette_stride_preserves_missing_windows() {
        let selectors = [Some(0), Some(1), None, Some(3), Some(4)];
        let widened = super::resample(&selectors, 2);
        assert_eq!(&widened[..4], &[Some(0), None, Some(4), None]);
        assert_eq!(widened.len(), 256);
    }
}
