//! Shared party combat rules directly from the resident archive and battle REL.
use crate::{
    battle_enemy_profile, battle_party_martial, battle_party_native::*, battle_party_normal,
    rel::Rel,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_action::ActionPose,
    battle_profile::{self as profile, VoicePolicy, VoiceSequence},
    battle_rules::Party,
    battle_voice::Sound,
    library::Mappings,
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::PartyCharacterId;

fn voice_policy(module: &Rel, usual: &[u8], character: usize, base: u32) -> Result<VoicePolicy> {
    let default = if base == 0 {
        VoiceSequence::default()
    } else {
        let index = u16::try_from(base.checked_add(7).context("chant voice overflow")?)?;
        ensure!(index < 0x8000, "invalid chant voice");
        VoiceSequence {
            chant: Some(Sound::Stream(index)),
            chant_ticks: duration(usual, Sound::Stream(index))?,
            self_chant: Some(Sound::Stream(index)),
            release: None,
            fallback: Some(Sound::Cue(index + 501)),
            fallback_ticks: duration(usual, Sound::Cue(index + 501))?,
        }
    };
    let mut policy = VoicePolicy {
        default,
        techniques: BTreeMap::new(),
    };
    let address = (5, 0x59d8 + character * 4);
    if let Some(&(section, offset)) = module.pointers.get(&address) {
        let mut terminated = false;
        for row in module.tail(section, offset)?.chunks_exact(6) {
            let technique = half(row, 0)?;
            if technique == 0 {
                terminated = true;
                break;
            }
            let chant = sound(half(row, 2)?).or(default.chant);
            policy.techniques.insert(
                technique,
                VoiceSequence {
                    chant,
                    chant_ticks: chant.map(|v| duration(usual, v)).transpose()?.unwrap_or(0),
                    self_chant: if character == 1 {
                        chant
                    } else {
                        default.self_chant
                    },
                    release: sound(half(row, 4)?),
                    fallback: default.fallback,
                    fallback_ticks: default.fallback_ticks,
                },
            );
        }
        ensure!(terminated, "unterminated party voice table");
    } else {
        ensure!(
            word(module.tail(address.0, address.1)?, 0)? == 0,
            "missing party voice table pointer"
        );
    }
    if character == 1 && base != 0 {
        for (technique, stream) in [(268, 232), (269, 231)] {
            let row = policy.techniques.entry(technique).or_insert(default);
            row.fallback = Some(Sound::Stream(stream));
            row.fallback_ticks = duration(usual, Sound::Stream(stream))?;
        }
    }
    Ok(policy)
}

fn chant(module: &Rel) -> Result<Vec<(u16, ActionPose)>> {
    let rows = module.slice(5, 0x11e0, 48)?;
    ensure!(
        signed(rows, 36)? == -2 && rows[38..].iter().all(|&v| v == 0),
        "unsupported chant termination"
    );
    rows[..36]
        .chunks_exact(12)
        .enumerate()
        .map(|(index, row)| {
            let at = u16::try_from(signed(row, 0)?)?
                .checked_add(index as u16)
                .context("chant time overflow")?;
            let rate = float(row, 8)?;
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
                at,
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

fn read(module: &Rel, usual: &[u8]) -> Result<profile::Table> {
    let bytes = module.slice(5, 0x3d30, 9 * 496)?;
    for row in bytes.chunks_exact(496) {
        ensure!(
            float(row, 0xa0)? > 0. && row[0xa8] == 0,
            "unsupported party casting playback"
        );
    }
    let default_strategy = module.slice(4, 0x10dc, 30)?;
    let vector_rows = module.slice(5, 0xc3c, 9 * 12)?;
    let contact = module.slice(4, 0x1d30, 18)?;
    let overlimit = module.slice(4, 0x1264, 18)?;
    let elements = module.slice(5, 0x1405, 8)?.try_into()?;
    // Maintained standing capsules; native posed reception markers remain separate.
    let body_colliders = std::array::from_fn(|i| {
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
    });
    let table = profile::Table {
        body_colliders,
        item_use_offsets: (0..9)
            .map(|i| vector(vector_rows, i * 12))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        normal_attacks: battle_party_normal::read(module)?,
        martial_attacks: battle_party_martial::read(usual, module)?,
        contact_sounds: profile::ContactSounds {
            party: (0..9)
                .map(|i| half(contact, i * 2))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            elements,
        },
        overlimit_voices: (0..9)
            .map(|i| half(overlimit, i * 2).map(sound))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        default_strategy: std::array::from_fn(|i| {
            std::array::from_fn(|kind| default_strategy[kind * 10 + i])
        }),
        companion_policy: profile::CompanionPolicy {
            tp_limits: module.slice(4, 0x2860, 9)?.try_into()?,
            healing_limits: module.slice(4, 0x286c, 9)?.try_into()?,
        },
        entry: profile::Entry {
            replacement_motion_rate: float(module.tail(4, 0x2a04)?, 0)?,
            fade_color: module.slice(4, 0x148, 3)?.try_into()?,
        },
        chant_sequences: [(3, chant(module)?)].into(),
        voice_sequences: (0..9)
            .map(|i| voice_policy(module, usual, i, word(bytes, i * 496 + 0x104)?))
            .collect::<Result<_>>()?,
        death_voice_pairs: module
            .slice(4, 0x177c, 28)?
            .chunks_exact(2)
            .map(|v| [v[0], v[1]])
            .collect(),
        lethal_rescue_names: [0x1dd0, 0x1de0, 0x1de8, 0x1df4, 0x1e08]
            .map(|offset| module.text(4, offset))
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        records: bytes
            .chunks_exact(496)
            .enumerate()
            .map(|(index, row)| {
                battle_enemy_profile::party_profile(row, usual).with_context(|| {
                    format!("party profile {}", PartyCharacterId::RESOURCE_NAMES[index])
                })
            })
            .collect::<Result<_>>()?,
    };
    table.validate()?;
    Ok(table)
}

pub fn cook_battle_party_rules(root: &Path, library: &Path) -> Result<()> {
    let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    let usual = fs::read(root.join("disc1/files/BTL/BTLusual.dat"))?;
    let table = read(&module, &usual)?;
    let names = Mappings::new(
        PartyCharacterId::ALL
            .into_iter()
            .map(|id| (u32::from(id.raw()), id.resource_name().unwrap().into()))
            .collect(),
    )?;
    let normals = resonance_content::battle_normal::document::NormalAttacks {
        mappings: names.clone(),
        characters: table
            .normal_attacks
            .iter()
            .enumerate()
            .map(|(i, attacks)| {
                Ok((
                    PartyCharacterId::RESOURCE_NAMES[i].into(),
                    resonance_content::battle_normal::document::Character::from_attacks(
                        attacks.clone(),
                    )?,
                ))
            })
            .collect::<Result<_>>()?,
    };
    #[derive(serde::Deserialize)]
    struct Name {
        name: String,
        variants: BTreeMap<u8, String>,
    }
    let named: BTreeMap<u16, Name> =
        serde_json::from_str(include_str!("battle-martial-names.json"))?;
    let mut mappings = BTreeMap::new();
    let mut techniques = BTreeMap::new();
    for (&id, member) in &table.martial_attacks.members {
        let name = named
            .get(&id)
            .map(|v| v.name.clone())
            .unwrap_or_else(|| format!("action-{id}"));
        let variants = member
            .variants
            .keys()
            .map(|&variant| {
                (
                    variant,
                    named
                        .get(&id)
                        .and_then(|v| v.variants.get(&variant))
                        .cloned()
                        .unwrap_or_else(|| format!("variant-{variant}")),
                )
            })
            .collect();
        ensure!(
            techniques
                .insert(
                    name.clone(),
                    resonance_content::battle_martial::document::Technique::from_member(
                        member.clone(),
                        variants
                    )?
                )
                .is_none(),
            "duplicate martial name"
        );
        mappings.insert(u32::from(id), name);
    }
    let martial = resonance_content::battle_martial::document::Catalogue {
        mappings: Mappings::new(mappings)?,
        bindings: table.martial_attacks.bindings.clone(),
        techniques,
    };
    let party = Party::from_table(
        table,
        resonance_content::battle_rules::NORMAL_ATTACKS.into(),
        resonance_content::battle_rules::MARTIAL_ATTACKS.into(),
        &names,
    )?;
    for (path, bytes) in [
        (
            resonance_content::battle_rules::NORMAL_ATTACKS,
            resonance_asset_writer::json::encode(&normals)?,
        ),
        (
            resonance_content::battle_rules::MARTIAL_ATTACKS,
            resonance_asset_writer::json::encode(&martial)?,
        ),
        (
            resonance_content::battle_rules::PARTY,
            resonance_asset_writer::json::encode(&party)?,
        ),
    ] {
        crate::output::write(&library.join(path), &bytes)?;
    }
    Ok(())
}
