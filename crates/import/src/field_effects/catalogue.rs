//! Complete embedded particle animation data, independent of particle controllers.
//!
//! Four-byte headers contain dimensions and a texture selector. Four-byte frames
//! contain UV coordinates and a timer threshold; a final control frame loops or expires.
//! The callback tables immediately before and after this range are not recipes.
use crate::dol;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

pub(super) const START: u32 = 0x8020_A414;
pub(super) const END: u32 = 0x8020_A80C;
const ENTRY_COUNT: usize = 79;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Catalogue {
    pub entries: Vec<Sequence>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Sequence {
    /// Byte offset from the start of the embedded recipe range.
    pub offset: u32,
    /// Native header dimensions. The renderer uses wrapping `(dimension - 1)`.
    pub dimensions: [u8; 2],
    pub image: ImageBinding,
    pub frames: Vec<Frame>,
    pub terminator: Terminator,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Frame {
    pub origin: [u8; 2],
    /// Native signed timer threshold; advancement occurs when elapsed > duration.
    pub duration: i16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Terminator {
    /// The native player ignores these two bytes; preserve them for reconstruction.
    pub unused: [u8; 2],
    pub action: EndAction,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EndAction {
    Expire,
    Loop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ImageBinding {
    /// Image index in the shared effect atlas bank.
    Shared(u8),
    /// Texture slot installed dynamically by a field script.
    Script(u8),
    /// A framebuffer snapshot supplied by the renderer, without a cooked image file.
    CapturedScene,
    /// Other native selectors use shared image zero; retain the selector for reconstruction.
    Fallback(i16),
}

impl ImageBinding {
    fn from_selector(selector: i16) -> Self {
        match selector {
            0 => Self::Shared(0),
            1 => Self::Shared(2),
            2 => Self::CapturedScene,
            3 => Self::Shared(4),
            4 => Self::Shared(3),
            5 => Self::Shared(5),
            6 => Self::Shared(6),
            7..=14 => Self::Script((selector - 7) as u8),
            15 => Self::Shared(7),
            selector => Self::Fallback(selector),
        }
    }

    fn selector(self) -> Result<i16> {
        Ok(match self {
            Self::Shared(0) => 0,
            Self::Shared(2) => 1,
            Self::CapturedScene => 2,
            Self::Shared(4) => 3,
            Self::Shared(3) => 4,
            Self::Shared(5) => 5,
            Self::Shared(6) => 6,
            Self::Shared(7) => 15,
            Self::Script(slot @ 0..=7) => i16::from(slot) + 7,
            Self::Fallback(selector) if !(0..=15).contains(&selector) => selector,
            image => bail!("unsupported particle image binding {image:?}"),
        })
    }
}

impl Catalogue {
    pub fn read(executable: &[u8]) -> Result<Self> {
        let bytes = dol::slice(executable, START, (END - START) as usize)?;
        let catalogue = Self::parse(bytes)?;
        ensure!(
            catalogue.entries.len() == ENTRY_COUNT,
            "unexpected particle recipe count"
        );
        Ok(catalogue)
    }

    fn parse(bytes: &[u8]) -> Result<Self> {
        let mut cursor = 0;
        let mut entries = Vec::new();
        while cursor < bytes.len() {
            let offset = cursor as u32;
            let header = word(bytes, &mut cursor)?;
            let mut frames = Vec::new();
            let terminator = loop {
                let row = word(bytes, &mut cursor).with_context(|| {
                    format!("unterminated particle recipe at offset {offset:#x}")
                })?;
                let duration = i16::from_be_bytes([row[2], row[3]]);
                let origin = [row[0], row[1]];
                // Initialization enters frame one directly; only later rows are checked for markers.
                match duration {
                    -1 | 0 if !frames.is_empty() => {
                        break Terminator {
                            unused: origin,
                            action: if duration == -1 {
                                EndAction::Loop
                            } else {
                                EndAction::Expire
                            },
                        };
                    }
                    _ => frames.push(Frame { origin, duration }),
                }
                // The native frame index is signed eight-bit, including the terminator.
                ensure!(
                    frames.len() <= 126,
                    "particle animation exceeds native frame index"
                );
            };
            entries.push(Sequence {
                offset,
                dimensions: [header[0], header[1]],
                image: ImageBinding::from_selector(i16::from_be_bytes([header[2], header[3]])),
                frames,
                terminator,
            });
        }
        let catalogue = Self { entries };
        ensure!(
            catalogue.encode()? == bytes,
            "particle catalogue reconstruction mismatch"
        );
        Ok(catalogue)
    }

    pub fn at_offset(&self, offset: u32) -> Result<&Sequence> {
        self.entries
            .iter()
            .find(|entry| entry.offset == offset)
            .with_context(|| format!("missing particle recipe at offset {offset:#x}"))
    }

    /// Reconstruct the native table from the typed records for source validation.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        for entry in &self.entries {
            ensure!(
                entry.offset as usize == bytes.len(),
                "noncontiguous particle recipes"
            );
            ensure!(
                !entry.frames.is_empty() && entry.frames.len() <= 126,
                "invalid particle animation length"
            );
            bytes.extend_from_slice(&entry.dimensions);
            bytes.extend_from_slice(&entry.image.selector()?.to_be_bytes());
            for (index, frame) in entry.frames.iter().enumerate() {
                ensure!(
                    index == 0 || !matches!(frame.duration, -1 | 0),
                    "particle control marker used as a noninitial frame"
                );
                bytes.extend_from_slice(&frame.origin);
                bytes.extend_from_slice(&frame.duration.to_be_bytes());
            }
            bytes.extend_from_slice(&entry.terminator.unused);
            let duration: i16 = match entry.terminator.action {
                EndAction::Expire => 0,
                EndAction::Loop => -1,
            };
            bytes.extend_from_slice(&duration.to_be_bytes());
        }
        Ok(bytes)
    }
}

fn word(bytes: &[u8], cursor: &mut usize) -> Result<[u8; 4]> {
    let end = cursor
        .checked_add(4)
        .context("particle table offset overflow")?;
    let row = bytes
        .get(*cursor..end)
        .context("truncated particle table")?;
    *cursor = end;
    Ok(row.try_into()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_catalogues_preserve_native_values_and_bindings() -> Result<()> {
        let bytes = [
            0, 0, 0, 1, 250, 251, 0, 2, 0xA5, 0x5A, 0xFF, 0xFF, 255, 255, 0, 14, 254, 253, 255,
            254, 0x12, 0x34, 0, 0, 64, 64, 0, 16, 0, 0, 0, 0, 0, 0, 255, 255, 64, 64, 255, 254, 0,
            0, 255, 255, 0, 0, 255, 255, 64, 64, 0, 2, 1, 2, 0, 1, 3, 4, 255, 254, 0xAB, 0xCD, 0,
            0,
        ];
        let catalogue = Catalogue::parse(&bytes)?;
        assert_eq!(catalogue.entries[0].dimensions, [0, 0]);
        assert_eq!(catalogue.entries[0].image, ImageBinding::Shared(2));
        assert_eq!(catalogue.entries[1].image, ImageBinding::Script(7));
        assert_eq!(catalogue.entries[2].image, ImageBinding::Fallback(16));
        assert_eq!(catalogue.entries[3].image, ImageBinding::Fallback(-2));
        assert_eq!(catalogue.entries[4].image, ImageBinding::CapturedScene);
        assert_eq!(catalogue.entries[1].frames[0].duration, -2);
        assert_eq!(catalogue.entries[2].frames[0].duration, 0);
        assert_eq!(catalogue.entries[3].frames[0].duration, -1);
        assert!(matches!(
            catalogue.entries[2].terminator.action,
            EndAction::Loop
        ));
        assert!(matches!(
            catalogue.entries[3].terminator.action,
            EndAction::Loop
        ));
        assert_eq!(catalogue.entries[4].frames[1].duration, -2);
        assert!(matches!(
            catalogue.entries[4].terminator.action,
            EndAction::Expire
        ));
        let json = serde_json::to_value(&catalogue)?;
        assert_eq!(
            json["entries"][0]["image"],
            serde_json::json!({"shared": 2})
        );
        let mut restored: Catalogue = serde_json::from_value(json)?;
        assert_eq!(restored.encode()?, bytes);
        for invalid in [
            ImageBinding::Shared(1),
            ImageBinding::Script(8),
            ImageBinding::Fallback(0),
        ] {
            restored.entries[0].image = invalid;
            assert!(restored.encode().is_err());
        }
        Ok(())
    }

    #[test]
    fn malformed_recipes_do_not_become_partial_catalogues() {
        for (label, bytes) in [
            ("truncated header", &[64, 64, 0][..]),
            ("truncated frame", &[64, 64, 0, 0, 1, 2, 0][..]),
            ("missing terminator", &[64, 64, 0, 0, 1, 2, 0, 1][..]),
            (
                "truncated terminator",
                &[64, 64, 0, 0, 1, 2, 0, 1, 0, 0, 255][..],
            ),
            (
                "missing terminator after initial control-valued frame",
                &[64, 64, 0, 0, 0, 0, 255, 255][..],
            ),
        ] {
            assert!(Catalogue::parse(bytes).is_err(), "accepted {label}");
        }
    }

    #[test]
    fn native_frame_index_must_reach_the_terminator() -> Result<()> {
        let sequence = |count| {
            let mut bytes = vec![64, 64, 0, 0];
            for index in 0..count {
                bytes.extend_from_slice(&[index as u8, 0, 0, 1]);
            }
            bytes.extend_from_slice(&[0, 0, 255, 255]);
            bytes
        };
        let bytes = sequence(126);
        let catalogue = Catalogue::parse(&bytes)?;
        assert_eq!(catalogue.entries[0].frames.len(), 126);
        assert_eq!(catalogue.encode()?, bytes);
        assert!(Catalogue::parse(&sequence(127)).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; read-only, no cooking or playback"]
    fn original_catalogues_cover_every_record_and_frame_on_both_discs() -> Result<()> {
        let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let executable = std::fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let catalogue = Catalogue::read(&executable)?;
            assert_eq!(catalogue.entries.len(), 79);
            assert_eq!(
                catalogue
                    .entries
                    .iter()
                    .map(|entry| entry.frames.len())
                    .sum::<usize>(),
                96
            );
            let mut shared = [0; 9];
            let mut script = [0; 8];
            for entry in &catalogue.entries {
                match entry.image {
                    ImageBinding::Shared(index) => shared[usize::from(index)] += 1,
                    ImageBinding::Script(slot) => script[usize::from(slot)] += 1,
                    ImageBinding::CapturedScene => {
                        panic!("unexpected original framebuffer binding")
                    }
                    ImageBinding::Fallback(selector) => {
                        panic!("unexpected original selector {selector}")
                    }
                }
            }
            assert_eq!(shared, [11, 0, 23, 1, 17, 1, 1, 17, 0]);
            assert_eq!(script, [1; 8]);
            let restored: Catalogue = serde_json::from_slice(&serde_json::to_vec(&catalogue)?)?;
            assert_eq!(
                restored.encode()?,
                dol::slice(&executable, START, (END - START) as usize)?
            );
        }
        Ok(())
    }
}
