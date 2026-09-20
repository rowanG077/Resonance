//! Resolve battle action cues through the original sound banks and voice archive.
pub(crate) mod all;
mod binding;
#[path = "audio_inventory.rs"]
mod inventory;
mod selection;
use crate::{
    all_assets::roles,
    media::{self, Workspace},
};
use anyhow::{Context, Result, ensure};
pub(crate) use inventory::recover;
use resonance_content::{
    battle::audio::{BattleAudio, BattleCue, BattleMusic, ImpactSoundTable, VoiceCue},
    field_audio::FieldAudio,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(crate) fn cook(
    extracted: &Path,
    output: &Path,
    coefficients: &Path,
    sound_ids: &BTreeSet<u16>,
    voice_ids: &BTreeSet<u16>,
    characters: &BTreeSet<u8>,
    enemies: &[u8],
) -> Result<BattleAudio> {
    let workspace = Workspace::open(extracted, output)?;
    let executable = fs::read(workspace.extracted.join("sys/main.dol"))?;
    let battle_sources = super::all::Sources::cooked(output, workspace.disc)?;
    let resident = roles::resident_banks(&workspace.extracted, &executable)?;
    let bank_catalogue = selection::banks(&workspace.extracted, &executable, &resident)?;
    let coefficients = fs::read(coefficients)?;
    let mut sounds = sound_ids.clone();
    sounds.extend(BattleCue::ALL.map(|cue| cue as u16));
    let impact_sounds: ImpactSoundTable = crate::embedded::read(
        &output.join("data"),
        "battle-contact-sounds",
        "US_r_Top2Btl.rel",
    )?;
    sounds.extend(impact_sounds.sounds());
    let mut voice_cues = BTreeMap::new();
    let voice_profiles = selection::voices(
        &workspace.output,
        workspace.disc,
        &battle_sources,
        &bank_catalogue,
        characters,
        enemies,
    )?;
    let voice_ids: BTreeSet<_> = voice_ids
        .iter()
        .copied()
        .chain(voice_profiles.cues())
        .collect();
    for &id in voice_ids.iter().filter(|&&id| id != 0) {
        let cue = if id & 0x8000 != 0 {
            VoiceCue::Stream(u32::from(id & 0x7fff))
        } else {
            let sound =
                i16::try_from(u32::from(id) + 501).context("actor voice sound ID overflow")?;
            sounds.insert(sound as u16);
            VoiceCue::Sound(sound)
        };
        voice_cues.insert(id, cue);
    }
    ensure!(
        sounds.iter().all(|&id| id <= i16::MAX as u16),
        "invalid battle sound ID"
    );
    let banks = selection::sounds(&bank_catalogue, &sounds)?;
    let mut sources = BTreeMap::new();
    for source in [
        resident[0].clone(),
        resident[1].clone(),
        crate::field_resources::resolve_path(
            &workspace.extracted.join("files"),
            "US_r_Top2Btl.rel",
        )?,
        battle_sources.usual.clone(),
        battle_sources.enemy.clone(),
    ]
    .into_iter()
    .chain(bank_catalogue.keys().cloned())
    .chain(
        BattleMusic::ALL
            .into_iter()
            .map(|music| {
                crate::field_resources::resolve_path(
                    &workspace.extracted.join("files"),
                    &media::music_path(&executable, music.id() as u16)?,
                )
            })
            .collect::<Result<Vec<_>>>()?,
    ) {
        sources.insert(
            source.clone(),
            media::hash_file(&workspace.extracted.join("files").join(source))?,
        );
    }
    let streams: BTreeSet<_> = voice_cues
        .values()
        .filter_map(|cue| match cue {
            VoiceCue::Stream(id) => Some(*id),
            VoiceCue::Sound(_) => None,
        })
        .collect();
    let voices = if streams.is_empty() {
        BTreeMap::new()
    } else {
        let voice_source = binding::source(&workspace.extracted, &executable)?;
        let (source, voices) =
            binding::voices(&workspace.output, workspace.disc, &voice_source, &streams)?;
        sources.insert(source.path, source.sha256);
        voices
    };
    let reverbs = media::song_reverbs(&executable, BattleMusic::Sylvarant.id() as u16)?;
    let manifest = BattleAudio {
        version: BattleAudio::VERSION,
        audio: FieldAudio {
            version: FieldAudio::VERSION,
            music: media::bind_music(
                &workspace,
                &executable,
                "battle-music",
                BattleMusic::ALL.map(|music| music.id() as u16),
                reverbs,
            )?,
            sounds: media::bind_sounds(
                &workspace,
                &executable,
                &coefficients,
                reverbs,
                "battle-sound",
                &banks,
            )?,
            voices,
            voice_gains: media::voice_gains(&executable)?,
            recipe: json!({"version":1,"sources":sources,"sound_banks":banks,
                "executable_sha256":crate::digest(&executable),"coefficients_sha256":crate::digest(&coefficients),
                "audio_device":false}),
        },
        voice_cues,
        voice_profiles,
        impact_sounds,
        voice_pan: media::voice_pan(&executable)?,
    };
    manifest.validate()?;
    Ok(manifest)
}

#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;
