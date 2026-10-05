//! Embedded artwork and sprite registrations. No runtime authoring or patching.
use crate::{
    drawing::{self, Image},
    timeline::Visit,
    tree::growth::{BranchCurve, CrownTier, GrowthTiming},
};
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
};
use serde::{Deserialize, de::DeserializeOwned};
use std::{collections::HashMap, sync::Mutex};
type MaskKey = (u64, Vec<[u64; 2]>);
pub(crate) type Polygon = Vec<[f64; 2]>;

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

fn json<T: DeserializeOwned>(file: &'static str, text: &str) -> Result<T, crate::Error> {
    serde_json::from_str(text).map_err(|source| crate::Error::Data { file, source })
}

pub(crate) fn load_visits() -> Result<Vec<Visit>, crate::Error> {
    let mut visits = EVENTS
        .iter()
        .map(|(file, text)| json::<Visit>(file, text))
        .collect::<Result<Vec<_>, _>>()?;
    visits.sort_by(|a, b| a.event.start.total_cmp(&b.event.start));
    Ok(visits)
}

#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub(crate) struct ImageRef(pub String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Art {
    pub meadow: ImageRef,
    pub plants: ImageRef,
    pub cast: HashMap<String, CharacterArt>,
    pub rigs: HashMap<String, WalkParts>,
    pub clips: HashMap<String, HashMap<String, Clip>>,
    pub trip: Vec<Exposure>,
    pub props: HashMap<String, PropArt>,
    pub noishe: Noishe,
    pub lighting: Sky,
    pub overlays: Overlays,
    pub world: Vec<Vec<crate::scene::Prop>>,
    pub duration: f64,
    pub growth_end: f64,
    pub dusk_duration: f64,
    pub support_x: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CharacterArt {
    pub image: ImageRef,
    pub size: f64,
    pub breathing_phase: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PropArt {
    pub image: ImageRef,
    /// Destination rectangle relative to the prop's attachment point.
    pub rect: [f64; 4],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TreeArt {
    pub origin: [f64; 2],
    pub bark_texture: ImageRef,
    pub timing: GrowthTiming,
    pub branches: Vec<BranchCurve>,
    pub crown: Vec<CrownTier>,
    pub shoots: Vec<LeafArt>,
    pub shoot_sides: [LeafArt; 2],
    pub canopies: Vec<LeafArt>,
    pub palette: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LeafArt {
    pub image: ImageRef,
    pub pivot: [f64; 2],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WalkParts {
    pub body: [ImageRef; 4],
    pub legs: [ImageRef; 2],
    pub pivots: [[f64; 2]; 2],
    pub ankles: [[f64; 2]; 2],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Clip {
    pub pivot: f64,
    pub frames: Vec<Cel>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cel {
    pub image: ImageRef,
    /// Right and left hand anchors, in source-cel pixels.
    pub hands: [[f64; 2]; 2],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Exposure {
    pub cel: usize,
    pub frames: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Noishe {
    pub body: ImageRef,
    pub tail: Part,
    pub legs: [NoisheLeg; 4],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Part {
    pub pivot: [f64; 2],
    pub image: ImageRef,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoisheLeg {
    pub image: ImageRef,
    pub pivot: [f64; 2],
    pub phase: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sky {
    pub sky: ImageRef,
    pub stars: ImageRef,
    pub cloud_veil: ImageRef,
    pub day_air: ImageRef,
    pub backdrop: ImageRef,
    pub moons: [Moon; 2],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Moon {
    pub image: ImageRef,
    pub pad: f64,
    pub orbit: Orbit,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Orbit {
    pub rise: f64,
    pub duration: f64,
    pub cx: f64,
    pub rx: f64,
    pub base: f64,
    pub height: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Overlays {
    pub book: BookOverlays,
    pub log: Polygon,
    pub pet: Polygon,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BookOverlays {
    pub acting: Vec<Polygon>,
    pub profile: Polygon,
}
pub(crate) struct Assets {
    pub data: Art,
    pub tree: TreeArt,
    images: HashMap<String, Image>,
    pub masks: Mutex<HashMap<MaskKey, Image>>,
}
impl Assets {
    pub fn load() -> Result<Self, crate::Error> {
        let data = json("scene.json", include_str!("../assets/scene.json"))?;
        let tree = json(
            "tree/growth.json",
            include_str!("../assets/tree/growth.json"),
        )?;
        let mut images = HashMap::new();
        for &(name, bytes, metadata) in ATLASES {
            let texture = bevy::image::Image::from_buffer(
                bytes,
                ImageType::Extension("ktx2"),
                CompressedImageFormats::NONE,
                false,
                ImageSampler::nearest(),
                RenderAssetUsages::MAIN_WORLD,
            )?;
            let atlas = drawing::image_rgba(
                texture.width() as i32,
                texture.height() as i32,
                texture.data.as_ref().expect("embedded RGBA8 atlas"),
            );
            let regions: HashMap<String, [u32; 4]> = json(name, metadata)?;
            for (sprite, rect @ [x, y, w, h]) in regions {
                if w == 0
                    || h == 0
                    || x as u64 + w as u64 > texture.width() as u64
                    || y as u64 + h as u64 > texture.height() as u64
                {
                    return Err(crate::Error::Atlas(format!(
                        "{name}/{sprite}: region outside texture"
                    )));
                }
                images.insert(
                    format!("{name}/{sprite}"),
                    drawing::image_region(&atlas, rect),
                );
            }
        }
        Ok(Self {
            data,
            tree,
            images,
            masks: Mutex::new(HashMap::new()),
        })
    }
    pub fn image(&self, reference: &ImageRef) -> Image {
        self.images[&reference.0].clone()
    }
}

pub(crate) fn clamp(x: f64) -> f64 {
    if x.is_finite() { x.clamp(0., 1.) } else { 0. }
}
pub(crate) fn smooth(x: f64) -> f64 {
    let x = clamp(x);
    x * x * (3. - 2. * x)
}
/// Snap half-pixels toward the right/bottom edge of the pixel-art grid.
pub(crate) fn round(x: f64) -> f64 {
    (x + 0.5).floor()
}
