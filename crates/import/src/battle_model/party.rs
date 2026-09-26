//! Bind original party battle banks to the shared decoded body packages.
use crate::{character::Clip, field::MapArchive, resource::PartyResource, scene::decoded::Package};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_model::{Party, party_path};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

pub(crate) struct Source {
    pub character: u8,
    pub body: String,
    pub motion: String,
    pub keys: [String; 2],
}

pub(crate) fn discover(
    extracted: &Path,
    disc: u8,
    catalogue: &crate::resource::Catalogue,
    sources: &mut BTreeMap<String, String>,
) -> Result<Vec<Source>> {
    let files = extracted.join("files");
    (1..=9)
        .map(|character| {
            let mut select = |kind| -> Result<_> {
                let path = crate::field_resources::resolve_path(
                    &files,
                    catalogue.party(kind, character, 0)?,
                )?;
                let label = format!("disc{disc}/{path}");
                if !sources.contains_key(&label) {
                    sources.insert(label.clone(), crate::media::hash_file(&files.join(&path))?);
                }
                Ok((path, sources[&label].clone()))
            };
            let (body, body_key) = select(PartyResource::Body)?;
            let (motion, motion_key) = select(PartyResource::BattleMotion)?;
            Ok(Source {
                character,
                body,
                motion,
                keys: [body_key, motion_key],
            })
        })
        .collect()
}

pub(crate) fn publish(source: &Source, decoded: &Package, output: &Path) -> Result<String> {
    let original = decoded.source(&source.body)?;
    let body = crate::compression::payload((*original).clone())?;
    let archive = MapArchive::decode(&decoded.source(&source.motion)?)?;
    ensure!(
        archive.sections.len() > 2,
        "missing party battle motion bank"
    );
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
    let record = Party {
        body_sha256: crate::digest(&original),
        motion_sha256: archive.source_sha256,
        body: super::rig(&body[primary], super::RigKind::Body)?,
        files: super::files(&parts, output)?,
        parts,
    };
    let path = party_path(source.character);
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&record)?)?;
    Ok(path)
}

/// Selected development cooking feeds the same publisher with the shared model
/// and animation decoders. Production receives these packages from its DAG.
pub fn publish_all(extracted: &Path, output: &Path) -> Result<Vec<(String, [String; 2])>> {
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let sources = discover(extracted, 1, &catalogue, &mut BTreeMap::new())?;
    let mut decoded = Package::default();
    for path in sources
        .iter()
        .flat_map(|source| [&source.body, &source.motion])
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
                [source.body.clone(), source.motion.clone()],
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
                assert!(record.body.transform_kinds.iter().all(|&kind| kind == 1));
                let archive = MapArchive::open(&extracted.join("files").join(&source.motion))?;
                let slots = archive
                    .sections
                    .iter()
                    .enumerate()
                    .skip(2)
                    .filter_map(|(member, range)| range.as_ref().map(|_| (member - 2) as u16))
                    .collect::<Vec<_>>();
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
                    "Party {}: {} indexed bones, {} original motion slots, {} hurt volumes, attachments {:?}",
                    source.character,
                    record.body.skeleton.bones.len(),
                    slots.len(),
                    record.body.volumes.iter().filter(|v| v.hurt).count(),
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
