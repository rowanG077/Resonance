//! Cooked bitmap glyphs and advances; no executable lookup tables at runtime.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A literal text run using the dialogue palette, shared by system notices and menus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextSpan {
    pub text: String,
    pub color: u8,
}
impl TextSpan {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.color <= 6
                && self.text.len() <= 8192
                && self
                    .text
                    .chars()
                    .all(|c| !c.is_control() || matches!(c, '\n' | '\u{c}')),
            "invalid system text span"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitmapFont {
    pub version: u32,
    pub texture: String,
    pub width: u32,
    pub height: u32,
    pub line_height: u32,
    pub glyphs: BTreeMap<char, Glyph>,
    pub source_sha256: String,
    pub executable_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Glyph {
    pub rect: [u32; 4],
    pub advance: u32,
}

/// Subtitle positions and advances are cooked from the movie's authored text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovieSubtitles {
    pub version: u32,
    pub movie: u32,
    pub cues: Vec<SubtitleCue>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleCue {
    pub frame: u32,
    pub lines: Vec<SubtitleLine>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleLine {
    pub position: [f32; 2],
    pub text: String,
}
impl MovieSubtitles {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && !self.cues.is_empty() && self.cues.len() <= 1024,
            "invalid movie subtitle track"
        );
        ensure!(
            self.cues.windows(2).all(|c| c[0].frame < c[1].frame),
            "unordered subtitle cues"
        );
        for cue in &self.cues {
            ensure!(
                cue.lines.len() <= 3
                    && cue.lines.iter().all(|l| {
                        l.position.iter().all(|v| v.is_finite()) && l.text.len() <= 1024
                    }),
                "invalid subtitle line"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueArt {
    pub version: u32,
    pub font: String,
    pub textures: Vec<UiTexture>,
    pub cursor: UiTexture,
    pub selection: SelectionArt,
    pub source_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiTexture {
    pub path: String,
    pub width: u32,
    pub height: u32,
}
impl UiTexture {
    pub fn validate(&self) -> Result<()> {
        crate::validate_asset_path(&self.path)?;
        ensure!(
            (1..=4096).contains(&self.width) && (1..=4096).contains(&self.height),
            "invalid UI texture size"
        );
        Ok(())
    }
}

/// A pixel rectangle in a shared image; no separately cropped texture is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiRegion {
    pub texture: UiTexture,
    pub rect: [u32; 4],
}
impl UiRegion {
    pub fn size(&self) -> [u32; 2] {
        [self.rect[2], self.rect[3]]
    }

    pub fn validate(&self) -> Result<()> {
        self.texture.validate()?;
        let [x, y, width, height] = self.rect;
        ensure!(
            width > 0
                && height > 0
                && x.checked_add(width)
                    .is_some_and(|end| end <= self.texture.width)
                && y.checked_add(height)
                    .is_some_and(|end| end <= self.texture.height),
            "UI region exceeds its shared texture"
        );
        Ok(())
    }
}

#[test]
fn regions_reject_empty_overflowing_and_out_of_image_rectangles() {
    let mut region = UiRegion {
        texture: UiTexture {
            path: "assets/atlas.ktx2".into(),
            width: 512,
            height: 512,
        },
        rect: [488, 480, 24, 32],
    };
    assert!(region.validate().is_ok());
    for rect in [
        [0, 0, 0, 24],
        [0, 0, 24, 0],
        [489, 480, 24, 32],
        [488, 481, 24, 32],
        [u32::MAX, 0, 24, 24],
    ] {
        region.rect = rect;
        assert!(region.validate().is_err());
    }
    region.rect = [0, 0, 24, 24];
    region.texture.path = "../atlas.ktx2".into();
    assert!(region.validate().is_err());
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionArt {
    pub mode: u8,
    pub color: [u8; 4],
    pub row_offsets: [i8; 9],
    pub bob_amplitude: f32,
    pub bob_step: f32,
}
impl DialogueArt {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 2 && self.textures.len() == 9,
            "invalid dialogue art manifest"
        );
        crate::validate_asset_path(&self.font)?;
        ensure!(
            self.selection.mode <= 2
                && self
                    .selection
                    .row_offsets
                    .iter()
                    .all(|v| (0..=32).contains(v))
                && self.selection.bob_amplitude.is_finite()
                && (0. ..=16.).contains(&self.selection.bob_amplitude)
                && self.selection.bob_step.is_finite()
                && (0. ..=1.).contains(&self.selection.bob_step),
            "invalid selection artwork"
        );
        for texture in self.textures.iter().chain([&self.cursor]) {
            texture.validate()?;
        }
        ensure!(
            self.source_sha256.len() == 64
                && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid dialogue art digest"
        );
        Ok(())
    }
}
impl BitmapFont {
    pub fn validate(&self) -> Result<()> {
        crate::validate_asset_path(&self.texture)?;
        ensure!(
            self.version == 1
                && (1..=4096).contains(&self.width)
                && (1..=4096).contains(&self.height)
                && (1..=64).contains(&self.line_height),
            "invalid font dimensions"
        );
        ensure!(
            !self.glyphs.is_empty() && self.glyphs.len() <= 16384,
            "invalid font glyph count"
        );
        for glyph in self.glyphs.values() {
            let [x, y, w, h] = glyph.rect;
            ensure!(
                w > 0
                    && h > 0
                    && x.checked_add(w).is_some_and(|e| e <= self.width)
                    && y.checked_add(h).is_some_and(|e| e <= self.height)
                    && glyph.advance <= 64,
                "invalid font glyph rectangle"
            );
        }
        for hash in [&self.source_sha256, &self.executable_sha256] {
            ensure!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid font source digest"
            );
        }
        Ok(())
    }
}
