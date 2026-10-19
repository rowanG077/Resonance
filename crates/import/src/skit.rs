//! Skit scripts, portrait image bindings and media clocks prepared without playback.
use crate::{all_assets::skits::Catalog, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::skit::{SkitCatalog, SkitResourcePaths};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
mod media;
pub(crate) mod portraits;
pub(crate) mod recipe;

/// Portrait IDs use resource group 13; its source comes from the resource directory.
pub(crate) fn portrait_path(extracted: &Path, executable: &[u8]) -> Result<String> {
    let resources = crate::resource::read(executable)?;
    crate::field_resources::resolve_path(&extracted.join("files"), resources.source(0xd0000)?)
}

struct Script {
    bytes: Vec<u8>,
    messages: Vec<symphonia_script::message::Message>,
    binding: ScriptBinding,
}

struct ScriptBinding {
    paths: SkitResourcePaths,
    requested_media: BTreeSet<u32>,
}

fn read_script(path: &Path) -> Result<Script> {
    let source = fs::read(path)?;
    let directory = format!("assets/{}", crate::digest(&source));
    let script =
        crate::all_assets::decode_script(&source)?.context("invalid skit script resource")?;
    Ok(Script {
        binding: ScriptBinding {
            paths: SkitResourcePaths {
                script: format!("{directory}/script.ssb"),
                messages: format!("{directory}/messages.json"),
                title: None,
            },
            requested_media: media::requests(script.bytes)?,
        },
        bytes: script.bytes.to_vec(),
        messages: script.messages,
    })
}

impl Script {
    fn publish(&self, output: &Path) -> Result<()> {
        write_atomic(&output.join(&self.binding.paths.script), &self.bytes)?;
        write_atomic(
            &output.join(&self.binding.paths.messages),
            &serde_json::to_vec(&self.messages)?,
        )?;
        Ok(())
    }
}

/// Scripts are decoded once so scheduling and publication use the same media requests.
pub(crate) struct Source {
    physical: Catalog,
    catalog: SkitCatalog,
    scripts: Vec<Script>,
    voices: crate::voice_directory::Directory,
    requested: BTreeSet<u32>,
}

impl Source {
    pub(crate) fn read(extracted: &Path, executable: &[u8]) -> Result<Self> {
        let physical = Catalog::read(extracted, executable)?;
        let mut catalog = SkitCatalog {
            version: 2,
            skits: physical.definitions()?,
            preview_order: physical.preview_order.clone(),
            resources: BTreeMap::new(),
            portraits: BTreeMap::new(),
            portrait_recipes: physical
                .portrait_recipes
                .iter()
                .map(recipe::Recipe::prepared)
                .collect(),
            media: BTreeMap::new(),
        };
        let files = extracted.join("files");
        let mut sources = BTreeMap::<String, Vec<u16>>::new();
        for skit in &catalog.skits {
            let source = crate::field_resources::resolve_path(&files, physical.script(skit.id)?)?;
            sources.entry(source).or_default().push(skit.id);
        }
        let direct: BTreeMap<_, _> = physical
            .event_resources()?
            .into_iter()
            .filter(|(id, _)| !catalog.skits.iter().any(|skit| skit.id == *id))
            .collect();
        for &id in direct.keys() {
            // The executable retains named, disabled rows whose script was removed
            // from the disc. They are not playable resources on this edition.
            if let Some(source) = crate::field_resources::find_path(&files, physical.script(id)?)? {
                sources.entry(source).or_default().push(id);
            }
        }
        let mut scripts = Vec::new();
        let mut requested = BTreeSet::new();
        for (source, ids) in sources {
            let path = files.join(source);
            let script = read_script(&path).with_context(|| path.display().to_string())?;
            requested.extend(&script.binding.requested_media);
            for id in ids {
                let mut paths = script.binding.paths.clone();
                paths.title = direct.get(&id).cloned().flatten();
                catalog.resources.insert(id, paths);
            }
            scripts.push(script);
        }
        Ok(Self {
            physical,
            catalog,
            scripts,
            voices: crate::voice_directory::Directory::read(executable)?,
            requested,
        })
    }

    pub(crate) fn voice_members(&self, extracted: &Path) -> Result<BTreeSet<(String, usize)>> {
        self.requested
            .iter()
            .map(|&id| {
                ensure!(
                    id & 0xf0000000 != 0x80000000,
                    "skit direct movie/stream request {id:#x} is not supported"
                );
                let path = self.voices.path(id & 0xffff0000)?;
                let path = crate::field_resources::resolve_path(&extracted.join("files"), &path)?;
                Ok((path, usize::from(id as u16)))
            })
            .collect()
    }

    pub(crate) fn publish(&self, extracted: &Path, output: &Path) -> Result<String> {
        let mut catalog = self.catalog.clone();
        for script in &self.scripts {
            script.publish(output)?;
        }
        let archive = fs::read(
            extracted
                .join("files")
                .join(&self.physical.portrait_archive),
        )?;
        let directory = format!("assets/{}", crate::digest(&archive));
        for portrait in self
            .physical
            .portraits
            .iter()
            .filter(|portrait| !portrait.images.is_empty())
        {
            let (id, asset) = portraits::decode(&archive, portrait, &directory)?.publish(output)?;
            ensure!(
                catalog.portraits.insert(id, asset).is_none(),
                "duplicate portrait"
            );
        }
        catalog.media = media::bind(output, extracted, &self.voices, self.requested.clone())?;
        catalog.validate()?;
        let path = "game/skits.json";
        write_atomic(&output.join(path), &serde_json::to_vec_pretty(&catalog)?)?;
        Ok(path.into())
    }
}

#[test]
#[cfg(unix)]
#[ignore = "requires both original discs and RESONANCE_COOKED audio; writes private temporary output"]
fn original_skit_resources_prepare_from_both_discs() -> Result<()> {
    let library = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_COOKED").context("set RESONANCE_COOKED")?,
    )
    .canonicalize()?;
    for disc in [1, 2] {
        let work = tempfile::tempdir()?;
        let output = work.path();
        for name in ["audio", "sources.json"] {
            std::os::unix::fs::symlink(library.join(name), output.join(name))?;
        }
        let _publication = crate::publication::Session::start_if_needed(output)?;
        let extracted =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../local/extracted/disc{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let physical = Catalog::read(&extracted, &executable)?;
        let definitions = physical.definitions()?;
        let mut expected: BTreeMap<_, _> = physical.event_resources()?.into_iter().collect();
        for skit in &definitions {
            expected.insert(skit.id, None);
        }
        Source::read(&extracted, &executable)?.publish(&extracted, output)?;
        let catalog: SkitCatalog =
            serde_json::from_slice(&fs::read(output.join("game/skits.json"))?)?;
        catalog.validate()?;
        assert_eq!(
            serde_json::to_value(&catalog.skits)?,
            serde_json::to_value(definitions)?,
            "disc {disc} notification definitions"
        );
        assert_eq!(catalog.preview_order, physical.preview_order);
        assert_eq!(
            serde_json::to_value(&catalog.portrait_recipes)?,
            serde_json::to_value(
                physical
                    .portrait_recipes
                    .iter()
                    .map(recipe::Recipe::prepared)
                    .collect::<Vec<_>>()
            )?,
            "disc {disc} portrait recipes"
        );
        assert_eq!(
            catalog.portraits.len(),
            physical
                .portraits
                .iter()
                .filter(|portrait| !portrait.images.is_empty())
                .count()
        );
        let mut available = BTreeSet::new();
        let mut requested = BTreeSet::new();
        for (id, title) in expected {
            let Some(path) =
                crate::field_resources::find_path(&extracted.join("files"), physical.script(id)?)?
            else {
                continue;
            };
            available.insert(id);
            let original = read_script(&extracted.join("files").join(path))?;
            let resource = &catalog.resources[&id];
            assert_eq!(resource.title, title, "disc {disc} skit {id} title");
            assert_eq!(
                fs::read(output.join(&resource.script))?,
                original.bytes,
                "disc {disc} skit {id} program"
            );
            let messages: Vec<symphonia_script::message::Message> =
                serde_json::from_slice(&fs::read(output.join(&resource.messages))?)?;
            assert_eq!(
                messages, original.messages,
                "disc {disc} skit {id} messages"
            );
            requested.extend(original.binding.requested_media);
        }
        assert_eq!(
            catalog.resources.keys().copied().collect::<BTreeSet<_>>(),
            available
        );
        assert_eq!(
            catalog.media.keys().copied().collect::<BTreeSet<_>>(),
            requested
        );
    }
    Ok(())
}
