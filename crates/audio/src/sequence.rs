//! Score sequencing with shared voices, a precise musical clock and two reverb buses.
use crate::{
    BLOCK_FRAMES, CONTROL_FRAMES,
    data::{EventKind, Resources, Score},
    music_voice::{Tables, Voice},
    reverb::Studio,
};
use anyhow::{Result, ensure};
use std::sync::Arc;
mod kernel;
pub mod shared;
pub mod stream;

/// Dry, auxiliary A and auxiliary B, each in unclipped stereo PCM units.
pub type BusFrame = [[i32; 2]; 3];

/// Native polyphony limit, shared by music and sound effects.
pub const VOICE_BUDGET: usize = 64;

/// Maximum retained values per voice and requests per entry per quantum. Each
/// voice retains at most 4 KiB; the voice limit also bounds the delivery queue.
pub(crate) const MESSAGE_BUDGET: usize = 1024;

#[derive(Clone, Copy)]
pub struct LiveControls {
    pub volume: f32,
    pub pan: Option<u8>,
    /// Higher values take precedence; None preserves authored note/macro priority.
    pub priority: Option<u16>,
    pub mono: bool,
    pub release: bool,
}
impl Default for LiveControls {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pan: None,
            priority: None,
            mono: false,
            release: false,
        }
    }
}

#[derive(Default)]
pub struct Preview {
    pub pcm: Vec<i16>,
    pub notes: u32,
    pub maximum_voices: usize,
    pub final_tick: u32,
    pub loop_starts: Vec<u32>,
    pub voice_lifetimes: Vec<VoiceLifetime>,
}

pub struct VoiceLifetime {
    pub slot: usize,
    pub start_frame: u32,
    pub end_frame: Option<u32>,
    pub macro_id: u16,
    pub key: u8,
}

struct Active<'a> {
    voice: Voice<'a>,
    /// Sequence notes use the channel bank; each SFX allocation owns a bank.
    sound_controls: Option<crate::music_voice::Controls>,
    channel: usize,
    end_tick: Option<u128>,
    lease: shared::Lease,
    retired: bool,
    lifetime: usize,
}

const TICKS_PER_BEAT: u128 = 384;
const TICK_DENOMINATOR: u128 = 60 * 1024 * crate::SOURCE_RATE as u128;

/// Exact fractional musical time, independent of loops and control block sizes.
#[derive(Default)]
struct MusicalClock(u128);
impl MusicalClock {
    fn advance(&mut self, frames: u64, bpm_1024: u32) {
        self.0 += u128::from(frames) * u128::from(bpm_1024) * TICKS_PER_BEAT;
    }
    fn tick(&self) -> u128 {
        self.0 / TICK_DENOMINATOR
    }
}

/// Render a bounded diagnostic window through the live synthesizer scheduler.
/// Statistics describe the prepared blocks, including the final queued block.
pub fn render_preview(
    loaded: Arc<crate::package::Loaded>,
    reverbs: [[f32; 5]; 2],
    frames: u32,
) -> Result<Preview> {
    render_preview_with_volume(loaded, reverbs, frames, |_| 1.0)
}

/// Group automation is a presentation input, independent of the reusable score.
/// Its callback supplies each control quantum's value before voice controls run.
pub fn render_preview_with_volume(
    loaded: Arc<crate::package::Loaded>,
    reverbs: [[f32; 5]; 2],
    frames: u32,
    mut group_volume: impl FnMut(u32) -> f32,
) -> Result<Preview> {
    ensure!(frames > 0, "music preview must contain at least one frame");
    let looping = loaded.score().origin == crate::data::ScoreOrigin::Sequence;
    let mut stream = stream::Stream::recorded(loaded, looping)?;
    let mut pcm = Vec::with_capacity(frames as usize * 2);
    let mut studio = Studio::new(reverbs)?;
    let mut block = [[[0; 2]; 3]; BLOCK_FRAMES];
    for start in (0..frames).step_by(BLOCK_FRAMES) {
        let controls = std::array::from_fn(|quantum| LiveControls {
            volume: group_volume(start + quantum as u32 * CONTROL_FRAMES as u32),
            ..Default::default()
        });
        let length = stream.render_block(controls, &mut block)?;
        block[length..].fill([[0; 2]; 3]);
        for buses in &block[..((frames - start) as usize).min(block.len())] {
            pcm.extend(
                studio
                    .process(*buses)
                    .map(|sample| sample.clamp(i16::MIN as i32, i16::MAX as i32) as i16),
            );
        }
    }
    let mut result = stream.take_preview();
    result.pcm = pcm;
    Ok(result)
}
