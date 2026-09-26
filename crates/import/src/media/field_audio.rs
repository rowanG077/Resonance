//! Field audio bindings, using original resource tables and Rust synthesis.
use super::{Workspace, hash_file, write_json};
use anyhow::{Context, Result, ensure};
use resonance_audio::package::Package;
use resonance_content::field_audio::{Asset, FieldAudio, Voice};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
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

/// A source environment shared by every field. Banks and arrangements are
/// prepared once; fields select references into those shared publications.
pub(crate) struct FieldAudioCooker {
    workspace: Workspace,
    executable: Vec<u8>,
    coefficients: Vec<u8>,
    additional_disc: Option<PathBuf>,
    catalogue: resources::Catalogue,
    pools: super::library::Pools,
    music: BTreeMap<u16, Asset>,
    sounds: BTreeMap<(String, u16), Asset>,
    voices: BTreeMap<u32, Voice>,
    gains: Vec<f32>,
    reverbs: [[f32; 5]; 2],
}

impl FieldAudioCooker {
    pub(crate) fn new(
        workspace: Workspace,
        coefficients: &[u8],
        additional_disc: Option<PathBuf>,
    ) -> Result<Self> {
        let executable = read_file(&workspace.extracted.join("sys/main.dol"))?;
        let pools = super::library::Pools::read(&workspace.extracted)?;
        Ok(Self {
            catalogue: resources::Catalogue::read(&workspace.extracted, &executable)?,
            gains: voice_gains(&executable)?,
            reverbs: super::music::title_reverbs(&executable)?,
            workspace,
            executable,
            coefficients: coefficients.into(),
            additional_disc,
            pools,
            music: BTreeMap::new(),
            sounds: BTreeMap::new(),
            voices: BTreeMap::new(),
        })
    }

    pub(crate) fn cook(&mut self, map_id: u32, map: &crate::field::MapArchive) -> Result<()> {
        let resources = self
            .catalogue
            .resources(map.section(6)?)
            .with_context(|| format!("inventory field {map_id} audio"))?;
        let missing = resources
            .voices
            .iter()
            .filter(|id| !self.voices.contains_key(id))
            .copied()
            .collect();
        let voices = binding::voices(
            &self.workspace,
            &self.executable,
            self.additional_disc.as_deref(),
            &missing,
        )
        .with_context(|| format!("bind field {map_id} voices"))?;
        self.voices.extend(voices);
        let metadata = self.workspace.output.join(crate::field::audio_path(map_id));
        let mut music = BTreeMap::new();
        for id in resources.music {
            if !self.music.contains_key(&id) {
                let package = super::music_library::package(
                    &self.workspace,
                    &self.executable,
                    &self.coefficients,
                    &self.pools,
                    id,
                    Some(self.reverbs),
                )?;
                self.music.insert(id, self.publish(&package)?);
            }
            music.insert(i16::try_from(id)?, self.music[&id].clone());
        }
        let mut sounds = BTreeMap::new();
        for (source, ids) in resources.banks {
            for id in ids {
                let key = (source.clone(), id);
                if !self.sounds.contains_key(&key) {
                    let bank = self.pools.bank(self.catalogue.bank(&source))?;
                    let (resources, score) = super::sound_library::sound(&bank, id)?;
                    let package = super::sound_library::package(
                        &self.workspace.output,
                        &resources,
                        score,
                        super::synthesis_tables(&self.executable, &self.coefficients)?,
                        self.reverbs,
                    )?;
                    self.sounds.insert(key.clone(), self.publish(&package)?);
                }
                sounds.insert(i16::try_from(id)?, self.sounds[&key].clone());
            }
        }
        let manifest = FieldAudio {
            version: FieldAudio::VERSION,
            music,
            sounds,
            voices: resources
                .voices
                .into_iter()
                .map(|id| (id, self.voices[&id].clone()))
                .collect(),
            voice_gains: self.gains.clone(),
        };
        manifest.validate()?;
        write_json(&metadata, &serde_json::to_value(manifest)?)
    }

    fn publish(&self, package: &Package) -> Result<Asset> {
        let hash = crate::digest(&serde_json::to_vec(package)?);
        write_package(
            &self.workspace,
            &format!("audio/programs/{hash}.json"),
            package,
        )
    }
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
    let sha256 = hash_file(&temporary)?;
    crate::publication::install(&temporary, &target, &sha256)?;
    Ok(Voice {
        asset: Asset {
            path: path.into(),
            sha256,
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
