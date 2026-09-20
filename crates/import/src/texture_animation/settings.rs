use super::arena::Layer;
use crate::read::FloatOperand;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

const HEADER_BYTES: usize = 800;

/// The complete arena settings section, including inactive fixed slots and storage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArenaSettings {
    light_position: [FloatOperand; 3],
    light_direction: [FloatOperand; 3],
    pub(crate) actor_color: [u8; 4],
    translation: [FloatOperand; 3],
    pub(crate) yaw_degrees: FloatOperand,
    pub(crate) layers: [Layer; 4],
    unreferenced_716: [u8; 8],
    effect_interval: u16,
    scenery_fade_step: u8,
    unreferenced_727: u8,
    pub(crate) minimum_alpha: [u8; 32],
    pub(crate) ambient: [u8; 4],
    fog_color: [u8; 4],
    fog_near: i16,
    fog_far: i16,
    pub(crate) motion_section_start: u8,
    pub(crate) motion_offsets: [i8; 15],
    pub(crate) camera_pitch_offset: FloatOperand,
    unreferenced_792: [u8; 8],
    section_tail: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SceneryBinding {
    pub(crate) model: usize,
    animation: Option<i16>,
}

impl SceneryBinding {
    pub(crate) fn animation_section(&self, sections: usize) -> Result<Option<usize>> {
        self.animation
            .map(|target| {
                let target =
                    usize::try_from(target).context("arena scenery motion precedes archive")?;
                // The native archive lookup returns null beyond its section count.
                Ok((target < sections).then_some(target))
            })
            .transpose()
            .map(Option::flatten)
    }
}

impl ArenaSettings {
    pub(crate) fn read(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() >= HEADER_BYTES, "truncated arena settings");
        let vector = |at| -> Result<_> {
            Ok([
                FloatOperand::read(bytes, at)?,
                FloatOperand::read(bytes, at + 4)?,
                FloatOperand::read(bytes, at + 8)?,
            ])
        };
        Ok(Self {
            light_position: vector(0)?,
            light_direction: vector(12)?,
            actor_color: bytes[24..28].try_into()?,
            translation: vector(28)?,
            yaw_degrees: FloatOperand::read(bytes, 40)?,
            layers: (0..4)
                .map(|layer| super::arena::read(bytes, layer))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            unreferenced_716: bytes[716..724].try_into()?,
            effect_interval: crate::read::u16(bytes, 724)?,
            scenery_fade_step: bytes[726],
            unreferenced_727: bytes[727],
            minimum_alpha: bytes[728..760].try_into()?,
            ambient: bytes[760..764].try_into()?,
            fog_color: bytes[764..768].try_into()?,
            fog_near: crate::read::u16(bytes, 768)? as i16,
            fog_far: crate::read::u16(bytes, 770)? as i16,
            motion_section_start: bytes[772],
            motion_offsets: std::array::from_fn(|index| bytes[773 + index] as i8),
            camera_pitch_offset: FloatOperand::read(bytes, 788)?,
            unreferenced_792: bytes[792..800].try_into()?,
            section_tail: bytes[HEADER_BYTES..].to_vec(),
        })
    }

    /// The native loader stores the derived model count in a signed halfword.
    pub(crate) fn scenery_model_count(&self, sections: usize) -> i16 {
        if self.motion_section_start == 0 {
            sections.wrapping_sub(11) as i16
        } else {
            i16::from(self.motion_section_start) - 11
        }
    }

    /// Resolve only the active scenery prefix. Physical cooking keeps all slots.
    pub(crate) fn scenery(&self, sections: usize) -> Result<Vec<SceneryBinding>> {
        let count = usize::try_from(self.scenery_model_count(sections))
            .context("negative arena scenery model count")?;
        ensure!(
            count <= 32 && 11 + count <= sections,
            "arena scenery range exceeds archive or alpha slots"
        );
        ensure!(
            self.motion_section_start == 0 || count <= 15,
            "arena scenery exceeds motion slots"
        );
        (0..count)
            .map(|slot| {
                let animation = if self.motion_section_start == 0 || self.motion_offsets[slot] == -1
                {
                    None
                } else {
                    Some(
                        i16::from(self.motion_section_start) + i16::from(self.motion_offsets[slot]),
                    )
                };
                Ok(SceneryBinding {
                    model: 11 + slot,
                    animation,
                })
            })
            .collect()
    }

    pub(crate) fn light_position(&self) -> Result<[f32; 3]> {
        Self::vector(self.light_position)
    }

    pub(crate) fn translation(&self) -> Result<[f32; 3]> {
        Self::vector(self.translation)
    }

    fn vector(values: [FloatOperand; 3]) -> Result<[f32; 3]> {
        Ok([
            values[0].finite()?,
            values[1].finite()?,
            values[2].finite()?,
        ])
    }

    #[cfg(test)]
    pub(crate) fn source_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = vec![0; HEADER_BYTES];
        for (at, values) in [
            (0, self.light_position),
            (12, self.light_direction),
            (28, self.translation),
        ] {
            for (axis, value) in values.into_iter().enumerate() {
                bytes[at + axis * 4..at + axis * 4 + 4]
                    .copy_from_slice(&value.bits().to_be_bytes());
            }
        }
        bytes[24..28].copy_from_slice(&self.actor_color);
        bytes[40..44].copy_from_slice(&self.yaw_degrees.bits().to_be_bytes());
        for (index, layer) in self.layers.iter().enumerate() {
            bytes[44 + index * 168..44 + (index + 1) * 168].copy_from_slice(&layer.source_bytes()?);
        }
        bytes[716..724].copy_from_slice(&self.unreferenced_716);
        bytes[724..726].copy_from_slice(&self.effect_interval.to_be_bytes());
        bytes[726] = self.scenery_fade_step;
        bytes[727] = self.unreferenced_727;
        bytes[728..760].copy_from_slice(&self.minimum_alpha);
        bytes[760..764].copy_from_slice(&self.ambient);
        bytes[764..768].copy_from_slice(&self.fog_color);
        bytes[768..770].copy_from_slice(&self.fog_near.to_be_bytes());
        bytes[770..772].copy_from_slice(&self.fog_far.to_be_bytes());
        bytes[772] = self.motion_section_start;
        for (index, offset) in self.motion_offsets.iter().enumerate() {
            bytes[773 + index] = *offset as u8;
        }
        bytes[788..792].copy_from_slice(&self.camera_pitch_offset.bits().to_be_bytes());
        bytes[792..800].copy_from_slice(&self.unreferenced_792);
        bytes.extend_from_slice(&self.section_tail);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_settings_roundtrip_without_admitting_inactive_bindings() -> Result<()> {
        let mut bytes: Vec<_> = (0..817).map(|index| (index * 73 + 19) as u8).collect();
        bytes[..4].copy_from_slice(&0x7fc0_1234u32.to_be_bytes());
        bytes[772] = 0;
        let settings = ArenaSettings::read(&bytes)?;
        let json = serde_json::to_value(&settings)?;
        assert_eq!(json["minimum_alpha"].as_array().unwrap().len(), 32);
        assert_eq!(json["motion_offsets"].as_array().unwrap().len(), 15);
        let settings: ArenaSettings = serde_json::from_value(json)?;
        assert_eq!(settings.source_bytes()?, bytes);
        assert_eq!(settings.scenery_model_count(11), 0);
        assert!(settings.scenery(11)?.is_empty());
        assert_eq!(settings.scenery(12)?[0].animation_section(12)?, None);
        assert!(settings.scenery(44).is_err());
        assert!(ArenaSettings::read(&bytes[..799]).is_err());
        Ok(())
    }

    #[test]
    fn scenery_uses_signed_offsets_and_only_minus_one_disables() -> Result<()> {
        let mut bytes = [0; HEADER_BYTES];
        bytes[772] = 15;
        bytes[773..777].copy_from_slice(&[254, 255, 2, 127]);
        bytes[728..732].copy_from_slice(&[1, 2, 3, 4]);
        let settings = ArenaSettings::read(&bytes)?;
        let bindings = settings.scenery(18)?;
        assert_eq!(
            bindings
                .iter()
                .map(|b| Ok((b.model, b.animation_section(18)?)))
                .collect::<Result<Vec<_>>>()?,
            [(11, Some(13)), (12, None), (13, Some(17)), (14, None)]
        );
        bytes[773] = 128;
        let bindings = ArenaSettings::read(&bytes)?.scenery(18)?;
        assert!(bindings[0].animation_section(18).is_err());
        Ok(())
    }
}
