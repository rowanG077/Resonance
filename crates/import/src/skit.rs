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
#[ignore = "requires both original discs, RESONANCE_COOKED audio and frozen skit catalogues; no playback"]
fn original_skit_preparation_matches_both_disc_catalogues_without_intermediate_assets() -> Result<()>
{
    use serde_json::Value;
    let library = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_COOKED")
            .context("set RESONANCE_COOKED to the validated shared library")?,
    )
    .canonicalize()?;
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooking-simplification");
    for disc in [1, 2] {
        let baseline = if disc == 1 {
            library.clone()
        } else {
            fixtures.join("skit-baseline-disc2")
        };
        let expected_path = baseline.join("game/skits.json");
        let mut expected: Value = serde_json::from_slice(&fs::read(expected_path)?)?;
        let work = tempfile::tempdir()?;
        for name in ["audio", "sources.json"] {
            std::os::unix::fs::symlink(library.join(name), work.path().join(name))?;
        }
        let extracted =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../local/extracted/disc{disc}"));
        let _publications = crate::publication::Session::start_if_needed(work.path())?;
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        Source::read(&extracted, &executable)?.publish(&extracted, work.path())?;
        let actual: Value =
            serde_json::from_slice(&fs::read(work.path().join("game/skits.json"))?)?;
        let resources = expected["resources"]
            .as_object()
            .context("missing resources")?
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for id in resources {
            let resource = &actual["resources"][&id];
            let original = &mut expected["resources"][&id];
            let script = resource["script"].as_str().unwrap();
            let messages = resource["messages"].as_str().unwrap();
            assert!(script.starts_with("assets/") && messages.starts_with("assets/"));
            assert_eq!(
                fs::read(work.path().join(script))?,
                fs::read(baseline.join(original["script"].as_str().unwrap()))?,
                "disc {disc} skit {id}"
            );
            let messages: Value = serde_json::from_slice(&fs::read(work.path().join(messages))?)?;
            let original_messages: Value = serde_json::from_slice(&fs::read(
                baseline.join(original["messages"].as_str().unwrap()),
            )?)?;
            assert_eq!(messages, original_messages, "disc {disc} skit {id}");
            *original = resource.clone();
        }
        // Event-only scripts extend the previous ambient catalogue. Their
        // complete byte/message fidelity is checked against the discs below.
        expected["resources"] = actual["resources"].clone();
        for (id, media) in expected["media"].as_object().context("missing media")? {
            assert_eq!(&actual["media"][id], media, "existing skit media changed");
        }
        expected["media"] = actual["media"].clone();
        assert_eq!(
            actual, expected,
            "complete disc {disc} skit catalogue changed"
        );
        for portrait in actual["portraits"]
            .as_object()
            .context("missing portraits")?
            .values()
        {
            let images = portrait["images"]
                .as_array()
                .context("missing portrait images")?;
            let first = images.first().context("empty portrait")?["texture"]
                .as_str()
                .context("missing portrait texture")?;
            let metadata = Path::new(first)
                .parent()
                .context("portrait directory")?
                .join("textures.json");
            assert_eq!(
                serde_json::to_value(crate::texture::read(&work.path().join(&metadata))?)?,
                serde_json::to_value(crate::texture::read(&library.join(&metadata))?)?,
                "portrait sampler, palette, or image inventory changed"
            );
            for image in images {
                let path = image["texture"]
                    .as_str()
                    .context("missing portrait texture")?;
                crate::texture::compare_images(&work.path().join(path), &library.join(path))?;
            }
        }
        assert!(
            !work.path().join("game/skits").exists(),
            "preparation copied shared scripts"
        );
    }
    Ok(())
}

#[test]
#[cfg(unix)]
#[ignore = "requires original disc and RESONANCE_COOKED audio; writes RESONANCE_WORLD_SKITS"]
fn original_world_skit_resources_are_prepared_alongside_notifications() -> Result<()> {
    let library = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_COOKED").context("set RESONANCE_COOKED")?,
    )
    .canonicalize()?;
    let output = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_SKITS")
            .context("set RESONANCE_WORLD_SKITS to an empty output directory")?,
    );
    fs::create_dir_all(&output)?;
    for name in ["audio", "sources.json"] {
        let target = output.join(name);
        if !target.exists() {
            std::os::unix::fs::symlink(library.join(name), target)?;
        }
    }
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    Source::read(&extracted, &fs::read(extracted.join("sys/main.dol"))?)?
        .publish(&extracted, &output)?;
    let catalog: SkitCatalog = serde_json::from_slice(&fs::read(output.join("game/skits.json"))?)?;
    catalog.validate()?;
    assert_eq!(catalog.resources.range(450..544).count(), 94);
    assert!(
        !catalog
            .skits
            .iter()
            .any(|skit| (450..544).contains(&skit.id))
    );
    let physical = Catalog::read(&extracted, &fs::read(extracted.join("sys/main.dol"))?)?;
    for id in catalog.resources.keys().copied() {
        let original = read_script(&extracted.join("files").join(
            crate::field_resources::resolve_path(&extracted.join("files"), physical.script(id)?)?,
        ))?;
        assert_eq!(
            fs::read(output.join(&catalog.resources[&id].script))?,
            original.bytes
        );
        let messages: Vec<symphonia_script::message::Message> =
            serde_json::from_slice(&fs::read(output.join(&catalog.resources[&id].messages))?)?;
        assert_eq!(messages, original.messages);
    }
    fs::copy(
        library.join("game/session-data.json"),
        output.join("game/session-data.json"),
    )?;
    Ok(())
}
