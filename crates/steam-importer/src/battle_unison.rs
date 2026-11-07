//! Native team-attack tables become the same named Unison document as GC.
use crate::{
    audio_effects,
    battle_normal::canonical,
    battle_voices, output, pe,
    source::Source,
    techniques,
    text::{Bank, Menu},
};
use anyhow::{Result, ensure};
use resonance_content::{
    battle_normal::Event,
    battle_rules::Unison,
    battle_unison::{self, Catalogue, Combination, Host, Placement, Prelude},
    library::Mappings,
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::BattleSceneId;
mod extra_lower;
mod extra_upper;
mod innocent;

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let text = source.require("TOS_TEXT.TXB")?;
    let menu = Menu {
        image: pe::Image::parse(&source.executable)?,
        bank: Bank::parse(&text)?,
        slot: source.language.slot(),
    };
    let techniques: resonance_content::arte::document::Document =
        serde_json::from_slice(&fs::read(library.join(resonance_content::arte::PATH))?)?;
    let characters: Mappings = serde_json::from_slice(&fs::read(
        library.join(resonance_content::battle_model::document::PARTY),
    )?)?;
    let mut combinations = combinations(&menu)?;
    for row in &mut combinations {
        row.execution = match innocent::read(source, &menu.image, row.native_id)? {
            Some(execution) => Some(execution),
            None => match extra_lower::read(source, &menu.image, row)? {
                Some(execution) => Some(execution),
                None => extra_upper::read(source, &menu.image, row.native_id)?,
            },
        };
    }
    let mut placements = BTreeMap::new();
    let mut groups = BTreeMap::new();
    for (index, row) in menu
        .image
        .slice(0x8d84b0, 255 * 96)?
        .as_chunks::<96>()
        .0
        .iter()
        .enumerate()
        .skip(1)
    {
        let id = techniques::logical_id(index as u32);
        let native_id = pe::half(row, 0)?;
        groups
            .entry(u32::from(native_id))
            .or_insert(techniques.mappings.target(id)?.to_owned());
        placements.insert(
            id.try_into()?,
            Placement {
                native_id,
                altitude: pe::word(row, 0x50)?,
                distance: pe::half(row, 0x54)?,
                duration: pe::half(row, 0x56)?,
            },
        );
    }
    for id in combinations
        .iter()
        .flat_map(|row| row.recipes.iter().flatten())
        .copied()
    {
        groups
            .entry(u32::from(id))
            .or_insert_with(|| format!("action-{id}"));
    }
    let scenes = scene_mappings(source, &menu.image, &groups, &combinations)?;
    let (host, voices) = host(&menu.image)?;
    let catalogue = Catalogue {
        host,
        voices,
        openers: openers(&menu.image)?,
        combinations,
        placements,
    };
    let openers = Mappings::new(
        (1..=9)
            .map(|id| {
                (
                    id,
                    match id {
                        3 => "genis-strike",
                        4 => "raine-strike",
                        _ => "weapon-strike",
                    }
                    .into(),
                )
            })
            .collect(),
    )?;
    let document = Unison::from_catalogue(
        &catalogue,
        &techniques.mappings,
        &characters,
        &scenes,
        Mappings::new(groups)?,
        &openers,
    )?;
    document.prepare(&techniques.mappings, &characters, &scenes)?;
    output::json(
        &library.join(resonance_content::battle_scene::document::TECHNIQUES),
        &scenes,
    )?;
    output::json(&library.join(battle_unison::PATH), &document)
}

fn combinations(menu: &Menu<'_>) -> Result<Vec<Combination>> {
    let image = &menu.image;
    ensure!(
        image.slice(0x472e57, 1)? == [0xb8]
            && image.slice(0x472f8f, 1)? == [0x3d]
            && image.slice(0x472f89, 3)? == [0x83, 0xc0, 0x40]
            && image.slice(0x472f7d, 3)? == [0x83, 0xf9, 0x18],
        "Unison recipe consumer changed"
    );
    let start = pe::word(image.slice(0x472e58, 4)?, 0)?;
    let end = pe::word(image.slice(0x472f90, 4)?, 0)?;
    ensure!(
        start == 0x8de490 && end == 0x8debd0,
        "Unison recipe bounds changed"
    );
    let fields = [(0, 4), (60, 4)]
        .into_iter()
        .chain((4..60).step_by(2).map(|at| (at, 2)))
        .collect::<Vec<_>>();
    let table = canonical(image.slice(start, (end - start) as usize)?, 64, &fields)?;
    let captions = menu.directory(0x8e3e9c)?;
    resonance_importer::decode_unison_combinations(&table, |index| menu.caption(captions, index))
}

fn scene_mappings(
    source: &Source,
    image: &pe::Image<'_>,
    groups: &BTreeMap<u32, String>,
    combinations: &[Combination],
) -> Result<Mappings> {
    let mut names = BTreeMap::new();
    // Publish physical package identities separately from their later artwork owner.
    for id in (201..=298u16).chain(combinations.iter().map(|row| row.native_id)) {
        let pointer = pe::word(image.slice(0x8e98a8 + u32::from(id) * 4, 4)?, 0)?;
        if pointer == 0 {
            continue;
        }
        let filename = image.text(pointer)?;
        ensure!(
            source.contains(filename)?,
            "missing scene package {filename}"
        );
        let name = BattleSceneId::try_from(id)
            .ok()
            .and_then(BattleSceneId::resource_name)
            .map(str::to_owned)
            .unwrap_or_else(|| match groups.get(&u32::from(id)) {
                Some(name) => format!("{name}-{id:04}"),
                None => format!("combination-{id:04}"),
            });
        names.insert(u32::from(id), name);
    }
    Mappings::new(names)
}

fn openers(image: &pe::Image<'_>) -> Result<Vec<battle_unison::Opener>> {
    for (address, expected) in [
        (0x46fe93, 0x8e3eb4),
        (0x46fe9d, 0x8e3ecc),
        (0x46fe89, 0x8e3ee8),
        (0x46fead, 0x8e3f00),
        (0x46fec3, 0x8e3f80),
    ] {
        ensure!(
            pe::word(image.slice(address, 4)?, 0)? == expected,
            "Unison opening records moved"
        );
    }
    let config = canonical(image.slice(0x8e3eb4, 24)?, 2, &[(0, 2)])?;
    let rules = canonical(
        image.slice(0x8e3ecc, 28)?,
        28,
        &[(0, 2), (8, 4), (14, 2), (16, 2)],
    )?;
    let pose = canonical(image.slice(0x8e3ee8, 24)?, 12, &[(0, 2), (8, 4)])?;
    let contact = |address| {
        canonical(
            image.slice(address, 64)?,
            32,
            &[(0, 2), (8, 4), (12, 4), (22, 2), (28, 4)],
        )
    };
    let contacts = [contact(0x8e3f00)?, contact(0x8e3f40)?];
    let commands = canonical(image.slice(0x8e3f80, 24)?, 2, &[(0, 2)])?;
    ensure!(
        image.slice(0x471263, 3)? == [0xc7, 0x45, 0xf0]
            && image.slice(0x4712cd, 8)? == [0x0f, 0xb6, 0x4c, 0x3d, 0xef, 0x98, 0x83, 0xc1],
        "Unison contact clock changed"
    );
    let mut extra = [0; 9];
    extra[..4].copy_from_slice(image.slice(0x471266, 4)?);
    let base = u16::from(image.slice(0x4712d5, 1)?[0]);
    let mut openers =
        resonance_importer::decode_unison_openers(resonance_importer::UnisonOpenerSource {
            config: &config,
            rules: &rules,
            pose: &pose,
            contacts: [&contacts[0], &contacts[1]],
            commands: &commands,
            contact_ticks: extra.map(|extra| base + u16::from(extra)),
        })?;
    for opener in &mut openers {
        for (_, event) in &mut opener.events {
            if let Event::Contact(contact) = event {
                contact.hit.sound = contact.hit.sound.map(audio_effects::id).transpose()?;
            }
        }
    }
    Ok(openers)
}

fn host(
    image: &pe::Image<'_>,
) -> Result<(Host, [Option<resonance_content::battle_voice::Sound>; 9])> {
    let scalar =
        |address| -> Result<f32> { Ok(f32::from_bits(pe::word(image.slice(address, 4)?, 0)?)) };
    let double = |address| -> Result<f32> {
        Ok(f64::from_le_bytes(image.slice(address, 8)?.try_into()?) as f32)
    };
    let negative = scalar(0x882a00)?;
    ensure!(negative == -1., "Unison placement axis changed");
    let voices = (0..9u32)
        .map(|index| {
            let row = image.slice(0x47300c + index * 8, 8)?;
            ensure!(
                row[..4] == [0xc7, 0x44, 0x24, 0x50 + index as u8 * 4],
                "Unison voice initializer changed"
            );
            battle_voices::sound(pe::word(row, 4)?)
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected nine Unison voices"))?;
    let host = Host {
        placement: [
            [0., 0., negative],
            [negative, 0., 0.],
            [1., 0., 0.],
            [0., 0., 1.],
        ],
        minimum_distance: scalar(0x882dc8)?,
        short_weapon_penalty: double(0x882dd8)?,
        restore_height: scalar(0x882e7c)?,
        windup_rate: scalar(0x72f3b8)?,
        windup_color: [image.slice(0x46fdd2, 1)?[0]; 3],
        prelude: Prelude {
            // Normalize the port's mirrored presentation to the GC layout.
            first_x: -double(0x883ef8)?,
            spacing: double(0x882d60)?,
            camera_eye: [0., scalar(0x8830a0)?, scalar(0x882f5c)?],
            camera_target: [0., scalar(0x8835c0)?, 0.],
            color: image.slice(0x473008, 3)?.try_into()?,
        },
    };
    host.validate()?;
    Ok((host, voices))
}
