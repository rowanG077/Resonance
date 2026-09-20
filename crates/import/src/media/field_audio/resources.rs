//! Resource inventory across every scenario branch and message, before cooking.
use anyhow::{Context, Result, ensure};
use resonance_audio_cook::bank::Bank;
use resonance_content::field_audio::ServiceCue;
use std::{collections::BTreeSet, path::Path};
use symphonia_script::{NativeCall, Op, Program, message, scenario};

pub(super) struct Resources {
    pub music: BTreeSet<u16>,
    pub sounds: BTreeSet<u16>,
    pub voices: BTreeSet<u32>,
    pub banks: Vec<(String, Vec<u16>)>,
}

impl Resources {
    pub fn read(extracted: &Path, executable: &[u8], script: &[u8]) -> Result<Self> {
        let music = arguments(script, NativeCall::AudioCommand, 1)?
            .into_iter()
            .filter(|id| *id >= 0)
            .map(u16::try_from)
            .collect::<Result<_, _>>()?;
        let mut sounds: BTreeSet<_> = arguments(script, NativeCall::PlaySoundSimple, 2)?
            .into_iter()
            .chain(arguments(script, NativeCall::PlaySound, 4)?)
            // Negative sound IDs update or stop an existing slot.
            .filter(|id| *id >= 0)
            .map(u16::try_from)
            .collect::<Result<_, _>>()?;
        sounds.extend(ServiceCue::ALL.iter().map(|cue| *cue as u16));
        let voices = voices(script)?;

        // The resident common bank is always visible. Event bank zero is loaded
        // initially; scripts select additional banks with a three-bit index.
        let directory = crate::event_bank_directory::Directory::read(executable)?;
        let mut sources = BTreeSet::from(["S/se.snd".to_owned()]);
        for selector in arguments(script, NativeCall::SelectAudioBank, 1)?
            .into_iter()
            .chain([0])
        {
            sources.insert(directory.entry(selector).source_path()?);
        }
        let mut owned = BTreeSet::new();
        let mut banks = Vec::new();
        for source in sources {
            let bytes = super::read_file(&extracted.join("files").join(&source))?;
            let bank = Bank::parse(&bytes)?;
            let ids: Vec<_> = bank.sound_ids().filter(|id| sounds.contains(id)).collect();
            for &id in &ids {
                ensure!(owned.insert(id), "ambiguous field sound {id}");
            }
            // Retain selected empty banks in the recipe's source hashes too.
            banks.push((source, ids));
        }
        let missing: Vec<_> = sounds.difference(&owned).collect();
        ensure!(
            missing.is_empty(),
            "field sounds {missing:?} are absent from its declared banks"
        );
        Ok(Self {
            music,
            sounds,
            voices,
            banks,
        })
    }
}

fn arguments(script: &[u8], call: NativeCall, arity: usize) -> Result<BTreeSet<i32>> {
    crate::field_resources::literal_arguments(script, call, arity)?
        .into_iter()
        .map(|args| {
            args[0].with_context(|| format!("dynamic {call:?} resource needs a cooking recipe"))
        })
        .collect()
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
                [&sources[0], &sources[1]]
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
            assert_eq!(resources.sounds.len(), 501);
            assert_eq!(
                resources.banks.into_iter().collect::<BTreeMap<_, _>>(),
                expected
            );
            assert!(resources.music.is_empty() && resources.voices.is_empty());
        }
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
            [6, 37].into()
        );
        let dynamic = scenario::assemble(&source.replace("push.s8 37", "load.s32 0x800")).unwrap();
        assert!(
            arguments(&dynamic, NativeCall::PlaySoundSimple, 2)
                .unwrap_err()
                .to_string()
                .contains("dynamic PlaySoundSimple resource")
        );
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
