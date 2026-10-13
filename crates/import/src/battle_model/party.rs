//! Bind party battle animations to decoded body packages.
use crate::{character::Clip, field::MapArchive, resource::PartyResource, scene::decoded::Package};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_model::{Party, party_path};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::Arc,
};

pub(crate) struct Source {
    pub character: u8,
    pub body: String,
    pub motion: String,
    pub supplementary_motion: Option<String>,
    pub wings: Option<String>,
    pub keys: Vec<String>,
}

pub(crate) fn discover(
    extracted: &Path,
    disc: u8,
    catalogue: &crate::resource::Catalogue,
    sources: &mut BTreeMap<String, String>,
) -> Result<Vec<Source>> {
    discover_selected(
        extracted,
        disc,
        catalogue,
        sources,
        &[1, 2, 3, 4, 5, 6, 7, 8, 9],
    )
}

fn discover_selected(
    extracted: &Path,
    disc: u8,
    catalogue: &crate::resource::Catalogue,
    sources: &mut BTreeMap<String, String>,
    characters: &[u8],
) -> Result<Vec<Source>> {
    ensure!(
        !characters.is_empty()
            && characters.iter().all(|id| (1..=9).contains(id))
            && characters.iter().collect::<BTreeSet<_>>().len() == characters.len(),
        "invalid party character selection"
    );
    let files = extracted.join("files");
    characters
        .iter()
        .map(|&character| {
            let mut select = |kind, costume| -> Result<_> {
                let path = crate::field_resources::resolve_path(
                    &files,
                    catalogue.party(kind, character, costume)?,
                )?;
                let label = format!("disc{disc}/{path}");
                if !sources.contains_key(&label) {
                    sources.insert(label.clone(), crate::media::hash_file(&files.join(&path))?);
                }
                Ok((path, sources[&label].clone()))
            };
            let (body, body_key) = select(PartyResource::Body, 0)?;
            let (motion, motion_key) = select(PartyResource::BattleMotion, 0)?;
            let mut keys = vec![body_key, motion_key];
            // Colette's transformed bank uses the default body and fills its missing motions.
            let supplementary_motion = if character == 2 {
                let (alternate_body, _) = select(PartyResource::Body, 3)?;
                ensure!(
                    alternate_body == body,
                    "supplementary battle bank uses a different body"
                );
                let (path, key) = select(PartyResource::BattleMotion, 3)?;
                keys.push(key);
                Some(path)
            } else {
                None
            };
            let wings = if character == 2 {
                let path = "col_wing.bin".to_owned();
                let label = format!("disc{disc}/{path}");
                let key = crate::media::hash_file(&files.join(&path))?;
                sources.insert(label, key.clone());
                keys.push(key);
                Some(path)
            } else {
                None
            };
            Ok(Source {
                character,
                body,
                motion,
                supplementary_motion,
                wings,
                keys,
            })
        })
        .collect()
}

pub(crate) fn publish(source: &Source, decoded: &Package, output: &Path) -> Result<String> {
    let original = decoded.source(&source.body)?;
    let body = crate::compression::payload((*original).clone())?;
    let archive = MapArchive::decode(&decoded.source(&source.motion)?)?;
    let mut null_motions = null_motions(&archive)?;
    let mut motions = archive
        .sections
        .iter()
        .enumerate()
        .skip(2)
        .filter_map(|(member, range)| range.as_ref().map(|range| (member, range)))
        .map(|(member, range)| {
            Ok((
                (member - 2).try_into()?,
                decoded.animation(&archive.bytes[range.clone()])?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    if let Some(path) = &source.supplementary_motion {
        let extra = MapArchive::decode(&decoded.source(path)?)?;
        for slot in supplementary_slots(&archive, &extra)? {
            motions.push((
                slot,
                decoded.animation(extra.section(usize::from(slot) + 2)?)?,
            ));
            null_motions.remove(&slot);
        }
        motions.sort_by_key(|(slot, _)| *slot);
    }
    let clips = motions
        .iter()
        .map(|(slot, animation)| Clip {
            slot: *slot,
            resource: None,
            animation,
        })
        .collect::<Vec<_>>();
    let parts = crate::character::cook_source_parts(
        output,
        &format!("battle/party/{}", source.character),
        &body,
        &clips,
        decoded,
    )?;
    let primary = crate::field::sections(&body)?
        .into_iter()
        .next()
        .flatten()
        .context("missing party primary body")?;
    let wings = source
        .wings
        .as_ref()
        .map(|path| -> Result<_> {
            let bytes = decoded.source(path)?;
            let sections = crate::field::sections(&bytes)?;
            let range = sections
                .get(2)
                .and_then(Clone::clone)
                .context("missing wing flight motion")?;
            let animation = decoded.animation(&bytes[range])?;
            let parts = crate::character::cook_source_parts(
                output,
                "battle/party/wings",
                &bytes,
                &[Clip {
                    slot: 12,
                    resource: None,
                    animation: &animation,
                }],
                decoded,
            )?;
            ensure!(parts.len() == 1, "wing artwork requires one model");
            Ok(parts.into_iter().next().unwrap())
        })
        .transpose()?;
    let record = Party {
        body_sha256: crate::digest(&original),
        motion_sha256: archive.source_sha256,
        null_motions,
        body: super::rig(&body[primary], super::RigKind::Body)?,
        files: super::files(parts.iter().chain(wings.iter()), output)?,
        parts,
        wings,
    };
    let path = party_path(source.character);
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&record)?)?;
    Ok(path)
}

/// Fill every authored alternate slot absent from the default bank.
fn supplementary_slots(default: &MapArchive, alternate: &MapArchive) -> Result<Vec<u16>> {
    alternate
        .sections
        .iter()
        .enumerate()
        .skip(2)
        .filter(|(member, range)| range.is_some() && default.optional_section(*member).is_none())
        .map(|(member, _)| Ok((member - 2).try_into()?))
        .collect()
}

fn null_motions(archive: &MapArchive) -> Result<BTreeSet<u16>> {
    ensure!(
        archive.sections.len() > 2,
        "missing party battle motion bank"
    );
    archive
        .sections
        .iter()
        .enumerate()
        .skip(2)
        .filter(|(_, range)| range.is_none())
        .map(|(member, _)| Ok((member - 2).try_into()?))
        .collect()
}

/// Refresh authored empty-slot metadata while retaining cached models and curves.
pub fn publish_cached(extracted: &Path, library: &Path, output: &Path) -> Result<Vec<String>> {
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let sources = discover(extracted, 1, &catalogue, &mut BTreeMap::new())?;
    sources
        .iter()
        .map(|source| {
            let path = party_path(source.character);
            let mut record: Party = serde_json::from_slice(&fs::read(library.join(&path))?)?;
            ensure!(
                record.body_sha256 == source.keys[0] && record.motion_sha256 == source.keys[1],
                "cached party {} uses different body or motion data",
                source.character
            );
            let archive = MapArchive::open(&extracted.join("files").join(&source.motion))?;
            record.null_motions = null_motions(&archive)?;
            if let Some(extra) = &source.supplementary_motion {
                let extra = MapArchive::open(&extracted.join("files").join(extra))?;
                for slot in supplementary_slots(&archive, &extra)? {
                    ensure!(
                        record
                            .parts
                            .iter()
                            .all(|part| part.clips.iter().any(|clip| clip.resource_slot == slot)),
                        "cached party {} lacks supplementary motion {slot}; republish its artwork",
                        source.character
                    );
                    record.null_motions.remove(&slot);
                }
            }
            ensure!(
                record
                    .parts
                    .iter()
                    .flat_map(|part| &part.clips)
                    .all(|clip| !record.null_motions.contains(&clip.resource_slot)),
                "cached party {} has an animation in an authored empty slot",
                source.character
            );
            crate::write_atomic(&output.join(&path), &serde_json::to_vec(&record)?)?;
            Ok(path)
        })
        .collect()
}

/// Selected development cooking feeds the same publisher with the shared model
/// and animation decoders. Production receives these packages from its DAG.
pub fn publish_all(extracted: &Path, output: &Path) -> Result<Vec<(String, Vec<String>)>> {
    publish_selected(extracted, output, &[1, 2, 3, 4, 5, 6, 7, 8, 9])
}

/// Publish selected default bodies with their required authored motion supplements.
pub fn publish_selected(
    extracted: &Path,
    output: &Path,
    characters: &[u8],
) -> Result<Vec<(String, Vec<String>)>> {
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let sources = discover_selected(extracted, 1, &catalogue, &mut BTreeMap::new(), characters)?;
    let mut decoded = Package::default();
    for path in sources
        .iter()
        .flat_map(|source| {
            [&source.body, &source.motion]
                .into_iter()
                .chain(source.supplementary_motion.as_ref())
                .chain(source.wings.as_ref())
        })
        .collect::<std::collections::BTreeSet<_>>()
    {
        let bytes = Arc::new(fs::read(extracted.join("files").join(path))?);
        decoded.extend(&Package::cook(
            &bytes,
            &format!("assets/{}", crate::digest(&bytes)),
            output,
            crate::all_assets::geometry::Input::File,
        )?);
        decoded.remember_source(path, bytes);
    }
    sources
        .iter()
        .map(|source| {
            Ok((
                publish(source, &decoded, output)
                    .with_context(|| format!("party {}", source.character))?,
                [&source.body, &source.motion]
                    .into_iter()
                    .chain(source.supplementary_motion.as_ref())
                    .chain(source.wings.as_ref())
                    .cloned()
                    .collect(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both extracted original discs; runs shared model/curve publication"]
    fn original_party_banks_keep_native_slots_and_shared_model_bindings() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut first = None;
        for disc in [1, 2] {
            let extracted = local.join(format!("disc{disc}"));
            let output = tempfile::tempdir()?;
            let paths = publish_all(&extracted, output.path())?;
            assert_eq!(paths.len(), 9);
            let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
            let sources = discover(&extracted, disc, &catalogue, &mut BTreeMap::new())?;
            let mut records = Vec::new();
            for (source, (path, _)) in sources.iter().zip(paths) {
                let bytes = fs::read(output.path().join(path))?;
                let record: Party = serde_json::from_slice(&bytes)?;
                assert_eq!(
                    [&record.body_sha256, &record.motion_sha256],
                    [&source.keys[0], &source.keys[1]]
                );
                assert!(
                    record
                        .body
                        .skeleton
                        .bones
                        .iter()
                        .map(|bone| &bone.name)
                        .eq(record.parts[0].bone_names.iter())
                );
                let archive = MapArchive::open(&extracted.join("files").join(&source.motion))?;
                let mut slots = archive
                    .sections
                    .iter()
                    .enumerate()
                    .skip(2)
                    .filter_map(|(member, range)| range.as_ref().map(|_| (member - 2) as u16))
                    .collect::<Vec<_>>();
                if let Some(extra) = &source.supplementary_motion {
                    let extra = MapArchive::open(&extracted.join("files").join(extra))?;
                    slots.extend(
                        extra
                            .sections
                            .iter()
                            .enumerate()
                            .skip(2)
                            .filter_map(|(member, range)| {
                                range.as_ref().map(|_| (member - 2) as u16)
                            }),
                    );
                    slots.sort_unstable();
                    slots.dedup();
                }
                assert_eq!(
                    record.parts[0]
                        .clips
                        .iter()
                        .map(|clip| clip.resource_slot)
                        .collect::<Vec<_>>(),
                    slots
                );
                for clip in &record.parts[0].clips {
                    assert!(record.files.contains_key(&clip.motion));
                    resonance_content::animation::Motion::decode(&fs::read(
                        output.path().join(&clip.motion),
                    )?)?
                    .validate(&record.body.skeleton)?;
                }
                eprintln!(
                    "Party {}: {} indexed bones, {} motion slots, attachments {:?}",
                    source.character,
                    record.body.skeleton.bones.len(),
                    slots.len(),
                    record.body.attachments
                );
                assert!(publish(source, &Package::default(), output.path()).is_err());
                records.push(bytes);
            }
            if let Some(first) = &first {
                assert_eq!(&records, first);
            }
            first = Some(records);
        }
        Ok(())
    }
}
