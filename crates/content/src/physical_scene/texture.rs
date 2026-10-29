use super::{TextureCatalogue, TextureImage, TextureSource};
use crate::texture::{Filter, Sampler, TextureLod};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

impl Default for TextureSource {
    fn default() -> Self {
        Self::Local(TextureCatalogue::default())
    }
}

impl TextureSource {
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Local(catalogue) if catalogue.textures.is_empty())
    }
}

fn clamp() -> [crate::TextureWrap; 2] {
    [crate::TextureWrap::Clamp; 2]
}
fn is_clamp(wrap: &[crate::TextureWrap; 2]) -> bool {
    *wrap == clamp()
}

// Bank sampling defaults to ordinary image display. The engine's separate
// material sampler retains its repeat default; neither default depends on the
// other. Explicit overrides replace the complete bank sampler.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sampling {
    #[serde(default = "clamp", skip_serializing_if = "is_clamp")]
    wrap: [crate::TextureWrap; 2],
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    min_filter: Filter,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    mag_filter: Filter,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    lod: TextureLod,
}
impl From<&Sampler> for Sampling {
    fn from(s: &Sampler) -> Self {
        Self {
            wrap: s.wrap,
            min_filter: s.min_filter,
            mag_filter: s.mag_filter,
            lod: s.lod,
        }
    }
}
impl From<Sampling> for Sampler {
    fn from(s: Sampling) -> Self {
        Self {
            wrap: s.wrap,
            min_filter: s.min_filter,
            mag_filter: s.mag_filter,
            lod: s.lod,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sampler: Option<Sampling>,
    images: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Image {
    Path(String),
    Settings(Settings),
}
impl Image {
    fn from_texture(texture: &TextureImage, inherited: &Sampler) -> Self {
        if texture.images.len() == 1 && texture.sampler == *inherited {
            Self::Path(texture.images[0].clone())
        } else {
            Self::Settings(Settings {
                sampler: (texture.sampler != *inherited).then(|| (&texture.sampler).into()),
                images: texture.images.clone(),
            })
        }
    }
    fn into_texture(self, inherited: &Sampler) -> TextureImage {
        match self {
            Self::Path(path) => TextureImage {
                sampler: inherited.clone(),
                images: vec![path],
            },
            Self::Settings(s) => TextureImage {
                sampler: s
                    .sampler
                    .map(Into::into)
                    .unwrap_or_else(|| inherited.clone()),
                images: s.images,
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Shared {
    sampler: Sampling,
    entries: Vec<Option<Image>>,
}
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Bank {
    Entries(Vec<Option<Image>>),
    Shared(Shared),
}

impl Serialize for TextureCatalogue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let default = Self::default_sampler();
        let common = self
            .textures
            .iter()
            .flatten()
            .next()
            .map(|t| &t.sampler)
            .filter(|s| self.textures.iter().flatten().all(|t| t.sampler == **s))
            .unwrap_or(&default);
        let entries = self
            .textures
            .iter()
            .map(|t| t.as_ref().map(|t| Image::from_texture(t, common)))
            .collect();
        if *common == default {
            Bank::Entries(entries)
        } else {
            Bank::Shared(Shared {
                sampler: common.into(),
                entries,
            })
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for TextureCatalogue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (entries, sampler) = match Bank::deserialize(deserializer)? {
            Bank::Entries(entries) => (entries, Self::default_sampler()),
            Bank::Shared(s) => (s.entries, s.sampler.into()),
        };
        Ok(Self {
            textures: entries
                .into_iter()
                .map(|t| t.map(|t| t.into_texture(&sampler)))
                .collect(),
        })
    }
}

impl Serialize for TextureImage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Image::from_texture(self, &TextureCatalogue::default_sampler()).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for TextureImage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Image::deserialize(deserializer)?.into_texture(&TextureCatalogue::default_sampler()))
    }
}
