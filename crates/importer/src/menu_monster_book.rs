//! Original enemy records directly into the named Monster Book document.
use crate::{dol, enemy_package::EnemyPackages, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    menu_data::Element,
    monster::{MONSTER_COUNT, MONSTER_VERSION, MonsterStats},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::MonsterId;

const RECORDS: u32 = 0x802113f4;
const LOCATIONS: u32 = 0x80211328;
const INVENTORY: u32 = 0x8019d650;

fn statistics(row: &[u8], experience: u32, gald: u32) -> Result<MonsterStats> {
    let positive = |at| -> Result<u16> { Ok(u16::try_from(dol::u16(row, at)? as i16)?) };
    Ok(MonsterStats {
        hp: dol::u32(row, 32)?,
        tp: positive(40)?,
        initial_hp: dol::u32(row, 36)?,
        initial_tp: positive(42)?,
        attack: positive(44)?,
        thrust: dol::u16(row, 46)? as i16,
        defense: positive(48)?,
        intelligence: dol::u16(row, 50)? as i16,
        accuracy: dol::u16(row, 52)? as i16,
        evasion: dol::u16(row, 54)? as i16,
        luck: row[56],
        level: row[29],
        experience,
        gald,
    })
}

fn variant(row: &[u8]) -> Result<MonsterStats> {
    let positive = |at| -> Result<u16> { Ok(u16::try_from(dol::u16(row, at)? as i16)?) };
    Ok(MonsterStats {
        hp: dol::u32(row, 0)?,
        tp: positive(8)?,
        initial_hp: dol::u32(row, 4)?,
        initial_tp: positive(10)?,
        attack: positive(20)?,
        thrust: dol::u16(row, 22)? as i16,
        defense: positive(24)?,
        intelligence: dol::u16(row, 26)? as i16,
        accuracy: dol::u16(row, 28)? as i16,
        evasion: dol::u16(row, 30)? as i16,
        luck: row[32],
        level: row[33],
        experience: dol::u32(row, 12)?,
        gald: dol::u32(row, 16)?,
    })
}

fn monster(exe: &[u8], id: u8, source: &[u8]) -> Result<Value> {
    ensure!(source.starts_with(b"em8\0"), "invalid enemy package");
    let metadata = source
        .get(usize::from(dol::u16(source, 4)?)..)
        .context("enemy metadata exceeds package")?;
    let stats = source
        .get(usize::from(dol::u16(source, 6)?)..)
        .context("enemy statistics exceed package")?;
    ensure!(
        metadata.len() >= 0x1ec && stats.len() >= 60,
        "truncated enemy data"
    );
    let row = dol::slice(exe, RECORDS + u32::from(id) * 12, 12)?;
    let location_id = u32::from(row[9]);
    ensure!(location_id < 51, "unknown monster location");
    let location = text::pointer(exe, LOCATIONS + location_id * 4)?;
    let family = u32::from(dol::u16(metadata, 0x2a)?);
    ensure!(family < 13, "unknown monster family");
    let category = text::pointer(exe, INVENTORY + (44 + family) * 4)?;

    let mut statistics = vec![self::statistics(
        stats,
        dol::u32(metadata, 0x40)?,
        dol::u32(metadata, 0x44)?,
    )?];
    let variants = usize::from(metadata[0x1e7]);
    if variants != 0 {
        let start = dol::u32(source, 0x1e0)? as usize;
        ensure!(start >= 0x1e8, "missing enemy variant table");
        let rows = source
            .get(start..start + variants * 36)
            .context("truncated enemy variants")?;
        for row in rows.chunks_exact(36) {
            statistics.push(variant(row)?);
        }
    }

    let attack_element = metadata[0];
    ensure!(attack_element <= 8, "invalid monster attack element");
    let affinities: [i8; 9] = std::array::from_fn(|i| metadata[1 + i] as i8);
    let elements = |test: fn(i8) -> bool| -> Vec<Element> {
        Element::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(i, element)| test(affinities[i + 1]).then_some(element))
            .collect()
    };
    let name = MonsterId::try_from(id)
        .ok()
        .and_then(MonsterId::resource_name)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("model-{id:04}"));
    let drops =
        [dol::u16(metadata, 0x48)?, dol::u16(metadata, 0x4a)?].map(|id| (id != 0).then_some(id));
    let steal = dol::u16(metadata, 0x4c)?;
    let mut record = json!({
        "version": MONSTER_VERSION,
        "id": id,
        "name": text::string(exe, dol::u32(row, 0)?)?,
        "location": location,
        "category": category,
        "unseen_count_group": row[8],
        "statistics": statistics,
        "drops": drops,
        "drop_chances": [metadata[0x4e], metadata[0x4f]],
        "grade": dol::u16(metadata, 0x1ea)? as i16,
        "steal": (steal != 0).then_some(steal),
        "steal_chance": metadata[0x50],
        "attack_element": attack_element.checked_sub(1).map(|index| Element::ALL[index as usize]),
        "affinities": affinities,
        "preview": format!("meshes/monsters/{name}/model.json"),
    });
    let weaknesses = elements(|value| value == 1);
    let resistances = elements(|value| value > 1);
    if affinities == [0; 9] {
        record.as_object_mut().unwrap().remove("affinities");
    }
    if !weaknesses.is_empty() {
        record["weaknesses"] = json!(weaknesses);
    }
    if !resistances.is_empty() {
        record["resistances"] = json!(resistances);
    }
    Ok(record)
}

fn labels(exe: &[u8]) -> Result<BTreeMap<String, String>> {
    let mut labels = BTreeMap::new();
    for (name, address) in [
        ("number", 0x8035d370),
        ("hp", 0x8035d374),
        ("tp", 0x8035d378),
        ("unknown_item", 0x801aa9ec),
    ] {
        labels.insert(name.into(), text::string(exe, address)?);
    }
    for (name, index) in [
        ("title", 43),
        ("normal", 57),
        ("hard", 58),
        ("mania", 59),
        ("attack", 60),
        ("experience", 61),
        ("gald", 62),
        ("defense", 63),
        ("drops", 64),
        ("steal", 65),
        ("location", 66),
        ("attack_element", 67),
        ("weak", 68),
        ("strong", 69),
        ("battle_rank", 72),
    ] {
        labels.insert(name.into(), text::pointer(exe, INVENTORY + index * 4)?);
    }
    labels.insert("unknown_stat".into(), text::pointer(exe, 0x8035d30c)?);
    Ok(labels)
}

pub fn cook_menu_monster_book(extracted: &Path, library: &Path) -> Result<()> {
    let output = library.join("game/menu/monster-book.json");
    ensure!(!output.exists(), "monster book output already exists");
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let mut packages = EnemyPackages::open(&extracted.join("disc1"))?;
    let records = (0..MONSTER_COUNT)
        .map(|id| {
            let id = u8::try_from(id)?;
            monster(&exe, id, &packages.read(id)?).with_context(|| format!("convert monster {id}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let document = json!({"labels": labels(&exe)?, "records": records});
    crate::output::json(&output, &document)?;
    Ok(())
}
