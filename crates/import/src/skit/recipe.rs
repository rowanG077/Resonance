//! Portrait channels are terminated by control records, not their declared counts.
use crate::{
    dol,
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const COUNT: usize = 230;
const TABLE: u32 = 0x8020f49c;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Recipe {
    pub flags: u16,
    pub reserved: u16,
    pub timelines: Vec<Timeline>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Channel {
    Eyes,
    Mouth,
    Extra,
}

impl Channel {
    pub fn index(self) -> usize {
        match self {
            Self::Eyes => 0,
            Self::Mouth => 1,
            Self::Extra => 2,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Timeline {
    pub channel: Channel,
    pub declared_frames: u16,
    pub records: Vec<Record>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub position: [i16; 2],
    pub image: Option<u16>,
    pub timing: Timing,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Timing {
    Frame { duration: i16 },
    Loop,
    Hold,
}

impl Recipe {
    pub fn prepared(&self) -> resonance_content::skit::PortraitRecipe {
        use resonance_content::skit::{PortraitFrame, PortraitRecipe};
        let mut result = PortraitRecipe::default();
        for timeline in &self.timelines {
            let channel = timeline.channel.index();
            for record in &timeline.records {
                match record.timing {
                    Timing::Frame { duration } => result.tracks[channel].push(PortraitFrame {
                        ticks: duration,
                        image: record.image,
                        position: record.position,
                    }),
                    Timing::Loop => result.repeat[channel] = true,
                    Timing::Hold => (),
                }
            }
        }
        result
    }
}

pub(crate) fn read(executable: &[u8]) -> Result<Vec<Recipe>> {
    let mut parsed = BTreeMap::new();
    dol::slice(executable, TABLE, COUNT * 4)?
        .chunks_exact(4)
        .map(|row| {
            let pointer = word(row, 0)?;
            if pointer == 0 {
                return Ok(Recipe::default());
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = parsed.entry(pointer) {
                let (recipe, _) = parse(|at, length| {
                    let address = pointer
                        .checked_add(u32::try_from(at)?)
                        .context("portrait recipe address overflow")?;
                    dol::slice(executable, address, length)
                })?;
                entry.insert(recipe);
            }
            Ok(parsed[&pointer].clone())
        })
        .collect()
}

fn parse<'a>(mut bytes: impl FnMut(usize, usize) -> Result<&'a [u8]>) -> Result<(Recipe, usize)> {
    let header = bytes(0, 4)?;
    let mut result = Recipe {
        flags: half(header, 0)?,
        reserved: half(header, 2)?,
        timelines: Vec::new(),
    };
    let mut at = 4;
    loop {
        let header = bytes(at, 4)?;
        at += 4;
        if word(header, 0)? == 0xfefefefe {
            return Ok((result, at));
        }
        let channel = match half(header, 0)? {
            0x7000 => Channel::Eyes,
            0x7003 => Channel::Mouth,
            0x7006 => Channel::Extra,
            tag => bail!("unknown portrait channel {tag:#x}"),
        };
        ensure!(
            !result.timelines.iter().any(|t| t.channel == channel),
            "duplicate portrait channel"
        );
        let mut records = Vec::new();
        loop {
            let row = bytes(at, 8)?;
            at += 8;
            let image = half(row, 6)? as i16;
            ensure!(image >= -1, "invalid portrait image {image}");
            let timing = match half(row, 4)? as i16 {
                0xfd => Timing::Loop,
                0xfe => Timing::Hold,
                duration => Timing::Frame { duration },
            };
            records.push(Record {
                position: [half(row, 0)? as i16, half(row, 2)? as i16],
                image: (image != -1).then_some(image as u16),
                timing,
            });
            if !matches!(timing, Timing::Frame { .. }) {
                break;
            }
        }
        result.timelines.push(Timeline {
            channel,
            declared_frames: half(header, 2)?,
            records,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_override_stale_counts_and_keep_signed_operands() -> Result<()> {
        let words = [
            7u16, 0x1234, 0x7006, 12, 0xfff8, 16, 0xffff, 0xffff, 0, 0, 0xfe, 3, 0xfefe, 0xfefe,
        ];
        let bytes: Vec<_> = words.into_iter().flat_map(u16::to_be_bytes).collect();
        let read = |at, length| bytes.get(at..at + length).context("truncated fixture");
        let (recipe, consumed) = parse(read)?;
        assert_eq!(consumed, bytes.len());
        assert_eq!(recipe.reserved, 0x1234);
        let timeline = &recipe.timelines[0];
        assert_eq!(timeline.declared_frames, 12);
        assert_eq!(timeline.records.len(), 2);
        assert_eq!(timeline.records[0].position, [-8, 16]);
        assert_eq!(timeline.records[0].image, None);
        assert_eq!(timeline.records[0].timing, Timing::Frame { duration: -1 });
        assert_eq!(timeline.records[1].timing, Timing::Hold);
        assert_eq!(timeline.records[1].image, Some(3));
        assert!(
            parse(|at, length| bytes[..bytes.len() - 2]
                .get(at..at + length)
                .context("truncated fixture"))
            .is_err()
        );
        Ok(())
    }
}
