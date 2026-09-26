//! Prepare original sound programs for the scene's mixer environment.
use anyhow::{Context, Result, ensure};
use resonance_audio::{
    data::{Command, Score},
    package::{Package, SampleAsset},
};
use resonance_audio_cook::{
    bank::{Bank, Page},
    decode::{self, Instruction},
    instrument,
};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::fs;
use std::{collections::BTreeMap, path::Path};

#[derive(Serialize, Deserialize)]
pub(crate) struct Resources {
    pub version: u32,
    pub programs: BTreeMap<u16, Vec<Instruction>>,
    pub samples: BTreeMap<u16, SampleAsset>,
    pub score: Option<Score>,
}

impl Resources {
    pub(crate) fn package(
        self,
        tables: resonance_audio::music_voice::Tables,
        reverbs: [[f32; 5]; 2],
    ) -> Result<Package> {
        ensure!(
            self.version == 1,
            "unsupported cooked sound version; rerun cook-all"
        );
        let programs = self
            .programs
            .into_iter()
            .map(|(id, instructions)| {
                let commands = instructions
                    .into_iter()
                    .enumerate()
                    .map(|(pc, instruction)| {
                        instruction
                            .mixer()
                            .with_context(|| format!("instrument {id}:{pc}"))
                    })
                    .collect::<Result<Vec<Command>>>()?;
                Ok((id, commands))
            })
            .collect::<Result<_>>()?;
        let resources = resonance_audio::data::Resources {
            programs,
            samples: BTreeMap::new(),
        };
        let score = self.score.context("cooked audio has no score")?;
        // Score validation needs program identities, without loading instrument PCM.
        score.validate(&resources)?;
        Ok(Package {
            version: resonance_audio::package::VERSION,
            programs: resources.programs,
            samples: self.samples,
            score,
            tables,
            reverbs,
        })
    }
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

pub(crate) fn sound(bank: &Bank<'_>, id: u16) -> Result<(decode::Resources, Score)> {
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
    let resources = decode::programs(bank, notes.iter().map(|note| note.macro_id))?;
    Ok((resources, super::sound_score(id, Some(notes))))
}

pub(crate) fn package(
    output: &Path,
    resources: &decode::Resources,
    score: Score,
    tables: resonance_audio::music_voice::Tables,
    reverbs: [[f32; 5]; 2],
) -> Result<Package> {
    Resources {
        version: 1,
        programs: resources.programs.clone(),
        samples: resources
            .samples
            .iter()
            .map(|(&id, sample)| Ok((id, super::write_shared_sample(output, sample)?)))
            .collect::<Result<_>>()?,
        score: Some(score),
    }
    .package(tables, reverbs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_audio_cook::decode::{Native, SampleOffset};

    #[test]
    fn native_operands_cannot_deserialize_as_a_simpler_mixer_command() -> Result<()> {
        let operation = Instruction::Native(Native::StartSample {
            sample: 4,
            format: 1,
            offset: 80,
            offset_mode: SampleOffset::InverseVolume,
        });
        let value = serde_json::to_value(operation)?;
        let restored: Instruction = serde_json::from_value(value.clone())?;
        assert!(matches!(
            restored,
            Instruction::Native(Native::StartSample {
                sample: 4,
                format: 1,
                offset: 80,
                offset_mode: SampleOffset::InverseVolume,
            })
        ));
        assert!(restored.mixer().is_err());
        assert!(serde_json::from_value::<Command>(value).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original discs, frozen audio and DSP coefficients; no playback"]
    fn original_sound_preparation_matches_frozen_packages_and_pcm() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let baseline = std::env::var_os("RESONANCE_AUDIO_BASELINE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| local.join("worktrees/generic-cooking/local/all-assets"));
        let sources = source_index(&baseline)?;
        let coefficients = fs::read(
            std::env::var_os("RESONANCE_DSP_COEFFICIENTS")
                .context("set RESONANCE_DSP_COEFFICIENTS to Dolphin's dsp_coef.bin")?,
        )?;
        let banks = [("S/se.snd", vec![1, 2, 3, 4]), ("S/se_ev00.snd", vec![425])];
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let output = tempfile::tempdir()?;
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let reverbs = super::super::music::title_reverbs(&executable)?;
            let pools = crate::media::library::Pools::read(&extracted)?;
            for (source, ids) in &banks {
                let paths = &sources[&format!("disc{disc}/{source}")];
                let bytes = fs::read(extracted.join("files").join(source))?;
                let bank = pools.bank(&bytes)?;
                for &id in ids {
                    let frozen = paths
                        .iter()
                        .find(|path| path.ends_with(&format!("/sound-{id}.json")))
                        .context("missing frozen sound")?;
                    let expected: Resources =
                        serde_json::from_slice(&fs::read(baseline.join(frozen))?)?;
                    let expected = expected.package(
                        super::super::synthesis_tables(&executable, &coefficients)?,
                        reverbs,
                    )?;
                    let (resources, score) = sound(&bank, id)?;
                    let actual = package(
                        output.path(),
                        &resources,
                        score,
                        super::super::synthesis_tables(&executable, &coefficients)?,
                        reverbs,
                    )?;
                    assert_eq!(
                        serde_json::to_vec(&actual)?,
                        serde_json::to_vec(&expected)?,
                        "disc{disc} {source} sound {id}"
                    );
                    for sample in actual.samples.values() {
                        assert_eq!(
                            fs::read(output.path().join(&sample.path))?,
                            fs::read(baseline.join(&sample.path))?,
                            "sound {id} shared PCM"
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
