//! Select an encounter's requests from the complete cooked audio catalogue.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_audio::{self, Audio, Selection},
    battle_voice::Sound,
    diagnostics::Diagnostics,
    field_audio::{Asset, MAX_MANIFEST_BYTES},
    prepared::{Cache, Files},
};
use std::{collections::BTreeMap, path::Path};

/// Metadata only; no score, sample or voice payload is loaded here.
pub fn read(files: &Files) -> Result<Option<Audio>> {
    let decoded = (|| {
        let bytes = files.read_verified(
            battle_audio::PATH,
            files.digest(battle_audio::PATH)?,
            MAX_MANIFEST_BYTES,
        )?;
        serde_json::from_slice::<
            Audio<resonance_content::field_audio::DecodedBank, serde_json::Value>,
        >(&bytes)
        .context("decode battle audio descriptor")
    })();
    match files
        .diagnostics()
        .attempt("battle audio descriptor", decoded)?
    {
        Some(audio) => audio.checked(files.diagnostics()),
        None => Ok(None),
    }
}

/// Called by the existing action preparation path, including rare alternatives.
pub fn collect(
    audio: Option<&Audio>,
    selected: &mut Selection,
    diagnostics: &Diagnostics,
    sound: Sound,
) -> Result<Option<Sound>> {
    let Some(audio) = audio else { return Ok(None) };
    diagnostics.attempt(
        "battle audio binding",
        (|| {
            match sound {
                Sound::Cue(id) => {
                    ensure!(
                        audio.assets.sounds.contains_key(&i16::try_from(id)?),
                        "unprepared battle sound {id}"
                    );
                    selected.sounds.insert(id);
                }
                Sound::Stream(id) => {
                    ensure!(
                        audio.assets.voices.contains_key(&u32::from(id)),
                        "unprepared battle stream {id}"
                    );
                    selected.streams.insert(id);
                }
            }
            Ok(sound)
        })(),
    )
}

pub fn prepare(
    root: &Path,
    mut files: Files,
    audio: Option<Audio>,
    selected: &Selection,
    cache: &mut Cache,
    cancelled: impl Fn() -> bool,
) -> Result<(Files, Option<Audio>)> {
    let Some(mut audio) = audio else {
        return Ok((
            files.with_dependencies(root, BTreeMap::new(), cache, cancelled)?,
            None,
        ));
    };
    let require = |kind, id: u32, present: bool| -> Result<()> {
        files.diagnostics().attempt(
            "battle audio binding",
            present
                .then_some(())
                .with_context(|| format!("unprepared battle {kind} {id}")),
        )?;
        Ok(())
    };
    for (kind, ids, available) in [
        ("music", &selected.music, &audio.assets.music),
        ("sound", &selected.sounds, &audio.assets.sounds),
    ] {
        for &id in ids {
            require(
                kind,
                u32::from(id),
                i16::try_from(id).is_ok_and(|id| available.contains_key(&id)),
            )?;
        }
    }
    for (kind, id) in selected
        .streams
        .iter()
        .map(|&id| ("stream", u32::from(id)))
        .chain(selected.voices.iter().map(|&id| ("event voice", id)))
    {
        require(kind, id, audio.assets.voices.contains_key(&id))?;
    }
    audio
        .assets
        .music
        .retain(|id, _| u16::try_from(*id).is_ok_and(|id| selected.music.contains(&id)));
    audio
        .assets
        .sounds
        .retain(|id, _| u16::try_from(*id).is_ok_and(|id| selected.sounds.contains(&id)));
    audio.assets.voices.retain(|id, _| {
        selected.voices.contains(id)
            || u16::try_from(*id).is_ok_and(|id| selected.streams.contains(&id))
    });

    let mut inventory = BTreeMap::new();
    for asset in audio
        .assets
        .music
        .values()
        .chain(audio.assets.sounds.values())
        .chain(audio.assets.voices.values().map(|voice| &voice.asset))
    {
        inventory.insert(asset.path.clone(), audio.files[&asset.path].clone());
    }
    files = files.with_dependencies(root, inventory.clone(), cache, &cancelled)?;

    // The existing package owns its sample references. Read just that small
    // projection here; the mixer validates the complete package before playback.
    #[derive(serde::Deserialize)]
    struct Samples {
        samples: BTreeMap<u16, Asset>,
    }
    let mut samples = BTreeMap::new();
    for asset in audio
        .assets
        .music
        .values()
        .chain(audio.assets.sounds.values())
    {
        let closure = (|| -> Result<()> {
            let bytes = files.read_verified(&asset.path, &asset.sha256, MAX_MANIFEST_BYTES)?;
            let package: Samples = serde_json::from_slice(&bytes)?;
            for sample in package.samples.values() {
                sample.validate()?;
                let file = audio
                    .files
                    .get(&sample.path)
                    .context("sample outside battle audio inventory")?;
                ensure!(
                    file.sha256 == sample.sha256
                        && file
                            .roles
                            .contains(&resonance_content::field_preload::Role::InstrumentSample),
                    "battle sample binding differs: {}",
                    sample.path
                );
                samples.insert(sample.path.clone(), file.clone());
            }
            Ok(())
        })();
        files
            .diagnostics()
            .attempt("battle audio samples", closure)?;
    }
    files = files.with_dependencies(root, samples.clone(), cache, cancelled)?;
    inventory.extend(samples);
    audio.files = inventory;
    Ok((files, Some(audio)))
}
