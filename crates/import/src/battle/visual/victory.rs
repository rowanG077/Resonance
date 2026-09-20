//! Result packages contain an animation program and an external skeletal clip.
use crate::{digest, field::sections, read::u32 as word, scene::SourceClip};
use anyhow::{Context, Result, ensure};
use resonance_content::battle::{
    actions::AnimationCommand,
    visual::{VictoryMotion, VictoryStyle},
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const RECORD_BYTES: usize = 0x38000;
const FIRST_EXTERNAL_CLIP: u16 = 256;

pub(super) struct Package {
    pub bindings: BTreeMap<VictoryStyle, VictoryMotion>,
    motions: Vec<Vec<u8>>,
}

impl Package {
    pub(super) fn open(path: &Path, character: u8) -> Result<Self> {
        ensure!((1..=9).contains(&character), "invalid victory character");
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(
            u64::from(character - 1) * 5 * RECORD_BYTES as u64,
        ))?;
        let mut bindings = BTreeMap::new();
        let mut motions = Vec::new();
        for style in VictoryStyle::ALL {
            let mut bytes = vec![0; RECORD_BYTES];
            file.read_exact(&mut bytes)
                .with_context(|| format!("victory character {character}, variant {style:?}"))?;
            let ranges = sections(&bytes)?;
            ensure!(ranges.len() == 2, "unsupported victory package members");
            let script = ranges[0]
                .clone()
                .context("missing victory animation program")?;
            let motion = &bytes[ranges[1]
                .clone()
                .context("missing victory skeletal motion")?];
            ensure!(
                word(motion, 0)? == 0x007b7960,
                "invalid victory motion signature"
            );
            let animations = super::super::actions::animations_at(&bytes, script.start)?;
            ensure!(
                animations.commands().any(|command| matches!(
                    command,
                    AnimationCommand::Play {
                        clip: VictoryMotion::NATIVE_CLIP,
                        ..
                    }
                )),
                "victory program does not use its external motion"
            );
            bindings.insert(
                style,
                VictoryMotion {
                    source_sha256: digest(&bytes),
                    clip: FIRST_EXTERNAL_CLIP + style as u16,
                    animations,
                },
            );
            motions.push(motion.to_vec());
        }
        Ok(Self { bindings, motions })
    }

    /// Native action clips fit in a byte. External result motions occupy a
    /// separate cooked range and are selected through the explicit bindings.
    pub(super) fn clips(&self) -> Vec<SourceClip<'_>> {
        self.motions
            .iter()
            .enumerate()
            .map(|(index, bytes)| SourceClip {
                slot: FIRST_EXTERNAL_CLIP + index as u16,
                bytes,
                resource: None,
            })
            .collect()
    }
}

#[test]
#[ignore = "requires both original extracted victory archives; no prepared assets"]
fn original_lloyd_victory_preserves_initial_binding_and_texture_instruction() -> Result<()> {
    use resonance_content::battle::actions::{AnimationInstruction, AnimationTrigger};
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    for disc in ["disc1", "disc2"] {
        let package = Package::open(&extracted.join(disc).join("files/BTL/BTLwin.bfp"), 1)?;
        assert_eq!(package.bindings.len(), VictoryStyle::ALL.len());
        let program = &package.bindings[&VictoryStyle::Healthy].animations;
        assert!(matches!(
            program.initial,
            Some(AnimationCommand::Play {
                clip: VictoryMotion::NATIVE_CLIP,
                blend: 15,
                ..
            })
        ));
        assert!(
            matches!(&program.instructions[&1], AnimationInstruction::Step(step)
            if matches!(step.trigger, AnimationTrigger::Tick(0))
                && matches!(step.command, AnimationCommand::Texture { layers: [4, 0] }))
        );
        assert!(
            matches!(&program.instructions[&2], AnimationInstruction::Step(step)
            if matches!(step.trigger, AnimationTrigger::Tick(111))
                && matches!(step.command, AnimationCommand::Play {
                    clip: VictoryMotion::NATIVE_CLIP, blend: 0, start: 96, looping: true, .. }))
        );
        assert!(matches!(
            program.instructions[&3],
            AnimationInstruction::End
        ));
    }
    Ok(())
}
