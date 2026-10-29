//! Editable dialogue drawing roles. Image dimensions belong to their KTX2 files.
use super::*;
use anyhow::Context;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Style {
    Plain,
    Ornate,
    Gold,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    color: [u8; 4],
    row_offsets: [i8; 9],
    bob_amplitude: f32,
    bob_radians_per_tick: f32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    font: String,
    symbols: String,
    shading: String,
    patterns: [String; 5],
    lined_fill: String,
    plain_fill: String,
    cursor: String,
    default_style: Style,
    selection: Selection,
}

impl Serialize for DialogueArt {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        Definition {
            font: self.font.clone(),
            symbols: self.textures[0].path.clone(),
            shading: self.textures[1].path.clone(),
            patterns: std::array::from_fn(|i| self.textures[i + 2].path.clone()),
            lined_fill: self.textures[7].path.clone(),
            plain_fill: self.textures[8].path.clone(),
            cursor: self.cursor.path.clone(),
            default_style: [Style::Plain, Style::Ornate, Style::Gold]
                [usize::from(self.selection.mode)],
            selection: Selection {
                color: self.selection.color,
                row_offsets: self.selection.row_offsets,
                bob_amplitude: self.selection.bob_amplitude,
                bob_radians_per_tick: self.selection.bob_step,
            },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DialogueArt {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let definition = Definition::deserialize(deserializer)?;
        let image = |path| UiTexture {
            path,
            width: 0,
            height: 0,
        };
        let art = Self {
            font: definition.font,
            textures: [definition.symbols, definition.shading]
                .into_iter()
                .chain(definition.patterns)
                .chain([definition.lined_fill, definition.plain_fill])
                .map(image)
                .collect(),
            cursor: image(definition.cursor),
            selection: SelectionArt {
                mode: match definition.default_style {
                    Style::Plain => 0,
                    Style::Ornate => 1,
                    Style::Gold => 2,
                },
                color: definition.selection.color,
                row_offsets: definition.selection.row_offsets,
                bob_amplitude: definition.selection.bob_amplitude,
                bob_step: definition.selection.bob_radians_per_tick,
            },
        };
        art.validate().map_err(serde::de::Error::custom)?;
        Ok(art)
    }
}

impl DialogueArt {
    /// Resolve required image sizes from captured headers. Font-only consumers
    /// can read this document without preparing any dialogue image.
    pub fn prepare_images<B: AsRef<[u8]>>(
        &mut self,
        mut read: impl FnMut(&str) -> Result<B>,
    ) -> Result<()> {
        self.validate()?;
        for image in self.textures.iter_mut().chain([&mut self.cursor]) {
            let [width, height] = read(&image.path)
                .and_then(|bytes| resonance_asset_writer::texture::dimensions(bytes.as_ref()))
                .with_context(|| format!("dialogue image {}", image.path))?;
            image.width = width;
            image.height = height;
            image
                .validate()
                .with_context(|| format!("dialogue image {}", image.path))?;
        }
        Ok(())
    }
}
