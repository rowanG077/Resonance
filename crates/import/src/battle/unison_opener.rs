//! The shared opener descriptor and both authored hit streams, before actor selection.
use super::{
    actions::{animations_at, commands_with_tail, hit_rule, hits},
    embedded::{self, Layout},
};
use crate::{
    read::{f32 as float, u16 as half, u32 as word},
    rel::Rel,
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle::actions::{
    AnimationProgram, HitAttachment, HitEmission, HitRule, HitWindow, Recovery, TimedCommand,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "battle-unison-opener";
const BYTES: usize = 228;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OpenerRecipe {
    pub duration: u16,
    pub combo_first: u16,
    pub combo_second: Option<u16>,
    pub buffer_until: u8,
    pub recovery: Recovery,
    pub effect: Option<u16>,
    pub reach: u16,
    pub airborne_reach: u16,
    pub animations: AnimationProgram,
    pub hits: [Vec<HitWindow>; 2],
    pub commands: Vec<TimedCommand>,
    pub loop_commands: bool,
    /// The rule is authored independently of whether either stream uses it.
    hit_rule: HitRule,
    /// Storage after the command terminator remains independent of the live program.
    trailing_commands: Vec<u8>,
}

fn zero(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.iter().all(|&byte| byte == 0),
        "nonzero Unison opener reserved storage"
    );
    Ok(())
}

pub(super) fn decode(bytes: &[u8]) -> Result<OpenerRecipe> {
    let bytes = bytes.get(..BYTES).context("truncated Unison opener")?;
    // Each fixed component includes its terminal record and remaining storage.
    zero(&bytes[10..12])?;
    let rules = &bytes[0x18..0x34];
    zero(&rules[18..20])?;
    zero(&rules[25..28])?;
    ensure!(
        half(bytes, 0x40)? == 0xfffe,
        "missing Unison opener animation terminator"
    );
    zero(&bytes[0x42..0x4c])?;
    let animations = animations_at(bytes, 0x34)?;
    let hit_stream = |start| -> Result<Vec<HitWindow>> {
        let rows = &bytes[start..start + 64];
        let hits = hits(rows, rules)?;
        zero(&rows[hits.len() * 32 + 2..])?;
        for (hit, row) in hits.iter().zip(rows.chunks_exact(32)) {
            zero(&row[21..22])?;
            zero(&row[24..28])?;
            let unused = match &hit.emission {
                HitEmission::Contact {
                    attachment: HitAttachment::Groups(groups),
                    ..
                } => 4 + groups.len(),
                HitEmission::Contact {
                    attachment: HitAttachment::BodyBone(_),
                    ..
                }
                | HitEmission::Effect { .. } => {
                    zero(&row[3..4])?;
                    6
                }
                _ => {
                    zero(&row[3..4])?;
                    5
                }
            };
            zero(&row[unused..8])?;
        }
        Ok(hits)
    };
    let (commands, loop_commands, tail) = commands_with_tail(&bytes[0xcc..])?;
    let nonzero = |value| (value != 0).then_some(value);
    Ok(OpenerRecipe {
        duration: half(bytes, 0)?,
        combo_first: half(bytes, 4)?,
        combo_second: nonzero(half(bytes, 6)?),
        buffer_until: bytes[8],
        recovery: Recovery {
            duration: half(bytes, 2)?,
            animation: (bytes[9] != 0).then_some(bytes[9]),
            rate: float(bytes, 12)?,
        },
        effect: nonzero(
            word(bytes, 16)?
                .try_into()
                .context("invalid Unison opener effect")?,
        ),
        reach: half(bytes, 20)?,
        airborne_reach: half(bytes, 22)?,
        animations,
        hits: [hit_stream(0x4c)?, hit_stream(0x8c)?],
        commands,
        loop_commands,
        hit_rule: hit_rule(rules)?,
        trailing_commands: tail.to_vec(),
    })
}

pub(super) fn read(rel: &Rel, layout: &Layout) -> Result<OpenerRecipe> {
    decode(rel.at((5, layout.unison_opener))?)
}

pub(crate) fn cook_all(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    embedded::write(
        file,
        output,
        FAMILY,
        &read(&Rel::read(file)?, &layout)?,
        serde_json::json!({"section":5, "offset":layout.unison_opener, "bytes":BYTES}),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle::actions::{ActionCommand, AnimationCommand, HitShapeKind};
    use std::{collections::BTreeMap, fs};

    fn record() -> [u8; BYTES] {
        let mut bytes = [0; BYTES];
        bytes[0x40..0x42].copy_from_slice(&(-2_i16).to_be_bytes());
        for start in [0x4c, 0x8c] {
            bytes[start + 32..start + 34].copy_from_slice(&(-1_i16).to_be_bytes());
        }
        bytes[0xcc..0xce].copy_from_slice(&(-1_i16).to_be_bytes());
        bytes
    }

    #[test]
    fn opener_keeps_descriptor_fields_and_checks_every_component() -> Result<()> {
        let mut bytes = record();
        bytes[4..10].copy_from_slice(&[0, 3, 0, 4, 5, 6]);
        bytes[19] = 7;
        bytes[23] = 8;
        let recipe = decode(&bytes)?;
        assert_eq!(
            (recipe.combo_first, recipe.combo_second, recipe.buffer_until),
            (3, Some(4), 5)
        );
        assert_eq!(
            (
                recipe.recovery.animation,
                recipe.effect,
                recipe.airborne_reach
            ),
            (Some(6), Some(7), 8)
        );
        for length in [0, 0x18, 0x34, 0x4c, 0x8c, 0xcc, BYTES - 1] {
            assert!(decode(&bytes[..length]).is_err());
        }
        for offset in [
            10,
            0x18 + 18,
            0x18 + 25,
            0x42,
            0x4c + 21,
            0x4c + 24,
            0x6e,
            0x8c + 7,
            0xae,
        ] {
            let mut bad = bytes;
            bad[offset] = 1;
            assert!(decode(&bad).is_err(), "unchecked storage at {offset:#x}");
        }
        for offset in [12, 0x3c, 0x94] {
            let mut bad = bytes;
            bad[offset..offset + 4].copy_from_slice(&f32::NAN.to_be_bytes());
            assert!(decode(&bad).is_err());
        }
        let mut bad = bytes;
        bad[0x8c + 18] = 1;
        assert!(
            decode(&bad).is_err(),
            "unselected hit stream was not validated"
        );
        bad = bytes;
        bad[0xce..0xd0].copy_from_slice(&1_u16.to_be_bytes());
        assert_eq!(decode(&bad)?.trailing_commands, bad[0xce..]);
        bytes[0x18..0x1a].copy_from_slice(&3_u16.to_be_bytes());
        bytes[0x34..0x36].copy_from_slice(&(-2_i16).to_be_bytes());
        for (start, end, terminator) in [(0x4c, 0x8c, -1_i16), (0x8c, 0xcc, -1)] {
            bytes[start..end].fill(0);
            bytes[start..start + 2].copy_from_slice(&terminator.to_be_bytes());
        }
        let empty = decode(&bytes)?;
        assert!(empty.animations.initial.is_some() && empty.hits.iter().all(Vec::is_empty));
        assert_eq!(empty.hit_rule.flags, 3);
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; only publishes small JSON records"]
    fn original_unison_opener_preserves_both_streams_and_deduplicates_every_module() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let result = (|| -> Result<()> {
            let mut publications = BTreeMap::<String, usize>::new();
            for disc in ["disc1", "disc2"] {
                let destination = output.join(disc);
                let mut count = 0;
                for file in fs::read_dir(extracted.join(disc).join("files"))? {
                    let file = file?.path();
                    let Some((module, layout)) = Layout::identify(&file) else {
                        continue;
                    };
                    let rel = Rel::read(&file)?;
                    let bytes = &rel.at((5, layout.unison_opener))?[..BYTES];
                    assert_eq!(
                        crate::digest(bytes),
                        "cb5fa01dc7feba132436b6936cdeae676160035fba8c5e26ab3f2878156bf4d7"
                    );
                    for offset in [layout.unison_opener, layout.unison_opener + BYTES] {
                        assert!(rel.local_targets().contains(&(5, offset)), "{module}");
                    }
                    let recipe = read(&rel, &layout)?;
                    assert_eq!(
                        (
                            recipe.duration,
                            recipe.recovery.duration,
                            recipe.recovery.rate,
                            recipe.reach
                        ),
                        (30, 10, 0.5, 200)
                    );
                    assert_eq!(
                        (
                            recipe.combo_first,
                            recipe.combo_second,
                            recipe.buffer_until,
                            recipe.recovery.animation,
                            recipe.effect,
                            recipe.airborne_reach
                        ),
                        (0, None, 0, None, None, 0)
                    );
                    assert!(matches!(
                        recipe.animations.initial.unwrap(),
                        AnimationCommand::Play {
                            clip: 25,
                            blend: 4,
                            layer: 8,
                            resource: -1,
                            rate: 0.5,
                            ..
                        }
                    ));
                    for (index, stream) in recipe.hits.iter().enumerate() {
                        let hit = &stream[0];
                        let source = &bytes[0x4c + index * 64..];
                        assert_eq!(hit.start.to_be_bytes(), source[..2]);
                        assert_eq!(hit.start, [10, 20][index]);
                        assert!(
                            matches!(&hit.emission, HitEmission::Contact { duration:8, attachment:HitAttachment::Groups(groups) } if groups.as_slice() == [[0,1].as_slice(), [0].as_slice()][index])
                        );
                        assert_eq!(
                            (
                                hit.shape.radius,
                                hit.shape.height,
                                hit.shape.kind,
                                hit.shape.reaction
                            ),
                            (30., 30., HitShapeKind::Box, 2)
                        );
                        assert_eq!(
                            (
                                hit.rule.flags,
                                hit.rule.hitstun,
                                hit.rule.contact_cooldown,
                                hit.rule.power_mode,
                                hit.rule.power
                            ),
                            (3, 30, 10, 1, 0)
                        );
                        assert_eq!(hit.shape.radius.to_be_bytes(), source[8..12]);
                        assert_eq!(hit.rule.flags.to_be_bytes(), bytes[0x18..0x1a]);
                    }
                    assert_eq!(recipe.commands.len(), 3);
                    for (index, step) in recipe.commands[..2].iter().enumerate() {
                        assert_eq!(step.tick, 0);
                        assert!(
                            matches!(step.command, ActionCommand::AttachmentTrail { slot, ticks:90 } if usize::from(slot) == index)
                        );
                    }
                    assert_eq!(recipe.commands[2].tick, 8);
                    assert!(matches!(
                        recipe.commands[2].command,
                        ActionCommand::ForwardSpeed(4.)
                    ));
                    assert!(!recipe.loop_commands);
                    assert!(recipe.trailing_commands.is_empty());
                    let paths = cook_all(&file, &destination)?
                        .context("missing Unison opener publication")?;
                    let restored =
                        crate::embedded::read::<OpenerRecipe>(&destination, FAMILY, module)?;
                    assert_eq!(
                        serde_json::to_value(restored)?,
                        serde_json::to_value(&recipe)?
                    );
                    let source: serde_json::Value =
                        serde_json::from_slice(&fs::read(destination.join(&paths[1]))?)?;
                    assert_eq!(source["module"], module);
                    assert_eq!(source["source_sha256"], crate::digest(&rel.bytes));
                    assert_eq!(source["data"], paths[0]);
                    assert_eq!(source["section"], 5);
                    assert_eq!(source["offset"], layout.unison_opener);
                    assert_eq!(source["bytes"], BYTES);
                    *publications.entry(paths[0].clone()).or_default() += 1;
                    count += 1;
                }
                assert_eq!(count, 7);
            }
            assert_eq!(publications.into_values().collect::<Vec<_>>(), [14]);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
