//! Image owners, tile coordinate sizes and shared, named portrait tracks.
use super::*;

pub const PATH: &str = "skits/portraits.json";
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Portraits {
    pub mappings: BTreeMap<u32, String>,
    pub images: BTreeMap<String, Vec<crate::font::UiTexture>>,
    pub animations: BTreeMap<String, Animation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    pub ids: Vec<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eyes: Option<Track>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouth: Option<Track>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<Track>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub repeat: bool,
    pub frames: Vec<Frame>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub duration_ticks: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<u16>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub position: [i16; 2],
}
impl Animation {
    pub fn from_recipe(ids: Vec<u16>, recipe: &PortraitRecipe) -> Result<Self> {
        let mut tracks = Vec::new();
        for (frames, repeat) in recipe.tracks.iter().zip(recipe.repeat) {
            tracks.push(if frames.is_empty() {
                None
            } else {
                Some(Track {
                    repeat,
                    frames: frames
                        .iter()
                        .map(|frame| {
                            Ok(Frame {
                                duration_ticks: u16::try_from(i32::from(frame.ticks) + 1)?,
                                image: frame.image,
                                position: frame.position,
                            })
                        })
                        .collect::<Result<_>>()?,
                })
            });
        }
        let [eyes, mouth, extra] = tracks.try_into().unwrap();
        Ok(Self {
            ids,
            eyes,
            mouth,
            extra,
        })
    }
    pub fn recipe(&self) -> Result<PortraitRecipe> {
        ensure!(
            !self.ids.is_empty(),
            "portrait animation needs selector IDs"
        );
        let mut result = PortraitRecipe::default();
        for (index, track) in [&self.eyes, &self.mouth, &self.extra]
            .into_iter()
            .enumerate()
        {
            if let Some(track) = track {
                ensure!(
                    !track.frames.is_empty() && track.frames.len() <= 255,
                    "portrait track needs 1–255 frames"
                );
                result.repeat[index] = track.repeat;
                result.tracks[index] = track
                    .frames
                    .iter()
                    .map(|frame| {
                        ensure!(
                            (1..=32767).contains(&frame.duration_ticks),
                            "portrait frame duration must be 1–32767 ticks"
                        );
                        Ok(PortraitFrame {
                            ticks: (frame.duration_ticks - 1) as i16,
                            image: frame.image,
                            position: frame.position,
                        })
                    })
                    .collect::<Result<_>>()?;
            }
        }
        Ok(result)
    }
}
impl Portraits {
    pub fn resolve(
        &self,
        mut read: impl FnMut(&str, usize) -> Result<Option<Arc<[u8]>>>,
    ) -> Result<(BTreeMap<u32, PortraitAsset>, BTreeMap<u16, PortraitRecipe>)> {
        let mut owners = BTreeMap::new();
        for (name, paths) in &self.images {
            validate_path(name)?;
            let mut images = Vec::new();
            let mut scale = None;
            let mut compressed = None;
            for source in paths {
                source.validate()?;
                let path = &source.path;
                let bytes = required(&mut read, path, 64 * 1024 * 1024)?;
                let size = [source.width, source.height];
                let physical = resonance_asset_writer::texture::dimensions(&bytes)?;
                let image_scale = [physical[0] / size[0], physical[1] / size[1]];
                ensure!(
                    physical == [size[0] * image_scale[0], size[1] * image_scale[1]]
                        && *scale.get_or_insert(image_scale) == image_scale,
                    "portrait {name}: every image needs the same whole-number resolution scale ({path})"
                );
                // Tiles are copied between textures as stored.
                let blocks = resonance_asset_writer::gpu::is_native_compressed(&bytes)?;
                ensure!(
                    *compressed.get_or_insert(blocks) == blocks,
                    "portrait {name}: every image needs the same texture format ({path})"
                );
                images.push(PortraitImage {
                    texture: path.clone(),
                    size,
                });
            }
            let size = images
                .first()
                .with_context(|| format!("portrait {name} has no base image"))?
                .size;
            owners.insert(
                name.clone(),
                PortraitAsset {
                    size,
                    scale: scale.unwrap_or([1; 2]),
                    images,
                },
            );
        }
        let portraits = self
            .mappings
            .iter()
            .map(|(id, name)| {
                Ok((
                    *id,
                    owners
                        .get(name)
                        .with_context(|| format!("unknown portrait {name}"))?
                        .clone(),
                ))
            })
            .collect::<Result<_>>()?;
        let mut recipes = BTreeMap::new();
        for (name, animation) in &self.animations {
            validate_path(name)?;
            let recipe = animation
                .recipe()
                .with_context(|| format!("portrait animation {name}"))?;
            for id in &animation.ids {
                ensure!(
                    recipes.insert(*id, recipe.clone()).is_none(),
                    "portrait selector {id} belongs to multiple animations"
                );
            }
        }
        Ok((portraits, recipes))
    }
}

#[cfg(test)]
mod tests;
