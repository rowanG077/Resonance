//! Image owners and shared, named portrait tracks. KTX2 owns image dimensions.
use super::*;

pub const PATH: &str = "skits/portraits.json";
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Portraits {
    pub mappings: BTreeMap<u32, String>,
    pub images: BTreeMap<String, Vec<String>>,
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
            for path in paths {
                validate_path(path)?;
                let bytes = required(&mut read, path, 64 * 1024 * 1024)?;
                let size = resonance_asset_writer::texture::dimensions(&bytes)
                    .with_context(|| format!("portrait image {path}"))?;
                images.push(PortraitImage {
                    texture: path.clone(),
                    size,
                });
            }
            let size = images
                .first()
                .with_context(|| format!("portrait {name} has no base image"))?
                .size;
            owners.insert(name.clone(), PortraitAsset { size, images });
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
