//! A skit owns its portraits and media clock while the field is suspended.
use anyhow::{Context, Result};
use resonance_content::skit::{
    PortraitAsset, PortraitFrame, PortraitRecipe, PortraitTile, SkitCatalog, TILE_SIZE,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Scene {
    pub portraits: BTreeMap<u8, Portrait>,
    pub subtitle: String,
    pub subtitle_started: u32,
    pub panel_started: Option<u32>,
    pub media: Option<Media>,
}
#[derive(Debug, Clone)]
pub struct Request {
    pub id: u16,
    pub skippable: bool,
    pub preview: bool,
    pub operation: crate::Operation,
}
#[derive(Debug)]
pub struct Media {
    pub id: u32,
    pub started: u32,
    pub frames: u32,
    pub sample_rate: u32,
}
impl Media {
    fn samples(&self, tick: u32) -> u64 {
        // 60 gameplay updates per second; rendering never advances media.
        u64::from(tick.saturating_sub(self.started)) * u64::from(self.sample_rate) / 60
    }
    pub fn finished(&self, tick: u32) -> bool {
        self.samples(tick) >= u64::from(self.frames)
    }
    pub fn position(&self, tick: u32) -> u32 {
        (self.samples(tick) * 100 / u64::from(self.sample_rate)) as u32
    }
}
pub struct Portrait {
    pub id: i32,
    pub resource: u32,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub angle: f32,
    // The shared actor interface retains X/Y angles; flat portraits only use Z.
    pub(crate) tilt: [f32; 2],
    pub color: [f32; 4],
    pub opacity: f32,
    pub scale: f32,
    pub talking: bool,
    pub timeline: Option<usize>,
    pub tiles: Vec<PortraitTile>,
    pub(crate) fade_step: f32,
    pub(crate) scale_target: f32,
    pub(crate) scale_step: f32,
    pub(crate) cursors: [usize; 3],
    pub(crate) forced: [Option<usize>; 2],
    pub(crate) counters: [i16; 3],
}
impl Portrait {
    pub(crate) fn initial_tiles(asset: &PortraitAsset) -> Vec<PortraitTile> {
        let count = asset.size[0].div_ceil(TILE_SIZE) * asset.size[1].div_ceil(TILE_SIZE);
        (0..count)
            .map(|block| PortraitTile { image: 0, block })
            .collect()
    }

    fn step(&mut self, asset: &PortraitAsset, recipe: Option<&PortraitRecipe>) -> Result<()> {
        self.opacity = (self.opacity + self.fade_step).clamp(0., self.color[3]);
        if (self.scale - self.scale_target).abs() <= self.scale_step.abs() {
            self.scale = self.scale_target;
        } else {
            self.scale += self.scale_step;
        }
        let Some(recipe) = recipe else {
            return Ok(());
        };
        let mut frames = [None; 3];
        for (channel, output) in frames.iter_mut().enumerate() {
            let track = &recipe.tracks[channel];
            if track.is_empty() {
                continue;
            }
            if let Some(Some(cursor)) = self.forced.get(channel) {
                self.cursors[channel] = *cursor;
            }
            let frame = track
                .get(self.cursors[channel])
                .context("portrait timeline cursor outside track")?;
            *output = Some(frame);
            if channel == 1 && !self.talking {
                self.cursors[channel] = 0;
                continue;
            }
            self.counters[channel] = self.counters[channel].wrapping_add(1);
            if self.counters[channel] > frame.ticks {
                self.counters[channel] = 0;
                self.cursors[channel] = (self.cursors[channel] as u8).wrapping_add(1) as usize;
                if self.cursors[channel] == track.len() {
                    self.cursors[channel] = if recipe.repeat[channel] {
                        0
                    } else {
                        track.len() - 1
                    };
                }
            }
        }
        for channel in [2, 0, 1] {
            if let Some(frame) = frames[channel] {
                self.patch(asset, frame)?;
            }
        }
        Ok(())
    }

    fn patch(&mut self, asset: &PortraitAsset, frame: &PortraitFrame) -> Result<()> {
        let Some(image) = frame.image else {
            return Ok(());
        };
        let size = asset
            .images
            .get(usize::from(image))
            .context("portrait patch image is absent")?
            .size;
        let [x, y] = frame
            .position
            .map(|value| i32::from(value) / TILE_SIZE as i32);
        let stride = (asset.size[0] / TILE_SIZE) as i32;
        let mut source = 0;
        for row in 0..size[1] / TILE_SIZE {
            for column in 0..size[0] / TILE_SIZE {
                let destination = x + stride * (y + row as i32) + column as i32;
                let destination =
                    usize::try_from(destination).context("portrait patch precedes canvas")?;
                let tile = if image == 0 {
                    *self
                        .tiles
                        .get(source as usize)
                        .context("portrait self-copy outside canvas")?
                } else {
                    PortraitTile {
                        image,
                        block: source,
                    }
                };
                *self
                    .tiles
                    .get_mut(destination)
                    .context("portrait patch exceeds canvas")? = tile;
                source += 1;
            }
        }
        Ok(())
    }
}
impl Scene {
    pub(crate) fn step(&mut self, catalog: &SkitCatalog) -> anyhow::Result<()> {
        for portrait in self.portraits.values_mut() {
            let asset = catalog
                .portraits
                .get(&portrait.resource)
                .ok_or_else(|| anyhow::anyhow!("uncooked skit portrait {}", portrait.resource))?;
            let recipe = portrait
                .timeline
                .map(|index| {
                    catalog
                        .portrait_recipes
                        .get(index)
                        .context("uncooked portrait timeline")
                })
                .transpose()?;
            portrait.step(asset, recipe)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::skit::PortraitImage;

    fn fixture() -> (PortraitAsset, Portrait) {
        let asset = PortraitAsset {
            size: [16; 2],
            images: [[16; 2], [8; 2], [8; 2], [8; 2]]
                .into_iter()
                .map(|size| PortraitImage {
                    texture: String::new(),
                    size,
                })
                .collect(),
        };
        let portrait = Portrait {
            id: 0,
            resource: 0,
            position: [0.; 2],
            size: [16.; 2],
            angle: 0.,
            tilt: [0.; 2],
            color: [1.; 4],
            opacity: 1.,
            scale: 1.,
            talking: true,
            timeline: Some(0),
            tiles: Portrait::initial_tiles(&asset),
            fade_step: 0.,
            scale_target: 1.,
            scale_step: 0.,
            cursors: [0; 3],
            forced: [None; 2],
            counters: [0; 3],
        };
        (asset, portrait)
    }

    fn frame(image: Option<u16>, ticks: i16, position: [i16; 2]) -> PortraitFrame {
        PortraitFrame {
            ticks,
            image,
            position,
        }
    }

    #[test]
    fn portrait_tiles_persist_and_copy_in_native_order_with_signed_linear_positions() -> Result<()>
    {
        let (asset, mut portrait) = fixture();
        let mut recipe = PortraitRecipe {
            tracks: std::array::from_fn(|channel| {
                vec![
                    frame(Some(channel as u16 + 1), 0, [0; 2]),
                    frame(None, 0, [0; 2]),
                ]
            }),
            repeat: [false; 3],
        };
        portrait.step(&asset, Some(&recipe))?;
        assert_eq!(portrait.tiles[0].image, 2); // Mouth overwrites eyes and extra.
        recipe.tracks[2][1].image = Some(3);
        portrait.step(&asset, Some(&recipe))?;
        assert_eq!(portrait.tiles[0].image, 3); // Skipped writes do not restore old expressions.
        recipe.tracks[2][1].image = None;
        portrait.step(&asset, Some(&recipe))?;
        assert_eq!(portrait.tiles[0].image, 3);
        portrait.patch(&asset, &frame(Some(1), 0, [-8, 8]))?;
        assert_eq!(portrait.tiles[1], PortraitTile { image: 1, block: 0 });
        portrait.patch(&asset, &frame(Some(1), 0, [-7, -7]))?;
        assert_eq!(portrait.tiles[0].image, 1);
        assert!(portrait.patch(&asset, &frame(Some(1), 0, [-8, 0])).is_err());
        assert!(portrait.patch(&asset, &frame(Some(1), 0, [0, 16])).is_err());
        let mut narrow = asset;
        narrow.size = [10, 8];
        narrow.images[0].size = narrow.size;
        portrait.tiles = vec![
            PortraitTile { image: 2, block: 0 },
            PortraitTile { image: 3, block: 0 },
        ];
        portrait.patch(&narrow, &frame(Some(0), 0, [8, 0]))?;
        assert_eq!(portrait.tiles[1], portrait.tiles[0]);
        Ok(())
    }

    #[test]
    fn portrait_timing_uses_signed_counters_byte_cursors_and_control_records() -> Result<()> {
        let (asset, mut portrait) = fixture();
        let mut recipe = PortraitRecipe {
            tracks: [
                vec![frame(Some(1), -1, [0; 2]), frame(Some(2), 1, [0; 2])],
                Vec::new(),
                Vec::new(),
            ],
            repeat: [false; 3],
        };
        portrait.step(&asset, Some(&recipe))?;
        assert_eq!((portrait.cursors[0], portrait.tiles[0].image), (1, 1));
        portrait.step(&asset, Some(&recipe))?;
        assert_eq!(portrait.counters[0], 1);
        portrait.step(&asset, Some(&recipe))?;
        assert_eq!((portrait.cursors[0], portrait.counters[0]), (1, 0));
        recipe.repeat[0] = true;
        portrait.counters[0] = 1;
        portrait.step(&asset, Some(&recipe))?;
        assert_eq!(portrait.cursors[0], 0);
        recipe.tracks[0][0].ticks = i16::MAX;
        portrait.counters[0] = i16::MAX;
        portrait.step(&asset, Some(&recipe))?;
        assert_eq!((portrait.cursors[0], portrait.counters[0]), (0, i16::MIN));
        recipe.tracks[0] = vec![frame(None, 0, [0; 2]); 257];
        portrait.cursors[0] = 255;
        portrait.counters[0] = 0;
        portrait.step(&asset, Some(&recipe))?;
        assert_eq!(portrait.cursors[0], 0);
        recipe.tracks[1] = vec![frame(Some(1), 0, [0; 2]), frame(Some(2), 0, [0; 2])];
        portrait.talking = false;
        portrait.forced[1] = Some(1);
        portrait.counters[1] = 7;
        portrait.step(&asset, Some(&recipe))?;
        assert_eq!(
            (
                portrait.tiles[0].image,
                portrait.cursors[1],
                portrait.counters[1]
            ),
            (2, 0, 7)
        );
        Ok(())
    }
}
