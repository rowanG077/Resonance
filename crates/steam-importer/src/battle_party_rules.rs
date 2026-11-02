//! Party profiles and policies directly into the shared named rules document.
use crate::{audio_effects, battle_profiles, battle_voices, output, pe, recoil, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_action::{ActionPose, TargetPolicy},
    battle_profile as profile,
    battle_rules::{self, Party},
    library::Mappings,
};
use std::{collections::BTreeMap, fs, path::Path};

fn json<T: serde::de::DeserializeOwned>(library: &Path, path: &str) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(library.join(path))?)?)
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let table = read(&image, library)?;
    table.validate()?;
    let names: Mappings = json(library, "battle/party/mappings.json")?;
    output::json(
        &library.join(battle_rules::PARTY),
        &Party::from_table(
            table,
            battle_rules::NORMAL_ATTACKS.into(),
            battle_rules::MARTIAL_ATTACKS.into(),
            &names,
        )?,
    )
}

fn limits(image: &pe::Image<'_>, first: u32, second: u32, last: u32) -> Result<[u8; 9]> {
    let mut result = [0; 9];
    result[..4].copy_from_slice(image.slice(first, 4)?);
    result[4..8].copy_from_slice(image.slice(second, 4)?);
    result[8] = image.slice(last, 1)?[0];
    Ok(result)
}

fn chant(image: &pe::Image<'_>) -> Result<Vec<(u16, ActionPose)>> {
    let rows = image.slice(0x8e9dc8, 48)?;
    ensure!(
        pe::half(rows, 36)? == 0xfffe && rows[38..].iter().all(|&v| v == 0),
        "invalid chant termination"
    );
    rows[..36]
        .as_chunks::<12>()
        .0
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let rate = f32::from_bits(pe::word(row, 8)?);
            let repeat = row[6] & 0x40 != 0;
            ensure!(
                row[4..6] == [0, 0]
                    && row[6] & !0x40 == 8
                    && row[7] == 255
                    && rate > 0.
                    && repeat == (index == 2),
                "unsupported chant pose"
            );
            Ok((
                pe::half(row, 0)?
                    .checked_add(index as u16)
                    .context("chant time overflow")?,
                ActionPose {
                    clip: u16::from(row[2]),
                    start: 0.,
                    end: None,
                    rate,
                    blend: row[3],
                    repeat,
                },
            ))
        })
        .collect()
}

fn rescue_names(library: &Path) -> Result<[String; 5]> {
    let skills: resonance_content::ex_skills::Document =
        json(library, resonance_content::ex_skills::PATH)?;
    let items: resonance_content::items::Document = json(library, resonance_content::items::PATH)?;
    let caption = |text: &serde_json::Value| -> Result<String> {
        Ok(text["name"]
            .as_str()
            .context("missing rescue caption")?
            .into())
    };
    Ok([
        caption(&skills.skills["angels-tear"].text)?,
        caption(&skills.skills["revive"].text)?,
        caption(&skills.skills["resurrect"].text)?,
        caption(&items.items["revive-ring"].text)?,
        caption(&items.items["reverse-doll"].text)?,
    ])
}

fn read(image: &pe::Image<'_>, library: &Path) -> Result<profile::Table> {
    let records = battle_profiles::party(image)?;
    let columns = recoil::strategy_defaults(image)?;
    let mut death_voice_pairs = Vec::new();
    for index in 0..7u32 {
        let code = image.slice(0x4310c3 + index * 8, 8)?;
        ensure!(
            code[..4] == [0xc7, 0x44, 0x24, 0x20 + index as u8 * 4],
            "death voice pairs changed"
        );
        death_voice_pairs.extend(code[4..].as_chunks::<2>().0.iter().copied());
    }
    let neutral = |address| -> Result<u16> {
        let code = image.slice(address, 5)?;
        ensure!(code[0] == 0xba, "neutral contact selector changed");
        audio_effects::id(pe::word(code, 1)?.try_into()?)
    };
    let slash = neutral(0x4658fa)?;
    let blunt = neutral(0x465907)?;
    ensure!(
        image.slice(0x4659ad, 8)? == [0x0f, 0xb7, 0x3c, 0x55, 0x50, 0x91, 0x91, 0],
        "elemental contact table changed"
    );
    let elements = (1..=8)
        .map(|index| audio_effects::id(pe::half(image.slice(0x919150, 20)?, index * 2)?))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    ensure!(
        image.slice(0x488ea9, 7)? == [0x8d, 0x04, 0x85, 0x5c, 0x93, 0x91, 0],
        "item offset table changed"
    );
    let item_use_offsets = image
        .slice(0x919368, 9 * 12)?
        .as_chunks::<12>()
        .0
        .iter()
        .map(|row| {
            Ok([
                f32::from_bits(pe::word(row, 8)?),
                f32::from_bits(pe::word(row, 4)?),
                f32::from_bits(pe::word(row, 0)?),
            ])
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let targets = (1..=8)
        .map(|index| {
            Ok(match pe::word(image.slice(0x919050 + index * 4, 4)?, 0)? {
                0x447ac0 => TargetPolicy::Nearest,
                0x447be0 => TargetPolicy::Farthest,
                0x447cf0 => TargetPolicy::Leader,
                0x447d10 => TargetPolicy::Spread,
                0x448260 => TargetPolicy::Flying,
                0x448420 => TargetPolicy::Casting,
                0x447e90 => TargetPolicy::LowestHp,
                0x447f80 => TargetPolicy::Protect,
                handler => anyhow::bail!("unknown party target handler {handler:#x}"),
            })
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    // The executable constructs these nine-byte columns on its stack.
    let tp = limits(image, 0x42e074, 0x42e07b, 0x42e082)?;
    let healing = limits(image, 0x42eba3, 0x42ebaa, 0x42eb98)?;
    let buffs = limits(image, 0x42ebb4, 0x42ebbb, 0x42ebc2)?;
    let idle = limits(image, 0x42f3ac, 0x42f3b3, 0x42f3a4)?;
    let normal: resonance_content::battle_normal::document::NormalAttacks =
        json(library, battle_rules::NORMAL_ATTACKS)?;
    let martial: resonance_content::battle_martial::document::Catalogue =
        json(library, battle_rules::MARTIAL_ATTACKS)?;
    Ok(profile::Table {
        records,
        normal_attacks: normal.prepare()?,
        martial_attacks: martial.prepare()?,
        body_colliders: std::array::from_fn(|i| {
            if matches!(i, 2 | 6) {
                profile::StandingCollider {
                    radius: 24.,
                    height: 110.,
                }
            } else {
                profile::StandingCollider {
                    radius: 30.,
                    height: 150.,
                }
            }
        }),
        default_strategy: std::array::from_fn(|actor| {
            std::array::from_fn(|kind| columns[kind * 10 + actor])
        }),
        target_strategies: targets,
        companion_policy: std::array::from_fn(|i| profile::CompanionLimits {
            tp_reserve_percent: tp[i],
            healing_threshold_percent: healing[i],
            buff_max_level_advantage: buffs[i] as i8,
            idle_tp_reserve_percent: idle[i],
        }),
        // Keep GC's maintained standing envelopes, movement and entry presentation.
        companion_positioning: profile::CompanionPositioning {
            idle_arrival_distance: 10.,
            formation_radius: 807.5,
            formation_occupancy_unit: 100.,
            approach_timeout_ticks: 300,
        },
        companion_normal_selection: profile::CompanionNormalSelection {
            airborne_target_height: 100.,
            colette_melee_gap: 150.,
        },
        entry: profile::Entry {
            replacement_motion_rate: 0.5,
            fade_color: [128; 3],
        },
        chant_sequences: BTreeMap::from([(3, chant(image)?)]),
        voice_sequences: battle_voices::casting_policies(image, library)?,
        death_voice_pairs,
        contact_sounds: profile::ContactSounds {
            party: [
                slash, slash, blunt, blunt, blunt, slash, slash, blunt, slash,
            ],
            elements,
        },
        overlimit_voices: battle_voices::overlimit(image)?,
        item_use_offsets,
        lethal_rescue_names: rescue_names(library)?,
    })
}
