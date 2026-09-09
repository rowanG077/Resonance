//! Field archive inspection and conversion. All original-format I/O stays here.
use crate::read::u32 as word;
use crate::{digest, write_atomic};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    ops::Range,
    path::Path,
};
use symphonia_script::{Program, message, scenario, semantics::NativeRegistry};

pub(crate) struct MapArchive {
    pub bytes: Vec<u8>,
    pub sections: Vec<Option<Range<usize>>>,
    pub source_sha256: String,
}

impl MapArchive {
    pub fn open(source: &Path) -> Result<Self> {
        let bytes = fs::read(source)?;
        let mut cabinet = cab::Cabinet::new(Cursor::new(&bytes))?;
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .context("map file name")?
            .to_ascii_uppercase();
        let mut expanded = Vec::new();
        cabinet
            .read_file(&name)?
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
            source_sha256: digest(&bytes),
        })
    }
    pub fn section(&self, index: usize) -> Result<&[u8]> {
        let range = self
            .sections
            .get(index)
            .and_then(Option::as_ref)
            .context("missing map section")?;
        Ok(&self.bytes[range.clone()])
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

/// Cook the classroom's static environment, scenario, dialogue and collision.
/// Character packages are resolved separately from the shared resource tables.
pub fn cook_classroom(extracted: &Path, output: &Path, ktx: &Path) -> Result<()> {
    use crate::media::{Tool, Workspace, hash_file};
    use crate::scene::{PartSource, cook_part};
    use resonance_content::{ScriptAsset, field::FieldAssets};
    let _workspace = Workspace::open(extracted, output)?;
    let ktx = Tool::resolve(ktx)?;
    let mut sources = BTreeMap::new();
    for path in [
        "sys/boot.bin",
        "sys/main.dol",
        "files/MAP/isa_i06.bin",
        "files/MAP/_custom.bin",
        "files/npc_all.bin",
        "files/d.d",
        "files/col_all.bin",
        "files/llo_all.bin",
        "files/gen_all.bin",
        "files/lloyd000.bin",
        "files/lloyd.bin",
        "files/collet000.bin",
        "files/collet.bin",
        "files/genius000.bin",
        "files/genius.bin",
        "files/refill000.bin",
        "files/refill.bin",
        "files/u_f_fontb0.dat",
        "files/system.tpl",
        "files/effect.cab",
        "files/toon.tpl",
    ] {
        sources.insert(path, hash_file(&extracted.join(path))?);
    }
    let recipe = serde_json::json!({"version":1,"sources":sources,"ktx_sha256":ktx.hash,"compiler_sha256":hash_file(&std::env::current_exe()?)?});
    let ktx = ktx.path.as_path();
    let metadata = output.join("fields/iselia-classroom.json");
    let cache = output.join("intermediate/fields/iselia-classroom-recipe.json");
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
        println!("Classroom assets are current");
        refresh_preloads(output)?;
        return Ok(());
    }
    let boot = fs::read(extracted.join("sys/boot.bin"))?;
    ensure!(
        boot.get(..8) == Some(b"GQSEAF\0\0"),
        "expected GQSEAF revision 0 disc 1"
    );
    let map = MapArchive::open(&extracted.join("files/MAP/isa_i06.bin"))?;
    let mut parts = Vec::new();
    for (draw_order, index) in [0, 2].into_iter().enumerate() {
        let (part, _, _) = cook_part(
            PartSource {
                name: &format!("fields/iselia-classroom/{index:02}"),
                source: map.section(index)?,
                resource: index as u16,
                draw_order: draw_order as u32,
                depth_write: index != 2,
                translation: [0.; 3],
                autoplay: None,
                animation_slots: &[],
                clip_prefix: "classroom",
                extra_clips: &[],
                texture_animations: Vec::new(),
            },
            output,
            ktx,
        )?;
        parts.push(part);
    }
    let script = map.section(6)?;
    Program::decode(script)?;
    let header = scenario::parse_header(script)?;
    let messages = message::parse(&script[header.auxiliary_offset()..])?;
    let script_path = "fields/iselia-classroom/events.ssb";
    let messages_path = "fields/iselia-classroom/messages.json";
    write_atomic(&output.join(script_path), script)?;
    write_atomic(&output.join(messages_path), &serde_json::to_vec(&messages)?)?;
    let (effects, effect_files) = crate::field_effects::cook(extracted, output, ktx)?;
    let mut assets = FieldAssets {
        version: 5,
        map_id: 340,
        source_sha256: map.source_sha256.clone(),
        script: ScriptAsset {
            path: script_path.into(),
            sha256: digest(script),
        },
        messages: messages_path.into(),
        parts,
        ground: collision(map.section(4)?)?,
        regions: collision(map.section(5)?)?,
        actors: crate::character::cook_classroom(extracted, output, ktx)?,
        contact_shadow: crate::field_shadow::cook(extracted, output, ktx)?,
        toon_ramp: crate::field_lighting::cook(extracted, output, ktx)?,
        effects,
        files: BTreeMap::new(),
    };
    let setup_source = MapArchive::open(&extracted.join("files/MAP/_custom.bin"))?;
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
        .collect();
    crate::font::cook_repertoire(extracted, output, ktx, &required)?;
    let session_data = crate::session::cook(extracted, output)?;
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
    ]
    .into();
    files.extend(ui.textures.into_iter().map(|texture| texture.path));
    files.extend(effect_files);
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
    let setup_files = cook_setup(extracted, output, ktx, &assets)?;
    let setup: FieldAssets =
        serde_json::from_slice(&fs::read(output.join("fields/new-game-setup.json"))?)?;
    let setup_messages: Vec<symphonia_script::message::Message> =
        serde_json::from_slice(&fs::read(output.join(&setup.messages))?)?;
    crate::font::validate_messages(&font, &setup_messages)?;
    assets.files.extend(setup_files);
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
        "Cooked classroom environment, {} messages and {} collision triangles",
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

/// Recipe-level media bindings for the two fields currently cooked here.
/// Keep these out of the generic manifest builder and refresh after either
/// geometry or media cooks, regardless of their order (including cache hits).
pub(crate) fn refresh_preloads(output: &Path) -> Result<()> {
    use resonance_content::field_preload::Inputs;
    for (field, movies) in [
        ("fields/iselia-classroom.json", Vec::new()),
        (
            "fields/new-game-setup.json",
            vec!["story-intro.json".to_string()],
        ),
    ] {
        match fs::metadata(output.join(field)) {
            Ok(_) => {
                crate::field_preload::cook(
                    output,
                    Inputs {
                        field: field.into(),
                        audio: ["fields/iselia-classroom-audio.json".into()].into(),
                        movies: movies.into_iter().collect(),
                    },
                )?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("inspect cooked field for preload refresh"),
        }
    }
    Ok(())
}

fn cook_setup(
    extracted: &Path,
    output: &Path,
    ktx: &Path,
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
            texture_animations: Vec::new(),
        },
        output,
        ktx,
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
    setup.map_id = 5;
    setup.source_sha256 = map.source_sha256.clone();
    setup.script = resonance_content::ScriptAsset {
        path: script_path.into(),
        sha256: digest(script),
    };
    setup.messages = messages_path.into();
    setup.parts = vec![part];
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

fn collision(bytes: &[u8]) -> Result<Vec<resonance_content::field::CollisionGroup>> {
    use resonance_content::field::CollisionGroup;
    let half = |at: usize| -> Result<u16> {
        Ok(u16::from_be_bytes(
            bytes
                .get(at..at + 2)
                .context("truncated collision halfword")?
                .try_into()?,
        ))
    };
    let count = word(bytes, 4)? as usize;
    ensure!(
        (1..=4096).contains(&count) && 8 + count * 20 <= bytes.len(),
        "invalid collision groups"
    );
    (0..count)
        .map(|i| {
            let at = 8 + i * 20;
            let points = usize::from(half(at)?);
            let triangles = usize::from(half(at + 2)?);
            let point_offset = word(bytes, at + 4)? as usize;
            let triangle_offset = word(bytes, at + 8)? as usize;
            ensure!(
                point_offset >= 8 + count * 20 && triangle_offset >= 8 + count * 20,
                "collision payload overlaps its header"
            );
            let vertices = (0..points)
                .map(|i| {
                    let at = point_offset + i * 12;
                    Ok([
                        f32::from_bits(word(bytes, at)?),
                        f32::from_bits(word(bytes, at + 4)?),
                        f32::from_bits(word(bytes, at + 8)?),
                    ])
                })
                .collect::<Result<_>>()?;
            let triangles = (0..triangles)
                .map(|i| {
                    let at = triangle_offset + i * 6;
                    Ok([half(at)?, half(at + 2)?, half(at + 4)?])
                })
                .collect::<Result<_>>()?;
            let group = CollisionGroup {
                surface: word(bytes, at + 12)?,
                vertices,
                triangles,
            };
            group.validate()?;
            Ok(group)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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
