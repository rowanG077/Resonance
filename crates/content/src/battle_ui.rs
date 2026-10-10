//! Battle artwork and fixed 640×480 layouts, without presentation state.
use crate::font::{BitmapFont, UiTexture};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const PATH: &str = "battle/ui.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Art {
    pub version: u32,
    /// Atlas sampling is linear with clamped edges.
    pub font: BitmapFont,
    /// Character IDs 1–9; four vertically stacked 64×64 expressions each.
    /// Portrait sampling is linear with repeated edges.
    #[serde(default)]
    pub portraits: [Option<UiTexture>; 9],
}

fn validate_portrait(image: &UiTexture) -> Result<()> {
    image.validate()?;
    ensure!(
        [image.width, image.height] == [64, 256],
        "invalid battle portrait expressions"
    );
    Ok(())
}

impl Art {
    pub const VERSION: u32 = 33;

    /// Decode required controls and independently admit optional feedback.
    pub fn load(files: &crate::prepared::Files) -> Result<Self> {
        let mut art = Self::decode(&files.read(PATH)?, files.diagnostics())?;
        for portrait in &mut art.portraits {
            if let Some(image) = portrait
                && files
                    .diagnostics()
                    .attempt("battle portrait image", files.read(&image.path))?
                    .is_none()
            {
                *portrait = None;
            }
        }
        Ok(art)
    }

    pub fn decode(bytes: &[u8], diagnostics: &crate::diagnostics::Diagnostics) -> Result<Self> {
        #[derive(Deserialize)]
        struct Source {
            #[serde(flatten)]
            art: Art,
            #[serde(default)]
            portraits: serde_json::Value,
        }
        let mut source: Source = serde_json::from_slice(bytes)?;
        if let Some(values) = diagnostics
            .attempt(
                "battle portraits",
                serde_json::from_value::<Option<[serde_json::Value; 9]>>(source.portraits)
                    .map_err(anyhow::Error::from),
            )?
            .flatten()
        {
            for (portrait, value) in source.art.portraits.iter_mut().zip(values) {
                *portrait = diagnostics
                    .attempt(
                        "battle portrait",
                        (|| {
                            let portrait: Option<UiTexture> = serde_json::from_value(value)?;
                            if let Some(image) = &portrait {
                                validate_portrait(image)?;
                            }
                            Ok(portrait)
                        })(),
                    )?
                    .flatten();
            }
        }
        source.art.validate_layout()?;
        Ok(source.art)
    }

    pub fn validate_required_dialogue(&self, font: &BitmapFont) -> Result<()> {
        font.validate_text(
            "0123456789 HP TP / CASTING Unknown PARTY ENEMY HITS DAMAGE STUN DEFEATED ESCAPED:",
        )
    }

    pub fn files(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.font.texture.as_str()).chain(
            self.portraits
                .iter()
                .flatten()
                .map(|image| image.path.as_str()),
        )
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_layout()?;
        for portrait in self.portraits.iter().flatten() {
            validate_portrait(portrait)?;
        }
        Ok(())
    }

    /// Required controls and shared geometry; optional feedback validates separately.
    pub fn validate_layout(&self) -> Result<()> {
        ensure!(
            self.version == Self::VERSION,
            "unsupported battle UI version"
        );
        self.font.validate()
    }
}
