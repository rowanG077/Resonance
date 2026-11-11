//! Editable dialogue drawing roles with authored image coordinate sizes.
use super::*;

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
    symbols: UiTexture,
    shading: UiTexture,
    patterns: [UiTexture; 5],
    lined_fill: UiTexture,
    plain_fill: UiTexture,
    cursor: UiTexture,
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
            symbols: self.textures[0].clone(),
            shading: self.textures[1].clone(),
            patterns: std::array::from_fn(|i| self.textures[i + 2].clone()),
            lined_fill: self.textures[7].clone(),
            plain_fill: self.textures[8].clone(),
            cursor: self.cursor.clone(),
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
        let art = Self {
            font: definition.font,
            textures: [definition.symbols, definition.shading]
                .into_iter()
                .chain(definition.patterns)
                .chain([definition.lined_fill, definition.plain_fill])
                .collect(),
            cursor: definition.cursor,
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
    /// Capture required pixels while preserving this document's coordinates.
    pub fn prepare_images<B: AsRef<[u8]>>(
        &self,
        mut read: impl FnMut(&str) -> Result<B>,
    ) -> Result<()> {
        self.validate()?;
        for image in self.textures.iter().chain([&self.cursor]) {
            crate::ui_art::prepare_image(image, &mut read)?;
        }
        Ok(())
    }
}
