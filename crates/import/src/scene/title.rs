//! Original title declarations and typed texture resources.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const PATH: &str = "embedded/title.json";

#[derive(Serialize, Deserialize)]
pub(crate) struct Resource {
    pub path: String,
    pub sha256: String,
}

impl Resource {
    fn read(extracted: &Path, path: String) -> Result<Self> {
        Ok(Self {
            sha256: crate::media::hash_file(&extracted.join("files").join(&path))?,
            path,
        })
    }

    pub fn textures(&self, extracted: &Path) -> Result<crate::texture::Decoded> {
        resonance_content::validate_asset_path(&self.path)?;
        let source = fs::read(extracted.join("files").join(&self.path))?;
        ensure!(
            crate::digest(&source) == self.sha256,
            "title texture source digest mismatch"
        );
        crate::texture::decode_source(&source)
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Recipe {
    pub game_id: String,
    pub revision: u8,
    pub executable_sha256: String,
    pub images: Resource,
    pub field: Resource,
    pub effects: Resource,
}

impl Recipe {
    pub fn read(extracted: &Path, executable: &[u8]) -> Result<Self> {
        let boot = fs::read(extracted.join("sys/boot.bin"))?;
        ensure!(
            boot.get(..6) == Some(b"GQSEAF") && boot.get(7) == Some(&0),
            "expected GQSEAF revision 0"
        );
        let roles = crate::all_assets::roles::effects_declaration(executable)?;
        Ok(Self {
            game_id: String::from_utf8(boot[..6].to_vec())?,
            revision: boot[7],
            executable_sha256: crate::digest(executable),
            images: Resource::read(
                extracted,
                crate::all_assets::roles::title_path(extracted, executable)?,
            )?,
            field: Resource::read(extracted, super::title_source(extracted, executable)?)?,
            effects: Resource::read(
                extracted,
                crate::all_assets::roles::declared_path(&extracted.join("files"), &roles)?,
            )?,
        })
    }

    pub fn cook(&self, output: &Path) -> Result<Vec<String>> {
        crate::write_atomic(&output.join(PATH), &serde_json::to_vec(self)?)?;
        Ok(vec![PATH.into()])
    }
}
