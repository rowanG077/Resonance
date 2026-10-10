//! Prepare original sound programs for the scene's mixer environment.
#[cfg(test)]
use anyhow::Context;
use anyhow::Result;
use resonance_audio::{
    data::{Command, Score},
    package::{Package, SampleAsset},
};
use resonance_audio_cook::{
    bank::{Bank, Page},
    decode, instrument,
};
use serde::Serialize;
#[cfg(test)]
use std::fs;
use std::{collections::BTreeMap, path::Path};

pub(crate) const VERSION: u32 = 4;

/// Compiled program export, before binding a scene's synthesis tables.
#[derive(Serialize)]
pub(crate) struct Resources {
    pub version: u32,
    pub programs: BTreeMap<u16, Vec<Command>>,
    pub samples: BTreeMap<u16, SampleAsset>,
    pub score: Option<Score>,
}

#[cfg(test)]
pub(super) fn source_index(root: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    serde_json::from_slice(&fs::read(root.join("sources.json")).with_context(|| {
        format!(
            "missing cooked source index; run cook-all --output {} first",
            root.display()
        )
    })?)
    .context("read cooked source index")
}

pub(crate) fn sound(
    bank: &Bank<'_>,
    id: u16,
    sustains: &resonance_audio_cook::parameters::Sustains,
) -> Result<(decode::Resources, Score)> {
    let sound = bank.sound(id)?;
    let notes = instrument::resolve(
        bank,
        Page {
            object: sound.object,
            priority: sound.priority,
            max_voices: sound.max_voices,
        },
        sound.key,
        sound.volume,
        sound.pan,
    )?;
    let resources = decode::programs(bank, notes.iter().map(|note| note.macro_id), sustains)?;
    Ok((resources, super::sound_score(id, Some(notes))))
}

pub(crate) fn package(
    output: &Path,
    resources: &decode::Resources,
    score: Score,
    tables: resonance_audio::music_voice::Tables,
    reverbs: [[f32; 5]; 2],
) -> Result<Package> {
    Ok(Package {
        version: resonance_audio::package::VERSION,
        programs: resources.programs.clone(),
        samples: resources
            .samples
            .iter()
            .map(|(&id, sample)| Ok((id, super::write_shared_sample(output, sample)?)))
            .collect::<Result<_>>()?,
        score,
        tables,
        reverbs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires both extracted discs; CPU package and PCM validation"]
    fn sound_packages_close_instrument_dependencies_and_preserve_sample_data() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let banks = [("S/se.snd", vec![1, 2, 3, 4]), ("S/se_ev00.snd", vec![425])];
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let output = tempfile::tempdir()?;
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let reverbs = super::super::music::title_reverbs(&executable)?;
            let pools = crate::media::library::Pools::read(&extracted)?;
            for (source, ids) in &banks {
                let bytes = fs::read(extracted.join("files").join(source))?;
                let bank = pools.bank(&bytes)?;
                for &id in ids {
                    let (resources, score) = sound(&bank, id, &pools.sustains)?;
                    let actual = package(
                        output.path(),
                        &resources,
                        score,
                        super::super::synthesis_tables(&executable)?,
                        reverbs,
                    )?;
                    let loaded = Package::load_with(
                        "cue.json",
                        &mut |path, _| {
                            if path == "cue.json" {
                                Ok(serde_json::to_vec(&actual)?)
                            } else {
                                Ok(fs::read(output.path().join(path))?)
                            }
                        },
                        &mut Default::default(),
                    )?;
                    assert_eq!(loaded.reverbs(), reverbs);
                    let native = loaded.resources();
                    assert!(native.programs.keys().eq(resources.programs.keys()));
                    assert!(native.samples.keys().eq(resources.samples.keys()));
                    assert!(!native.samples.is_empty());
                    for (&sample_id, sample) in &native.samples {
                        let decoded = bank.sample(sample_id)?;
                        assert_eq!(sample.key, decoded.key);
                        assert_eq!(sample.rate, decoded.rate);
                        assert_eq!(sample.loop_start, decoded.loop_start);
                        assert_eq!(sample.loop_length, decoded.loop_length);
                        assert_eq!(
                            sample.pcm, decoded.pcm,
                            "disc{disc} {source} sound {id} sample {sample_id}"
                        );
                        assert_eq!(sample.loop_pcm, decoded.loop_pcm);
                    }
                }
            }
            super::super::sound_buses::render_sound_buses(
                &extracted,
                &extracted.join("files/S/se_ev00.snd"),
                425,
                output.path(),
            )?;
            let mut wave = hound::WavReader::open(output.path().join("sound-425-direct.wav"))?;
            assert!(wave.samples::<i16>().any(|s| s.is_ok_and(|s| s != 0)));
        }
        Ok(())
    }
}
