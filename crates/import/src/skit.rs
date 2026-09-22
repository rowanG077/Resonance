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

#[derive(Clone)]
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
            },
            requested_media: media::requests(script.bytes)?,
        },
        bytes: script.bytes.to_vec(),
        messages: script.messages,
    })
}

impl Script {
    fn publish(&self, output: &Path) -> Result<ScriptBinding> {
        write_atomic(&output.join(&self.binding.paths.script), &self.bytes)?;
        write_atomic(
            &output.join(&self.binding.paths.messages),
            &serde_json::to_vec(&self.messages)?,
        )?;
        Ok(self.binding.clone())
    }
}

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<String> {
    let _publications = crate::publication::Session::start_if_needed(output)?;
    crate::disc_number(extracted)?;
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let physical = Catalog::read(extracted, &executable)?;
    let mut catalog = SkitCatalog {
        version: 2,
        skits: physical.definitions()?,
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
    let mut requested = BTreeSet::new();
    for (source, ids) in sources {
        let path = files.join(source);
        let binding = read_script(&path)
            .with_context(|| path.display().to_string())?
            .publish(output)?;
        requested.extend(binding.requested_media);
        for id in ids {
            catalog.resources.insert(id, binding.paths.clone());
        }
    }
    let archive = fs::read(files.join(&physical.portrait_archive))?;
    let directory = format!("assets/{}", crate::digest(&archive));
    for portrait in physical
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
    catalog.media = media::bind(
        output,
        extracted,
        &crate::voice_directory::Directory::read(&executable)?,
        requested,
    )?;
    catalog.validate()?;
    let path = "game/skits.json";
    write_atomic(&output.join(path), &serde_json::to_vec_pretty(&catalog)?)?;
    Ok(path.into())
}
#[test]
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
        cook(&extracted, work.path())?;
        let actual: Value =
            serde_json::from_slice(&fs::read(work.path().join("game/skits.json"))?)?;
        let resources = actual["resources"]
            .as_object()
            .context("missing resources")?;
        for (id, resource) in resources {
            let original = &mut expected["resources"][id];
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
