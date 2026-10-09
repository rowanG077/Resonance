//! Prepared credits, independent of the source text control language.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "game/credits.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub program: Credits,
    pub pictures: Vec<crate::font::UiTexture>,
    pub music: crate::field_audio::Voice,
    pub final_hold_ticks: u32,
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
        let p = &self.program;
        ensure!(
            p.version == 1 && p.canvas == [640, 480],
            "invalid credits canvas"
        );
        ensure!(
            p.style.glyph_size.iter().all(|&v| v > 0) && p.style.line_height > 0,
            "invalid credits text size"
        );
        ensure!(
            p.scroll.height_pixels >= i32::from(p.scroll.stop_margin_pixels)
                && p.scroll.speed_divisor_ticks > 0,
            "invalid credits scroll"
        );
        self.music.validate()?;
        for picture in &self.pictures {
            picture.validate()?;
        }
        for op in &p.operations {
            if let Operation::Picture { index } = op {
                ensure!(
                    usize::from(*index) < self.pictures.len(),
                    "missing credits picture"
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credits {
    pub version: u8,
    pub canvas: [u16; 2],
    pub style: Style,
    pub scroll: Scroll,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Style {
    pub glyph_size: [u16; 2],
    pub color: [u8; 4],
    pub line_height: u16,
    pub tab_width: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scroll {
    pub height_pixels: i32,
    /// Speed is height_pixels / speed_divisor_ticks pixels per update.
    pub speed_divisor_ticks: u32,
    /// Scrolling stops once its offset reaches height_pixels - stop_margin_pixels.
    pub stop_margin_pixels: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    Text {
        text: String,
    },
    /// Center the remaining line using the font’s glyph advances.
    CenterLine,
    Newline,
    Tab,
    /// Reset X and advance Y; the command's terminating newline is consumed.
    VerticalSpace {
        pixels: i32,
    },
    /// Draw at the current cursor without advancing it.
    Picture {
        index: u8,
    },
    /// Reserved controls without a layout operation.
    IgnoredControl {
        code: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        argument: Option<u8>,
    },
}
