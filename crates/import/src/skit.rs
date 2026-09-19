//! Skit scripts, portrait image bindings and media clocks prepared without playback.
use crate::{
    all_assets::{pool, skits::Catalog},
    write_atomic,
};
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
    receipt: ScriptReceipt,
}

#[derive(Clone)]
struct ScriptReceipt {
    paths: SkitResourcePaths,
    requested_media: BTreeSet<u32>,
}

fn read_script(path: &Path) -> Result<Script> {
    let source = fs::read(path)?;
    let directory = format!("assets/{}", crate::digest(&source));
    let script =
        crate::all_assets::decode_script(&source)?.context("invalid skit script resource")?;
    Ok(Script {
        receipt: ScriptReceipt {
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
    fn publish(&self, output: &Path) -> Result<ScriptReceipt> {
        write_atomic(&output.join(&self.receipt.paths.script), &self.bytes)?;
        write_atomic(
            &output.join(&self.receipt.paths.messages),
            &serde_json::to_vec(&self.messages)?,
        )?;
        Ok(self.receipt.clone())
    }
}

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<String> {
    let _publications = crate::publication::Session::start_if_needed(output)?;
    const MEMORY_BUDGET: usize = 256 * 1024 * 1024;
    const RECEIPT_BYTES: usize = 32 * 1024;
    crate::disc_number(extracted)?;
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    // Table discovery establishes the graph; decoded scripts and pixels remain worker outputs.
    let physical = Catalog::read(extracted, &executable)?;
    let definitions = physical.definitions()?;
    let mut sources = BTreeMap::<String, Vec<u16>>::new();
    let files = extracted.join("files");
    for skit in &definitions {
        let source = crate::field_resources::resolve_path(&files, physical.script(skit.id)?)?;
        sources.entry(source).or_default().push(skit.id);
    }
    let mut scripts = Vec::new();
    let mut portraits = Vec::new();
    let voice_directory = crate::voice_directory::Directory::read(&executable)?;
    let mut dag = pool::Dag::new();
    for (source, ids) in sources {
        let path = files.join(&source);
        let memory = usize::try_from(fs::metadata(&path)?.len())?
            .checked_mul(48)
            .and_then(|size| size.checked_add(65536))
            .context("skit estimate overflows")?;
        let decoded = dag.add(&source, [], move |_, _| {
            read_script(&path).with_context(|| path.display().to_string())
        });
        dag.estimate(decoded, memory, memory)?;
        let publication = dag.add(
            format!("publish {source}"),
            [decoded.dependency()],
            move |_, inputs| inputs.get(decoded)?.publish(output),
        );
        dag.estimate(publication, RECEIPT_BYTES, memory)?;
        scripts.push((ids, publication));
    }
    let portrait_path = files.join(&physical.portrait_archive);
    let archive_bytes = usize::try_from(fs::metadata(&portrait_path)?.len())?;
    let archive = dag.add("portrait archive", [], move |_, _| {
        let bytes = fs::read(&portrait_path)?;
        let directory = format!("assets/{}", crate::digest(&bytes));
        Ok((bytes, directory))
    });
    dag.estimate(archive, archive_bytes + 1024, archive_bytes)?;
    for portrait in physical
        .portraits
        .iter()
        .filter(|portrait| !portrait.images.is_empty())
    {
        let memory = portrait
            .images
            .iter()
            .map(|image| usize::from(image.width) * usize::from(image.height) * 8)
            .sum::<usize>()
            + 65536;
        let decoded = dag.add(
            format!("portrait {}", portrait.member),
            [archive.dependency()],
            move |_, inputs| {
                let source = inputs.get(archive)?;
                portraits::decode(&source.0, portrait, &source.1)
            },
        );
        dag.estimate(decoded, memory, memory)?;
        let publication = dag.add(
            format!("publish portrait {}", portrait.member),
            [decoded.dependency()],
            move |_, inputs| inputs.get(decoded)?.publish(output),
        );
        dag.estimate(publication, RECEIPT_BYTES, memory * 2)?;
        portraits.push(publication);
    }
    let media = dag.add(
        "skit media clocks",
        scripts.iter().map(|(_, output)| output.dependency()),
        |_, inputs| {
            let mut requested = BTreeSet::new();
            for (_, script) in &scripts {
                requested.extend(&inputs.get(*script)?.requested_media);
            }
            media::bind(output, extracted, &voice_directory, requested)
        },
    );
    dag.estimate(media, 4 * 1024 * 1024, 8 * 1024 * 1024)?;
    let dependencies = scripts
        .iter()
        .map(|(_, output)| output.dependency())
        .chain(portraits.iter().map(|output| output.dependency()))
        .chain([media.dependency()]);
    let (scripts, portraits, physical, definitions) =
        (&scripts, &portraits, &physical, &definitions);
    let publication = dag.add("skit catalogue", dependencies, move |_, inputs| {
        let mut catalog = SkitCatalog {
            version: 2,
            skits: definitions.clone(),
            resources: BTreeMap::new(),
            portraits: BTreeMap::new(),
            portrait_recipes: physical
                .portrait_recipes
                .iter()
                .map(recipe::Recipe::prepared)
                .collect(),
            media: inputs.get(media)?.as_ref().clone(),
        };
        for (ids, source) in scripts {
            let receipt = inputs.get(*source)?;
            for &id in ids {
                catalog.resources.insert(id, receipt.paths.clone());
            }
        }
        for &portrait in portraits {
            let receipt = inputs.get(portrait)?;
            ensure!(
                catalog
                    .portraits
                    .insert(receipt.0, receipt.1.clone())
                    .is_none(),
                "duplicate portrait"
            );
        }
        catalog.validate()?;
        let path = "game/skits.json";
        write_atomic(&output.join(path), &serde_json::to_vec_pretty(&catalog)?)?;
        Ok(path.to_owned())
    });
    dag.estimate(publication, 1024, 16 * 1024 * 1024)?;
    let workers = std::thread::available_parallelism()
        .map_or(1, |count| count.get())
        .min(crate::all_assets::MAX_WORKERS);
    let mut published = None;
    let statuses = dag.run_bounded(
        workers,
        MEMORY_BUDGET,
        || (),
        |completion| {
            if let Some(result) = completion.get(publication) {
                published = Some(result);
            }
        },
    )?;
    for status in statuses {
        status?;
    }
    Ok(published
        .context("skit catalogue was not published")??
        .as_ref()
        .clone())
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
        let expected_path = if disc == 1 {
            fixtures.join("skit-binding-baseline.json")
        } else {
            baseline.join("game/skits.json")
        };
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
