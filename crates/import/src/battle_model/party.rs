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

const TRANSFORMED_COSTUME: u8 = 3;

pub(crate) struct Source {
    pub character: u8,
    pub body: String,
    pub motion: String,
    pub alternate_motion: Option<String>,
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
            // Colette's transformed costume shares the default body.
            let alternate_motion = if character == 2 {
                let (alternate_body, _) = select(PartyResource::Body, TRANSFORMED_COSTUME)?;
                ensure!(
                    alternate_body == body,
                    "alternate battle bank uses a different body"
                );
                let (path, key) = select(PartyResource::BattleMotion, TRANSFORMED_COSTUME)?;
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
                alternate_motion,
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
    let null_motions = null_motions(&archive)?;
    let motions = archive
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
    let mut record = Party {
        body_sha256: crate::digest(&original),
        motion_sha256: archive.source_sha256,
        null_motions,
        motion_variants: BTreeMap::new(),
        body: super::rig(&body[primary], super::RigKind::Body)?,
        files: super::files(parts.iter().chain(wings.iter()), output)?,
        parts,
        wings,
    };
    if let Some(path) = &source.alternate_motion {
        let alternate = MapArchive::decode(&decoded.source(path)?)?;
        publish_variant(&body, &alternate, &mut record, output, |bytes| {
            decoded.animation(bytes)
        })?;
    }
    let path = party_path(source.character);
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&record)?)?;
    Ok(path)
}

/// Bind a complete alternate bank without publishing body artwork again.
fn publish_variant(
    body: &[u8],
    archive: &MapArchive,
    record: &mut Party,
    output: &Path,
    mut animation: impl FnMut(&[u8]) -> Result<Arc<crate::animation::AuthoredAnimation>>,
) -> Result<()> {
    use resonance_content::field_preload::{File, Role};
    let layers = crate::field::sections(body)?
        .into_iter()
        .take(2)
        .flatten()
        .collect::<Vec<_>>();
    ensure!(
        layers.len() == record.parts.len(),
        "alternate motion body layers differ"
    );
    let mut clips = Vec::new();
    for (range, part) in layers.into_iter().zip(&record.parts) {
        let (_, resource) = crate::geometry::model_resource(&body[range])?;
        let model =
            crate::model::Model::parse(&resource[crate::geometry::skeleton_range(resource)?])?;
        let bindings = crate::animation::ModelBindings::from_model(&model);
        ensure!(
            bindings.names.as_ref() == Some(&part.bone_names),
            "alternate motion body bindings differ"
        );
        let mut glb = crate::scene::glb::Glb {
            json: serde_json::Value::Null,
            binary: Arc::new(Vec::new()),
            motions: Vec::new(),
        };
        let mut layer = Vec::new();
        for (slot, range) in archive.sections.iter().skip(2).enumerate() {
            if let Some(range) = range {
                let motion = animation(&archive.bytes[range.clone()])?.motion(&bindings)?;
                layer.push(resonance_content::SceneClip {
                    resource_slot: slot.try_into()?,
                    ..glb.animate(motion)?
                });
            }
        }
        for motion in glb.motions {
            crate::write_atomic(&output.join(&motion.path), &motion.bytes)?;
            record.files.insert(
                motion.path,
                File {
                    sha256: crate::digest(&motion.bytes),
                    bytes: motion.bytes.len() as u64,
                    roles: [Role::Data].into(),
                },
            );
        }
        clips.push(layer);
    }
    record.motion_variants.insert(
        TRANSFORMED_COSTUME,
        resonance_content::battle_model::PartyMotions {
            source_sha256: archive.source_sha256.clone(),
            null_motions: null_motions(archive)?,
            clips,
        },
    );
    Ok(())
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

/// Refresh motion metadata and alternate curves while retaining cached artwork.
pub fn publish_cached(extracted: &Path, library: &Path, output: &Path) -> Result<Vec<String>> {
    publish_cached_selected(extracted, library, output, &[1, 2, 3, 4, 5, 6, 7, 8, 9])
}

/// Refresh only selected party descriptors and their costume-selected curves.
pub fn publish_cached_selected(
    extracted: &Path,
    library: &Path,
    output: &Path,
    characters: &[u8],
) -> Result<Vec<String>> {
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let sources = discover_selected(extracted, 1, &catalogue, &mut BTreeMap::new(), characters)?;
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
            for part in &mut record.parts {
                part.clips
                    .retain(|clip| !record.null_motions.contains(&clip.resource_slot));
            }
            if let Some(extra) = &source.alternate_motion {
                let extra = MapArchive::open(&extracted.join("files").join(extra))?;
                let body = crate::compression::payload(fs::read(
                    extracted.join("files").join(&source.body),
                )?)?;
                publish_variant(&body, &extra, &mut record, output, |bytes| {
                    Ok(Arc::new(crate::animation::read_member(bytes)?))
                })?;
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

/// Publish selected bodies with their independent costume motion banks.
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
                .chain(source.alternate_motion.as_ref())
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
                    .chain(source.alternate_motion.as_ref())
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
                let slots = archive
                    .sections
                    .iter()
                    .enumerate()
                    .skip(2)
                    .filter_map(|(member, range)| range.as_ref().map(|_| (member - 2) as u16))
                    .collect::<Vec<_>>();
                assert_eq!(record.null_motions, null_motions(&archive)?);
                if let Some(extra) = &source.alternate_motion {
                    let extra = MapArchive::open(&extracted.join("files").join(extra))?;
                    let bank = &record.motion_variants[&TRANSFORMED_COSTUME];
                    assert_eq!(bank.source_sha256, extra.source_sha256);
                    assert_eq!(bank.null_motions, null_motions(&extra)?);
                    assert_eq!(bank.clips.len(), record.parts.len());
                    let slots = extra
                        .sections
                        .iter()
                        .enumerate()
                        .skip(2)
                        .filter_map(|(member, range)| range.as_ref().map(|_| (member - 2) as u16))
                        .collect::<Vec<_>>();
                    for layer in &bank.clips {
                        assert_eq!(
                            layer
                                .iter()
                                .map(|clip| clip.resource_slot)
                                .collect::<Vec<_>>(),
                            slots
                        );
                    }
                    assert_ne!(bank.clips[0][0].motion, record.parts[0].clips[0].motion);
                }
                assert_eq!(
                    record.parts[0]
                        .clips
                        .iter()
                        .map(|clip| clip.resource_slot)
                        .collect::<Vec<_>>(),
                    slots
                );
                for clip in record.parts[0].clips.iter().chain(
                    record
                        .motion_variants
                        .values()
                        .flat_map(|bank| bank.clips[0].iter()),
                ) {
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
