//! Local, classroom-scoped texture overrides; original cooked files stay valid.
use anyhow::{Context, Result, ensure};
use bevy::prelude::*;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Component, Path},
    sync::Arc,
};

#[derive(Resource, Default, Deserialize)]
pub(super) struct Overrides {
    textures: BTreeMap<String, String>,
    /// Read before field preparation so overrides obey the same no-late-I/O
    /// contract as the verified cooked inventory.
    #[serde(skip)]
    pub bytes: Arc<BTreeMap<String, Arc<[u8]>>>,
}
impl Overrides {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("overrides/classroom.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut overrides: Self = serde_json::from_slice(&std::fs::read(&path)?)
            .context("invalid classroom texture overrides")?;
        for replacement in overrides.textures.values() {
            ensure!(
                Path::new(replacement)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)))
                    && root.join(replacement).is_file(),
                "missing or invalid HD texture: {replacement}"
            );
            if !overrides.bytes.contains_key(replacement) {
                Arc::make_mut(&mut overrides.bytes).insert(
                    replacement.clone(),
                    std::fs::read(root.join(replacement))?.into(),
                );
            }
        }
        info!(
            "Classroom HD textures: {} replacement bindings",
            overrides.textures.len()
        );
        Ok(overrides)
    }
    pub fn path<'a>(&'a self, map: u32, original: &'a str) -> &'a str {
        if map == 340 {
            self.textures.get(original).map_or(original, String::as_str)
        } else {
            original
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overrides_are_limited_to_the_classroom() {
        let overrides = Overrides {
            textures: [(
                "characters/lloyd/body.ktx2".into(),
                "overrides/body.png".into(),
            )]
            .into(),
            ..default()
        };
        assert_eq!(
            overrides.path(340, "characters/lloyd/body.ktx2"),
            "overrides/body.png"
        );
        assert_eq!(
            overrides.path(332, "characters/lloyd/body.ktx2"),
            "characters/lloyd/body.ktx2"
        );
        assert_eq!(overrides.path(340, "other.ktx2"), "other.ktx2");
    }
}
