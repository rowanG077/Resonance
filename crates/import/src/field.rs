//! Field archive inspection and conversion. All original-format I/O stays here.
#[path = "field/collision.rs"]
pub(crate) mod collision_data;

use crate::read::u32 as word;
use crate::{digest, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::field::FieldAssets;
use std::{
    collections::BTreeMap,
    fs,
    ops::Range,
    path::{Path, PathBuf},
};
use symphonia_script::{Program, message, scenario, semantics::NativeRegistry};

/// Resolve a field's archive through the executable's indexed resource catalog.
pub fn source_for_id(extracted: &Path, map: u32) -> Result<PathBuf> {
    crate::disc_number(extracted)?;
    let catalogue = crate::field_catalogue::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let name = catalogue
        .records
        .get(map as usize)
        .context("field is outside the resource catalogue")?
        .resource
        .as_deref()
        .context("field has no resource declaration")?;
    let files = extracted.join("files");
    Ok(files.join(crate::field_resources::resolve_path(
        &files,
        &format!("MAP/{name}"),
    )?))
}

pub(crate) struct MapArchive {
    pub bytes: Vec<u8>,
    pub sections: Vec<Option<Range<usize>>>,
    pub source_sha256: String,
    pub member: String,
}

impl MapArchive {
    pub fn open(source: &Path) -> Result<Self> {
        Self::decode(&fs::read(source)?)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        let (member, expanded) = crate::compression::cabinet(bytes)?;
        let sections = sections(&expanded)?;
        Ok(Self {
            bytes: expanded,
            sections,
            source_sha256: digest(bytes),
            member,
        })
    }
    pub fn section(&self, index: usize) -> Result<&[u8]> {
        self.optional_section(index).context("missing map section")
    }
    pub fn optional_section(&self, index: usize) -> Option<&[u8]> {
        let range = self.sections.get(index)?.as_ref()?;
        Some(&self.bytes[range.clone()])
    }
}

pub(crate) fn sections(bytes: &[u8]) -> Result<Vec<Option<Range<usize>>>> {
    let count = word(bytes, 0)? as usize;
    ensure!((1..=256).contains(&count), "invalid map section count");
    let end = 4 + count * 4;
    ensure!(end <= bytes.len(), "truncated map section table");
    let offsets: Vec<_> = (0..count)
        .map(|i| word(bytes, 4 + i * 4).map(|v| v as usize))
        .collect::<Result<_>>()?;
    for &start in &offsets {
        ensure!(
            start == 0 || (end..bytes.len()).contains(&start),
            "invalid map section offset {start:#x}"
        );
    }
    // Some sections alias an earlier resource. Their range ends at the next
    // greater offset, not necessarily the next entry in table order.
    Ok(offsets
        .iter()
        .map(|&start| {
            (start != 0).then(|| {
                let end = offsets
                    .iter()
                    .copied()
                    .filter(|&s| s > start)
                    .min()
                    .unwrap_or(bytes.len());
                start..end
            })
        })
        .collect())
}

/// Export source sections, checked script assembly, messages and native inventory.
/// This is inspection output; it does not claim a complete runtime field package.
pub fn inspect(source: &Path, output: &Path) -> Result<()> {
    let map = MapArchive::open(source)?;
    let registry = NativeRegistry::gqseaf();
    let mut inventory = Vec::new();
    for (index, range) in map.sections.iter().enumerate() {
        let Some(range) = range else {
            continue;
        };
        let bytes = map.section(index)?;
        let name = format!("section-{index:02}");
        write_atomic(&output.join(format!("{name}.bin")), bytes)?;
        let mut entry = serde_json::json!({"index":index,"offset":range.start,"size":bytes.len(),"sha256":digest(bytes)});
        if let Ok(header) = scenario::parse_header(bytes)
            && header.code_base() >= 8 + usize::from(header.registry_count) * 12
            && header.auxiliary_offset() > header.code_base()
            && header.auxiliary_offset() < bytes.len()
        {
            let (assembly, analysis) = scenario::disassemble(bytes)?;
            write_atomic(&output.join(format!("{name}.ssasm")), assembly.as_bytes())?;
            let mut procedures = BTreeMap::<u8, Vec<u32>>::new();
            for instruction in analysis
                .instructions
                .values()
                .filter(|i| i.mnemonic == "proc")
            {
                procedures
                    .entry(instruction.operands[0] as u8)
                    .or_default()
                    .push(instruction.pc);
            }
            let procedures: Vec<_> = procedures.into_iter().map(|(opcode, pcs)| {
                let native = registry.get(opcode);
                serde_json::json!({"opcode":opcode,"pcs":pcs,"name":native.map(|n| &n.name),"handler":native.map(|n| &n.handler)})
            }).collect();
            entry["scenario"] = serde_json::to_value(analysis.summary())?;
            entry["procedures"] = serde_json::to_value(procedures)?;
            entry["program_validation_error"] =
                serde_json::to_value(Program::decode(bytes).err().map(|e| e.to_string()))?;
            match message::parse(&bytes[header.auxiliary_offset()..]) {
                Ok(messages) => {
                    write_atomic(
                        &output.join(format!("{name}.messages.json")),
                        &serde_json::to_vec_pretty(&messages)?,
                    )?;
                    entry["messages"] = serde_json::json!({"path":format!("{name}.messages.json"),"count":messages.len()});
                }
                Err(error) => entry["message_error"] = serde_json::json!(error.to_string()),
            }
        }
        inventory.push(entry);
    }
    write_atomic(
        &output.join("map.json"),
        &serde_json::to_vec_pretty(&serde_json::json!({
            "version":1,"source_sha256":map.source_sha256,"expanded_sha256":digest(&map.bytes),"sections":inventory,
        }))?,
    )?;
    println!(
        "Inspected {} field sections into {}",
        inventory.len(),
        output.display()
    );
    Ok(())
}

/// Bind one catalogue entry from decoded inputs. The same path handles ordinary
/// fields, script-only scenes and New Game setup.
pub(crate) fn prepare(
    map_id: u32,
    output: &Path,
    physical: &crate::scene::binding::Map<'_>,
    shared: &crate::shared::Prepared,
    recovered: &crate::scene::recovered::RecoveredModels,
    declared: &crate::field_resources::Declarations,
) -> Result<FieldAssets> {
    use resonance_content::ScriptAsset;
    let script = physical.script()?;
    let mut resources =
        crate::field_resources::binding::Resources::decoded(&shared.catalogue, recovered);
    let mut declared_assets = crate::character::Sources::read(&mut resources, &declared.resources)?;
    declared_assets.include_field(physical, Some(recovered))?;
    let (parts, doors) = physical.layers(&declared_assets.animations)?;
    let messages = physical.messages()?;
    crate::font::validate_messages(&shared.font, &messages)?;
    let [script_path, messages_path] = physical.publish_script()?;
    let (overlays, overlay_files) =
        crate::field_overlay::cook(&mut resources, physical, output, &declared_assets.textures)?;
    let characters = crate::character::cook_field(
        output,
        map_id,
        physical,
        &declared_assets,
        &declared_assets.animations,
        &mut resources,
        declared,
    )?;
    let mut assets = FieldAssets {
        version: resonance_content::field::FIELD_VERSION,
        map_id,
        source_sha256: physical.source_sha256.clone(),
        script: ScriptAsset {
            path: script_path.clone(),
            sha256: digest(&script),
        },
        messages: messages_path.clone(),
        parts,
        ground: physical
            .section(4)
            .map(|_| physical.collision(4))
            .transpose()?
            .unwrap_or_default(),
        regions: physical
            .section(5)
            .map(|_| physical.collision(5))
            .transpose()?
            .unwrap_or_default(),
        doors,
        actors: characters.actors,
        unbound_geometry: characters.unbound,
        resource_catalogue: (!declared.dynamic.is_empty())
            .then(|| shared.resource_catalogue.clone()),
        contact_shadow: shared.effects.shadow.clone(),
        toon_ramp: shared.toon_ramp.clone(),
        effects: shared.effects.path.clone(),
        blink: shared.effects.blink.clone(),
        particles: shared.effects.particles.clone(),
        overlays,
        save_point_tutorial: if declared.save_point {
            shared.save_point_tutorial.clone()
        } else {
            Vec::new()
        },
        files: shared.files.clone(),
    };
    let mut files: std::collections::BTreeSet<_> = [script_path, messages_path].into();
    files.extend(overlay_files);
    files.extend(characters.files);
    for part in assets
        .parts
        .iter()
        .chain(assets.actors.iter().flat_map(|actor| &actor.parts))
    {
        files.insert(part.mesh.clone());
        files.extend(part.textures.iter().cloned());
        files.extend(part.clips.iter().map(|clip| clip.motion.clone()));
    }
    for path in files {
        assets
            .files
            .insert(path.clone(), crate::media::hash_file(&output.join(path))?);
    }
    assets.validate()?;
    Ok(assets)
}

pub(crate) fn publish(output: &Path, assets: &FieldAssets) -> Result<String> {
    publish_field(
        output,
        &resonance_content::field::metadata_path(assets.map_id),
        assets,
    )
}

pub(crate) fn audio_path(map_id: u32) -> String {
    resonance_content::field::audio_path(map_id)
}

fn for_each_field(
    output: &Path,
    mut visit: impl FnMut(String, FieldAssets, String) -> Result<()>,
) -> Result<()> {
    let directory = output.join("fields");
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|name| name.strip_prefix("map-"))
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        if id.parse::<u32>().is_err() {
            continue;
        }
        let path = format!("fields/{}", name.to_string_lossy());
        let bytes = fs::read(entry.path())?;
        let assets =
            serde_json::from_slice(&bytes).with_context(|| format!("invalid field {path}"))?;
        visit(path, assets, digest(&bytes))?;
    }
    Ok(())
}

fn publish_field(output: &Path, path: &str, field: &FieldAssets) -> Result<String> {
    let bytes = serde_json::to_vec_pretty(field)?;
    write_atomic(&output.join(path), &bytes)?;
    Ok(digest(&bytes))
}

pub(crate) fn finish(output: &Path) -> Result<()> {
    for_each_field(output, |path, assets, hash| {
        let manifest = preload(output, path, &assets, &hash)?;
        ensure!(
            manifest.missing_inputs.is_empty(),
            "field {} has missing inputs: {:?}",
            assets.map_id,
            manifest.missing_inputs
        );
        Ok(())
    })
}

fn preload(
    output: &Path,
    path: String,
    assets: &FieldAssets,
    hash: &str,
) -> Result<resonance_content::field_preload::Manifest> {
    use symphonia_script::NativeCall;
    let script = fs::read(output.join(&assets.script.path))?;
    let mut movies = std::collections::BTreeSet::new();
    for args in
        crate::field_resources::literal_arguments(&script, NativeCall::PlayMovieBlocking, 1)?
    {
        match args[0] {
            Some(-1) => {}
            Some(id) => {
                ensure!(id >= 0, "invalid movie ID {id}");
                movies.insert(format!("movies/{id}.json"));
            }
            None => anyhow::bail!("unresolved movie dependency in {}", assets.script.path),
        }
    }
    crate::field_preload::cook_field(
        output,
        resonance_content::field_preload::Inputs {
            field: path,
            audio: [audio_path(assets.map_id)].into(),
            movies,
        },
        assets,
        hash,
    )
}

#[cfg(test)]
pub(crate) fn collision(bytes: &[u8]) -> Result<Vec<resonance_content::field::CollisionGroup>> {
    Ok(collision_data::Mesh::read(bytes, collision_data::Format::Detect)?.groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_uses_its_payload_name_and_rejects_ambiguity() {
        use std::io::Write;
        for names in [&["ISA_I0~1.BIN"][..], &["A.BIN", "B.BIN"][..]] {
            let mut builder = cab::CabinetBuilder::new();
            let folder = builder.add_folder(cab::CompressionType::None);
            for name in names {
                folder.add_file(*name);
            }
            let mut writer = builder.build(std::io::Cursor::new(Vec::new())).unwrap();
            while let Some(mut file) = writer.next_file().unwrap() {
                for value in [1u32, 8, 42] {
                    file.write_all(&value.to_be_bytes()).unwrap();
                }
            }
            let archive = MapArchive::decode(&writer.finish().unwrap().into_inner());
            if names.len() == 1 {
                assert_eq!(archive.unwrap().section(0).unwrap(), 42u32.to_be_bytes());
            } else {
                assert!(archive.is_err());
            }
        }
    }

    #[test]
    fn section_aliases_do_not_hide_following_payloads() {
        let mut map = Vec::new();
        for value in [4u32, 20, 24, 20, 0, 1, 2] {
            map.extend(value.to_be_bytes());
        }
        assert_eq!(
            sections(&map).unwrap(),
            vec![Some(20..24), Some(24..28), Some(20..24), None]
        );
        map[4..8].copy_from_slice(&4u32.to_be_bytes());
        assert!(sections(&map).is_err());
    }
}
