//! Native world locations and controller rules directly into editable landmarks.
use crate::{dol, rel::Rel, text};
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::{
    EntryOverride, EntryPlacement, FlightFloor, Guidepost, Interaction, Landmark, Landmarks,
    Marker, Mount, PartyCharacter, ShipCameraClearance, ShopVariant,
};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, num::NonZeroU16, path::Path};

const LOCATIONS: [(u32, usize); 2] = [(0x8026ae80, 100), (0x8026b650, 83)];
const WORLD_TABLES: u32 = 0x8035a1c8;
const SHOP_BINDINGS: u32 = 0x80227f00;
const REWARDS: u32 = 0x8026bccc;
const REQUIREMENTS: u32 = 0x8026bd24;

#[derive(Deserialize)]
struct EntryRecipes {
    provenance: String,
    entries: BTreeMap<u16, EntrySource>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntrySource {
    direction: Option<u8>,
    placement: EntryPlacement,
    mount: Option<Mount>,
    height_offset: Option<usize>,
    heading_offset: Option<usize>,
    camera_yaw_offset: Option<usize>,
}

fn float(bytes: &[u8], at: usize) -> Result<f32> {
    Ok(f32::from_bits(dol::u32(bytes, at)?))
}

fn rel_float(rel: &Rel, at: usize) -> Result<f32> {
    float(rel.slice(5, at, 4)?, 0)
}

fn list(exe: &[u8], address: u32) -> Result<Option<Vec<u8>>> {
    if address == 0 {
        return Ok(None);
    }
    let count = dol::slice(exe, address, 1)?[0] as usize;
    ensure!(count <= 52, "world shop list exceeds catalogue");
    let values = dol::slice(exe, address, count + 1)?[1..].to_vec();
    ensure!(values.iter().all(|&id| id < 52), "invalid world shop ID");
    Ok(Some(values))
}

fn landmark(landmarks: &mut Landmarks, id: u16) -> Result<&mut Landmark> {
    landmarks
        .worlds
        .iter_mut()
        .flatten()
        .find(|entry| entry.id == id)
        .with_context(|| format!("missing world landmark {id}"))
}

fn native_locations(exe: &[u8]) -> Result<Landmarks> {
    let mut worlds: [Vec<Landmark>; 2] = Default::default();
    for (world, entries) in worlds.iter_mut().enumerate() {
        let address = dol::u32(dol::slice(exe, WORLD_TABLES + world as u32 * 4, 4)?, 0)?;
        ensure!(
            address == LOCATIONS[world].0,
            "unknown world location table"
        );
        let rows: Vec<_> = dol::slice(exe, address, LOCATIONS[world].1 * 20)?
            .chunks_exact(20)
            .collect();
        ensure!(
            rows.last().is_some_and(|row| {
                [dol::u32(row, 0).ok(), dol::u32(row, 4).ok()].contains(&Some(u32::MAX))
            }),
            "missing world location terminator"
        );
        for (index, row) in rows.into_iter().enumerate().skip(1) {
            let x = dol::u32(row, 0)? as i32;
            let z = dol::u32(row, 4)? as i32;
            if x == -1 || z == -1 {
                break;
            }
            let id = (world * 256 + index) as u16;
            let name = text::string(exe, dol::u32(row, 16)?)?;
            let shop_address = if index < 11 {
                dol::u32(
                    dol::slice(exe, SHOP_BINDINGS + (world * 44 + index * 4) as u32, 4)?,
                    0,
                )?
            } else {
                0
            };
            let mut shops = list(exe, shop_address)?.unwrap_or_default();
            let mut shop_variants = Vec::new();
            let variant = match id {
                7 => Some((62, 500_000, 0x8035a190, 0x8035a194)),
                8 => Some((19, 301, 0x8035a198, 0x8035a19c)),
                262 => Some((16, 22_006_000, 0x8035a1b8, 0x8035a1bc)),
                _ => None,
            };
            if let Some((global, at_least, before, after)) = variant {
                shops = list(exe, before)?.context("missing before-stock list")?;
                shop_variants.push(ShopVariant {
                    global,
                    at_least,
                    shops: list(exe, after)?.context("missing after-stock list")?,
                });
            }
            let interaction = match row[14] & 0x7f {
                0 => Interaction::Disabled,
                1 => Interaction::Active,
                2 => Interaction::Blocked,
                value => anyhow::bail!("unknown world interaction {value}"),
            };
            let marker = match row[15] {
                0 => Marker::None,
                id @ 1..=17 => Marker::Model(id),
                0xfe => Marker::FieldPoint,
                0xff => Marker::Unmodeled,
                value => anyhow::bail!("unknown world marker {value}"),
            };
            let elevation = float(row, 8)?;
            entries.push(Landmark {
                id,
                name: name.clone(),
                position: [x as f32, z as f32],
                height: if elevation == 0. {
                    None
                } else if elevation == 0.01 {
                    Some(0.)
                } else {
                    Some(elevation)
                },
                radius: dol::u16(row, 12)? as f32,
                interaction,
                marker,
                map_listed: row[14] & 0x80 != 0,
                visit_alias: matches!(id, 43 | 44).then_some(7),
                shops,
                shop_variants,
                automatic: name.starts_with('*'),
                ship_camera_clearance: None,
                flight_floor: None,
                entry_overrides: Vec::new(),
                reward_item: None,
                required_character: None,
                guidepost: None,
            });
        }
    }
    let mut result = Landmarks { worlds };
    for row in dol::slice(exe, REWARDS, 22 * 4)?.chunks_exact(4) {
        let id = dol::u16(row, 0)?;
        if id == 0 {
            break;
        }
        ensure!(
            landmark(&mut result, id)?
                .reward_item
                .replace(dol::u16(row, 2)?)
                .is_none(),
            "duplicate world reward"
        );
    }
    for row in dol::slice(exe, REQUIREMENTS, 43 * 4)?.chunks_exact(4) {
        let id = dol::u16(row, 0)?;
        if id == 0 {
            break;
        }
        let character = PartyCharacter::try_from(u8::try_from(dol::u16(row, 2)?)?)?;
        ensure!(
            landmark(&mut result, id)?
                .required_character
                .replace(character)
                .is_none(),
            "duplicate world party requirement"
        );
    }
    Ok(result)
}

fn controller_rules(landmarks: &mut Landmarks, rel: &Rel) -> Result<()> {
    landmark(landmarks, 266)?.ship_camera_clearance = Some(ShipCameraClearance {
        near_margin: rel_float(rel, 0x954)?,
        far_margin: rel_float(rel, 0xb8c)?,
        height: f64::from_be_bytes(rel.slice(5, 0xb90, 8)?.try_into()?) as f32,
    });
    for (id, margin, height, active_only) in [
        (23, 0x960, 0xa94, true),
        (25, 0x960, 0xa94, true),
        (266, 0x954, 0xa8c, true),
        (274, 0x960, 0xa8c, false),
    ] {
        landmark(landmarks, id)?.flight_floor = Some(FlightFloor {
            margin: rel_float(rel, margin)?,
            height: rel_float(rel, height)?,
            active_only,
        });
    }
    let recipes: EntryRecipes = serde_json::from_str(include_str!("world-entries.json"))?;
    ensure!(
        !recipes.provenance.is_empty(),
        "missing world entry provenance"
    );
    let heading_scale = rel_float(rel, 0x958)?;
    let camera_scale = rel_float(rel, 0xa0c)?;
    ensure!(
        heading_scale > 0. && camera_scale > 0.,
        "invalid saved entry angle scales"
    );
    for (id, source) in recipes.entries {
        let mut placement = source.placement;
        if let Some(offset) = source.height_offset {
            let EntryPlacement::LandmarkCenter { height } = &mut placement else {
                anyhow::bail!("height constant needs landmark center placement");
            };
            *height = rel_float(rel, offset)?.trunc() / 100.;
        }
        let rule = EntryOverride {
            direction: source.direction,
            placement,
            mount: source.mount,
            heading: source
                .heading_offset
                .map(|at| -> Result<f32> { Ok(rel_float(rel, at)?.trunc() / heading_scale) })
                .transpose()?,
            camera_yaw: source
                .camera_yaw_offset
                .map(|at| -> Result<f32> { Ok(rel_float(rel, at)?.trunc() / camera_scale) })
                .transpose()?,
        };
        rule.validate()?;
        landmark(landmarks, id)?.entry_overrides.push(rule);
    }
    let guideposts = rel.slice(6, 0x8c, 13 * 12)?;
    ensure!(
        guideposts[12 * 12..] == [0; 12],
        "invalid guidepost terminator"
    );
    ensure!(
        rel.pointers
            .range((6, 0x8c)..(6, 0x8c + 13 * 12))
            .all(|(&(section, at), _)| section == 6
                && (at - 0x8c) % 12 == 0
                && at < 0x8c + 12 * 12),
        "unexpected guidepost relocation"
    );
    for (index, row) in guideposts[..12 * 12].chunks_exact(12).enumerate() {
        let id = dol::u16(row, 4)?;
        let flags = [dol::u16(row, 6)?, dol::u16(row, 8)?, dol::u16(row, 10)?];
        ensure!(
            flags[0] != 0 && flags.iter().all(|&flag| flag < 4096),
            "invalid guidepost flags"
        );
        let (section, at) = rel.pointer(6, 0x8c + index * 12)?;
        let post = Guidepost {
            name: rel.text(section, at)?,
            discovery_flag: NonZeroU16::new(flags[0]).unwrap(),
            unlock_flags: flags[1..]
                .iter()
                .filter_map(|&flag| NonZeroU16::new(flag))
                .collect(),
        };
        post.validate()?;
        ensure!(
            landmark(landmarks, id)?.guidepost.replace(post).is_none(),
            "duplicate world guidepost"
        );
    }
    Ok(())
}

fn convert(exe: &[u8], module: Vec<u8>) -> Result<Landmarks> {
    let mut landmarks = native_locations(exe)?;
    controller_rules(&mut landmarks, &Rel::read(module)?)?;
    let labels: BTreeMap<u16, String> =
        serde_json::from_str(include_str!("world-location-labels.json"))?;
    for entry in landmarks.worlds.iter_mut().flatten() {
        if let Some(name) = labels.get(&entry.id) {
            entry.name.clone_from(name);
        }
    }
    landmarks.validate()?;
    Ok(landmarks)
}

pub fn cook_world_landmarks(root: &Path, library: &Path) -> Result<()> {
    let output = library.join("world/landmarks.json");
    let extracted = root.join("disc1");
    let document = convert(
        &fs::read(extracted.join("sys/main.dol"))?,
        fs::read(extracted.join("files/US_r_Top2field.rel"))?,
    )?;
    crate::output::json(&output, &document)?;
    Ok(())
}
