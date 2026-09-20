use crate::read::FloatOperand;
use anyhow::{Context, Result, ensure};
use resonance_content::battle::visual::{ArenaUvChannel, ArenaUvMode};
use serde::{Deserialize, Serialize};

const STRIDE: usize = 168;
const ROW_BYTES: usize = 40;

impl FloatOperand {
    fn bind(self, consumed: bool) -> Result<f32> {
        let value = f32::from_bits(self.bits());
        ensure!(
            !consumed || value.is_finite(),
            "non-finite active arena UV operand"
        );
        // Unconsumed operands remain exact in the physical record. Runtime channel
        // storage requires finite values even for fields its selected mode ignores.
        Ok(if value.is_finite() { value } else { 0. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Mode {
    Frames,
    Scroll,
    Oscillate,
    Inert { value: u8 },
}

impl Mode {
    fn byte(self) -> Result<u8> {
        Ok(match self {
            Self::Frames => 1,
            Self::Scroll => 2,
            Self::Oscillate => 3,
            Self::Inert { value } => {
                ensure!(
                    !(1..=3).contains(&value),
                    "active arena mode encoded as inert"
                );
                value
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    mode: Mode,
    texture: u8,
    interval: u8,
    frames: u8,
    speed: [FloatOperand; 2],
    angular_speed: [FloatOperand; 2],
    initial_tick: u8,
    initial_frame: u8,
    unreferenced_22: [u8; 2],
    initial_offset: [FloatOperand; 2],
    initial_angle: [FloatOperand; 2],
}

impl Record {
    fn read(row: &[u8]) -> Result<Self> {
        let pair = |at| -> Result<_> {
            Ok([
                FloatOperand::read(row, at)?,
                FloatOperand::read(row, at + 4)?,
            ])
        };
        Ok(Self {
            mode: match row[0] {
                1 => Mode::Frames,
                2 => Mode::Scroll,
                3 => Mode::Oscillate,
                value => Mode::Inert { value },
            },
            texture: row[1],
            interval: row[2],
            frames: row[3],
            speed: pair(4)?,
            angular_speed: pair(12)?,
            initial_tick: row[20],
            initial_frame: row[21],
            unreferenced_22: [row[22], row[23]],
            initial_offset: pair(24)?,
            initial_angle: pair(32)?,
        })
    }

    fn channel(&self) -> Result<ArenaUvChannel> {
        let mode = match self.mode.byte()? {
            1 => ArenaUvMode::Frames,
            2 => ArenaUvMode::Scroll,
            3 => ArenaUvMode::Oscillate,
            _ => ArenaUvMode::Disabled,
        };
        let pair = |values: [FloatOperand; 2], consumed| -> Result<_> {
            Ok([values[0].bind(consumed)?, values[1].bind(consumed)?])
        };
        let channel = ArenaUvChannel {
            mode,
            texture: u16::from(self.texture),
            interval: self.interval,
            frames: self.frames,
            speed: pair(
                self.speed,
                matches!(mode, ArenaUvMode::Scroll | ArenaUvMode::Oscillate),
            )?,
            angular_speed: pair(self.angular_speed, mode == ArenaUvMode::Oscillate)?,
            initial_tick: self.initial_tick,
            initial_frame: self.initial_frame,
            initial_offset: pair(self.initial_offset, mode == ArenaUvMode::Scroll)?,
            initial_angle: pair(self.initial_angle, mode == ArenaUvMode::Oscillate)?,
        };
        channel.validate()?;
        Ok(channel)
    }
}

/// The fixed four slots are retained independently of the native active prefix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Layer {
    pub active_count: u8,
    slots: [Record; 4],
    animation_rate: FloatOperand,
    flags: u8,
    unreferenced_166: [u8; 2],
}

impl Layer {
    pub(crate) fn animation_rate(&self) -> Result<f32> {
        self.animation_rate.finite()
    }

    pub(crate) fn additive(&self) -> bool {
        self.flags & 2 != 0
    }

    pub(crate) fn channels(&self) -> Result<Vec<ArenaUvChannel>> {
        self.slots
            .get(..usize::from(self.active_count))
            .context("arena UV bindings exceed four channels")?
            .iter()
            .map(Record::channel)
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn source_bytes(&self) -> Result<[u8; STRIDE]> {
        let mut bytes = [0; STRIDE];
        for (record, row) in self
            .slots
            .iter()
            .zip(bytes[..160].chunks_exact_mut(ROW_BYTES))
        {
            row[..4].copy_from_slice(&[
                record.mode.byte()?,
                record.texture,
                record.interval,
                record.frames,
            ]);
            row[20..24].copy_from_slice(&[
                record.initial_tick,
                record.initial_frame,
                record.unreferenced_22[0],
                record.unreferenced_22[1],
            ]);
            for (at, pair) in [
                (4, record.speed),
                (12, record.angular_speed),
                (24, record.initial_offset),
                (32, record.initial_angle),
            ] {
                for (offset, value) in pair.into_iter().enumerate() {
                    row[at + offset * 4..at + offset * 4 + 4]
                        .copy_from_slice(&value.bits().to_be_bytes());
                }
            }
        }
        bytes[160..164].copy_from_slice(&self.animation_rate.bits().to_be_bytes());
        bytes[164] = self.flags;
        bytes[165] = self.active_count;
        bytes[166..].copy_from_slice(&self.unreferenced_166);
        Ok(bytes)
    }
}

pub(crate) fn read(bytes: &[u8], layer: usize) -> Result<Layer> {
    ensure!(layer < 4, "invalid arena layer");
    let start = 44 + layer * STRIDE;
    let bytes = bytes
        .get(start..start + STRIDE)
        .context("truncated arena UV settings")?;
    Ok(Layer {
        active_count: bytes[165],
        slots: bytes[..160]
            .chunks_exact(ROW_BYTES)
            .map(Record::read)
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        animation_rate: FloatOperand::read(bytes, 160)?,
        flags: bytes[164],
        unreferenced_166: [bytes[166], bytes[167]],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_operands_and_inert_modes_roundtrip_without_runtime_admission() -> Result<()> {
        let mut bytes = [0; 800];
        let row = &mut bytes[44..44 + STRIDE];
        row[165] = 1;
        row[0] = 255;
        row[1] = 254;
        row[4..8].copy_from_slice(&0x7fc0_1234u32.to_be_bytes());
        row[22..24].copy_from_slice(&[0x81, 0x82]);
        row[40] = 1; // Inactive frame row with zero frame count and arbitrary operands.
        row[44..48].copy_from_slice(&f32::NEG_INFINITY.to_be_bytes());
        row[164] = 0xc5;
        row[166..168].copy_from_slice(&[0x83, 0x84]);
        let physical = read(&bytes, 0)?;
        let json = serde_json::to_value(&physical)?;
        assert_eq!(
            json["slots"][0]["mode"],
            serde_json::json!({"kind":"inert","value":255})
        );
        assert_eq!(
            json["slots"][0]["speed"][0],
            serde_json::json!({"bits":0x7fc0_1234u32})
        );
        let physical: Layer = serde_json::from_value(json)?;
        assert_eq!(physical.source_bytes()?, bytes[44..44 + STRIDE]);
        let channels = physical.channels()?;
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].mode, ArenaUvMode::Disabled);
        assert_eq!(channels[0].texture, 254);
        assert_eq!(channels[0].offset(999), [0.; 2]);
        bytes[44 + 165] = 2;
        assert!(read(&bytes, 0)?.channels().is_err());
        bytes[44 + 165] = 1;
        bytes[44] = 2;
        assert!(read(&bytes, 0)?.channels().is_err());
        bytes[44 + 165] = 5;
        let physical = read(&bytes, 0)?;
        assert_eq!(physical.source_bytes()?, bytes[44..44 + STRIDE]);
        assert!(physical.channels().is_err());
        Ok(())
    }
}
