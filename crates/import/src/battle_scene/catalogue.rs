//! Physical scene cooking is independent of native action admission.
use super::*;
use serde::{Deserialize, Serialize};

// Version 6 adds participant tracks; version 5 artwork remains reusable.
// Reuse always refreshes effect metadata with the current decoder/compiler.
const VERSION: u32 = 6;

#[derive(Serialize, Deserialize)]
struct Catalogue {
    version: u32,
    module_sha256: String,
    entries: BTreeMap<String, Entry>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Entry {
    Cooked {
        source_sha256: String,
        descriptor_sha256: String,
    },
    Failed {
        error: String,
    },
}

/// Asset conversion results, not a statement about runtime action support.
#[derive(Clone, Default)]
pub struct Publication {
    pub paths: Vec<String>,
    pub failures: BTreeMap<String, String>,
    pub reused: usize,
    pub converted: usize,
}

/// Cook every physical magic/skill package without recooking fields or movies.
/// Callers must inspect `failures`; successful siblings remain available.
pub fn publish_all(extracted: &Path, output: &Path) -> Result<Publication> {
    let _session = crate::media::OutputSession::open(output)?;
    let _publications = crate::publication::Session::start_if_needed(output)?;
    publish_all_source(extracted, &Sources::read(extracted)?, output, "battle")
}

pub(crate) fn publish_all_source(
    extracted: &Path,
    sources: &Sources,
    output: &Path,
    prefix: &str,
) -> Result<Publication> {
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let mut shared = crate::battle_effect::SharedPalettes::read(&usual, output)?;
    let module_path = extracted.join("files").join(&sources.module);
    let module = Rel::read(&module_path)?;
    let module_sha256 = crate::media::hash_file(&module_path)?;
    let inventory_path = format!("{prefix}/scenes/catalogue.json");
    // A stale or damaged cache is rebuilt; source/decode errors below are reported.
    let previous = fs::read(output.join(&inventory_path))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Catalogue>(&bytes).ok())
        .filter(|old| (5..=VERSION).contains(&old.version) && old.module_sha256 == module_sha256);
    let mut catalogue = Catalogue {
        version: VERSION,
        module_sha256,
        entries: BTreeMap::new(),
    };
    let mut publication = Publication::default();
    let element_sounds = crate::battle_profile::element_contact_sounds(&module)?;
    let magic = extracted.join("files").join(&sources.magic);
    let mut jobs = super::magic_ranges(&module, &magic)?
        .into_iter()
        .map(|(id, range)| {
            (
                magic.clone(),
                range,
                format!("{prefix}/scenes/{id:03}.json"),
                Some(id),
            )
        })
        .collect::<Vec<_>>();
    let skill = extracted.join("files").join(&sources.skill);
    let selectors = crate::arte::read(&fs::read(extracted.join("sys/main.dol"))?)?.skill_packages;
    let ranges = super::skill_ranges(&module, &skill)?;
    ensure!(
        selectors
            .values()
            .all(|id| ranges.iter().any(|(slot, _)| slot == id)),
        "skill selector has no physical package"
    );
    for (slot, range) in ranges {
        let mut names = selectors
            .iter()
            .filter(|(_, package)| **package == slot)
            .map(|(id, _)| format!("{prefix}/scenes/skills/{id:03}.json"))
            .collect::<Vec<_>>();
        // Unreferenced physical packages still cook; no action binding is invented.
        if names.is_empty() {
            names.push(format!("{prefix}/scenes/skills/packages/{slot:03}.json"));
        }
        jobs.extend(
            names
                .into_iter()
                .map(|path| (skill.clone(), range.clone(), path, None)),
        );
    }
    for (archive, range, path, spell) in jobs {
        let result = (|| -> Result<(String, String, Scene, bool)> {
            let bytes = read_range(&archive, range)?;
            let source_sha256 = crate::digest(&bytes);
            let cached = previous
                .as_ref()
                .and_then(|old| old.entries.get(&path))
                .and_then(|entry| reusable(output, &path, &source_sha256, entry));
            let reused = cached.is_some();
            let mut scene = match cached {
                Some(scene) => scene,
                None => {
                    let effects = format!("{}.effects.json", path.trim_end_matches(".json"));
                    super::read(
                        &bytes,
                        &effects,
                        output,
                        spell,
                        &element_sounds,
                        &mut shared,
                    )?
                }
            };
            if reused {
                super::scene_effect_metadata(&bytes, output, &mut scene, &mut shared)?;
                scene.projectiles = super::projectiles(&bytes, spell, &element_sounds)?;
            }
            if let Some(id) = spell {
                super::scene_settings(&module, id, &mut scene)?;
                super::scene_actions(&bytes, id, &element_sounds, &mut scene)?;
            }
            let descriptor = serde_json::to_vec(&scene)?;
            crate::write_atomic(&output.join(&path), &descriptor)?;
            Ok((source_sha256, crate::digest(&descriptor), scene, reused))
        })();
        let entry = match result {
            Ok((source_sha256, descriptor_sha256, scene, reused)) => {
                publication.reused += usize::from(reused);
                publication.converted += usize::from(!reused);
                publication.paths.push(path.clone());
                publication.paths.extend(scene.files.into_keys());
                Entry::Cooked {
                    source_sha256,
                    descriptor_sha256,
                }
            }
            Err(error) => {
                let error = format!("{error:#}");
                publication.failures.insert(path.clone(), error.clone());
                Entry::Failed { error }
            }
        };
        catalogue.entries.insert(path, entry);
    }
    crate::write_atomic(
        &output.join(&inventory_path),
        &serde_json::to_vec_pretty(&catalogue)?,
    )?;
    publication.paths.push(inventory_path);
    publication.paths.sort();
    publication.paths.dedup();
    Ok(publication)
}

fn reusable(output: &Path, path: &str, source_hash: &str, entry: &Entry) -> Option<Scene> {
    let Entry::Cooked {
        source_sha256,
        descriptor_sha256,
    } = entry
    else {
        return None;
    };
    if source_sha256 != source_hash {
        return None;
    }
    let bytes = fs::read(output.join(path)).ok()?;
    if crate::digest(&bytes) != *descriptor_sha256 {
        return None;
    }
    let scene: Scene = serde_json::from_slice(&bytes).ok()?;
    if scene.source_sha256 != source_hash {
        return None;
    }
    for (path, file) in &scene.files {
        resonance_content::validate_asset_path(path).ok()?;
        let path = output.join(path);
        if fs::metadata(&path).ok()?.len() != file.bytes
            || crate::media::hash_file(&path).ok()? != file.sha256
        {
            return None;
        }
    }
    Some(scene)
}

/// Shared preload carries descriptors; encounter Inputs chooses their artwork.
pub(crate) fn catalogue_paths(output: &Path, prefix: &str) -> Result<Vec<String>> {
    let path = format!("{prefix}/scenes/catalogue.json");
    let catalogue: Catalogue = serde_json::from_slice(&fs::read(output.join(&path))?)?;
    ensure!(
        catalogue.version == VERSION,
        "scene catalogue version differs"
    );
    Ok(std::iter::once(path)
        .chain(
            catalogue
                .entries
                .into_iter()
                .filter_map(|(path, entry)| matches!(entry, Entry::Cooked { .. }).then_some(path)),
        )
        .collect())
}
