//! Drawing roles and direct images; numbered GPU bindings are prepared in memory.
use super::*;
use crate::diagnostics::Diagnostics;

#[cfg(test)]
mod tests;

const IMAGES: [&str; 17] = [
    "panel_left",
    "panel_right",
    "panel_middle",
    "scroll_down",
    "scroll_up",
    "icons",
    "lloyd_portrait",
    "colette_portrait",
    "genis_portrait",
    "raine_portrait",
    "sheena_portrait",
    "zelos_portrait",
    "presea_portrait",
    "regal_portrait",
    "kratos_portrait",
    "sylvarant_map",
    "tethealla_map",
];
const STYLES: [&str; 3] = ["plain", "ornate", "gold"];
const EDGES: [&str; 11] = [
    "top",
    "bottom",
    "left",
    "right",
    "top_left",
    "top_right",
    "bottom_left",
    "bottom_right",
    "bottom_right_flourish",
    "top_left_flourish",
    "bottom_left_flourish",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Image {
    Path(String),
    Settings {
        image: String,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        repeat: bool,
        #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
        opaque: bool,
    },
}
impl From<&MenuTexture> for Image {
    fn from(value: &MenuTexture) -> Self {
        if !value.repeat && !value.opaque {
            Self::Path(value.path.clone())
        } else {
            Self::Settings {
                image: value.path.clone(),
                repeat: value.repeat,
                opaque: value.opaque,
            }
        }
    }
}
impl From<Image> for MenuTexture {
    fn from(value: Image) -> Self {
        let (path, repeat, opaque) = match value {
            Image::Path(path) => (path, false, false),
            Image::Settings {
                image,
                repeat,
                opaque,
            } => (image, repeat, opaque),
        };
        Self {
            path,
            width: 0,
            height: 0,
            repeat,
            opaque,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Window {
    fill: Image,
    /// Normally all styles use the shared background choices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    patterns: Option<[Image; 5]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frame: Option<BTreeMap<String, Image>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    heading: Option<Image>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cursor: Option<Image>,
    cursor_motion: [f32; 2],
    outset: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    flourish_outset: [u8; 2],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    foot_outset: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    left_joins: [u8; 2],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    left_strip: [u8; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    fill: [u8; 4],
    popup_fill: [u8; 4],
    shade: [[u8; 4]; 2],
    palette: BTreeMap<String, [u8; 4]>,
    #[serde(default)]
    images: serde_json::Value,
    #[serde(default)]
    patterns: serde_json::Value,
    #[serde(default)]
    windows: serde_json::Value,
    #[serde(default)]
    labels: serde_json::Value,
    #[serde(default)]
    sprites: serde_json::Value,
}

impl MenuArt {
    pub fn decode(bytes: &[u8], diagnostics: &Diagnostics) -> Result<Self> {
        Self::prepare(serde_json::from_slice(bytes)?, diagnostics)
    }

    fn prepare(source: Source, diagnostics: &Diagnostics) -> Result<Self> {
        let palette = crate::ui_art::ordered(source.palette, super::sprites::COLORS)?;
        let sprites = MenuSprites::decode(source.sprites, &palette, diagnostics)?;
        let mut textures = BTreeMap::new();
        for (name, image) in
            decode_art_rows::<String, Image>(source.images, diagnostics, "menu image")?
        {
            if let Some(index) = diagnostics.attempt(
                &format!("menu image {name}"),
                IMAGES
                    .iter()
                    .position(|&role| role == name)
                    .context("unknown menu image role"),
            )? {
                textures.insert(index + 13, image.into());
            }
        }
        let patterns: Vec<serde_json::Value> =
            decode_art_group(source.patterns, diagnostics, "menu background patterns")?;
        diagnostics.attempt(
            "menu background patterns",
            (|| {
                ensure!(patterns.len() == 5, "expected five menu background choices");
                Ok(())
            })(),
        )?;
        for (index, value) in patterns.into_iter().take(5).enumerate() {
            if let Some(image) = diagnostics.attempt(
                &format!("menu background {}", index + 1),
                serde_json::from_value::<Image>(value).map_err(anyhow::Error::from),
            )? {
                textures.insert(45 + index, image.into());
            }
        }
        let mut windows = BTreeMap::new();
        for (name, window) in
            decode_art_rows::<String, Window>(source.windows, diagnostics, "menu window")?
        {
            let result = (|| {
                let index = STYLES
                    .iter()
                    .position(|&style| style == name)
                    .context("unknown window style")?;
                let mut images = BTreeMap::new();
                let fill = [30, 0, 31][index];
                images.insert(fill, window.fill.into());
                let mut patterns = [45, 46, 47, 48, 49, fill];
                if let Some(overrides) = window.patterns {
                    for (i, image) in overrides.into_iter().enumerate() {
                        let slot = 64 + index * 5 + i;
                        patterns[i] = slot;
                        images.insert(slot, image.into());
                    }
                }
                let slices = window
                    .frame
                    .map(|mut frame| -> Result<_> {
                        let mut slices = [fill; 12];
                        for (i, name) in EDGES.into_iter().enumerate() {
                            let image = frame
                                .remove(name)
                                .with_context(|| format!("missing frame {name}"))?;
                            let slot = [53, 1, 32][index] + i;
                            images.insert(slot, image.into());
                            slices[i + 1] = slot;
                        }
                        ensure!(frame.is_empty(), "unknown frame piece");
                        Ok(slices)
                    })
                    .transpose()?;
                let heading = window.heading.map(|image| {
                    let slot = [52, 12, 43][index];
                    images.insert(slot, image.into());
                    slot
                });
                let cursor = window.cursor.map(|image| {
                    let slot = [50, 51, 44][index];
                    images.insert(slot, image.into());
                    slot
                });
                Ok((
                    index,
                    WindowArt {
                        patterns,
                        slices,
                        heading,
                        cursor,
                        cursor_motion: window.cursor_motion,
                        outset: window.outset,
                        flourish_outset: window.flourish_outset,
                        foot_outset: window.foot_outset,
                        left_joins: window.left_joins,
                        left_strip: window.left_strip,
                    },
                    images,
                ))
            })();
            if let Some((index, window, images)) =
                diagnostics.attempt(&format!("menu window {name}"), result)?
            {
                windows.insert(index, window);
                textures.extend(images);
            }
        }
        Ok(Self {
            fill: source.fill,
            popup_fill: source.popup_fill,
            shade: source.shade,
            palette,
            textures,
            windows,
            labels: decode_labels(source.labels, diagnostics, "menu artwork label")?,
            sprites,
        })
    }

    /// Resolve sizes from captured image headers, without decoding pixels. Invalid
    /// optional images stay unavailable only to the pages which draw them.
    pub fn prepare_resources<B: AsRef<[u8]>>(&mut self, mut read: impl FnMut(&str) -> Result<B>) {
        self.sprites.prepare_catalogues(&mut read);
        for texture in self.textures.values_mut() {
            let result = (|| {
                crate::validate_asset_path(&texture.path)?;
                resonance_asset_writer::texture::dimensions(read(&texture.path)?.as_ref())
            })();
            // Selection validates the image. An unused broken portrait must not
            // prevent another page from opening, even in strict diagnostic mode.
            let [width, height] = result.unwrap_or([0, 0]);
            texture.width = width;
            texture.height = height;
        }
    }
}

impl Serialize for MenuArt {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Definition<'a> {
            images: BTreeMap<&'static str, Image>,
            patterns: Vec<Image>,
            windows: BTreeMap<&'static str, Window>,
            fill: [u8; 4],
            popup_fill: [u8; 4],
            shade: [[u8; 4]; 2],
            palette: BTreeMap<&'a str, &'a [u8; 4]>,
            labels: &'a BTreeMap<String, String>,
            sprites: serde_json::Value,
        }
        let image = |index: usize| -> Result<Image> {
            self.textures
                .get(&index)
                .map(Image::from)
                .with_context(|| format!("missing menu image {index}"))
        };
        let result = (|| -> Result<_> {
            let patterns = (45..50).map(&image).collect::<Result<Vec<_>>>()?;
            let mut windows = BTreeMap::new();
            for (&index, window) in &self.windows {
                let name = *STYLES.get(index).context("unknown menu window style")?;
                let choices = window.patterns[..5]
                    .iter()
                    .map(|&i| image(i))
                    .collect::<Result<Vec<_>>>()?;
                let frame = window
                    .slices
                    .map(|slices| {
                        ensure!(
                            image(slices[0])? == image(window.patterns[5])?,
                            "frame and window fill must share an image"
                        );
                        EDGES
                            .into_iter()
                            .zip(&slices[1..])
                            .map(|(name, &index)| Ok((name.into(), image(index)?)))
                            .collect::<Result<_>>()
                    })
                    .transpose()?;
                windows.insert(
                    name,
                    Window {
                        fill: image(window.patterns[5])?,
                        patterns: if choices == patterns {
                            None
                        } else {
                            Some(choices.try_into().unwrap())
                        },
                        frame,
                        heading: window.heading.map(&image).transpose()?,
                        cursor: window.cursor.map(&image).transpose()?,
                        cursor_motion: window.cursor_motion,
                        outset: window.outset,
                        flourish_outset: window.flourish_outset,
                        foot_outset: window.foot_outset,
                        left_joins: window.left_joins,
                        left_strip: window.left_strip,
                    },
                );
            }
            Ok(Definition {
                images: IMAGES
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, name)| {
                        self.textures
                            .get(&(i + 13))
                            .map(|value| (name, Image::from(value)))
                    })
                    .collect(),
                patterns,
                windows,
                fill: self.fill,
                popup_fill: self.popup_fill,
                shade: self.shade,
                palette: crate::ui_art::named(&self.palette, super::sprites::COLORS)?,
                labels: &self.labels,
                sprites: self.sprites.encode(&self.palette)?,
            })
        })();
        result
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for MenuArt {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        Self::prepare(Source::deserialize(deserializer)?, &Diagnostics::new(true))
            .map_err(serde::de::Error::custom)
    }
}
