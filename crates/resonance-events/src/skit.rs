//! A skit owns its portraits and media clock while the field is suspended.
use resonance_content::skit::{PortraitAsset, SkitCatalog};
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
    pub images: [u16; 3],
    pub(crate) fade_step: f32,
    pub(crate) scale_target: f32,
    pub(crate) scale_step: f32,
    pub(crate) cursors: [usize; 3],
    pub(crate) forced: [Option<usize>; 2],
    pub(crate) counters: [u16; 3],
}
impl Portrait {
    fn step(&mut self, asset: &PortraitAsset) {
        self.opacity = (self.opacity + self.fade_step).clamp(0., self.color[3]);
        if (self.scale - self.scale_target).abs() <= self.scale_step.abs() {
            self.scale = self.scale_target;
        } else {
            self.scale += self.scale_step;
        }
        for channel in 0..3 {
            let track = &asset.tracks[channel];
            if track.is_empty() {
                continue;
            }
            if let Some(Some(cursor)) = self.forced.get(channel) {
                self.cursors[channel] = *cursor;
            }
            self.images[channel] = track[self.cursors[channel]].image;
            if channel == 1 && !self.talking {
                self.cursors[channel] = 0;
                continue;
            }
            self.counters[channel] += 1;
            if self.counters[channel] > track[self.cursors[channel]].ticks {
                self.counters[channel] = 0;
                self.cursors[channel] += 1;
                if self.cursors[channel] == track.len() {
                    self.cursors[channel] = if asset.repeat[channel] {
                        0
                    } else {
                        track.len() - 1
                    };
                }
            }
        }
    }
}
impl Scene {
    pub(crate) fn step(&mut self, catalog: &SkitCatalog) -> anyhow::Result<()> {
        for portrait in self.portraits.values_mut() {
            let asset = catalog
                .portraits
                .get(&portrait.resource)
                .ok_or_else(|| anyhow::anyhow!("uncooked skit portrait {}", portrait.resource))?;
            portrait.step(asset);
        }
        Ok(())
    }
}
