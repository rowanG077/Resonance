//! Small authoring adapters for named UI roles and direct image paths.
use crate::font::UiTexture;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(test)]
mod tests;

/// Capture and validate the selected image without changing authored coordinates.
pub fn prepare_image<B: AsRef<[u8]>>(
    image: &UiTexture,
    mut read: impl FnMut(&str) -> Result<B>,
) -> Result<()> {
    image.validate()?;
    read(&image.path)
        .and_then(|bytes| resonance_asset_writer::texture::dimensions(bytes.as_ref()))
        .with_context(|| format!("UI image {}", image.path))?;
    Ok(())
}
pub fn named<'a, T>(values: &'a [T], names: &'a [&str]) -> Result<BTreeMap<&'a str, &'a T>> {
    anyhow::ensure!(values.len() == names.len(), "incomplete named UI roles");
    Ok(names.iter().copied().zip(values).collect())
}
pub fn ordered<T>(mut values: BTreeMap<String, T>, names: &[&str]) -> Result<Vec<T>> {
    let result = names
        .iter()
        .map(|name| {
            values
                .remove(*name)
                .with_context(|| format!("missing UI role {name}"))
        })
        .collect::<Result<_>>()?;
    anyhow::ensure!(
        values.is_empty(),
        "unknown UI role: {}",
        values.keys().next().unwrap()
    );
    Ok(result)
}

macro_rules! roles {
    ($module:ident, $names:expr) => {
        pub mod $module {
            use super::*;
            pub const NAMES: &[&str] = &$names;
            pub fn serialize<S: serde::Serializer, T: Serialize>(
                values: &[T; NAMES.len()],
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                named(values, NAMES)
                    .map_err(serde::ser::Error::custom)?
                    .serialize(serializer)
            }
            pub fn deserialize<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
                deserializer: D,
            ) -> Result<[T; NAMES.len()], D::Error> {
                let values = ordered(BTreeMap::<String, T>::deserialize(deserializer)?, NAMES)
                    .map_err(serde::de::Error::custom)?;
                Ok(values.try_into().ok().unwrap())
            }
        }
    };
}
roles!(
    characters,
    symphonia_constants::PartyCharacterId::RESOURCE_NAMES
);
roles!(vitals, ["hp", "tp"]);
roles!(worlds, ["sylvarant", "tethe_alla"]);
roles!(sides, ["left", "right"]);
roles!(damage_targets, ["party", "enemy"]);
roles!(damage_sizes, ["normal", "guarded", "critical"]);
roles!(results, ["experience", "statistics", "notice"]);
roles!(
    commands,
    [
        "techniques",
        "unison",
        "strategy",
        "equipment",
        "items",
        "escape"
    ]
);
roles!(selectors, ["user", "target"]);
roles!(vertical, ["top", "bottom"]);
roles!(lines, ["first_line", "second_line"]);
roles!(
    targets,
    ["frame", "player_1", "player_2", "player_3", "player_4"]
);
roles!(
    status,
    [
        "attack",
        "defense",
        "up",
        "magic_attack",
        "physical_barrier",
        "magical_barrier",
        "accuracy_curse",
        "reduced_item_effect",
        "holy_song",
        "down"
    ]
);
roles!(unison, ["a_b", "x_y"]);
roles!(notices, ["arte", "spell", "item", "outcome"]);
roles!(
    scan,
    [
        "neutral",
        "water",
        "wind",
        "fire",
        "earth",
        "lightning",
        "ice",
        "light",
        "darkness",
        "headings"
    ]
);
roles!(escape, ["frame", "needle", "fill"]);
roles!(game_over_choices, ["load", "title"]);

pub mod portraits {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        values: &[Option<UiTexture>; 9],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        characters::NAMES
            .iter()
            .zip(values)
            .filter_map(|(name, image)| image.as_ref().map(|image| (*name, image)))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[Option<UiTexture>; 9], D::Error> {
        let mut values = BTreeMap::<String, UiTexture>::deserialize(deserializer)?;
        let result = std::array::from_fn(|i| values.remove(characters::NAMES[i]));
        if !values.is_empty() {
            return Err(serde::de::Error::custom("unknown portrait character"));
        }
        Ok(result)
    }
}
