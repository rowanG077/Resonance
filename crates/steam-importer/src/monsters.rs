//! Native Monster Book records and model owners in the shared catalogue format.
use crate::{
    output,
    package::Package,
    pe, preview_model,
    source::Source,
    text::{Bank, Menu},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    menu_data::{Element, MONSTER_BOOK_PATH},
    monster::{MONSTER_VERSION, MonsterStats},
};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};
use symphonia_constants::MonsterId;

fn count(image: &pe::Image<'_>) -> Result<usize> {
    let bound = image.slice(0x514405, 7)?;
    ensure!(
        bound[0] == 0x3d && bound[5] == 0x7c,
        "unsupported enemy catalogue bound"
    );
    let count = pe::word(bound, 1)? as usize;
    ensure!((1..=4096).contains(&count), "invalid enemy catalogue count");
    ensure!(
        pe::word(image.slice(0x8e5b50 + count as u32 * 4, 4)?, 0)? == 0,
        "unterminated enemy directory"
    );
    Ok(count)
}
pub(crate) fn symbol(id: u16) -> String {
    u8::try_from(id)
        .ok()
        .and_then(|id| MonsterId::try_from(id).ok())
        .and_then(MonsterId::resource_name)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("model-{id:04}"))
}
fn package<'a>(image: &'a pe::Image<'_>, id: u16) -> Result<&'a str> {
    image.text(pe::word(image.slice(0x8e5b50 + u32::from(id) * 4, 4)?, 0)?)
}
pub(crate) fn catalogue(source: &Source, library: &Path) -> Result<()> {
    let text = source.require("TOS_TEXT.TXB")?;
    let menu = Menu {
        image: pe::Image::parse(&source.executable)?,
        bank: Bank::parse(&text)?,
        slot: source.language.slot(),
    };
    let names = menu.directory(0x8e4008)?;
    let locations = menu.directory(0x8e3ff0)?;
    let categories = menu.directory(0x8b93c4)?;
    let mut records = Vec::new();
    for id in 0..count(&menu.image)? {
        let row = menu.image.slice(0x8e4020 + id as u32 * 12, 12)?;
        let bytes = source.require(package(&menu.image, id as u16)?)?;
        let metadata = metadata(&bytes)?;
        let stats_at = usize::from(pe::half(&bytes, 6)?);
        let stats = bytes
            .get(stats_at..)
            .context("enemy statistics exceed package")?;
        let mut statistics = vec![self::statistics(
            stats,
            pe::word(metadata, 0x40)?,
            pe::word(metadata, 0x44)?,
        )?];
        let variants = usize::from(metadata[0x1f3]);
        ensure!(variants < 16, "too many enemy variants");
        if variants != 0 {
            let start = pe::word(&bytes, 0x1e0)? as usize;
            ensure!(start >= 0x1f8, "enemy variants overlap header");
            for row in bytes
                .get(start..start + variants * 36)
                .context("truncated enemy variants")?
                .chunks_exact(36)
            {
                statistics.push(variant(row)?);
            }
        }
        let family = pe::half(metadata, 0x2a)?;
        ensure!(family < 13 && row[9] < 54, "invalid enemy family/location");
        let attack = metadata[0];
        ensure!(attack <= 8, "invalid attack element");
        let affinities: [i8; 9] = std::array::from_fn(|i| metadata[1 + i] as i8);
        let elements = |test: fn(i8) -> bool| {
            Element::ALL
                .into_iter()
                .enumerate()
                .filter_map(|(i, e)| test(affinities[i + 1]).then_some(e))
                .collect::<Vec<_>>()
        };
        let optional = |v| (v != 0).then_some(v);
        records.push(json!({
            "version":MONSTER_VERSION,"id":id,"name":menu.caption(names,pe::word(row,0)?)?,
            "location":menu.caption(locations,u32::from(row[9]))?,"category":menu.caption(categories,u32::from(family))?,
            "unseen_count_group":row[8],"statistics":statistics,
            "drops":[optional(pe::half(metadata,0x48)?),optional(pe::half(metadata,0x4a)?)],
            "drop_chances":[metadata[0x4e],metadata[0x4f]],"grade":pe::half(metadata,0x1f6)? as i16,
            "steal":optional(pe::half(metadata,0x4c)?),"steal_chance":metadata[0x50],
            "attack_element":attack.checked_sub(1).map(|i|Element::ALL[usize::from(i)]),"affinities":affinities,
            "weaknesses":elements(|v| v == 1),"resistances":elements(|v| v > 1),
            "preview":format!("meshes/monsters/{}/model.json",symbol(id as u16)),
        }));
    }
    let captions = menu.directory(0x8b943c)?;
    let ranks = menu.directory(0x8b93dc)?;
    let mut labels = BTreeMap::new();
    for (key, index) in [
        ("unknown_stat", 0),
        ("number", 6),
        ("unknown_item", 9),
        ("title", 10),
        ("hp", 11),
        ("tp", 12),
        ("attack", 13),
        ("experience", 14),
        ("gald", 15),
        ("defense", 16),
        ("drops", 17),
        ("steal", 18),
        ("location", 19),
        ("attack_element", 20),
        ("weak", 21),
        ("strong", 22),
        ("battle_rank", 26),
    ] {
        labels.insert(key, menu.caption(captions, index)?);
    }
    for (index, key) in ["normal", "hard", "mania"].into_iter().enumerate() {
        labels.insert(key, menu.caption(ranks, index as u32)?);
    }
    output::json(
        &library.join(MONSTER_BOOK_PATH),
        &json!({"records":records,"labels":labels}),
    )
}
fn metadata(bytes: &[u8]) -> Result<&[u8]> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
    let at = usize::from(pe::half(bytes, 4)?);
    ensure!(at >= 0x1f8, "enemy metadata overlaps header");
    bytes
        .get(at..at + 0x200)
        .context("truncated enemy metadata")
}

fn statistics(row: &[u8], experience: u32, gald: u32) -> Result<MonsterStats> {
    let positive = |at| -> Result<u16> { Ok(u16::try_from(pe::half(row, at)? as i16)?) };
    Ok(MonsterStats {
        hp: pe::word(row, 32)?,
        tp: positive(40)?,
        initial_hp: pe::word(row, 36)?,
        initial_tp: positive(42)?,
        attack: positive(44)?,
        thrust: pe::half(row, 46)? as i16,
        defense: positive(48)?,
        intelligence: pe::half(row, 50)? as i16,
        accuracy: pe::half(row, 52)? as i16,
        evasion: pe::half(row, 54)? as i16,
        luck: row[56],
        level: row[29],
        experience,
        gald,
    })
}

fn variant(row: &[u8]) -> Result<MonsterStats> {
    let positive = |at| -> Result<u16> { Ok(u16::try_from(pe::half(row, at)? as i16)?) };
    Ok(MonsterStats {
        hp: pe::word(row, 0)?,
        tp: positive(8)?,
        initial_hp: pe::word(row, 4)?,
        initial_tp: positive(10)?,
        attack: positive(20)?,
        thrust: pe::half(row, 22)? as i16,
        defense: positive(24)?,
        intelligence: pe::half(row, 26)? as i16,
        accuracy: pe::half(row, 28)? as i16,
        evasion: pe::half(row, 30)? as i16,
        luck: row[32],
        level: row[33],
        experience: pe::word(row, 12)?,
        gald: pe::word(row, 16)?,
    })
}

pub(crate) fn models(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    for id in 0..count(&image)? {
        let id = id as u16;
        eprintln!("  Monster model: {} ({id})", symbol(id));
        let bytes = source.require(package(&image, id)?)?;
        let metadata = metadata(&bytes)?;
        let owner = format!("meshes/monsters/{}", symbol(id));
        let body = preview_model::reference(&bytes, 0x18)?.context("enemy has no body")?;
        let outline = preview_model::reference(&bytes, 0x1c)?.unwrap_or_default();
        let motions = preview_model::reference(&bytes, 0x20)?
            .map(|name| (0, name))
            .into_iter()
            .collect::<Vec<_>>();
        let (mut preview, _) =
            preview_model::cook(source, library, [&body, &outline], &owner, &motions)?;
        components(source, library, &bytes, metadata, &owner, &mut preview)?;
        preview.scale = f32::from_bits(pe::word(metadata, 0x128)?);
        preview.elevation = f32::from_bits(pe::word(metadata, 0x12c)?);
        preview.hidden_geometry.extend(
            preview.parts[0]
                .scene
                .bone_names
                .iter()
                .filter(|name| name.to_ascii_lowercase().starts_with("pa"))
                .cloned(),
        );
        preview.hidden_geometry.sort();
        preview.hidden_geometry.dedup();
        preview.validate_definition()?;
        output::json(&library.join(&owner).join("model.json"), &preview)?;
    }
    Ok(())
}

// Carried bodies retain their ordinary bone attachments; no menu-only renderer.
fn components(
    source: &Source,
    library: &Path,
    bytes: &[u8],
    metadata: &[u8],
    owner: &str,
    preview: &mut resonance_content::model_preview::ModelPreview,
) -> Result<()> {
    let anchor = |prefix: &str| -> Result<String> {
        let matches = preview.parts[0]
            .scene
            .bone_names
            .iter()
            .filter(|name| name.to_ascii_lowercase().starts_with(prefix))
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "missing or ambiguous monster anchor {prefix}"
        );
        Ok(matches[0].clone())
    };
    let count = usize::from(metadata[0x1f0]);
    ensure!(count <= 8, "too many carried monster bodies");
    let before_body = metadata[0x130] & 1 != 0;
    let flags = pe::half(metadata, 0xb8)?;
    let mut added = Vec::new();
    if !before_body && flags & 0x8400 != 0 {
        let body =
            preview_model::reference(bytes, 0x180)?.context("missing secondary monster body")?;
        let outline = preview_model::reference(bytes, 0x198)?.unwrap_or_default();
        let slot = u16::from(metadata[0xc1]);
        let motion = preview_model::reference(bytes, 0x20 + usize::from(slot) * 4)?
            .or(preview_model::reference(bytes, 0x1b0)?);
        let motions = motion
            .map(|name| (slot, name))
            .into_iter()
            .collect::<Vec<_>>();
        let (model, _) = preview_model::cook(
            source,
            library,
            [&body, &outline],
            &format!("{owner}/secondary"),
            &motions,
        )?;
        let bone = if flags & 0x8000 != 0 {
            Some(anchor("pa00")?)
        } else {
            None
        };
        for mut part in model.parts {
            part.attached_to = bone.clone();
            added.push(part);
        }
    }
    for slot in 0..count {
        let flags = metadata[0x130 + slot * 24];
        ensure!(
            before_body == (flags & 1 != 0),
            "mixed carried-body submission order"
        );
        let Some(member) = preview_model::member(bytes, 0x160 + slot * 4)? else {
            continue;
        };
        let package = Package::parse(member)?;
        ensure!(
            matches!(package.len(), 5 | 6),
            "invalid carried-body directory"
        );
        let Some(body) = package.reference(1)? else {
            continue;
        };
        let outline = package.reference(2)?.unwrap_or("");
        let component_owner = format!("{owner}/carried-{slot}");
        let (model, _) =
            preview_model::cook(source, library, [body, outline], &component_owner, &[])?;
        let bone = anchor(&format!("kk0{slot}"))?;
        for mut part in model.parts {
            part.attached_to = Some(bone.clone());
            part.additive = !before_body && flags & 0x10 != 0;
            for material in &mut part.scene.materials {
                material.depth_write = !part.additive;
            }
            added.push(part);
        }
        if slot == 0
            && !before_body
            && package.len() == 6
            && let Some(variant) = package.reference(5)?
        {
            let (scene, _) =
                preview_model::part(source, library, variant, &component_owner, "variant", &[])?;
            added.push(resonance_content::model_preview::PreviewPart {
                scene,
                animation: None,
                attached_to: Some(bone),
                additive: true,
                uv_offsets: Vec::new(),
            });
        }
    }
    let mut order = 0;
    let sequence = if before_body {
        (2..2 + added.len()).chain(0..2).collect::<Vec<_>>()
    } else {
        (0..2 + added.len()).collect()
    };
    preview.parts.extend(added);
    for index in sequence {
        let part = &mut preview.parts[index];
        part.scene.resource = index.try_into()?;
        for material in &mut part.scene.materials {
            material.draw_order += order;
        }
        order += part.scene.materials.len() as u32;
    }
    Ok(())
}
