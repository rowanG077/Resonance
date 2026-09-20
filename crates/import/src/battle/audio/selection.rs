//! Select prepared audio from physical bank ownership and cooked actor settings.
use crate::{
    all_assets::roles,
    battle::{all::Sources, visual::binding::Directory},
};
use anyhow::{Context, Result, ensure};
use resonance_audio_cook::bank::Bank;
use resonance_content::battle::audio::{NativeVoices, VoiceProfile, VoiceProfiles};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const ACTOR_GROUP_START: u16 = 10;
const ENEMY_GROUP_START: u16 = 19;
const VOICE_SOUND_OFFSET: u16 = 501;

pub(super) struct SoundBank {
    pub group: u16,
    pub ids: BTreeSet<u16>,
}

pub(super) fn banks(
    extracted: &Path,
    executable: &[u8],
    resident: &[String; 2],
) -> Result<BTreeMap<String, SoundBank>> {
    let party = roles::party_banks(extracted, executable)?;
    read_banks(
        &extracted.join("files"),
        &resident[1],
        &party,
        super::all::bank_sources(extracted, executable)?
            .into_iter()
            .filter(|path| path != &resident[0]),
    )
}

pub(super) fn read_banks(
    files: &Path,
    common: &str,
    party: &[String],
    paths: impl IntoIterator<Item = String>,
) -> Result<BTreeMap<String, SoundBank>> {
    let mut banks = BTreeMap::new();
    let mut groups = BTreeMap::new();
    // Public party voices remain registered even though their objects are local pools.
    // Prefer native declarations over identical physical aliases.
    for path in std::iter::once(common.to_owned())
        .chain(party.iter().cloned())
        .chain(paths)
    {
        let bytes = fs::read(files.join(&path))?;
        let bank = Bank::parse(&bytes).with_context(|| format!("battle sound bank {path}"))?;
        let group = bank.group()?;
        if path != common && !party.contains(&path) && group < ENEMY_GROUP_START {
            continue;
        }
        let hash = crate::digest(&bytes);
        if let Some(previous) = groups.insert(group, hash.clone()) {
            ensure!(previous == hash, "conflicting battle sound group {group}");
            continue;
        }
        banks.insert(
            path,
            SoundBank {
                group,
                ids: bank.sound_ids().collect(),
            },
        );
    }
    Ok(banks)
}

pub(super) fn sounds(
    banks: &BTreeMap<String, SoundBank>,
    requested: &BTreeSet<u16>,
) -> Result<Vec<(String, Vec<u16>)>> {
    let mut owned = BTreeSet::new();
    let mut selected = Vec::new();
    for (path, bank) in banks {
        let ids: Vec<_> = bank.ids.intersection(requested).copied().collect();
        for &id in &ids {
            ensure!(owned.insert(id), "ambiguous battle sound {id}");
        }
        if !ids.is_empty() {
            selected.push((path.clone(), ids));
        }
    }
    let missing: Vec<_> = requested.difference(&owned).collect();
    ensure!(
        missing.is_empty(),
        "battle sounds {missing:?} are absent from the original banks"
    );
    Ok(selected)
}

#[derive(Deserialize)]
struct Actor {
    effects: VoiceSettings,
    combat: Flags,
}
#[derive(Deserialize)]
struct VoiceSettings {
    voice_base: u16,
    death_voice: u16,
}
#[derive(Deserialize)]
struct Flags {
    flags: u32,
}
#[derive(Deserialize)]
struct Party {
    character: u8,
    settings: Actor,
}
#[derive(Deserialize)]
struct VoiceSelection {
    voice_count: usize,
    streamed_voices: BTreeSet<u16>,
}

pub(super) fn voices(
    output: &Path,
    disc: u8,
    sources: &Sources,
    banks: &BTreeMap<String, SoundBank>,
    characters: &BTreeSet<u8>,
    enemies: &[u8],
) -> Result<VoiceProfiles> {
    let directory = Directory::open(output, disc, &sources.usual)?;
    let (_, bytes) = directory.resolve("battle/all/usual/11.json")?;
    let selection: VoiceSelection = serde_json::from_slice(&bytes)?;
    ensure!(
        selection.voice_count <= 0x8000
            && selection
                .streamed_voices
                .iter()
                .all(|&id| usize::from(id) < selection.voice_count),
        "invalid battle voice selection"
    );
    let mut ranges = BTreeMap::new();
    for bank in banks
        .values()
        .filter(|bank| bank.group >= ACTOR_GROUP_START)
    {
        let end = bank
            .ids
            .last()
            .context("empty actor voice bank")?
            .checked_add(1)
            .and_then(|id| id.checked_sub(VOICE_SOUND_OFFSET))
            .context("invalid actor voice bank range")?;
        for &id in &bank.ids {
            ensure!(
                ranges.insert(id, end).is_none(),
                "ambiguous actor voice sound {id}"
            );
        }
    }
    let profile = |actor: Actor| -> Result<VoiceProfile> {
        let base = actor.effects.voice_base;
        let voices = if base == 0 {
            NativeVoices::Voiceless
        } else {
            ensure!(base < 0x8000, "invalid native actor voice base");
            let end = *ranges
                .get(&(base + VOICE_SOUND_OFFSET))
                .context("missing actor voice bank")?;
            ensure!(
                end > base && end - base <= 128 && usize::from(end) <= selection.voice_count,
                "invalid native actor voice range"
            );
            NativeVoices::Voiced {
                base,
                ids: (base..end)
                    .map(|id| {
                        id | if selection.streamed_voices.contains(&id) {
                            0x8000
                        } else {
                            0
                        }
                    })
                    .collect(),
            }
        };
        Ok(VoiceProfile {
            voices,
            death_override: actor.effects.death_voice,
            low_hp: actor.combat.flags & 0x10 != 0,
        })
    };
    let rows: Vec<Party> = crate::embedded::read(
        &output.join("data"),
        "battle-party-settings",
        "US_r_Top2Btl.rel",
    )?;
    let mut party = BTreeMap::new();
    for row in rows
        .into_iter()
        .filter(|row| characters.contains(&row.character))
    {
        ensure!(
            party
                .insert(row.character, profile(row.settings)?)
                .is_none(),
            "duplicate party voice settings"
        );
    }
    ensure!(
        characters
            .iter()
            .all(|id| (1..=9).contains(id) && party.contains_key(id)),
        "missing party voice settings"
    );
    let mut profiles = BTreeMap::new();
    if !enemies.is_empty() {
        let directory = Directory::open(output, disc, &sources.enemy)?;
        for &id in enemies {
            let (_, bytes) = directory.resolve(&format!("battle/all/enemy-{id}/header-4.json"))?;
            profiles.insert(id, profile(serde_json::from_slice(&bytes)?)?);
        }
    }
    Ok(VoiceProfiles {
        party,
        enemies: profiles,
    })
}
