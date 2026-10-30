//! Credits text, pictures and an ordinary recording. Runtime timing is derived
//! from the captured WAV, preserving an explicit playback-rate override only.
use super::{Credits, Manifest, Operation, Scroll, Style};
use crate::{field_audio::Voice, font::UiTexture, library::Snapshot, preparation};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const PATH: &str = "ui/credits/credits.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub style: Style,
    pub scroll: Scroll,
    pub blocks: Vec<Block>,
    pub music: Recording,
    pub final_hold_ticks: u32,
}

/// Text advances one normal line after its last line. Images do not advance;
/// an explicit gap positions the following block. All distances are pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Block {
    Text {
        lines: Vec<String>,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        align: Alignment,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_spacing: Option<u16>,
    },
    Gap {
        gap: i32,
    },
    Image {
        image: String,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        offset_x: f32,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Center,
    Left,
}

impl Definition {
    pub fn prepare(&self, mut read: impl FnMut(&str) -> Result<Arc<[u8]>>) -> Result<Manifest> {
        crate::library::validate_path(&self.music.path)?;
        let bytes =
            read(&self.music.path).with_context(|| format!("{PATH} music {}", self.music.path))?;
        let music = Voice::from_wave(&self.music.path, self.music.sample_rate, &bytes)
            .with_context(|| format!("{PATH} music {}", self.music.path))?;
        let mut pictures: Vec<UiTexture> = Vec::new();
        let mut operations = Vec::new();
        for block in &self.blocks {
            match block {
                Block::Gap { gap } => operations.push(Operation::VerticalSpace { pixels: *gap }),
                Block::Image { image, offset_x } => {
                    ensure!(offset_x.is_finite(), "invalid credits image offset");
                    crate::library::validate_path(image)?;
                    let index = match pictures.iter().position(|p| p.path == *image) {
                        Some(index) => index,
                        None => {
                            let bytes =
                                read(image).with_context(|| format!("{PATH} image {image}"))?;
                            let [width, height] =
                                resonance_asset_writer::texture::dimensions(&bytes)
                                    .with_context(|| format!("{PATH} image {image}"))?;
                            pictures.push(UiTexture {
                                path: image.clone(),
                                width,
                                height,
                            });
                            pictures.len() - 1
                        }
                    };
                    operations.push(Operation::PositionX { pixels: *offset_x });
                    operations.push(Operation::Picture {
                        index: index.try_into().context("too many credits images")?,
                    });
                }
                Block::Text {
                    lines,
                    align,
                    line_spacing,
                } => {
                    ensure!(!lines.is_empty(), "empty credits text block");
                    let spacing = line_spacing.unwrap_or(self.style.line_height);
                    ensure!(spacing > 0, "invalid credits line spacing");
                    for (index, line) in lines.iter().enumerate() {
                        ensure!(
                            !line.contains(['\r', '\n']),
                            "credits lines must be separate array entries"
                        );
                        if *align == Alignment::Center {
                            operations.push(Operation::CenterLine);
                        }
                        for (i, text) in line.split('\t').enumerate() {
                            if i > 0 {
                                operations.push(Operation::Tab);
                            }
                            if !text.is_empty() {
                                operations.push(Operation::Text { text: text.into() });
                            }
                        }
                        operations.push(if index + 1 == lines.len() {
                            Operation::Newline
                        } else {
                            Operation::VerticalSpace {
                                pixels: i32::from(spacing),
                            }
                        });
                    }
                }
            }
        }
        let manifest = Manifest {
            program: Credits {
                style: self.style.clone(),
                scroll: self.scroll.clone(),
                operations,
            },
            pictures,
            music,
            final_hold_ticks: self.final_hold_ticks,
        };
        manifest.validate().with_context(|| PATH.to_owned())?;
        Ok(manifest)
    }
}

pub fn capture(snapshot: &mut Snapshot, cancelled: &impl Fn() -> bool) -> Result<()> {
    ensure!(!cancelled(), "credits preparation cancelled");
    let definition: Definition = snapshot.json(PATH, preparation::JSON_LIMIT)?;
    let manifest = definition.prepare(|path| {
        ensure!(!cancelled(), "credits preparation cancelled");
        snapshot.read(path, preparation::ASSET_LIMIT)
    })?;
    let dialogue: crate::font::DialogueArt =
        snapshot.json("ui/dialogue.json", preparation::JSON_LIMIT)?;
    dialogue.validate()?;
    let font = preparation::font(snapshot, &dialogue.font)?;
    for operation in &manifest.program.operations {
        if let Operation::Text { text } = operation {
            for c in text.chars() {
                ensure!(
                    font.glyphs.contains_key(&c),
                    "credits text needs missing glyph {c:?} in {}",
                    dialogue.font
                );
            }
        }
    }
    ensure!(!cancelled(), "credits preparation cancelled");
    Ok(())
}
