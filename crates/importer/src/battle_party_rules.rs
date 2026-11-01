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

fn target_strategies(module: &Rel) -> Result<[resonance_content::battle_action::TargetPolicy; 8]> {
    use resonance_content::battle_action::TargetPolicy as Target;
    (0..8)
        .map(|index| {
            let &(section, function) = module
                .pointers
                .get(&(5, 0xd30 + index * 4))
                .context("missing strategy target dispatch relocation")?;
            ensure!(
                section == 1,
                "strategy target dispatch must refer to battle code"
            );
            Ok(match function {
                0x368f8 => Target::Nearest,
                0x367a4 => Target::Farthest,
                0x36798 => Target::Leader,
                0x364cc => Target::Spread,
                0x35df0 => Target::Flying,
                0x35bc4 => Target::Casting,
                0x3639c => Target::LowestHp,
                0x361d4 => Target::Protect,
                _ => anyhow::bail!("unsupported strategy target handler {function:#x}"),
            })
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid target dispatch width"))
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
    let approach_limit = module.slice(1, 0x3100c, 4)?;
    ensure!(
        float(module.tail(4, 0x2890)?, 0)? == float(module.tail(4, 0x1b7c)?, 0)?,
        "unsupported distinct companion target-height thresholds"
    );
    ensure!(
        half(approach_limit, 0)? == 0x2c00,
        "unsupported approach limit comparison"
    );
    let table = profile::Table {
        companion_normal_selection: profile::CompanionNormalSelection {
            airborne_target_height: float(module.tail(4, 0x2890)?, 0)?,
            colette_melee_gap: float(module.tail(4, 0x1b80)?, 0)?,
        },
        companion_positioning: profile::CompanionPositioning {
            idle_arrival_distance: float(module.tail(4, 0x1a9c)?, 0)?,
            formation_radius: float(module.tail(4, 0x1648)?, 0)?,
            formation_occupancy_unit: float(module.tail(4, 0x15f4)?, 0)?,
            approach_timeout_ticks: half(approach_limit, 2)?,
        },
        target_strategies: target_strategies(module)?,
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
        companion_policy: {
            let tp = module.slice(4, 0x2860, 9)?;
            let healing = module.slice(4, 0x286c, 9)?;
            let buffs = module.slice(4, 0x2878, 9)?;
            let idle_tp = module.slice(4, 0x2884, 9)?;
            std::array::from_fn(|i| profile::CompanionLimits {
                tp_reserve_percent: tp[i],
                healing_threshold_percent: healing[i],
                buff_max_level_advantage: buffs[i] as i8,
                idle_tp_reserve_percent: idle_tp[i],
            })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires extracted GQSEAF battle REL and resident archive"]
    fn native_strategy_tables_survive_named_rules_and_mod_edits() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_EXTRACTED")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted"));
        let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
        let table = read(
            &module,
            &fs::read(root.join("disc1/files/BTL/BTLusual.dat"))?,
        )?;
        assert_eq!(
            table.companion_positioning,
            profile::CompanionPositioning {
                idle_arrival_distance: 10.,
                formation_radius: 807.5,
                formation_occupancy_unit: 100.,
                approach_timeout_ticks: 300,
            }
        );
        assert_eq!(
            table.target_strategies,
            resonance_content::battle_action::TargetPolicy::PARTY
        );
        assert_eq!(
            table.companion_policy.map(|limits| (
                limits.tp_reserve_percent,
                limits.healing_threshold_percent,
                limits.buff_max_level_advantage,
                limits.idle_tp_reserve_percent,
            )),
            [
                (0, 0, 0, 0),
                (0, 40, 0, 0),
                (25, 70, 2, 25),
                (50, 60, 0, 50),
                (75, 50, 0, 75),
                (100, 0, 10, 100),
                (30, 65, 2, 30),
                (25, 80, 4, 25),
                (50, 70, 8, 50),
            ]
        );
        let names = Mappings::new(
            PartyCharacterId::ALL
                .into_iter()
                .map(|id| (u32::from(id.raw()), id.resource_name().unwrap().into()))
                .collect(),
        )?;
        assert_eq!(table.normal_attacks[1][2].chain_retry_at, Some(71));
        assert_eq!(table.normal_attacks[4][4].chain_retry_at, Some(43));
        let character = resonance_content::battle_normal::document::Character::from_attacks(
            table.normal_attacks[1].clone(),
        )?;
        let mut edited = serde_json::to_value(character)?;
        edited["attacks"]["thrust"]["chain_retry_at"] = 72.into();
        let edited: resonance_content::battle_normal::document::Character =
            serde_json::from_value(edited)?;
        assert_eq!(edited.prepare()?[2].chain_retry_at, Some(72));
        let doc = Party::from_table(
            table.clone(),
            "normal.json".into(),
            "martial.json".into(),
            &names,
        )?;
        let mut json = serde_json::to_value(doc)?;
        let prepare = |json| -> Result<profile::Table> {
            serde_json::from_value::<Party>(json)?.prepare(
                &names,
                table.normal_attacks.clone(),
                table.martial_attacks.clone(),
            )
        };
        let restored = prepare(json.clone())?;
        assert_eq!(
            restored.companion_normal_selection,
            profile::CompanionNormalSelection {
                airborne_target_height: 100.,
                colette_melee_gap: 150.,
            }
        );
        assert_eq!(restored.target_strategies, table.target_strategies);
        assert_eq!(restored.companion_positioning, table.companion_positioning);
        assert_eq!(restored.companion_policy, table.companion_policy);
        json["technique_strategies"]["support"]["buff_max_level_advantage"] = (-3).into();
        json["technique_strategies"]["support"]["idle_tp_reserve_percent"] = 17.into();
        json["target_strategies"]["attack-close"] = "farthest".into();
        json["target_strategies"]["scatter"] = "farthest".into();
        json["companion_positioning"]["formation_radius"] = 600.into();
        json["companion_positioning"]["idle_arrival_distance"] = 7.into();
        json["companion_positioning"]["approach_timeout_ticks"] = 123.into();
        json["companion_positioning"]["formation_occupancy_unit"] = 50.into();
        json["companion_normal_selection"]["colette_melee_gap"] = 99.into();
        json["companion_normal_selection"]["airborne_target_height"] = 125.into();
        let modified = prepare(json.clone())?;
        assert_eq!(
            modified.companion_normal_selection,
            profile::CompanionNormalSelection {
                airborne_target_height: 125.,
                colette_melee_gap: 99.,
            }
        );
        assert_eq!(
            modified.companion_positioning,
            profile::CompanionPositioning {
                idle_arrival_distance: 7.,
                formation_radius: 600.,
                formation_occupancy_unit: 50.,
                approach_timeout_ticks: 123,
            }
        );
        let mut invalid = json.clone();
        invalid["companion_positioning"]["idle_arrival_distance"] = 0.into();
        assert!(prepare(invalid).is_err());
        assert_eq!(
            modified.target_strategy(0, 1)?,
            resonance_content::battle_action::TargetPolicy::Farthest
        );
        assert_eq!(
            modified.target_strategy(0, 0)?,
            resonance_content::battle_action::TargetPolicy::Farthest
        );
        json["target_strategies"]["attack-close"] = "self_target".into();
        assert!(prepare(json).is_err());
        let mut expected = table.companion_policy;
        expected[8].buff_max_level_advantage = -3;
        expected[8].idle_tp_reserve_percent = 17;
        assert_eq!(modified.companion_policy, expected);
        Ok(())
    }
}
