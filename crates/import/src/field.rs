//! Field archive inspection and conversion. All original-format I/O stays here.
#[path = "field/collision.rs"]
pub(crate) mod collision_data;

use crate::read::u32 as word;
use crate::{digest, write_atomic};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
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
}

impl MapArchive {
    pub fn open(source: &Path) -> Result<Self> {
        Self::decode(&fs::read(source)?)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cabinet = cab::Cabinet::new(Cursor::new(bytes))?;
        // Some payloads retain DOS short names, independent of the outer archive.
        let names: Vec<_> = cabinet
            .folder_entries()
            .flat_map(|folder| folder.file_entries())
            .map(|entry| entry.name().to_owned())
            .collect();
        ensure!(names.len() == 1, "field archive needs one payload");
        let mut expanded = Vec::new();
        cabinet
            .read_file(&names[0])?
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut expanded)?;
        ensure!(
            expanded.len() <= 64 * 1024 * 1024,
            "expanded map exceeds 64 MiB"
        );
        let sections = sections(&expanded)?;
        Ok(Self {
            bytes: expanded,
            sections,
            source_sha256: digest(bytes),
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

pub fn cook_field(extracted: &Path, map_id: u32, output: &Path) -> Result<()> {
    use crate::media::{Workspace, hash_file};
    use resonance_content::{ScriptAsset, field::FieldAssets};
    let _workspace = Workspace::open(extracted, output)?;
    let source = source_for_id(extracted, map_id)?;
    let name = if map_id == 340 {
        "iselia-classroom".into()
    } else {
        format!("map-{map_id}")
    };
    let prefix = format!("fields/{name}");
    let map = MapArchive::open(&source)?;
    let setup_source = MapArchive::open(&extracted.join("files/MAP/_custom.bin"))?;
    let mut sources = BTreeMap::new();
    for path in [
        "sys/boot.bin",
        "sys/main.dol",
        "files/MAP/_custom.bin",
        "files/u_f_fontb0.dat",
        "files/system.tpl",
        "files/effect.cab",
        "files/mahou.cab",
    ] {
        sources.insert(path.to_owned(), hash_file(&extracted.join(path))?);
    }
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let toon = format!(
        "files/{}",
        crate::all_assets::roles::toon_path(extracted, &executable)?
    );
    sources.insert(toon.clone(), hash_file(&extracted.join(toon))?);
    let resources = crate::resource::read(&executable)?;
    let files = extracted.join("files");
    let mut paths = std::collections::BTreeSet::new();
    for character in 1..=resources.party_bodies.len() as u8 {
        paths.extend([
            resources.party(crate::resource::PartyResource::Body, character, 0)?,
            resources.field_motion(character)?,
            resources.field_service(character)?,
        ]);
    }
    for script in [map.section(6)?, setup_source.section(6)?] {
        for resource in crate::field_resources::declarations(script)?.resources {
            paths.insert(resources.source(resource)?);
        }
    }
    for path in paths {
        let path = format!(
            "files/{}",
            crate::field_resources::resolve_path(&files, path)?
        );
        sources.insert(path.clone(), hash_file(&extracted.join(path))?);
    }
    sources.insert(
        source
            .strip_prefix(extracted)?
            .to_string_lossy()
            .into_owned(),
        hash_file(&source)?,
    );
    let recipe = serde_json::json!({"version":2,"sources":sources,"texture_recipe":crate::texture::RECIPE,"compiler_sha256":hash_file(&std::env::current_exe()?)?});
    let metadata = output.join(format!("{prefix}.json"));
    let cache = output.join(format!("intermediate/{prefix}-recipe.json"));
    if let Ok(bytes) = fs::read(&metadata)
        && let Ok(assets) = serde_json::from_slice::<FieldAssets>(&bytes)
        && let Ok(previous) = fs::read(&cache)
        && let Ok(previous) = serde_json::from_slice::<serde_json::Value>(&previous)
        && previous["recipe"] == recipe
        && previous["manifest_sha256"] == digest(&bytes)
        && assets.validate().is_ok()
        && assets
            .files
            .iter()
            .all(|(path, hash)| hash_file(&output.join(path)).is_ok_and(|actual| actual == *hash))
    {
        println!("Field {map_id} assets are current");
        refresh_preloads(output)?;
        return Ok(());
    }
    let boot = fs::read(extracted.join("sys/boot.bin"))?;
    ensure!(
        boot.get(..8) == Some(b"GQSEAF\0\0"),
        "expected GQSEAF revision 0 disc 1"
    );
    let field_source = source
        .strip_prefix(extracted.join("files"))?
        .to_str()
        .context("non-UTF-8 field source path")?;
    let physical = crate::scene::binding::Map::open(output, 1, field_source, &map.source_sha256)?;
    let script = map.section(6)?;
    let declared = crate::field_resources::declarations(script)?;
    let declared_assets = crate::character::Sources::read(&files, &resources, &declared.resources)?;
    let mut cooked_resources =
        crate::field_resources::binding::Resources::open(output, extracted, &resources)?;
    let shared_clips = declared_assets.scene_clips(&mut cooked_resources)?;
    let (parts, doors) = physical.layers(&shared_clips)?;
    Program::decode(script)?;
    let header = scenario::parse_header(script)?;
    let messages = message::parse(&script[header.auxiliary_offset()..])?;
    let script_path = format!("{prefix}/events.ssb");
    let messages_path = format!("{prefix}/messages.json");
    write_atomic(&output.join(&script_path), script)?;
    write_atomic(
        &output.join(&messages_path),
        &serde_json::to_vec(&messages)?,
    )?;
    let effects = crate::field_effects::cook(extracted, output)?;
    let effect_files = effects.files;
    let (overlays, overlay_files) = crate::field_overlay::cook(
        extracted,
        &map,
        &physical,
        output,
        &declared_assets.textures,
    )?;
    let mut assets = FieldAssets {
        version: 8,
        map_id,
        source_sha256: map.source_sha256.clone(),
        script: ScriptAsset {
            path: script_path.clone(),
            sha256: digest(script),
        },
        messages: messages_path.clone(),
        parts,
        ground: collision(map.section(4)?)?,
        regions: map
            .optional_section(5)
            .map(collision)
            .transpose()?
            .unwrap_or_default(),
        doors,
        actors: crate::character::cook_field(
            extracted,
            output,
            map_id,
            &map,
            &declared_assets,
            &shared_clips,
        )?,
        contact_shadow: effects.shadow,
        toon_ramp: crate::field_lighting::cook(extracted, output)?,
        effects: effects.path,
        blink: effects.blink,
        particles: effects.particles,
        overlays,
        save_point_tutorial: if crate::field_resources::declarations(script)?.save_point {
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            crate::font::system_text(crate::dol::slice(&executable, 0x8017A274, 256)?)?
        } else {
            Vec::new()
        },
        files: BTreeMap::new(),
    };
    let setup_script = setup_source.section(6)?;
    let setup_header = scenario::parse_header(setup_script)?;
    let setup_messages = message::parse(&setup_script[setup_header.auxiliary_offset()..])?;
    let required = messages
        .iter()
        .chain(&setup_messages)
        .flat_map(|m| &m.tokens)
        .filter_map(|t| match t {
            message::Token::Text { text } => Some(text.chars()),
            _ => None,
        })
        .flatten()
        .chain(
            assets
                .save_point_tutorial
                .iter()
                .flat_map(|span| span.text.chars()),
        )
        .collect();
    crate::font::cook_repertoire(extracted, output, &required)?;
    let session_data = crate::session::cook(extracted, output)?;
    let text = crate::session::cook_text(extracted, output)?;
    let skits = crate::skit::cook(extracted, output)?;
    let ui: resonance_content::font::DialogueArt =
        serde_json::from_slice(&fs::read(output.join("ui/dialogue.json"))?)?;
    let font: resonance_content::font::BitmapFont =
        serde_json::from_slice(&fs::read(output.join(&ui.font))?)?;
    crate::font::validate_messages(&font, &messages)?;
    let mut files: std::collections::BTreeSet<_> = [
        script_path.to_string(),
        messages_path.to_string(),
        "ui/dialogue.json".into(),
        "ui/story-subtitles.json".into(),
        ui.font,
        ui.cursor.path,
        font.texture.clone(),
        assets.contact_shadow.texture.clone(),
        assets.toon_ramp.clone(),
        assets.effects.clone(),
        session_data,
        text,
        skits,
    ]
    .into();
    files.extend(ui.textures.into_iter().map(|texture| texture.path));
    files.extend(effect_files);
    files.extend(overlay_files);
    for part in assets
        .parts
        .iter()
        .chain(assets.actors.iter().flat_map(|actor| &actor.parts))
    {
        files.insert(part.mesh.clone());
        files.extend(part.textures.iter().cloned());
    }
    assets.files = files
        .into_iter()
        .map(|path| Ok((path.clone(), hash_file(&output.join(path))?)))
        .collect::<Result<_>>()?;
    if map_id == 340 {
        assets.files.extend(cook_setup(extracted, output, &assets)?);
        let setup: FieldAssets =
            serde_json::from_slice(&fs::read(output.join("fields/new-game-setup.json"))?)?;
        let setup_messages: Vec<symphonia_script::message::Message> =
            serde_json::from_slice(&fs::read(output.join(&setup.messages))?)?;
        crate::font::validate_messages(&font, &setup_messages)?;
    }
    assets.validate()?;
    let bytes = serde_json::to_vec_pretty(&assets)?;
    write_atomic(&metadata, &bytes)?;
    write_atomic(
        &cache,
        &serde_json::to_vec_pretty(
            &serde_json::json!({"recipe":recipe,"manifest_sha256":digest(&bytes)}),
        )?,
    )?;
    println!(
        "Cooked field {map_id} environment, {} messages and {} collision triangles",
        messages.len(),
        assets
            .ground
            .iter()
            .map(|g| g.triangles.len())
            .sum::<usize>()
    );
    refresh_preloads(output)?;
    Ok(())
}

/// Rebind only the shared assets just cooked, then refresh dependent descriptors.
pub(crate) fn refresh_shared(output: &Path, paths: &[String]) -> Result<()> {
    let directory = output.join("fields");
    if !directory.exists() {
        return Ok(());
    }
    let hashes = paths
        .iter()
        .map(|path| Ok((path.clone(), crate::media::hash_file(&output.join(path))?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut changed = std::collections::BTreeSet::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.path().extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(mut field) = serde_json::from_slice::<resonance_content::field::FieldAssets>(
            &fs::read(entry.path())?,
        ) else {
            continue;
        };
        field.files.extend(hashes.clone());
        write_atomic(&entry.path(), &serde_json::to_vec_pretty(&field)?)?;
        changed.insert(
            entry
                .path()
                .strip_prefix(output)?
                .to_string_lossy()
                .into_owned(),
        );
    }
    // A field can include another field descriptor, such as new-game setup.
    for _ in 0..=changed.len() {
        let mut dirty = false;
        for path in &changed {
            let mut field: resonance_content::field::FieldAssets =
                serde_json::from_slice(&fs::read(output.join(path))?)?;
            let mut updated = false;
            for (dependency, hash) in &mut field.files {
                if changed.contains(dependency) {
                    let current = crate::media::hash_file(&output.join(dependency))?;
                    if *hash != current {
                        *hash = current;
                        updated = true;
                    }
                }
            }
            if updated {
                write_atomic(&output.join(path), &serde_json::to_vec_pretty(&field)?)?;
                dirty = true;
            }
        }
        if !dirty {
            return refresh_preloads(output);
        }
    }
    anyhow::bail!("cyclic field descriptor dependencies")
}

/// Refresh after geometry or media cooks, including cache hits. Separate media
/// inputs remain explicitly missing until their cook has completed.
pub(crate) fn refresh_preloads(output: &Path) -> Result<()> {
    use resonance_content::field_preload::Inputs;
    let directory = output.join("fields");
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.path().extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(assets) = serde_json::from_slice::<resonance_content::field::FieldAssets>(
            &fs::read(entry.path())?,
        ) else {
            continue;
        };
        let audio = if matches!(assets.map_id, 5 | 340) {
            "fields/iselia-classroom-audio.json".into()
        } else {
            format!("fields/map-{}-audio.json", assets.map_id)
        };
        crate::field_preload::cook(
            output,
            Inputs {
                field: entry
                    .path()
                    .strip_prefix(output)?
                    .to_string_lossy()
                    .into_owned(),
                audio: [audio].into(),
                movies: if assets.map_id == 5 {
                    ["story-intro.json".into()].into()
                } else {
                    Default::default()
                },
            },
        )?;
    }
    Ok(())
}

fn cook_setup(
    extracted: &Path,
    output: &Path,
    classroom: &resonance_content::field::FieldAssets,
) -> Result<BTreeMap<String, String>> {
    use crate::scene::{PartSource, cook_part};
    let map = MapArchive::open(&extracted.join("files/MAP/_custom.bin"))?;
    let (part, _, _) = cook_part(
        PartSource {
            name: "fields/new-game-setup/00",
            source: map.section(0)?,
            resource: 0,
            draw_order: 0,
            depth_write: true,
            translation: [0.; 3],
            autoplay: Some(map.section(1)?),
            animation_slots: &[],
            clip_prefix: "setup",
            extra_clips: &[],
            shared_clips: &[],
            texture_animations: Vec::new(),
        },
        output,
    )?;
    let script = map.section(6)?;
    Program::decode(script)?;
    let header = scenario::parse_header(script)?;
    let messages = message::parse(&script[header.auxiliary_offset()..])?;
    let script_path = "fields/new-game-setup/events.ssb";
    let messages_path = "fields/new-game-setup/messages.json";
    write_atomic(&output.join(script_path), script)?;
    write_atomic(&output.join(messages_path), &serde_json::to_vec(&messages)?)?;
    let mut files = BTreeMap::new();
    for path in [script_path, messages_path, &part.mesh]
        .into_iter()
        .chain(part.textures.iter().map(String::as_str))
    {
        files.insert(
            path.to_string(),
            crate::media::hash_file(&output.join(path))?,
        );
    }
    let mut setup = classroom.clone();
    let physical =
        crate::scene::binding::Map::open(output, 1, "MAP/_custom.bin", &map.source_sha256)?;
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let declared = crate::field_resources::declarations(script)?;
    let declared_assets =
        crate::character::Sources::read(&extracted.join("files"), &catalogue, &declared.resources)?;
    let (overlays, overlay_files) = crate::field_overlay::cook(
        extracted,
        &map,
        &physical,
        output,
        &declared_assets.textures,
    )?;
    for path in overlay_files {
        files.insert(path.clone(), crate::media::hash_file(&output.join(path))?);
    }
    setup.map_id = 5;
    setup.source_sha256 = map.source_sha256.clone();
    setup.script = resonance_content::ScriptAsset {
        path: script_path.into(),
        sha256: digest(script),
    };
    setup.messages = messages_path.into();
    setup.parts = vec![part];
    setup.overlays = overlays;
    setup.actors.retain(|actor| actor.resource == 1);
    setup.ground = collision(map.section(4)?)?;
    setup.regions.clear();
    setup.files.extend(files.clone());
    setup.validate()?;
    let path = "fields/new-game-setup.json";
    let bytes = serde_json::to_vec_pretty(&setup)?;
    write_atomic(&output.join(path), &bytes)?;
    files.insert(path.into(), digest(&bytes));
    Ok(files)
}

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
            let mut writer = builder.build(Cursor::new(Vec::new())).unwrap();
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
