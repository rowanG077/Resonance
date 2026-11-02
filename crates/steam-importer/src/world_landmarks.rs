//! Travel locations are the single shared source for maps, shops and guideposts.
use crate::{
    output, pe,
    source::{Language, Source},
    text,
};
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::{
    EntryOverride, EntryPlacement, Guidepost, Interaction, Landmark, Landmarks, Marker, Mount,
    PartyCharacter, ShipCameraClearance, ShopVariant,
};
use std::{collections::BTreeMap, num::NonZeroU16, path::Path};

pub(crate) fn cook(source: &Source, output: &Path) -> Result<()> {
    let bytes = source.require("TOS_TEXT.TXB")?;
    let landmarks = read(
        &pe::Image::parse(&source.executable)?,
        &text::Bank::parse(&bytes)?,
        source.language,
    )?;
    output::json(&output.join("world/landmarks.json"), &landmarks)
}
fn landmark(landmarks: &mut Landmarks, id: u16) -> Result<&mut Landmark> {
    landmarks
        .worlds
        .iter_mut()
        .flatten()
        .find(|entry| entry.id == id)
        .with_context(|| format!("missing landmark {id}"))
}
fn float(image: &pe::Image<'_>, at: u32) -> Result<f32> {
    Ok(f32::from_bits(pe::word(image.slice(at, 4)?, 0)?))
}
fn double(image: &pe::Image<'_>, at: u32) -> Result<f32> {
    Ok(f64::from_le_bytes(image.slice(at, 8)?.try_into()?) as f32)
}
fn shop_list(image: &pe::Image<'_>, at: u32) -> Result<Vec<u8>> {
    if at == 0 {
        return Ok(Vec::new());
    }
    let count = usize::from(image.slice(at, 1)?[0]);
    ensure!(count <= 52, "invalid shop-list length");
    let values = image.slice(at + 1, count)?.to_vec();
    ensure!(values.iter().all(|&id| id < 52), "invalid shop ID");
    Ok(values)
}

fn read(image: &pe::Image<'_>, bank: &text::Bank<'_>, language: Language) -> Result<Landmarks> {
    let slot = language.slot();
    let word = |at| pe::word(image.slice(at, 4)?, 0);
    let caption =
        |directory, index| bank.menu(word(directory + index * 4)?, encoding_rs::WINDOWS_1252);
    let names = word(0x8b94fc + slot * 4)?;
    let mut landmarks = Landmarks {
        worlds: Default::default(),
    };
    for (world, address, count) in [(0, 0x8b7700, 100), (1, 0x8b7ed0, 83)] {
        let rows = image.slice(address, count * 20)?;
        ensure!(
            pe::word(rows, (count - 1) * 20)? == u32::MAX,
            "missing Steam landmark terminator"
        );
        for (index, row) in rows.chunks_exact(20).enumerate().skip(1).take(count - 2) {
            let name_id = pe::word(row, 16)?;
            let mut name = caption(names, name_id)?;
            // Several unlisted locations have blank Western translations.
            // Retain their shipped English name in the shared landmark entry.
            if name.is_empty() {
                name = caption(word(0x8b9500)?, name_id)?;
            }
            ensure!(
                !name.is_empty(),
                "Steam landmark {} has no name",
                world * 256 + index
            );
            let height = f32::from_bits(pe::word(row, 8)?);
            landmarks.worlds[world].push(Landmark {
                id: (world * 256 + index) as u16,
                position: [
                    pe::word(row, 0)? as i32 as f32,
                    pe::word(row, 4)? as i32 as f32,
                ],
                height: (height != 0.).then_some(if height == 0.01 { 0. } else { height }),
                radius: f32::from(pe::half(row, 12)?),
                // Steam moves the menu's listed bit to bit 0; contact at
                // 0x4b2dec shifts it out before selecting the handler.
                interaction: match row[14] >> 1 {
                    0 => Interaction::Disabled,
                    1 => Interaction::Active,
                    2 => Interaction::Blocked,
                    other => anyhow::bail!("unsupported Steam landmark interaction {other}"),
                },
                marker: match row[15] {
                    0 => Marker::None,
                    id @ 1..=17 => Marker::Model(id),
                    0xfe => Marker::FieldPoint,
                    0xff => Marker::Unmodeled,
                    other => anyhow::bail!("unsupported Steam landmark marker {other}"),
                },
                map_listed: row[14] & 1 != 0,
                visit_alias: matches!((world * 256 + index) as u16, 43 | 44).then_some(7),
                shops: if index < 11 {
                    shop_list(
                        image,
                        word(0x8b95a0 + (world as u32 * 11 + index as u32) * 4)?,
                    )?
                } else {
                    Vec::new()
                },
                shop_variants: Vec::new(),
                flight_floor: None,
                reward_item: None,
                required_character: None,
                guidepost: None,
                automatic: name.starts_with('*'),
                name,
                entry_overrides: Vec::new(),
                ship_camera_clearance: None,
            });
        }
    }
    for (id, item) in pairs(image, 0x8b8558)? {
        landmark(&mut landmarks, id)?.reward_item = Some(item);
    }
    for (id, member) in pairs(image, 0x8b85b0)? {
        landmark(&mut landmarks, id)?.required_character =
            Some(PartyCharacter::try_from(u8::try_from(member)?)?);
    }
    for (id, global, at_least, before, after) in [
        (7, 62, 500_000, 0x8b9534, 0x8b9538),
        (8, 19, 301, 0x8b953c, 0x8b9540),
        (262, 16, 0x014fc8f0, 0x8b955c, 0x8b9560),
    ] {
        let entry = landmark(&mut landmarks, id)?;
        entry.shops = shop_list(image, before)?;
        entry.shop_variants.push(ShopVariant {
            global,
            at_least,
            shops: shop_list(image, after)?,
        });
    }
    entries(image, &mut landmarks)?;
    // Exire's ship-camera clearance (0x4b4f15..0x4b4f5c).
    landmarks.worlds[1][9].ship_camera_clearance = Some(ShipCameraClearance {
        near_margin: double(image, 0x882a78)?,
        far_margin: double(image, 0x883318)?,
        height: double(image, 0x883310)?,
    });
    let names = word(0x8df978 + slot * 4)?;
    let rows = image.slice(0x8dfa00, 13 * 12)?;
    ensure!(
        rows[12 * 12..] == [0; 12],
        "missing Steam guidepost terminator"
    );
    for row in rows[..12 * 12].chunks_exact(12) {
        let flags = [pe::half(row, 6)?, pe::half(row, 8)?, pe::half(row, 10)?];
        let post = Guidepost {
            name: caption(names, pe::word(row, 0)?)?,
            discovery_flag: NonZeroU16::new(flags[0])
                .context("missing guidepost discovery flag")?,
            unlock_flags: flags[1..]
                .iter()
                .filter_map(|&flag| NonZeroU16::new(flag))
                .collect(),
        };
        post.validate()?;
        ensure!(
            landmark(&mut landmarks, pe::half(row, 4)?)?
                .guidepost
                .replace(post)
                .is_none(),
            "duplicate guidepost"
        );
    }
    landmarks.validate()?;
    Ok(landmarks)
}

fn pairs(image: &pe::Image<'_>, address: u32) -> Result<BTreeMap<u16, u16>> {
    let mut pairs = BTreeMap::new();
    for index in 0..64 {
        let row = image.slice(address + index * 4, 4)?;
        let location = pe::half(row, 0)?;
        if location == 0 {
            return Ok(pairs);
        }
        ensure!(
            pairs.insert(location, pe::half(row, 2)?).is_none(),
            "duplicate Steam landmark handler"
        );
    }
    anyhow::bail!("unterminated Steam landmark handlers")
}

fn entries(image: &pe::Image<'_>, landmarks: &mut Landmarks) -> Result<()> {
    // Field-return selection at 0x4b1660. Immediate coordinates and saved
    // angles use the same normalized map/heading units as ordinary transfers.
    let immediate = |at| -> Result<f32> { Ok(pe::word(image.slice(at, 4)?, 0)? as f32) };
    for (id, direction, x, y, heading, camera) in [
        (2, Some(6), 0x4b16b2, 0x4b16bc, 0x4b16cc, 0x4b16d6),
        (280, None, 0x4b1760, 0x4b176a, 0x4b177a, 0x4b1784),
    ] {
        landmarks.worlds[id / 256][id % 256 - 1]
            .entry_overrides
            .push(EntryOverride {
                direction,
                placement: EntryPlacement::Fixed {
                    position: [immediate(x)?, immediate(y)?, 0.],
                },
                mount: None,
                heading: Some(immediate(heading)? / double(image, 0x882cf8)?),
                camera_yaw: Some(f32::from(image.slice(camera, 1)?[0]) / double(image, 0x882cf0)?),
            });
    }
    landmarks.worlds[1][7].entry_overrides.push(EntryOverride {
        direction: None,
        placement: EntryPlacement::Perimeter {
            allow_disabled: true,
        },
        mount: Some(Mount::Ship),
        heading: None,
        camera_yaw: None,
    });
    for (index, height) in [(9, float(image, 0x8834b0)?.trunc() / 100.), (47, 0.)] {
        landmarks.worlds[1][index]
            .entry_overrides
            .push(EntryOverride {
                direction: None,
                placement: EntryPlacement::LandmarkCenter { height },
                mount: Some(Mount::Rheairds),
                heading: Some(0.),
                camera_yaw: Some(0.),
            });
    }
    Ok(())
}
