//! Native Unison rows directly into one named, editable battle document.
use crate::{dol, rel::Rel, text};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_action::{ActionHit, ActionPose, ContactAttachments, HitElement, MeleeContact, Power},
    battle_normal::Event,
    battle_projectile::{DamageKind, HitShape},
    battle_rules::Unison,
    battle_unison::{Catalogue, Combination, Host, Opener, Placement, Prelude},
    battle_voice::Sound,
    library::Mappings,
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::PartyCharacterId;

fn float(bytes: &[u8], at: usize) -> Result<f32> {
    let value = f32::from_bits(dol::u32(bytes, at)?);
    ensure!(value.is_finite(), "non-finite Unison value");
    Ok(value)
}

fn vector(bytes: &[u8], at: usize) -> Result<[f32; 3]> {
    Ok([
        float(bytes, at)?,
        float(bytes, at + 4)?,
        float(bytes, at + 8)?,
    ])
}

fn host(module: &Rel) -> Result<Host> {
    let bytes = module.slice(4, 0, 0xf78)?;
    let zero = float(bytes, 0xf14)?;
    let host = Host {
        placement: [
            vector(bytes, 0xec8)?,
            vector(bytes, 0xed4)?,
            vector(bytes, 0xee0)?,
            vector(bytes, 0xeec)?,
        ],
        minimum_distance: float(bytes, 0xf74)?,
        short_weapon_penalty: float(bytes, 0xf70)?,
        restore_height: float(bytes, 0xf2c)?,
        windup_rate: float(bytes, 0xf44)?,
        windup_color: bytes[0xe60..0xe63].try_into()?,
        prelude: Prelude {
            first_x: float(bytes, 0xf3c)?,
            spacing: float(bytes, 0xf40)?,
            camera_eye: [zero, float(bytes, 0xf30)?, float(bytes, 0xf34)?],
            camera_target: [zero, float(bytes, 0xf38)?, zero],
            color: bytes[0xef8..0xefb].try_into()?,
        },
    };
    host.validate()?;
    Ok(host)
}

fn opening_hit(rules: &[u8], contact: &[u8]) -> Result<ActionHit> {
    ensure!(
        contact[18] == 0 && contact[17] == 0 && dol::u32(rules, 8)? == 0,
        "unsupported Unison opening hit"
    );
    let flags = dol::u16(rules, 0)?;
    ensure!(flags & !0x83ff == 0, "unsupported Unison opening hit flags");
    let power = match rules[13] {
        0 | 2 => Power::Normal,
        1 => Power::Percent(dol::u16(rules, 14)?),
        mode => anyhow::bail!("unsupported opening power mode {mode}"),
    };
    let element = match rules[2] {
        0 => HitElement::Inherited,
        10 => HitElement::Neutral,
        other => anyhow::bail!("unsupported Unison opening element {other}"),
    };
    ensure!(
        rules[12] == 0 && rules[22..28] == [0; 6],
        "unsupported opening condition/effect"
    );
    let cue = dol::u16(rules, 16)?;
    Ok(ActionHit {
        kind: DamageKind::Slash,
        effect: None,
        effect_on_guard: flags & 0x800 != 0,
        suppress_guard_sound: false,
        capture: flags & 0x10 != 0,
        sound: (cue != 0).then_some(cue),
        power,
        element,
        arte: flags & 0x20 != 0,
        prevents_defeat: flags & 1 != 0,
        finisher_grade: flags & 0x200 != 0,
        hp_drain: flags & 0x400 != 0,
        suppress_reaction: false,
        force_reaction: false,
        alternate_motion: flags & 2 != 0,
        knock_down: flags & 4 != 0,
        launch: flags & 8 != 0,
        hits_down: flags & 0x40 != 0,
        breaks_guard: flags & 0x80 != 0,
        unbreakable_guard: false,
        lift_guard: flags & 0x100 != 0,
        guard_pressure: rules[7],
        bypass_guard: contact[19] == 0,
        hitstun: rules[3],
        cooldown: rules[4],
        stun_chance: rules[5],
        stagger: rules[6],
        armor_damage: rules[20],
        recoil: contact[20],
        recoil_delay: rules[21],
        condition: None,
    })
}

fn opening_contact(module: &Rel, offset: usize) -> Result<MeleeContact> {
    let row = module.slice(5, offset, 34)?;
    let rules = module.slice(5, 0x978, 28)?;
    ensure!(
        dol::u16(row, 32)? == u16::MAX && dol::u16(row, 22)? == 0,
        "unsupported Unison opening contact list"
    );
    let count = usize::from(row[3]);
    ensure!(
        (1..=4).contains(&count) && row[16] == 0 && row[19] <= 1,
        "unsupported Unison opening contact"
    );
    Ok(MeleeContact {
        duration: u16::from(row[2]) + 1,
        attachments: ContactAttachments::Groups(row[4..4 + count].to_vec()),
        shape: HitShape::Box,
        radius: float(row, 8)?,
        height: float(row, 12)?,
        hit: opening_hit(rules, row)?,
    })
}

fn openers(module: &Rel) -> Result<Vec<Opener>> {
    let pose = module.slice(5, 0x994, 12)?;
    ensure!(
        dol::u16(pose, 0)? == 0 && pose[6] == 8 && pose[7] == 255,
        "unsupported Unison opening pose"
    );
    let rate = float(pose, 8)?;
    let pose = ActionPose {
        clip: u16::from(pose[2]),
        start: f32::from(pose[4]) * rate.abs(),
        end: (pose[5] != 0).then_some(f32::from(pose[5]) * rate.abs()),
        rate,
        blend: pose[3],
        repeat: pose[6] & 0x40 != 0,
    };
    let elapsed = |tick: u16| -> Result<u16> {
        tick.checked_add(if tick == 0 {
            0
        } else {
            u16::from(pose.blend.saturating_sub(1))
        })
        .context("Unison opening timeline overflow")
    };
    let mut events = vec![(0, Event::Pose(pose))];
    let commands = module.tail(5, 0xa2c)?;
    let mut at = 0;
    loop {
        let tick = dol::u16(commands, at)? as i16;
        if tick == -1 {
            break;
        }
        ensure!(tick >= 0, "invalid Unison command time");
        let (event, size) = match dol::u16(commands, at + 2)? {
            0 => (
                Event::Forward(f32::from(dol::u16(commands, at + 4)? as i16) / 10.),
                6,
            ),
            13 => (
                Event::Trail {
                    slot: dol::u16(commands, at + 4)?.try_into()?,
                    duration: dol::u16(commands, at + 6)?,
                },
                8,
            ),
            code => anyhow::bail!("unsupported Unison opening command {code}"),
        };
        events.push((elapsed(tick as u16)?, event));
        at += size;
    }
    let config = module.slice(5, 0x960, 4)?;
    let duration = dol::u16(config, 0)?;
    let recovery = dol::u16(config, 2)?;
    let contact_events = |offset| -> Result<Vec<_>> {
        let row = module.slice(5, offset, 2)?;
        let mut result = events.clone();
        result.push((
            elapsed(dol::u16(row, 0)?)?,
            Event::Contact(opening_contact(module, offset)?),
        ));
        result.sort_by_key(|(tick, _)| *tick);
        Ok(result)
    };
    let common = Opener {
        duration,
        recovery,
        contact_ticks: duration,
        events: contact_events(0x9ac)?,
    };
    let mut openers = vec![common; 9];
    openers[2].events = contact_events(0x9ec)?;
    for index in [2, 3] {
        openers[index].contact_ticks = 45;
    }
    Ok(openers)
}

fn action_names() -> Result<(Mappings, Mappings)> {
    let physical: BTreeMap<u16, String> =
        serde_json::from_str(include_str!("technique-names.json"))?;
    let techniques = Mappings::new(
        physical
            .iter()
            .map(|(&id, name)| (u32::from(id), name.clone()))
            .collect(),
    )?;
    // The Unison action ID space reuses the same names as the technique table.
    // Sparse numeric ranges translate between the two native ID spaces.
    const RANGES: &[(u16, u16, i16)] = &[
        (1, 42, 0),
        (43, 44, 180),
        (45, 55, 0),
        (62, 73, 63),
        (77, 78, 63),
        (80, 80, 63),
        (81, 127, 64),
        (131, 137, 64),
        (138, 139, -95),
        (140, 140, 88),
        (200, 262, -138),
        (263, 263, -41),
        (264, 266, -39),
        (267, 272, -211),
        (273, 275, -81),
        (276, 277, -139),
        (278, 281, -32),
        (282, 282, -143),
        (283, 283, -33),
        (284, 286, -49),
        (287, 289, -48),
        (290, 290, -52),
        (291, 291, -49),
        (292, 292, -48),
        (293, 293, -50),
        (295, 295, -153),
        (296, 296, -45),
    ];
    let mut groups = BTreeMap::new();
    for &(first, last, shift) in RANGES {
        for id in first..=last {
            let physical_id = u16::try_from(i32::from(id) + i32::from(shift))?;
            groups.insert(
                u32::from(id),
                physical
                    .get(&physical_id)
                    .with_context(|| format!("missing technique name {physical_id}"))?
                    .clone(),
            );
        }
    }
    Ok((techniques, Mappings::new(groups)?))
}

fn combinations(exe: &[u8]) -> Result<Vec<Combination>> {
    let table = dol::slice(exe, 0x802086c8, 19 * 64)?;
    table
        .chunks_exact(64)
        .enumerate()
        .map(|(index, row)| {
            let participants = usize::from(dol::u16(row, 6)?);
            ensure!(
                (2..=4).contains(&participants) && row[0x3a..0x3c] == [0; 2],
                "invalid Unison combination row"
            );
            let mut recipes = Vec::new();
            for source in row[8..56].chunks_exact(8) {
                if source.iter().all(|&byte| byte == 0) {
                    continue;
                }
                ensure!(
                    source[participants * 2..].iter().all(|&byte| byte == 0),
                    "Unison recipe exceeds participant count"
                );
                recipes.push(
                    (0..participants)
                        .map(|i| dol::u16(source, i * 2))
                        .collect::<Result<Vec<_>>>()?,
                );
            }
            Ok(Combination {
                id: (index + 1) as u8,
                name: text::string(exe, dol::u32(row, 0)?)?,
                native_id: dol::u16(row, 4)?,
                participants: participants as u8,
                recipes,
                duration: dol::u16(row, 0x38)?,
                camera_pitch: float(row, 0x3c)?,
            })
        })
        .collect()
}

fn scene_name(combination: &Combination) -> String {
    let slug = if combination.native_id == 303 {
        "mirage-thrust-and-power-thrust".into()
    } else {
        combination
            .name
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    };
    format!("{slug}-{:04}", combination.native_id)
}

fn convert(root: &Path) -> Result<Unison> {
    let disc = root.join("disc1");
    let exe = fs::read(disc.join("sys/main.dol"))?;
    let module = Rel::read(fs::read(disc.join("files/US_r_Top2Btl.rel"))?)?;
    let placements = dol::slice(&exe, 0x80202f90, 253 * 88)?
        .chunks_exact(88)
        .enumerate()
        .skip(1)
        .map(|(id, row)| {
            Ok((
                id as u16,
                Placement {
                    native_id: dol::u16(row, 0)?,
                    altitude: dol::u32(row, 0x48)?,
                    distance: dol::u16(row, 0x4c)?,
                    duration: dol::u16(row, 0x4e)?,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let voices: [Option<Sound>; 9] = (0..9)
        .map(|index| {
            let encoded = dol::u16(module.slice(4, 0xefc + index * 2, 2)?, 0)?;
            Ok(match encoded {
                0 => None,
                value if value & 0x8000 != 0 => Some(Sound::Stream(value & 0x7fff)),
                value => Some(Sound::Cue(
                    value.checked_add(501).context("Unison cue overflow")?,
                )),
            })
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected nine Unison voices"))?;
    let catalogue = Catalogue {
        host: host(&module)?,
        voices,
        openers: openers(&module)?,
        combinations: combinations(&exe)?,
        placements,
    };
    catalogue.validate()?;
    let (techniques, groups) = action_names()?;
    let characters = Mappings::new(
        PartyCharacterId::RESOURCE_NAMES
            .iter()
            .enumerate()
            .map(|(index, name)| ((index + 1) as u32, (*name).to_owned()))
            .collect(),
    )?;
    let scenes = Mappings::new(
        catalogue
            .combinations
            .iter()
            .map(|row| (u32::from(row.native_id), scene_name(row)))
            .collect(),
    )?;
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
    Unison::from_catalogue(
        &catalogue,
        &techniques,
        &characters,
        &scenes,
        groups,
        &openers,
    )
}

pub fn cook_battle_unison(root: &Path, library: &Path) -> Result<()> {
    let document = convert(root)?;
    let output = library.join("battle/unison.json");
    crate::output::json(&output, &document)?;
    Ok(())
}
