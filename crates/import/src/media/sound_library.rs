//! Bind physical sound programs and shared PCM to the scene's mixer environment.
use super::{Workspace, field_audio::write_package};
use anyhow::{Context, Result, ensure};
use resonance_audio::{
    data::{Command, Score},
    package::{Package, SampleAsset},
};
use resonance_audio_cook::decode::Instruction;
use resonance_content::field_audio::Asset;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Serialize, Deserialize)]
pub(crate) struct Resources {
    pub version: u32,
    pub programs: BTreeMap<u16, Vec<Instruction>>,
    pub samples: BTreeMap<u16, SampleAsset>,
    pub score: Option<Score>,
}

impl Resources {
    pub(super) fn package(
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

pub(super) fn source_index(root: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    serde_json::from_slice(&fs::read(root.join("sources.json")).with_context(|| {
        format!(
            "missing cooked source index; run cook-all --output {} first",
            root.display()
        )
    })?)
    .context("read cooked source index")
}

pub(crate) fn bind_sounds(
    workspace: &Workspace,
    executable: &[u8],
    coefficients: &[u8],
    reverbs: [[f32; 5]; 2],
    prefix: &str,
    banks: &[(impl AsRef<str>, Vec<u16>)],
) -> Result<BTreeMap<i16, Asset>> {
    let sources = source_index(&workspace.output)?;
    let mut assets = BTreeMap::new();
    for (source, ids) in banks {
        let source = format!("disc{}/{}", workspace.disc, source.as_ref());
        let outputs = sources
            .get(&source)
            .with_context(|| format!("missing cooked {source}; rerun cook-all"))?;
        for &id in ids {
            let key = i16::try_from(id).context("sound ID exceeds runtime range")?;
            ensure!(!assets.contains_key(&key), "ambiguous cooked sound {id}");
            let suffix = format!("/sound-{id}.json");
            let mut paths = outputs
                .iter()
                .filter(|path| path.starts_with("audio/banks/") && path.ends_with(&suffix));
            let path = paths
                .next()
                .with_context(|| format!("missing cooked {source} sound {id}"))?;
            ensure!(
                paths.next().is_none(),
                "ambiguous cooked {source} sound {id}"
            );
            resonance_content::validate_asset_path(path)?;
            let resources: Resources =
                serde_json::from_slice(&fs::read(workspace.output.join(path))?)
                    .with_context(|| format!("read cooked sound {path}; rerun cook-all"))?;
            let package = resources
                .package(super::synthesis_tables(executable, coefficients)?, reverbs)
                .with_context(|| format!("bind {source} sound {id}"))?;
            assets.insert(
                key,
                write_package(workspace, &format!("audio/{prefix}-{id}.json"), &package)?,
            );
        }
    }
    Ok(assets)
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
    #[ignore = "requires cook-all and original mixer tables; no source bank, conversion or playback"]
    fn original_shared_sounds_bind_without_source_banks_or_sample_conversion() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let library = local.join("all-assets");
        let sources = source_index(&library)?;
        let ids = vec![1, 2, 3, 4];
        let source = "S/se.snd";
        let paths = &sources["disc1/S/se.snd"];
        let root = crate::temporary_path(&std::env::temp_dir().join("resonance-sound-binding"));
        let result = (|| -> Result<()> {
            let extracted = root.join("extracted");
            let output = root.join("cooked");
            fs::create_dir_all(extracted.join("sys"))?;
            let mut selected = Vec::new();
            for id in &ids {
                let suffix = format!("/sound-{id}.json");
                let path = paths
                    .iter()
                    .find(|path| path.ends_with(&suffix))
                    .context("missing source cue")?;
                let bytes = fs::read(library.join(path))?;
                let resources: Resources = serde_json::from_slice(&bytes)?;
                crate::write_atomic(&output.join(path), &bytes)?;
                for sample in resources.samples.values() {
                    let destination = output.join(&sample.path);
                    if !destination.is_file() {
                        fs::create_dir_all(destination.parent().unwrap())?;
                        fs::copy(library.join(&sample.path), destination)?;
                    }
                }
                selected.push(path.clone());
            }
            let index =
                BTreeMap::from([("disc1/S/se.snd", &selected), ("disc2/S/se.snd", &selected)]);
            crate::write_atomic(&output.join("sources.json"), &serde_json::to_vec(&index)?)?;
            let executable = fs::read(local.join("extracted/disc1/sys/main.dol"))?;
            let coefficients = fs::read(
                std::env::var_os("RESONANCE_DSP_COEFFICIENTS")
                    .context("set RESONANCE_DSP_COEFFICIENTS to Dolphin's dsp_coef.bin")?,
            )?;
            let reverbs = super::super::music::title_reverbs(&executable)?;
            for disc in [1, 2] {
                let mut boot = *b"GQSEAF\0\0";
                boot[6] = disc - 1;
                fs::write(extracted.join("sys/boot.bin"), boot)?;
                let workspace = Workspace::open(&extracted, &output)?;
                let bound = bind_sounds(
                    &workspace,
                    &executable,
                    &coefficients,
                    reverbs,
                    "binding-test",
                    &[(source, ids.clone())],
                )?;
                assert_eq!(bound.len(), 4);
                assert!(!extracted.join("files").exists());
                for asset in bound.values() {
                    let package: Package =
                        serde_json::from_slice(&fs::read(output.join(&asset.path))?)?;
                    assert!(
                        package
                            .samples
                            .values()
                            .all(|sample| sample.path.starts_with("audio/samples/"))
                    );
                    let resonance_audio::data::EventKind::Notes { voices, .. } =
                        &package.score.first_events[0].kind
                    else {
                        anyhow::bail!("sound has no initial voices");
                    };
                    assert!(
                        voices
                            .iter()
                            .all(|voice| voice.priority == 10 && voice.max_voices == 255)
                    );
                }
                let mut missing = ids.clone();
                missing.push(32767);
                assert!(
                    bind_sounds(
                        &workspace,
                        &executable,
                        &coefficients,
                        reverbs,
                        "binding-test",
                        &[(source, missing)]
                    )
                    .is_err()
                );
            }
            Ok(())
        })();
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        result
    }
}
