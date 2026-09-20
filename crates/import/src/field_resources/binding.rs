//! Resolve script handles into verified physical resource directories without conversion.
use crate::{all_assets::PhysicalDirectory, resource::Catalogue};
use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) struct Resources<'a> {
    root: &'a Path,
    extracted: &'a Path,
    catalogue: &'a Catalogue,
    disc: u8,
    sources: BTreeMap<String, Vec<String>>,
    directories: BTreeMap<String, String>,
}

fn read<T: DeserializeOwned>(root: &Path, path: &str) -> Result<T> {
    resonance_content::validate_asset_path(path)?;
    serde_json::from_slice(
        &fs::read(root.join(path))
            .with_context(|| format!("missing cooked {path}; run cook-all first"))?,
    )
    .with_context(|| format!("invalid cooked {path}"))
}

impl<'a> Resources<'a> {
    pub(crate) fn animation(&mut self, id: u32) -> Result<crate::animation::AuthoredAnimation> {
        let directory = self.directory(id)?;
        read(self.root, &format!("{directory}/animation.json"))
    }

    pub(crate) fn open(
        root: &'a Path,
        extracted: &'a Path,
        catalogue: &'a Catalogue,
    ) -> Result<Self> {
        Ok(Self {
            root,
            extracted,
            catalogue,
            disc: crate::disc_number(extracted)?,
            sources: read(root, "sources.json")?,
            directories: BTreeMap::new(),
        })
    }

    pub(crate) fn directory(&mut self, id: u32) -> Result<String> {
        let declared = self.catalogue.source(id)?;
        let directory = if let Some(directory) = self.directories.get(declared) {
            directory.clone()
        } else {
            let files = self.extracted.join("files");
            let source = super::resolve_path(&files, declared)?;
            let key = format!("disc{}/{source}", self.disc);
            let paths = self
                .sources
                .get(&key)
                .with_context(|| format!("missing cooked {key}"))?;
            let [directory] = paths.as_slice() else {
                anyhow::bail!("expected one physical resource directory for {key}");
            };
            ensure!(
                *directory == format!("assets/{}", crate::media::hash_file(&files.join(source))?),
                "cooked resource source digest mismatch for {key}"
            );
            resonance_content::validate_asset_path(directory)?;
            let directory = self.payload(directory.clone())?;
            self.directories.insert(declared.into(), directory.clone());
            directory
        };
        if id >> 16 == 0 {
            return Ok(directory);
        }
        let archive: PhysicalDirectory = read(self.root, &format!("{directory}/archive.json"))?;
        archive.validate()?;
        let entry = archive
            .members
            .get((id & 0xffff) as usize)
            .context("script resource outside cooked archive")?;
        // Empty native archive entries fall back to entry zero, including its alias identity.
        let canonical = entry
            .or(archive.members[0])
            .context("missing cooked script resource and fallback")?;
        self.payload(format!("{directory}/{canonical}"))
    }

    fn payload(&self, mut directory: String) -> Result<String> {
        for _ in 0..=16 {
            ensure!(
                self.root.join(&directory).is_dir(),
                "missing cooked resource directory {directory}"
            );
            let path = format!("{directory}/cabinet.json");
            if !self.root.join(&path).try_exists()? {
                return Ok(directory);
            }
            let members: Vec<String> = read(self.root, &path)?;
            let [member] = members.as_slice() else {
                anyhow::bail!("script resource requires one cabinet payload");
            };
            resonance_content::validate_asset_path(member)?;
            directory = format!("{directory}/{member}");
        }
        anyhow::bail!("cooked script resource nesting exceeds 16 levels")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn script_texture_binding_preserves_aliases_pages_and_source_ownership() -> Result<()> {
        let work = crate::temporary_path(&std::env::temp_dir().join("script-texture-binding"));
        let root = work.join("cooked");
        let extracted = work.join("extracted");
        let result = (|| -> Result<()> {
            let write = |path: &str, value: serde_json::Value| {
                crate::write_atomic(&root.join(path), &serde_json::to_vec(&value)?)
            };
            crate::write_atomic(&extracted.join("sys/boot.bin"), b"GQSEAF\0\0")?;
            crate::write_atomic(&extracted.join("files/Bank.bin"), b"bank")?;
            crate::write_atomic(&extracted.join("files/Standalone.tpl"), b"standalone")?;
            let bank = format!("assets/{}", crate::digest(b"bank"));
            let standalone = format!("assets/{}", crate::digest(b"standalone"));
            write(
                "sources.json",
                json!({"disc1/Bank.bin":[bank], "disc1/Standalone.tpl":[standalone]}),
            )?;
            write(
                &format!("{bank}/archive.json"),
                json!({"count":3,"members":[0,null,0]}),
            )?;
            write(&format!("{bank}/0/cabinet.json"), json!(["PALETTE~1.TPL"]))?;
            write(
                &format!("{standalone}/cabinet.json"),
                json!(["TEXTURE.TPL"]),
            )?;
            fs::create_dir_all(root.join(&standalone).join("TEXTURE.TPL"))?;
            let directory = format!("{bank}/0/PALETTE~1.TPL");
            let images = [
                format!("{directory}/palette-0.ktx2"),
                format!("{directory}/palette-1.ktx2"),
            ];
            for image in &images {
                crate::write_atomic(&root.join(image), b"cooked image")?;
            }
            let texture = json!({"dimensions":[8,8],"format":"ci4",
                "sampler":{"wrap":["repeat","mirror"],"min_filter":"nearest","mag_filter":"linear","lod":{"bias":-0.25,"min":0,"max":0,"edge":true}},
                "palette":{"format":"rgb565","colors":vec![[255,0,0,255];32]},"images":images});
            let manifest = format!("{directory}/textures.json");
            write(&manifest, json!({"textures":[texture]}))?;
            let catalogue = Catalogue {
                standalone: vec![Some("standalone.TPL".into())],
                groups: vec![crate::resource::Group {
                    path: Some("bank.BIN".into()),
                    storage: [0; 3],
                }],
                party_bodies: vec![],
                party_battle_motions: vec![],
                party_field_motions: vec![],
                field_services: vec![],
            };
            let mut resources = Resources::open(&root, &extracted, &catalogue)?;
            assert_eq!(resources.directory(0)?, format!("{standalone}/TEXTURE.TPL"));
            for id in 0x10000..=0x10002 {
                assert_eq!(resources.directory(id)?, directory);
            }
            assert!(resources.directory(0x10003).is_err());
            assert_eq!(
                serde_json::to_value(crate::texture::bind(&root, &directory)?)?,
                json!([texture])
            );
            write(
                &format!("{bank}/archive.json"),
                json!({"count":3,"members":[2,null,0]}),
            )?;
            assert!(resources.directory(0x10002).is_err());
            write(
                &format!("{bank}/archive.json"),
                json!({"count":3,"members":[null,null,null]}),
            )?;
            assert!(resources.directory(0x10001).is_err());
            write(&format!("{standalone}/cabinet.json"), json!(["../outside"]))?;
            assert!(
                Resources::open(&root, &extracted, &catalogue)?
                    .directory(0)
                    .is_err()
            );
            write(
                &format!("{bank}/archive.json"),
                json!({"count":3,"members":[0,null,0]}),
            )?;
            crate::write_atomic(&extracted.join("files/Bank.bin"), b"changed")?;
            assert!(
                Resources::open(&root, &extracted, &catalogue)?
                    .directory(0x10000)
                    .unwrap_err()
                    .to_string()
                    .contains("source digest mismatch")
            );
            fs::remove_file(root.join(&manifest))?;
            assert!(crate::texture::bind(&root, &directory).is_err());
            for textures in [json!([null]), json!([])] {
                write(&manifest, json!({"textures":textures}))?;
                assert!(crate::texture::bind(&root, &directory).is_err());
            }
            let mut incomplete = texture.clone();
            incomplete["images"] = json!([images[0]]);
            write(&manifest, json!({"textures":[incomplete]}))?;
            assert!(crate::texture::bind(&root, &directory).is_err());
            for path in ["../outside.ktx2", "assets/another-owner/image.ktx2"] {
                let mut wrong = texture.clone();
                wrong["images"][0] = json!(path);
                crate::write_atomic(&root.join("assets/another-owner/image.ktx2"), b"exists")?;
                write(&manifest, json!({"textures":[wrong]}))?;
                assert!(crate::texture::bind(&root, &directory).is_err());
            }
            write(&manifest, json!({"textures":[texture]}))?;
            fs::remove_file(root.join(&images[1]))?;
            assert!(crate::texture::bind(&root, &directory).is_err());
            Ok(())
        })();
        if work.exists() {
            fs::remove_dir_all(work)?;
        }
        result
    }
}
