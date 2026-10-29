//! Compact palette choices at the editable boundary. Runtime/native preparation
//! uses explicit selector tables; missing entries are never filled with image 0.
use super::EffectTexture;
use crate::{font::UiTexture, texture::Sampler};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Definition {
    #[serde(with = "crate::texture::images")]
    images: Vec<UiTexture>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    sampler: Sampler,
    /// Without overrides, palette selectors follow image order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    palette_sets: Option<Vec<Palette>>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Palette {
    /// Every selector uses the same image (an unindexed texture).
    Constant(usize),
    /// Omitted trailing entries and explicit nulls are unavailable choices.
    Choices(Vec<Option<usize>>),
}

impl From<EffectTexture> for Definition {
    fn from(texture: EffectTexture) -> Self {
        let identity = (0..256)
            .map(|i| (i < texture.images.len()).then_some(i))
            .collect::<Vec<_>>();
        let mut sets = texture.palette_sets;
        for set in &mut sets {
            while set.last() == Some(&None) {
                set.pop();
            }
        }
        let mut default = identity;
        while default.last() == Some(&None) {
            default.pop();
        }
        let palette_sets = if sets.len() == 1 && sets[0] == default {
            None
        } else {
            Some(
                sets.into_iter()
                    .map(|set| {
                        if set.len() == 256
                            && let Some(image) = set[0]
                            && set.iter().all(|value| *value == Some(image))
                        {
                            Palette::Constant(image)
                        } else {
                            Palette::Choices(set)
                        }
                    })
                    .collect(),
            )
        };
        Self {
            images: texture.images,
            sampler: texture.sampler,
            palette_sets,
        }
    }
}

impl TryFrom<Definition> for EffectTexture {
    type Error = anyhow::Error;

    fn try_from(definition: Definition) -> Result<Self> {
        ensure!(!definition.images.is_empty(), "empty effect texture");
        definition.sampler.validate()?;
        let palette_sets = match definition.palette_sets {
            None => vec![
                (0..256)
                    .map(|i| (i < definition.images.len()).then_some(i))
                    .collect(),
            ],
            Some(sets) => {
                ensure!(sets.len() <= 256, "too many effect palette sets");
                sets.into_iter()
                    .map(|set| {
                        let mut choices = match set {
                            Palette::Constant(image) => vec![Some(image); 256],
                            Palette::Choices(choices) => choices,
                        };
                        ensure!(choices.len() <= 256, "too many effect palette choices");
                        ensure!(
                            choices
                                .iter()
                                .flatten()
                                .all(|&i| i < definition.images.len()),
                            "effect palette image is missing"
                        );
                        choices.resize(256, None);
                        Ok(choices)
                    })
                    .collect::<Result<_>>()?
            }
        };
        Ok(Self {
            images: definition.images,
            sampler: definition.sampler,
            palette_sets,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn editable_choices_keep_holes_constants_and_image_order_distinct() -> Result<()> {
        let images = json!(["red.ktx2", "blue.ktx2"]);
        let default = json!({"images":images});
        let texture: EffectTexture = serde_json::from_value(default.clone())?;
        assert_eq!(texture.palette_image(0, 1)?, 1);
        assert!(texture.palette_image(0, 2).is_err());
        assert!(texture.palette_image(1, 0).is_err());
        assert_eq!(serde_json::to_value(texture)?, default);

        let overrides = json!({"images":images,"palette_sets":[[1,null,0],1,[]]});
        let texture: EffectTexture = serde_json::from_value(overrides.clone())?;
        assert_eq!(texture.palette_image(0, 0)?, 1);
        assert!(texture.palette_image(0, 1).is_err());
        assert_eq!(texture.palette_image(0, 2)?, 0);
        assert!(texture.palette_image(0, 3).is_err());
        assert_eq!(texture.palette_image(1, 255)?, 1);
        assert!(texture.palette_image(2, 0).is_err());
        assert_eq!(serde_json::to_value(texture)?, overrides);

        for palettes in [
            json!([[2]]),
            json!([2]),
            json!([vec![0; 257]]),
            json!(vec![0; 257]),
        ] {
            assert!(
                serde_json::from_value::<EffectTexture>(
                    json!({"images":images,"palette_sets":palettes})
                )
                .is_err()
            );
        }
        Ok(())
    }
}
