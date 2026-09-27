//! Resource inventory across every scenario branch and message, before cooking.
use anyhow::{Context, Result, ensure};
use resonance_audio_cook::bank::Bank;
use resonance_content::field_audio::ServiceCue;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use symphonia_script::{NativeCall, Op, Program, message, scenario};

pub(super) struct Resources {
    pub music: BTreeSet<u16>,
    pub voices: BTreeSet<u32>,
    pub banks: Vec<(String, Vec<u16>)>,
}

impl Resources {
    #[cfg(test)]
    pub fn read(extracted: &Path, executable: &[u8], script: &[u8]) -> Result<Self> {
        Catalogue::read(extracted, executable)?.resources(script)
    }
}

pub(super) struct Catalogue {
    music: BTreeSet<u16>,
    banks: BTreeMap<String, SoundBank>,
}

struct SoundBank {
    bytes: Vec<u8>,
    ids: BTreeSet<u16>,
}

impl Catalogue {
    pub fn read(extracted: &Path, executable: &[u8]) -> Result<Self> {
        let directory = crate::event_bank_directory::Directory::read(executable)?;
        let [_, common] = crate::all_assets::roles::resident_banks(extracted, executable)?;
        let sources = directory
            .entries
            .iter()
            .map(|entry| entry.source_path())
            .chain([Ok(common.clone())])
            .collect::<Result<BTreeSet<_>>>()?;
        let banks = sources
            .into_iter()
            .map(|source| {
                let bytes = super::read_file(&extracted.join("files").join(&source))?;
                let ids = Bank::parse(&bytes)?.sound_ids().collect();
                Ok((source, SoundBank { bytes, ids }))
            })
            .collect::<Result<_>>()?;
        let music = crate::music_directory::Directory::read(executable)?
            .active()
            .map(|entry| u16::try_from(entry.id))
            .collect::<Result<_, _>>()?;
        Ok(Self { music, banks })
    }

    pub fn bank(&self, source: &str) -> &[u8] {
        &self.banks[source].bytes
    }

    pub fn resources(&self, script: &[u8]) -> Result<Resources> {
        self.with_native(script, &[], &[])
    }

    pub fn with_native(
        &self,
        script: &[u8],
        native_music: &[u16],
        native_sounds: &[u16],
    ) -> Result<Resources> {
        let mut music = arguments(script, NativeCall::AudioCommand, 1)?
            .map(|ids| {
                ids.into_iter()
                    .filter(|id| *id >= 0)
                    .map(u16::try_from)
                    .collect::<Result<_, _>>()
            })
            .transpose()?
            .unwrap_or_else(|| self.music.clone());
        music.extend(native_music);
        let simple = arguments(script, NativeCall::PlaySoundSimple, 2)?;
        let extended = arguments(script, NativeCall::PlaySound, 4)?;
        let ambient: Option<BTreeSet<i32>> =
            crate::field_resources::literal_arguments(script, NativeCall::SetActorAmbientSound, 4)?
                .into_iter()
                .map(|args| args[1])
                .collect();
        let dynamic_sounds = simple.is_none() || extended.is_none() || ambient.is_none();
        let mut sounds: BTreeSet<_> = simple
            .into_iter()
            .flatten()
            .chain(extended.into_iter().flatten())
            .chain(ambient.into_iter().flatten())
            // Negative sound IDs update or stop an existing slot.
            .filter(|id| *id >= 0)
            .map(u16::try_from)
            .collect::<Result<_, _>>()?;
        sounds.extend(ServiceCue::ALL.iter().map(|cue| *cue as u16));
        sounds.extend(native_sounds);
        let voices = voices(script)?;

        // Bank selection persists across fields. Resolve sound ownership from
        // the shared catalogue instead of requiring a local bank-load call.
        if dynamic_sounds {
            sounds.extend(self.banks.values().flat_map(|bank| &bank.ids));
        }
        let mut owned = BTreeSet::new();
        let mut banks = Vec::new();
        for (source, bank) in &self.banks {
            let ids: Vec<_> = bank.ids.intersection(&sounds).copied().collect();
            for &id in &ids {
                ensure!(owned.insert(id), "ambiguous field sound {id}");
            }
            if !ids.is_empty() {
                banks.push((source.clone(), ids));
            }
        }
        let missing: Vec<_> = sounds.difference(&owned).collect();
        ensure!(
            missing.is_empty(),
            "field sounds {missing:?} are absent from the sound catalogue"
        );
        Ok(Resources {
            music,
            voices,
            banks,
        })
    }
}

/// A dynamic ID selects from its complete declared catalogue, never a guessed ID.
fn arguments(script: &[u8], call: NativeCall, arity: usize) -> Result<Option<BTreeSet<i32>>> {
    Ok(
        crate::field_resources::literal_arguments(script, call, arity)?
            .into_iter()
            .map(|args| args[0])
            .collect(),
    )
}

fn voices(script: &[u8]) -> Result<BTreeSet<u32>> {
    let offset = scenario::parse_header(script)?.auxiliary_offset();
    if offset == 0 {
        return Ok(BTreeSet::new());
    }
    let mut voices = BTreeSet::new();
    for message in message::parse(
        script
            .get(offset..)
            .context("invalid message table offset")?,
    )? {
        for token in message.tokens {
            if let message::Token::Control {
                opcode: 9,
                expression,
            } = token
            {
                let mut bytes = vec![0, 4, 0, 0, 0, 0, 0, 0];
                bytes.extend(expression);
                let program = Program::decode(&bytes)?;
                let mut pc = 0;
                let mut ops = Vec::new();
                loop {
                    let (op, next) = program
                        .instruction(pc)
                        .context("invalid voice expression")?;
                    ops.push(op);
                    if op == Op::End {
                        break;
                    }
                    ensure!(
                        ops.len() < 4,
                        "dynamic message voice needs a cooking recipe"
                    );
                    pc = next;
                }
                let [Op::Push(id), Op::Calculate(0), Op::End] = ops.as_slice() else {
                    anyhow::bail!("dynamic message voice needs a cooking recipe");
                };
                if *id != -1 {
                    voices.insert(u32::try_from(*id).context("invalid message voice ID")?);
                }
            }
        }
    }
    Ok(voices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_dependencies_do_not_require_a_local_bank_switch() -> Result<()> {
        let catalogue = Catalogue {
            music: [1, 2].into(),
            banks: [
                (
                    "common".into(),
                    ServiceCue::ALL.iter().map(|cue| *cue as u16).collect(),
                ),
                ("event".into(), [447].into()),
            ]
            .into_iter()
            .map(|(source, ids)| {
                (
                    source,
                    SoundBank {
                        bytes: Vec::new(),
                        ids,
                    },
                )
            })
            .collect(),
        };
        let source = ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\n\
            push.s16 447\ncalc 0\narg\npush.s8 0\ncalc 0\narg\nproc 0x52\nend\n";
        let resources = catalogue.resources(&scenario::assemble(source)?)?;
        assert!(resources.banks.contains(&("event".into(), vec![447])));
        let dynamic = source.replace("push.s16 447", "load.s32 0x800");
        assert_eq!(
            catalogue.resources(&scenario::assemble(&dynamic)?)?.banks,
            resources.banks
        );
        let missing = source.replace("push.s16 447", "push.s16 32767");
        assert!(catalogue.resources(&scenario::assemble(&missing)?).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; inventories every field without audio playback"]
    fn original_all_fields_have_audio_resources() -> Result<()> {
        let mut failures = Vec::new();
        let mut count = 0;
        for disc in [1, 2] {
            let extracted = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            let executable = std::fs::read(extracted.join("sys/main.dol"))?;
            let catalogue = Catalogue::read(&extracted, &executable)?;
            for source in crate::field_catalogue::map_paths(&extracted)? {
                let map = crate::field::MapArchive::open(&extracted.join("files").join(&source))?;
                match map
                    .section(6)
                    .and_then(|script| catalogue.resources(script))
                {
                    Ok(_) => count += 1,
                    Err(error) => failures.push(format!("disc{disc}/{source}: {error:#}")),
                }
            }
        }
        ensure!(
            failures.is_empty(),
            "{count} fields passed; {} failures:\n{}",
            failures.len(),
            failures.join("\n")
        );
        println!("Inventoried {count} field audio resource sets across both discs");
        Ok(())
    }

    #[test]
    #[ignore = "requires both original discs; inventories source banks without audio playback"]
    fn original_event_bank_inventory_preserves_every_declared_sound() -> Result<()> {
        use crate::read::{u16 as half, u32 as word};
        use std::{collections::BTreeMap, fs};

        const HEADER: &str = ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\n";
        for disc in [1, 2] {
            let extracted = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let mut sources = vec!["S/se.snd".to_owned()];
            for row in crate::dol::slice(&executable, 0x801f97e0, 8 * 8)?.chunks_exact(8) {
                sources.push(crate::source_path(&crate::dol::text(
                    &executable,
                    word(row, 0)?,
                )?)?);
            }
            let initial = Resources::read(
                &extracted,
                &executable,
                &scenario::assemble(&format!("{HEADER}end\n"))?,
            )?;
            assert_eq!(
                initial.banks.iter().map(|bank| &bank.0).collect::<Vec<_>>(),
                [&sources[0]]
            );

            let mut expected = BTreeMap::new();
            let mut script = HEADER.to_owned();
            for selector in -8..0 {
                script += &format!(
                    "push.s32 {selector}\ncalc 0\narg\nproc 0x{:02x}\n",
                    NativeCall::SelectAudioBank as u8
                );
            }
            for source in sources {
                let bytes = fs::read(extracted.join("files").join(&source))?;
                let project = word(&bytes, 4)? as usize;
                let table = project + word(&bytes, project + 28)? as usize;
                let ids = (0..usize::from(half(&bytes, table)?))
                    .map(|index| half(&bytes, table + 4 + index * 10))
                    .collect::<Result<Vec<_>>>()?;
                for id in &ids {
                    script += &format!(
                        "push.s32 {id}\ncalc 0\narg\npush.s8 0\ncalc 0\narg\nproc 0x{:02x}\n",
                        NativeCall::PlaySoundSimple as u8
                    );
                }
                expected.insert(source, ids);
            }
            script += "end\n";
            let resources =
                Resources::read(&extracted, &executable, &scenario::assemble(&script)?)?;
            assert_eq!(
                resources
                    .banks
                    .iter()
                    .map(|(_, ids)| ids.len())
                    .sum::<usize>(),
                501
            );
            assert_eq!(
                resources.banks.into_iter().collect::<BTreeMap<_, _>>(),
                expected
            );
            assert!(resources.music.is_empty() && resources.voices.is_empty());
        }
        Ok(())
    }

    #[test]
    #[ignore = "refreshes disposable RESONANCE_WORLD_ASSETS audio inventories from original scripts"]
    fn original_fields_prepare_all_scripted_sounds() -> Result<()> {
        use resonance_content::{
            field::FieldAssets, field_audio::FieldAudio, field_preload::Manifest,
        };
        use std::{fs, path::PathBuf};

        let root = PathBuf::from(
            std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
        );
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let catalogue = Catalogue::read(&extracted, &fs::read(extracted.join("sys/main.dol"))?)?;
        let mut fields = Vec::new();
        let mut sounds = BTreeMap::new();
        for entry in fs::read_dir(root.join("fields"))? {
            let path = entry?.path();
            if !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("map-")
                || !path.to_string_lossy().ends_with(".preload.json")
            {
                continue;
            }
            let manifest: Manifest = serde_json::from_slice(&fs::read(path)?)?;
            for path in &manifest.inputs.audio {
                let bank: FieldAudio = serde_json::from_slice(&fs::read(root.join(path))?)?;
                sounds.extend(bank.sounds);
            }
            fields.push(manifest);
        }
        // Reuse already published programs, cooking only missing shared sounds.
        let missing: Vec<_> = catalogue
            .banks
            .iter()
            .filter_map(|(source, bank)| {
                let ids: Vec<_> = bank
                    .ids
                    .iter()
                    .copied()
                    .filter(|id| !sounds.contains_key(&(*id as i16)))
                    .collect();
                (!ids.is_empty()).then(|| (source.clone(), ids))
            })
            .collect();
        if !missing.is_empty() {
            let mut cooker = super::super::FieldAudioCooker::new(
                crate::media::Workspace::open(&extracted, &root)?,
                None,
            )?;
            let path = "audio/field-sound-refresh.json";
            cooker.cook_resources(
                path,
                Resources {
                    music: BTreeSet::new(),
                    voices: BTreeSet::new(),
                    banks: missing,
                },
            )?;
            let bank: FieldAudio = serde_json::from_slice(&fs::read(root.join(path))?)?;
            sounds.extend(bank.sounds);
            fs::remove_file(root.join(path))?;
        }
        let mut refreshed = 0;
        for manifest in fields {
            let path = root.join(&manifest.inputs.field);
            let mut field: FieldAssets = serde_json::from_slice(&fs::read(&path)?)?;
            let required = catalogue.resources(&fs::read(root.join(&field.script.path))?)?;
            let mut changed = false;
            for audio in &manifest.inputs.audio {
                let path = root.join(audio);
                let mut bank: FieldAudio = serde_json::from_slice(&fs::read(&path)?)?;
                let before = bank.sounds.len();
                for id in required.banks.iter().flat_map(|(_, ids)| ids) {
                    bank.sounds.entry(*id as i16).or_insert(
                        sounds
                            .get(&(*id as i16))
                            .with_context(|| format!("sound {id} needs cooking"))?
                            .clone(),
                    );
                }
                if bank.sounds.len() != before {
                    crate::write_atomic(&path, &serde_json::to_vec_pretty(&bank)?)?;
                    field
                        .files
                        .insert(audio.clone(), crate::media::hash_file(&path)?);
                    changed = true;
                }
            }
            if changed {
                crate::write_atomic(&path, &serde_json::to_vec_pretty(&field)?)?;
                ensure!(
                    crate::field_preload::cook(&root, manifest.inputs)?
                        .missing_inputs
                        .is_empty(),
                    "missing audio inputs"
                );
                refreshed += 1;
            }
        }
        println!("Refreshed {refreshed} field audio inventories");
        Ok(())
    }

    #[test]
    fn inventories_both_audio_branches_without_guessing_dynamic_ids() {
        let source = ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\n\
            push.s8 0\ncalc 0\nbranch_false other\n\
            push.s8 6\ncalc 0\narg\nload.s32 0x800\ncalc 0\narg\nproc 0x52\njump done\n\
            other:\npush.s8 37\ncalc 0\narg\npush.s8 0\ncalc 0\narg\nproc 0x52\n\
            done:\nend\n";
        let script = scenario::assemble(source).unwrap();
        assert_eq!(
            arguments(&script, NativeCall::PlaySoundSimple, 2).unwrap(),
            Some([6, 37].into())
        );
        let dynamic = scenario::assemble(&source.replace("push.s8 37", "load.s32 0x800")).unwrap();
        assert_eq!(
            arguments(&dynamic, NativeCall::PlaySoundSimple, 2).unwrap(),
            None
        );
    }

    #[test]
    fn inventories_actor_ambient_sound_ids_without_confusing_the_actor_argument() {
        let catalogue = Catalogue {
            music: BTreeSet::new(),
            banks: [(
                "ambient.snd".into(),
                SoundBank {
                    bytes: Vec::new(),
                    ids: ServiceCue::ALL
                        .iter()
                        .map(|cue| *cue as u16)
                        .chain([286, 300])
                        .collect(),
                },
            )]
            .into(),
        };
        let source = ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\n\
            push.s16 300\ncalc 0\narg\npush.s16 286\ncalc 0\narg\n\
            push.s8 100\ncalc 0\narg\npush.s16 800\ncalc 0\narg\nproc 0x53\nend\n";
        let script = scenario::assemble(source).unwrap();
        let sounds = &catalogue.resources(&script).unwrap().banks[0].1;
        assert!(sounds.contains(&286));
        assert!(!sounds.contains(&300));
        let dynamic =
            scenario::assemble(&source.replace("push.s16 286", "load.s32 0x800")).unwrap();
        let sounds = &catalogue.resources(&dynamic).unwrap().banks[0].1;
        assert!(sounds.contains(&286) && sounds.contains(&300));
    }

    #[test]
    #[ignore = "audits every RESONANCE_WORLD_ASSETS field against original script sound dependencies"]
    fn original_prepared_fields_include_every_scripted_sound() -> Result<()> {
        use resonance_content::{
            field::FieldAssets, field_audio::FieldAudio, field_preload::Manifest,
        };
        use std::{fs, path::PathBuf};
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
        );
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let catalogue = Catalogue::read(&extracted, &fs::read(extracted.join("sys/main.dol"))?)?;
        let mut missing = Vec::new();
        let mut fields = 0;
        for entry in fs::read_dir(root.join("fields"))? {
            let path = entry?.path();
            if !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("map-")
                || !path.to_string_lossy().ends_with(".preload.json")
            {
                continue;
            }
            let manifest: Manifest = serde_json::from_slice(&fs::read(path)?)?;
            let field: FieldAssets =
                serde_json::from_slice(&fs::read(root.join(&manifest.inputs.field))?)?;
            let required = catalogue.resources(&fs::read(root.join(&field.script.path))?)?;
            let mut available = BTreeSet::new();
            for audio in &manifest.inputs.audio {
                let bank: FieldAudio = serde_json::from_slice(&fs::read(root.join(audio))?)?;
                for (id, asset) in bank.sounds {
                    ensure!(
                        manifest
                            .files
                            .get(&asset.path)
                            .is_some_and(|f| f.sha256 == asset.sha256),
                        "field {} sound {id} is absent from its preload inventory",
                        manifest.map_id
                    );
                    available.insert(id as u16);
                }
            }
            for id in required.banks.into_iter().flat_map(|(_, ids)| ids) {
                if !available.contains(&id) {
                    missing.push((manifest.map_id, id));
                }
            }
            fields += 1;
        }
        missing.sort_unstable();
        ensure!(
            missing.is_empty(),
            "missing prepared (field, sound) dependencies: {missing:?}"
        );
        println!("Audited {fields} fields: no missing scripted or actor ambient sounds");
        Ok(())
    }

    #[test]
    fn inventories_unreferenced_message_voices_and_rejects_dynamic_expressions() {
        fn script(expressions: &[&[u8]]) -> Vec<u8> {
            let mut bytes = vec![0, 4, 0, 0, 0, 5, 0, 0, 0x20, 0xff];
            let mut body = Vec::new();
            for expression in expressions {
                bytes.extend(
                    u16::try_from(expressions.len() * 2 + body.len())
                        .unwrap()
                        .to_be_bytes(),
                );
                body.push(9);
                body.extend_from_slice(expression);
                body.push(0);
            }
            bytes.extend(body);
            bytes
        }
        let first = [2, 0, 0, 1, 0, 10, 0x30, 0, 0x20, 0xff];
        let second = [2, 0, 0, 130, 0, 10, 0x30, 0, 0x20, 0xff];
        let stop = [0, 0xff, 0x30, 0, 0x20, 0xff];
        assert_eq!(
            voices(&script(&[&first, &second, &stop])).unwrap(),
            [0xa0001, 0xa0082].into()
        );
        let dynamic = [0x12, 0, 8, 0, 0x30, 0, 0x20, 0xff];
        assert!(
            voices(&script(&[&first, &dynamic]))
                .unwrap_err()
                .to_string()
                .contains("dynamic message voice")
        );
        assert!(
            voices(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff])
                .unwrap()
                .is_empty()
        );
    }
}
