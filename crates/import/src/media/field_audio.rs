//! Field audio recipes, using original resource tables and Rust synthesis.
use super::{Workspace, hash_file, write_json};
use anyhow::{Context, Result, ensure};
use resonance_audio::package::Package;
use resonance_content::field_audio::{Asset, FieldAudio, Voice};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};
mod binding;
mod resources;

/// Resolve the saved setting through CRI attenuation and the stream mixer.
/// The runtime consumes amplitudes, without executable addresses or codec tables.
pub(crate) fn voice_gains(executable: &[u8]) -> Result<Vec<f32>> {
    crate::stream_mixer::Tables::read(executable)?.gains(&super::sound_buses::tables(executable)?)
}

pub(crate) fn voice_pan(executable: &[u8]) -> Result<Vec<[f32; 2]>> {
    crate::stream_mixer::Tables::read(executable)?.pan(&super::sound_buses::tables(executable)?)
}

/// Cook every declared scenario branch, message voice and native service cue.
pub fn cook_field_audio(
    extracted: &Path,
    output: &Path,
    map_id: u32,
    coefficients: &Path,
    additional_disc: Option<&Path>,
) -> Result<()> {
    let workspace = Workspace::open(extracted, output)?;
    if let Some(disc) = additional_disc {
        ensure!(
            crate::disc_number(disc)? == 2,
            "additional voice disc must be disc 2"
        );
    }
    let map = crate::field::MapArchive::open(&crate::field::source_for_id(extracted, map_id)?)
        .with_context(|| format!("read field {map_id} source archive"))?;
    let executable = read_file(&extracted.join("sys/main.dol"))?;
    let resources = resources::Resources::read(extracted, &executable, map.section(6)?)
        .with_context(|| format!("inventory field {map_id} audio"))?;
    let coefficients = read_file(coefficients)?;
    let mut sources = BTreeMap::new();
    let (voices, voice_sources) =
        binding::voices(&workspace, &executable, additional_disc, &resources.voices)
            .with_context(|| format!("bind field {map_id} voices"))?;
    for source in ["S/inst.snd".to_owned()]
        .into_iter()
        .chain(resources.banks.iter().map(|(source, _)| source.clone()))
        .chain(
            resources
                .music
                .iter()
                .map(|id| music_path(&executable, *id))
                .collect::<Result<Vec<_>>>()?,
        )
    {
        sources.insert(
            source.clone(),
            hash_file(&extracted.join("files").join(&source))
                .with_context(|| format!("hash field {map_id} audio source {source}"))?,
        );
    }
    let recipe = json!({"version":6,"map_id":map_id,"map_sha256":map.source_sha256,"sound_banks":resources.banks,
        "executable_sha256":crate::digest(&executable),"coefficients_sha256":crate::digest(&coefficients),
        "compiler_sha256":hash_file(&std::env::current_exe()?)?,"sources":sources,"voice_sources":voice_sources,"audio_device":false});
    let metadata = output.join(if map_id == 340 {
        "fields/iselia-classroom-audio.json".into()
    } else {
        format!("fields/map-{map_id}-audio.json")
    });
    if let Some(previous) = super::json_file(&metadata)
        && let Ok(previous) = serde_json::from_value::<FieldAudio>(previous)
        && previous.recipe == recipe
        && previous
            .music
            .keys()
            .copied()
            .eq(resources.music.iter().map(|id| *id as i16))
        && previous
            .sounds
            .keys()
            .copied()
            .eq(resources.sounds.iter().map(|id| *id as i16))
        && previous
            .voices
            .keys()
            .copied()
            .eq(resources.voices.iter().copied())
        && current(output, &previous)?
    {
        println!("Field {map_id} audio is current");
        return crate::field::refresh_preloads(output);
    }
    let reverbs = super::music::title_reverbs(&executable)?;
    let manifest = FieldAudio {
        version: FieldAudio::VERSION,
        voice_gains: voice_gains(&executable)?,
        music: super::bind_music(
            &workspace,
            &executable,
            "field-music",
            resources.music,
            reverbs,
        )?,
        sounds: super::bind_sounds(
            &workspace,
            &executable,
            &coefficients,
            reverbs,
            "field-sound",
            &resources.banks,
        )?,
        voices,
        recipe,
    };
    manifest.validate()?;
    write_json(&metadata, &serde_json::to_value(&manifest)?)?;
    println!(
        "Cooked field {map_id}: {} scores, {} cues, {} voices without playback",
        manifest.music.len(),
        manifest.sounds.len(),
        manifest.voices.len()
    );
    crate::field::refresh_preloads(output)
}

pub(crate) enum VoiceFormat {
    Ahx,
    Adx,
}

/// Preserve native PCM and its header rate. Playback clocks belong to bindings.
pub(crate) fn decode_voice_to(
    workspace: &Workspace,
    path: &str,
    member: &crate::afs::Member<'_>,
    format: VoiceFormat,
) -> Result<Voice> {
    resonance_content::validate_asset_path(path)?;
    let target = workspace.output.join(path);
    fs::create_dir_all(target.parent().context("voice target has no parent")?)?;
    let temporary = crate::temporary_path(&target);
    let (channels, source_rate, frames) = match format {
        VoiceFormat::Ahx => {
            let mut decoder = ahx_rs::Decoder::new(member.data)?;
            let info = decoder.metadata();
            ensure!(
                (8_000..=96_000).contains(&info.sample_rate())
                    && (1..=32_000_000).contains(&info.samples()),
                "invalid AHX voice dimensions"
            );
            let mut writer = voice_writer(&temporary, info.channels(), info.sample_rate())?;
            while let Some(pcm) = decoder.next_block()? {
                for &sample in pcm {
                    writer.write_sample(sample)?;
                }
            }
            writer.finalize()?;
            (info.channels(), info.sample_rate(), info.samples())
        }
        VoiceFormat::Adx => {
            let mut decoder = super::adx::Decoder::new(member.data)?;
            let info = (decoder.channels, decoder.sample_rate, decoder.frames);
            let mut writer = voice_writer(&temporary, info.0, info.1)?;
            while let Some(pcm) = decoder.next_block()? {
                for &sample in pcm {
                    writer.write_sample(sample)?;
                }
            }
            writer.finalize()?;
            info
        }
    };
    let wave = hound::WavReader::open(&temporary)?;
    ensure!(
        wave.duration() == frames,
        "decoded voice sample count differs from its header"
    );
    drop(wave);
    fs::rename(&temporary, &target)?;
    Ok(Voice {
        asset: Asset {
            path: path.into(),
            sha256: hash_file(&target)?,
        },
        frames,
        sample_rate: source_rate,
        source_sample_rate: source_rate,
        channels,
        source_name: member.name.into(),
        source_sha256: crate::digest(member.data),
    })
}

fn voice_writer(
    path: &Path,
    channels: u16,
    sample_rate: u32,
) -> Result<hound::WavWriter<std::io::BufWriter<fs::File>>> {
    Ok(hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?)
}

fn current(root: &Path, manifest: &FieldAudio) -> Result<bool> {
    if manifest.validate().is_err() {
        return Ok(false);
    }
    for asset in manifest.music.values().chain(manifest.sounds.values()) {
        if hash_file(&root.join(&asset.path)).ok().as_ref() != Some(&asset.sha256)
            || Package::load(root, &asset.path).is_err()
        {
            return Ok(false);
        }
    }
    for voice in manifest.voices.values() {
        if hash_file(&root.join(&voice.asset.path)).ok().as_ref() != Some(&voice.asset.sha256) {
            return Ok(false);
        }
        let wave = hound::WavReader::open(root.join(&voice.asset.path))
            .with_context(|| format!("read cooked voice {}", voice.asset.path))?;
        let spec = wave.spec();
        if wave.duration() != voice.frames
            || spec.sample_rate != voice.source_sample_rate
            || spec.channels != voice.channels
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn write_package(workspace: &Workspace, path: &str, package: &Package) -> Result<Asset> {
    resonance_content::validate_asset_path(path)?;
    write_json(
        &workspace.output.join(path),
        &serde_json::to_value(package)?,
    )?;
    Package::load(&workspace.output, path)?;
    Ok(Asset {
        sha256: hash_file(&workspace.output.join(path))
            .with_context(|| format!("hash cooked audio {path}"))?,
        path: path.into(),
    })
}

pub(crate) fn sound_score(
    id: u16,
    notes: Option<Vec<resonance_audio::data::Note>>,
) -> resonance_audio::data::Score {
    use resonance_audio::{
        data::{Event, EventKind, Score},
        music_voice::Controls,
    };
    Score {
        origin: resonance_audio::data::ScoreOrigin::SoundEffect,
        initial_bpm_1024: 120 * 1024,
        loop_start_tick: 0,
        end_tick: 65535,
        has_master_track: false,
        tempos: Vec::new(),
        controls: [Controls::default(); 16],
        first_events: notes
            .into_iter()
            .map(|voices| Event {
                tick: 0,
                channel: 0,
                kind: EventKind::Notes {
                    source: resonance_audio::data::VoiceSource::SoundEffect { id },
                    voices,
                    length: 65535,
                },
            })
            .collect(),
        loop_events: Vec::new(),
    }
}
fn read_file(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("read {}", path.display()))
}

pub(crate) fn music_path(executable: &[u8], id: u16) -> Result<String> {
    crate::music_directory::Directory::read(executable)?.path(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both original discs; compares derived controls without audio playback"]
    fn original_stream_voice_gains_and_pan_preserve_derived_bits() -> Result<()> {
        for disc in [1, 2] {
            let executable = fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("../../local/extracted/disc{disc}/sys/main.dol")),
            )?;
            let mix = super::super::sound_buses::tables(&executable)?;
            let volume = crate::dol::slice(&executable, 0x802b4680, 1000)?;
            let expected: Vec<_> = crate::dol::slice(&executable, 0x801f9bc4, 128)?
                .iter()
                .map(|&db| mix.volume[usize::from(volume[usize::from(db) * 10])].to_bits())
                .collect();
            assert_eq!(
                voice_gains(&executable)?
                    .into_iter()
                    .map(f32::to_bits)
                    .collect::<Vec<_>>(),
                expected
            );
            let expected: Vec<_> = crate::dol::slice(&executable, 0x802b4a68, 31 * 4)?
                .chunks_exact(4)
                .map(|word| {
                    let pan = u32::from_be_bytes(word.try_into().unwrap()) as u8;
                    mix.gains(127 << 16, 16383, pan, [0; 2])[0]
                        .map(|gain| (f32::from(gain) / 32768.).to_bits())
                })
                .collect();
            assert_eq!(
                voice_pan(&executable)?
                    .into_iter()
                    .map(|gains| gains.map(f32::to_bits))
                    .collect::<Vec<_>>(),
                expected
            );
        }
        Ok(())
    }
}
