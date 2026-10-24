//! Field audio bindings, using original resource tables and Rust synthesis.
use anyhow::{Result, ensure};
use std::{fs, path::Path};
pub(crate) mod resources;

/// Reuse the ordinary scenario/message inventory for battle-owned event scenes.
pub(crate) fn event_requirements(
    extracted: &Path,
    executable: &[u8],
    script: &[u8],
) -> Result<resonance_content::battle_audio::Selection> {
    let resources = resources::Catalogue::read(extracted, executable)?.resources(script)?;
    Ok(resonance_content::battle_audio::Selection {
        music: resources.music,
        voices: resources.voices,
        sounds: resources
            .banks
            .into_iter()
            .flat_map(|(_, ids)| ids)
            .collect(),
        streams: Default::default(),
    })
}

/// Resolve the saved setting through CRI attenuation and the stream mixer.
/// The runtime consumes amplitudes, without executable addresses or codec tables.
pub(crate) fn voice_gains(executable: &[u8]) -> Result<Vec<f32>> {
    crate::stream_mixer::Tables::read(executable)?.gains(&super::sound_buses::tables(executable)?)
}

#[derive(Clone)]
pub(crate) struct WorldVoiceArchive {
    pub source: String,
    pub member: usize,
    pub first_resource: u32,
}

pub(crate) enum VoiceFormat {
    Ahx,
    Adx,
}

/// Decode a native stream directly into an ordinary PCM WAV. The caller owns
/// naming and publication; no cooked descriptor is written or read here.
pub(crate) fn decode_voice_wave(
    target: &Path,
    bytes: &[u8],
    format: VoiceFormat,
) -> Result<(u16, u32, u32)> {
    let (channels, source_rate, frames) = match format {
        VoiceFormat::Ahx => {
            let mut decoder = ahx_rs::Decoder::new(bytes)?;
            let info = decoder.metadata();
            ensure!(
                (8_000..=96_000).contains(&info.sample_rate())
                    && (1..=32_000_000).contains(&info.samples()),
                "invalid AHX voice dimensions"
            );
            let mut writer = voice_writer(target, info.channels(), info.sample_rate())?;
            while let Some(pcm) = decoder.next_block()? {
                for &sample in pcm {
                    writer.write_sample(sample)?;
                }
            }
            writer.finalize()?;
            (info.channels(), info.sample_rate(), info.samples())
        }
        VoiceFormat::Adx => {
            let mut decoder = super::adx::Decoder::new(bytes)?;
            let info = (decoder.channels, decoder.sample_rate, decoder.frames);
            let mut writer = voice_writer(target, info.0, info.1)?;
            while let Some(pcm) = decoder.next_block()? {
                for &sample in pcm {
                    writer.write_sample(sample)?;
                }
            }
            writer.finalize()?;
            info
        }
    };
    let wave = hound::WavReader::open(target)?;
    ensure!(
        wave.duration() == frames,
        "decoded voice sample count differs from its header"
    );
    Ok((channels, source_rate, frames))
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
        end_tick: 0,
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
                    length: 0,
                },
            })
            .collect(),
        loop_events: Vec::new(),
    }
}
pub(crate) fn music_path(executable: &[u8], id: u16) -> Result<String> {
    crate::music_directory::Directory::read(executable)?.path(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both original discs; compares derived controls without audio playback"]
    fn original_stream_voice_gains_preserve_derived_bits() -> Result<()> {
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
        }
        Ok(())
    }
}
