//! Controlled menu-test inventory, separate from unmodified story fixtures.
use super::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::ParseIntError,
    str::FromStr,
};

const RAM_SIZE: usize = 0x1800000;

#[derive(clap::Args, Default, PartialEq)]
pub(super) struct Changes {
    /// Set saved dialogue volume in both copies; the next spoken line applies it.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=127))]
    voice_volume: Option<u8>,
    /// Set the saved dialogue/menu frame style in both checkpoint copies.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=2))]
    window_style: Option<u8>,
    /// Set the saved dialogue/menu background pattern in both copies.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=5))]
    window_background: Option<u8>,
    /// Change live story progress; apply the reported story_origin after native field initialization.
    #[arg(long)]
    story: Option<i32>,
    /// Set the test party order using one-based character IDs, preserving the field leader.
    #[arg(long, num_args = 1..)]
    formation: Vec<u8>,
    /// Mark skits as already viewed in both test copies, preserving notification settings.
    #[arg(long, num_args = 1.., value_parser = clap::value_parser!(u16).range(1..=860))]
    view_skit: Vec<u16>,
    /// Set synopsis history as ID:VALUE:LEVEL:UNIX_SECONDS in both copies.
    #[arg(long, num_args = 1..)]
    record_synopsis: Vec<String>,
    /// Set CHARACTER:HP:TP:CONDITIONS for a test member (zero-based; conditions may be hex).
    #[arg(long, num_args = 1..)]
    member_vitals: Vec<String>,
    /// Learn a technique as CHARACTER:TECHNIQUE (zero-based character, cooked technique ID).
    #[arg(long, num_args = 1..)]
    learn_technique: Vec<String>,
    /// Learn dishes by their zero-based cooked recipe IDs in both checkpoint copies.
    #[arg(long, num_args = 1..)]
    learn_recipe: Vec<u8>,
    #[arg(long, num_args = 1..)]
    grant_item: Vec<u16>,
    /// Set held quantities as ID:COUNT, within the cooked stack limit; zero removes a held item.
    #[arg(long, num_args = 1..)]
    item_count: Vec<String>,
    /// Catalogue every classified item without adding it to inventory.
    #[arg(long)]
    discover_all: bool,
    /// Mark map locations as visited in both copies, without advancing the story.
    #[arg(long, num_args = 1..)]
    visit_location: Vec<u16>,
    /// Reveal these shops' inventories in both copies' map directory.
    #[arg(long, num_args = 1..)]
    visit_shop: Vec<u8>,
    /// Reveal a monster's base record, scan and item/location information in both copies.
    #[arg(long, num_args = 1..)]
    catalogue_monster: Vec<u8>,
    /// Record an encounter without revealing scanned statistics or items.
    #[arg(long, num_args = 1..)]
    encounter_monster: Vec<u8>,
    /// Reveal a repeat-battle variant as ID:VARIANT (requires cooked monster records).
    #[arg(long, num_args = 1..)]
    monster_variant: Vec<String>,
    /// Learn manual topics by their cooked learned_flag IDs in both checkpoint copies.
    #[arg(long, num_args = 1..)]
    learn_manual_topic: Vec<u16>,
    /// Collect named figurines in both checkpoint copies without advancing the story.
    #[arg(long, num_args = 1..)]
    collect_figurine: Vec<u16>,
    /// Learn a cooked compound recipe as CHARACTER:RECIPE (both zero-based).
    #[arg(long, num_args = 1..)]
    learn_ex_compound: Vec<String>,
    /// Learn a compound and retain its newly learned highlight in both copies.
    #[arg(long, num_args = 1..)]
    new_ex_compound: Vec<String>,
}

fn pair<A, B>(tag: &str, expected: &'static str) -> Result<(A, B)>
where
    A: FromStr<Err = ParseIntError>,
    B: FromStr<Err = ParseIntError>,
{
    let (a, b) = tag.split_once(':').context(expected)?;
    Ok((a.parse()?, b.parse()?))
}

fn fields<T>(
    tag: &str,
    expected: &'static str,
    parse: impl FnMut(&str) -> Result<T, ParseIntError>,
) -> Result<[T; 4]> {
    let fields = tag.split(':').map(parse).collect::<Result<Vec<_>, _>>()?;
    fields.try_into().map_err(|_| anyhow::anyhow!(expected))
}

fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(at..at + 4)
            .context("truncated state word")?
            .try_into()?,
    ))
}

pub(super) fn run(
    source: &Path,
    native: &Path,
    output: &Path,
    changes: &Changes,
    cooked: &Path,
) -> Result<()> {
    ensure!(!output.exists(), "fixture output already exists");
    let Changes {
        voice_volume,
        window_style,
        window_background,
        story,
        formation,
        view_skit,
        record_synopsis,
        member_vitals,
        learn_technique,
        learn_recipe,
        grant_item: grants,
        item_count,
        discover_all,
        visit_location,
        visit_shop,
        catalogue_monster,
        encounter_monster,
        monster_variant,
        learn_manual_topic,
        collect_figurine,
        learn_ex_compound,
        new_ex_compound,
    } = changes;
    ensure!(
        changes != &Changes::default(),
        "fixture needs a story, inventory, knowledge or audio setting change"
    );
    let data = fs::read(source)?;
    let original_native = fs::read(native)?;
    let mut save: Value = serde_json::from_slice(&original_native)?;
    let menu: Value = serde_json::from_slice(&fs::read(cooked.join("game/menu-data.json"))?)?;
    let items = menu["items"].as_array().context("missing cooked items")?;
    ensure!(
        items.len() == 528
            && grants
                .iter()
                .all(|&id| id > 0 && usize::from(id) < items.len()),
        "invalid granted item"
    );
    ensure!(
        data.get(..6) == Some(b"GQSEAF") && data.len() >= 48,
        "expected a GQSEAF state"
    );
    let le32 = |at| -> Result<u32> {
        Ok(u32::from_le_bytes(
            data.get(at..at + 4)
                .context("truncated state header")?
                .try_into()?,
        ))
    };
    ensure!(
        le32(24)? == 0xbaad_babe + 191,
        "expected Dolphin 2606 state version"
    );
    let version_length = le32(28)? as usize;
    ensure!(
        (1..=256).contains(&version_length),
        "invalid Dolphin version length"
    );
    let header = 32 + version_length;
    ensure!(
        le32(header)? == 0x0001_0001 && le32(header + 4)? == 0,
        "expected single-block LZ4 state"
    );
    let size = u64::from_le_bytes(
        data.get(header + 8..header + 16)
            .context("missing state size")?
            .try_into()?,
    );
    ensure!(
        (1..=256 * 1024 * 1024).contains(&size),
        "invalid expanded state size"
    );
    let compressed_at = header + 20;
    let compressed_size = le32(header + 16)? as usize;
    ensure!(
        compressed_at + compressed_size == data.len(),
        "truncated or multi-block state"
    );
    // This development tool uses the Nix shell's codec; no player dependency.
    let codec = unsafe {
        libloading::Library::new("liblz4.so.1")
            .or_else(|_| libloading::Library::new("liblz4.dylib"))?
    };
    type Codec = unsafe extern "C" fn(*const u8, *mut u8, i32, i32) -> i32;
    let decode: libloading::Symbol<'_, Codec> = unsafe { codec.get(b"LZ4_decompress_safe")? };
    let encode: libloading::Symbol<'_, Codec> = unsafe { codec.get(b"LZ4_compress_default")? };
    let mut raw = vec![0; size as usize];
    let decoded = unsafe {
        decode(
            data[compressed_at..].as_ptr(),
            raw.as_mut_ptr(),
            compressed_size.try_into()?,
            raw.len().try_into()?,
        )
    };
    ensure!(decoded == size as i32, "corrupt state compression");
    let candidates: Vec<_> = raw
        .windows(6)
        .enumerate()
        .filter_map(|(at, marker)| {
            (marker == b"GQSEAF"
                && raw.get(at + 0x1c..at + 0x20) == Some(&[0xc2, 0x33, 0x9f, 0x3d])
                && raw.get(at + 0x28..at + 0x2c) == Some(&[1, 0x80, 0, 0])
                && at + RAM_SIZE <= raw.len())
            .then_some(at)
        })
        .collect();
    ensure!(candidates.len() == 1, "cannot uniquely locate main memory");
    let ram_at = candidates[0];
    let ram = &mut raw[ram_at..ram_at + RAM_SIZE];
    let session = word(ram, 0x35a768)?
        .checked_sub(0x8000_0000)
        .context("invalid session pointer")? as usize;
    ensure!(
        session + 0x1f60 <= ram.len() && session.is_multiple_of(4),
        "invalid session extent"
    );
    let mut story_change = Value::Null;
    if let Some(story) = story {
        ensure!(*story >= 0, "negative story threshold");
        let globals = word(ram, 0x35a578)?
            .checked_sub(0x8000_0000)
            .context("invalid script globals pointer")? as usize;
        ensure!(globals.is_multiple_of(4), "unaligned script globals");
        let old = word(ram, globals + 0x40)? as i32;
        ensure!(
            save["state"]["progress"]["script_globals"][16] == old,
            "starting story progress differs"
        );
        ram[globals + 0x40..globals + 0x44].copy_from_slice(&story.to_be_bytes());
        story_change = json!({"from":old,"to":story});
    }
    let party = &mut save["state"]["progress"]["party"];
    if voice_volume.is_some() || window_style.is_some() || window_background.is_some() {
        let settings = &mut party["settings"];
        let mut preferences: resonance_content::menu_data::CustomizeSettings =
            serde_json::from_value(
                settings
                    .get("preferences")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            )?;
        if let Some(volume) = voice_volume {
            ensure!(
                preferences.volumes.voice == ram[session + 0x194],
                "starting voice volumes differ"
            );
            ram[session + 0x194] = *volume;
            preferences.volumes.voice = *volume;
        }
        if window_style.is_some() || window_background.is_some() {
            let style = &mut ram[session + 0x197];
            ensure!(
                (*style >> 4) & 3 == preferences.window && *style & 15 == preferences.background,
                "starting window preferences differ"
            );
            preferences.window = window_style.unwrap_or(preferences.window);
            preferences.background = window_background.unwrap_or(preferences.background);
            *style = (*style & 0xc0) | (preferences.window << 4) | preferences.background;
        }
        settings["preferences"] = serde_json::to_value(preferences)?;
    }
    if !learn_recipe.is_empty() {
        let mut cooking = party
            .get("cooking")
            .cloned()
            .unwrap_or_else(|| json!({"known":1,"recipe":0,"chef":0,"full":false}));
        let mut known = word(ram, session + 0x1e18)?;
        ensure!(
            cooking["known"] == known
                && cooking["recipe"] == ram[session + 0x1e1d]
                && cooking["chef"] == ram[session + 0x1e1e]
                && cooking["full"] == (ram[session + 0x1e1c] != 0),
            "starting cooking progress differs"
        );
        let recipes = menu["cooking"]["recipes"]
            .as_array()
            .context("missing cooked recipes")?;
        for &recipe in learn_recipe {
            ensure!(
                usize::from(recipe) < recipes.len() && recipe < 32,
                "unknown recipe {recipe}"
            );
            known |= 1 << recipe;
        }
        ram[session + 0x1e18..session + 0x1e1c].copy_from_slice(&known.to_be_bytes());
        cooking["known"] = json!(known);
        party["cooking"] = cooking;
    }
    for tag in member_vitals {
        let [character, hp, tp, conditions] =
            fields(tag, "expected CHARACTER:HP:TP:CONDITIONS", |value| {
                value
                    .strip_prefix("0x")
                    .map_or_else(|| value.parse(), |hex| u32::from_str_radix(hex, 16))
            })?;
        ensure!(character < 9, "unknown party member");
        let at = session + 0x2b8 + character as usize * 0x118;
        let member = &mut party["members"][character as usize];
        let half = |offset| u32::from(u16::from_be_bytes([ram[at + offset], ram[at + offset + 1]]));
        ensure!(
            member["hp"] == half(0x12)
                && member["tp"] == half(0x14)
                && member["conditions"].as_u64().unwrap_or(0) == u64::from(word(ram, at + 0x1c)?),
            "starting member vitals differ"
        );
        ensure!(
            hp <= half(0x36) && tp <= half(0x38),
            "fixture vitals exceed maximum"
        );
        ensure!(
            (hp == 0) == (conditions & 0x8000_0000 != 0),
            "inconsistent knockout condition"
        );
        ram[at + 0x12..at + 0x14].copy_from_slice(&(hp as u16).to_be_bytes());
        ram[at + 0x14..at + 0x16].copy_from_slice(&(tp as u16).to_be_bytes());
        ram[at + 0x1c..at + 0x20].copy_from_slice(&conditions.to_be_bytes());
        member["hp"] = json!(hp);
        member["tp"] = json!(tp);
        member["conditions"] = json!(conditions);
    }
    if !formation.is_empty() {
        let old: Vec<u8> = serde_json::from_value(party["formation"].clone())?;
        let source_order = &mut ram[session + 0xe9d..session + 0xea5];
        ensure!(
            source_order
                .iter()
                .copied()
                .filter(|&id| id != 0)
                .collect::<Vec<_>>()
                == old,
            "starting party formation differs"
        );
        let leader = party["field_leader"].as_u64().unwrap_or(1) as u8;
        ensure!(
            formation.len() <= source_order.len()
                && formation.iter().all(|id| (1..=9).contains(id))
                && formation.iter().collect::<BTreeSet<_>>().len() == formation.len()
                && formation.contains(&leader),
            "invalid fixture formation or missing field leader"
        );
        source_order.fill(0);
        source_order[..formation.len()].copy_from_slice(formation);
        party["formation"] = serde_json::to_value(formation)?;
    }
    if !view_skit.is_empty() {
        let mut viewed: BTreeSet<u16> =
            serde_json::from_value(party.get("viewed_skits").cloned().unwrap_or(json!([])))?;
        for &id in view_skit {
            let byte = &mut ram[session + 0xd9d + usize::from(id / 8)];
            let mask = 1 << (id % 8);
            ensure!(
                viewed.contains(&id) == (*byte & mask != 0),
                "starting viewed skit differs"
            );
            *byte |= mask;
            viewed.insert(id);
        }
        party["viewed_skits"] = serde_json::to_value(viewed)?;
    }
    if !learn_technique.is_empty() {
        let definitions: resonance_content::session::SessionData =
            serde_json::from_slice(&fs::read(cooked.join("game/session-data.json"))?)?;
        definitions.validate()?;
        for tag in learn_technique {
            let (character, technique): (usize, u16) = pair(tag, "expected CHARACTER:TECHNIQUE")?;
            let allowed = &definitions
                .characters
                .get(character)
                .context("unknown technique owner")?
                .allowed_techniques;
            let index = allowed
                .iter()
                .position(|&id| id == technique)
                .context("technique is unavailable to this character")?;
            let source = &ram[0x202dc8 + character * 0x29..0x202dc8 + (character + 1) * 0x29];
            ensure!(
                usize::from(source[0]) == allowed.len()
                    && source[1..=allowed.len()]
                        .iter()
                        .map(|&id| u16::from(id))
                        .eq(allowed.iter().copied()),
                "cooked technique order differs from original table"
            );
            let at = session + 0x2b8 + character * 0x118 + 0x70;
            let bits = u64::from_be_bytes(ram[at..at + 8].try_into()?);
            let mut known: BTreeSet<u16> =
                serde_json::from_value(party["members"][character]["techniques"].clone())?;
            let disabled: BTreeSet<u16> = serde_json::from_value(
                party["members"][character]
                    .get("disabled_techniques")
                    .cloned()
                    .unwrap_or(json!([])),
            )?;
            let enabled = u64::from_be_bytes(ram[at + 8..at + 16].try_into()?);
            ensure!(
                allowed
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &id)| (bits & (1 << i) != 0).then_some(id))
                    .collect::<BTreeSet<_>>()
                    == known,
                "starting learned techniques differ"
            );
            ensure!(
                allowed.iter().enumerate().all(|(i, id)| !known.contains(id)
                    || (enabled & (1 << i) == 0) == disabled.contains(id)),
                "starting technique AI flags differ"
            );
            ram[at..at + 8].copy_from_slice(&(bits | (1 << index)).to_be_bytes());
            if !disabled.contains(&technique) {
                ram[at + 8..at + 16].copy_from_slice(&(enabled | (1 << index)).to_be_bytes());
            }
            known.insert(technique);
            party["members"][character]["techniques"] = serde_json::to_value(known)?;
        }
    }
    let mut inventory: BTreeMap<u16, u8> = serde_json::from_value(party["items"].clone())?;
    let mut found: BTreeSet<u16> = serde_json::from_value(party["found_items"].clone())?;
    let mut recent: Vec<u16> = serde_json::from_value(party["recent_items"].clone())?;
    let source_inventory: BTreeMap<_, _> = (1..528u16)
        .filter_map(|id| {
            let count = ram[session + 0xead + usize::from(id)];
            (count != 0).then_some((id, count))
        })
        .collect();
    let source_found = (1..528u16)
        .filter(|&id| {
            word(ram, session + 0x1124 + usize::from(id / 32) * 4).unwrap() & (1 << (id % 32)) != 0
        })
        .collect::<BTreeSet<_>>();
    let source_recent: Vec<_> = ram[session + 0x10e4..session + 0x1124]
        .chunks_exact(2)
        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
        .take_while(|&id| id != 0)
        .collect();
    ensure!(
        source_inventory == inventory && source_found == found && source_recent == recent,
        "starting inventories/discoveries/recent items differ; pair source-observed state first"
    );
    let observed_ram = &*ram;
    let mut travel = json!({
        "current_location": u16::from_be_bytes(ram[0x2cb52e..0x2cb530].try_into()?),
        "visited_locations": (0..2u16).flat_map(|world| (0..128u16).filter_map(move |i| {
            let at = session + 0x1df8 + usize::from(world) * 16 + usize::from(i / 32) * 4;
            (word(observed_ram, at).unwrap() & (1 << (i % 32)) != 0).then_some(world * 256 + i + 1)
        })).collect::<BTreeSet<_>>(),
        "visited_shops": (0..52u8).filter(|&i| word(ram, session + 0x1de8 + usize::from(i / 32) * 4).unwrap()
            & (1 << (i % 32)) != 0).collect::<BTreeSet<_>>(),
    });
    if travel["current_location"] == 0 {
        travel["current_location"] = Value::Null;
    }
    ensure!(
        party["travel"].is_null() || party["travel"] == travel,
        "starting travel histories differ; pair source-observed state first"
    );
    let observed_travel = travel.clone();
    let mut monsters: BTreeMap<u8, Value> = (0..251u8)
        .filter_map(|id| {
            let flags = ram[session + 0x10 + usize::from(id)];
            let variant = (ram[session + 0x110 + usize::from(id / 2)] >> (id % 2 * 4)) & 15;
            (flags & 1 != 0).then_some((
                id,
                json!({"scanned": flags & 2 != 0,
            "drops":[flags & 4 != 0, flags & 8 != 0], "steal":flags & 16 != 0,
            "location":flags & 32 != 0,"variant":variant}),
            ))
        })
        .collect();
    ensure!(
        party["monsters"].is_null() || party["monsters"] == serde_json::to_value(&monsters)?,
        "starting monster knowledge differs; pair source-observed state first"
    );
    let observed_monsters = monsters.clone();
    for &id in encounter_monster {
        ensure!(id < 251, "invalid encountered monster {id}");
        ram[session + 0x10 + usize::from(id)] |= 1;
        monsters.entry(id).or_insert_with(|| {
            json!({"scanned":false,"drops":[false,false],
            "steal":false,"location":false,"variant":0})
        });
    }
    for &id in catalogue_monster {
        ensure!(id < 251, "invalid catalogue monster {id}");
        ram[session + 0x10 + usize::from(id)] |= 63;
        let variant = (ram[session + 0x110 + usize::from(id / 2)] >> (id % 2 * 4)) & 15;
        monsters.insert(id, json!({"scanned":true,"drops":[true,true],"steal":true,"location":true,"variant":variant}));
    }
    for value in monster_variant {
        let (id, variant): (u8, u8) = pair(value, "expected monster ID:VARIANT")?;
        let record: resonance_content::monster::Monster =
            serde_json::from_slice(&fs::read(cooked.join(format!("monsters/{id:03}.json")))?)?;
        record.validate(items.len())?;
        ensure!(
            record.id == id && variant > 0 && usize::from(variant) < record.statistics.len(),
            "monster {id} has no variant {variant}"
        );
        let entry = monsters
            .get_mut(&id)
            .context("encounter or catalogue the monster first")?;
        entry["variant"] = json!(variant);
        let byte = &mut ram[session + 0x110 + usize::from(id / 2)];
        let shift = id % 2 * 4;
        *byte = (*byte & !(15 << shift)) | (variant << shift);
    }
    party["monsters"] = serde_json::to_value(monsters)?;
    let mut locations: BTreeSet<u16> = serde_json::from_value(travel["visited_locations"].clone())?;
    let mut shops: BTreeSet<u8> = serde_json::from_value(travel["visited_shops"].clone())?;
    for &id in visit_location {
        let location = &menu["world_map"]["locations"][id.to_string()];
        ensure!(location.is_object(), "unknown map location {id}");
        let id = location["visit_alias"].as_u64().map_or(id, |v| v as u16);
        locations.insert(id);
        let at =
            session + 0x1df8 + usize::from(id / 256) * 16 + usize::from((id % 256 - 1) / 32) * 4;
        let bits = word(ram, at)? | (1 << ((id % 256 - 1) % 32));
        ram[at..at + 4].copy_from_slice(&bits.to_be_bytes());
    }
    for &id in visit_shop {
        ensure!(
            usize::from(id)
                < menu["world_map"]["shops"]
                    .as_array()
                    .context("missing shops")?
                    .len(),
            "unknown map shop {id}"
        );
        for id in [
            Some(id),
            match id {
                17 => Some(50),
                50 => Some(17),
                18 => Some(40),
                40 => Some(18),
                29 => Some(45),
                45 => Some(29),
                _ => None,
            },
        ]
        .into_iter()
        .flatten()
        {
            shops.insert(id);
            let at = session + 0x1de8 + usize::from(id / 32) * 4;
            let bits = word(ram, at)? | (1 << (id % 32));
            ram[at..at + 4].copy_from_slice(&bits.to_be_bytes());
        }
    }
    travel["visited_locations"] = serde_json::to_value(locations)?;
    travel["visited_shops"] = serde_json::to_value(shops)?;
    party["travel"] = travel;
    for &id in grants {
        inventory.entry(id).or_insert(1);
        found.insert(id);
        recent.retain(|&old| old != id);
        recent.insert(0, id);
        recent.truncate(32);
        ram[session + 0xead + usize::from(id)] = inventory[&id];
    }
    if !item_count.is_empty() {
        let data: Value =
            serde_json::from_slice(&fs::read(cooked.join("game/session-data.json"))?)?;
        let mut changed = BTreeSet::new();
        for tag in item_count {
            let (id, count): (u16, u8) = pair(tag, "expected ID:COUNT")?;
            ensure!(
                id > 0 && usize::from(id) < items.len() && changed.insert(id),
                "invalid or duplicate item count: {tag}"
            );
            let limit = data["items"][usize::from(id)]["stack_limit"]
                .as_u64()
                .context("missing item stack limit")?;
            ensure!(
                u64::from(count) <= limit,
                "item count exceeds cooked stack limit: {tag}"
            );
            if count == 0 {
                inventory.remove(&id);
            } else if inventory.insert(id, count).is_none() {
                found.insert(id);
                recent.retain(|&old| old != id);
                recent.insert(0, id);
                recent.truncate(32);
            }
            ram[session + 0xead + usize::from(id)] = count;
        }
    }
    if *discover_all {
        found.extend(
            items
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(_, item)| item["category"].as_u64().is_some_and(|c| c != 0))
                .map(|(id, _)| id as u16),
        );
    }
    for &id in &found {
        let at = session + 0x1124 + usize::from(id / 32) * 4;
        let bits = word(ram, at)? | (1 << (id % 32));
        ram[at..at + 4].copy_from_slice(&bits.to_be_bytes());
    }
    for i in 0..32 {
        let at = session + 0x10e4 + i * 2;
        ram[at..at + 2].copy_from_slice(&recent.get(i).copied().unwrap_or(0).to_be_bytes());
    }
    party["items"] = serde_json::to_value(inventory)?;
    party["found_items"] = serde_json::to_value(found)?;
    party["recent_items"] = serde_json::to_value(recent)?;
    if !learn_ex_compound.is_empty() || !new_ex_compound.is_empty() {
        let ex: resonance_content::menu_data::ExSkillData =
            serde_json::from_value(menu["ex_skills"].clone())?;
        ex.validate(items.len())?;
        for (tag, recent) in learn_ex_compound
            .iter()
            .map(|s| (s, false))
            .chain(new_ex_compound.iter().map(|s| (s, true)))
        {
            let (character, recipe): (usize, u8) = pair(tag, "expected CHARACTER:RECIPE")?;
            ensure!(
                ex.characters
                    .get(character)
                    .is_some_and(|c| usize::from(recipe) < c.compounds.len()),
                "unknown EX compound {tag}"
            );
            let member = &mut party["members"][character];
            for (key, offset) in [
                ("compound_ex_skills", 0x110),
                ("recent_compound_ex_skills", 0x114),
            ] {
                if offset == 0x114 && !recent {
                    continue;
                }
                let mut known: BTreeSet<u8> =
                    serde_json::from_value(member.get(key).cloned().unwrap_or(json!([])))?;
                let at = session + 0x2b8 + character * 0x118 + offset;
                let bits = word(ram, at)?;
                ensure!(
                    (0..24u8)
                        .filter(|i| bits & (1 << i) != 0)
                        .collect::<BTreeSet<_>>()
                        == known,
                    "starting EX knowledge differs: character {character}, {key}"
                );
                ram[at..at + 4].copy_from_slice(&(bits | (1 << recipe)).to_be_bytes());
                known.insert(recipe);
                member[key] = serde_json::to_value(known)?;
            }
        }
    }
    if !collect_figurine.is_empty() {
        let mut owned: BTreeSet<u16> =
            serde_json::from_value(party.get("figurines").cloned().unwrap_or(json!([])))?;
        for &id in collect_figurine {
            ensure!(
                usize::from(id) < resonance_content::figurine::FIGURINE_COUNT,
                "unknown figurine {id}"
            );
            let at = session + 0x1e98 + usize::from(id / 8);
            let bit = 1 << (id % 8);
            ensure!(
                (ram[at] & bit != 0) == owned.contains(&id),
                "starting figurine collection differs: {id}"
            );
            ram[at] |= bit;
            owned.insert(id);
        }
        party["figurines"] = serde_json::to_value(owned)?;
    }
    if !learn_manual_topic.is_empty() {
        let manual: resonance_content::menu_data::TrainingManual =
            serde_json::from_value(menu["manual"].clone())?;
        manual.validate()?;
        let known: BTreeSet<_> = manual
            .chapters
            .iter()
            .flat_map(|c| &c.topics)
            .map(|t| t.learned_flag)
            .collect();
        let mut flags: BTreeSet<u16> =
            serde_json::from_value(save["state"]["progress"]["event_flags"].clone())?;
        for &id in learn_manual_topic {
            ensure!(known.contains(&id), "unknown manual topic flag {id}");
            let at = session + 0xc9d + usize::from(id / 8);
            let bit = 1 << (id % 8);
            ensure!(
                (ram[at] & bit != 0) == flags.contains(&id),
                "starting manual topic knowledge differs: {id}"
            );
            ram[at] |= bit;
            flags.insert(id);
        }
        save["state"]["progress"]["event_flags"] = serde_json::to_value(flags)?;
    }
    if !record_synopsis.is_empty() {
        const EPOCH: u64 = 946_684_800;
        let frequency = u64::from(word(ram, 0xf8)? / 4);
        ensure!(frequency != 0, "invalid source calendar clock");
        let tick = save["state"]["progress"]["tick"].clone();
        let records = &mut save["state"]["progress"]["event_records"];
        for tag in record_synopsis {
            let [id, value, level, seconds] = fields(
                tag,
                "expected ID:VALUE:LEVEL:UNIX_SECONDS",
                str::parse::<u64>,
            )?;
            ensure!(
                id < 200 && value <= 3 && (1..=250).contains(&level),
                "invalid synopsis record"
            );
            let ticks = seconds
                .checked_sub(EPOCH)
                .and_then(|s| s.checked_mul(frequency))
                .context("synopsis date is outside the source calendar")?;
            let at = session + 0x1168 + id as usize * 16;
            let old = &records[id.to_string()];
            let source_ticks = u64::from(word(ram, at + 8)?) << 32 | u64::from(word(ram, at + 12)?);
            ensure!(
                if old.is_null() {
                    ram[at] == 0
                } else {
                    old["value"] == ram[at]
                        && old["extra"] == ram[at + 1]
                        && old["level"] == ram[at + 2]
                        && old["recorded_at"] == EPOCH + source_ticks / frequency
                },
                "starting synopsis record differs: {id}"
            );
            ram[at] = value as u8;
            ram[at + 2] = level as u8;
            ram[at + 8..at + 16].copy_from_slice(&ticks.to_be_bytes());
            records[id.to_string()] = json!({"value":value,"extra":ram[at + 1],
                "level":level,"recorded_at":seconds,"tick":tick});
        }
    }
    let mut encoded = vec![0; raw.len() + raw.len() / 255 + 16];
    let length = unsafe {
        encode(
            raw.as_ptr(),
            encoded.as_mut_ptr(),
            raw.len().try_into()?,
            encoded.len().try_into()?,
        )
    };
    ensure!(length > 0, "state compression failed");
    encoded.truncate(length as usize);
    let mut state = data[..header + 16].to_vec();
    state.extend_from_slice(&(length as u32).to_le_bytes());
    state.extend(encoded);
    let prefix = fs::read(PathBuf::from(format!("{}.dtm", source.display())))?;
    let native_bytes = serde_json::to_vec_pretty(&save)?;
    let hash = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let report = json!({"kind":"controlled_inventory_fixture","story_origin":story_change,"formation":formation,"member_vitals":member_vitals,"learned_techniques":learn_technique,"granted_items":grants,"discover_all":discover_all,
        "observed_travel":observed_travel,"visited_locations":visit_location,"visited_shops":visit_shop,
        "observed_monsters":observed_monsters,"catalogued_monsters":catalogue_monster,
        "encountered_monsters":encounter_monster,"monster_variants":monster_variant,
        "learned_manual_topics":learn_manual_topic,"voice_volume":voice_volume,
        "learned_recipes":learn_recipe,"viewed_skits":view_skit,"synopsis_records":record_synopsis,
        "window_style":window_style,"window_background":window_background,
        "collected_figurines":collect_figurine,
        "item_counts":item_count,
        "learned_ex_compounds":learn_ex_compound,"new_ex_compounds":new_ex_compound,
        "source_state":{"path":source,"sha256":hash(&data)},"source_native":{"path":native,"sha256":hash(&original_native)},
        "dolphin_state_sha256":hash(&state),"native_save_sha256":hash(&native_bytes),"prefix_sha256":hash(&prefix)});
    fs::create_dir_all(output)?;
    fs::write(output.join("GQSEAF.s01"), state)?;
    fs::write(output.join("GQSEAF.s01.dtm"), prefix)?;
    fs::write(output.join("native.json"), native_bytes)?;
    fs::write(
        output.join("fixture.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
