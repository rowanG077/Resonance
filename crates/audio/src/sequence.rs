//! Score sequencing with millisecond controls, shared voices, and two reverb buses.
use crate::{
    data::{EventKind, Resources, Score},
    music_voice::{Tables, Voice},
    reverb::Studio,
};
use anyhow::{Result, ensure};
use std::collections::VecDeque;
mod kernel;
pub mod stream;

/// Dry, auxiliary A and auxiliary B, each in unclipped stereo PCM units.
pub type BusFrame = [[i32; 2]; 3];

#[derive(Clone, Copy)]
enum ClockStart {
    /// The first score after audio-driver initialization (the title).
    Cold,
    /// Field scores start while the audio driver is already running.
    Running,
}

#[derive(Clone, Copy)]
pub struct LiveControls {
    pub volume: f32,
    pub pan: Option<u8>,
    pub mono: bool,
    pub release: bool,
}
impl Default for LiveControls {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pan: None,
            mono: false,
            release: false,
        }
    }
}

pub struct Preview {
    pub pcm: Vec<i16>,
    pub notes: u32,
    pub maximum_voices: usize,
    pub final_tick: u32,
    pub loop_starts: Vec<u32>,
    pub free_voices: Vec<usize>,
    pub lfo_counters: [u32; 64],
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
    channel: usize,
    end_tick: Option<u32>,
    clock: usize,
    slot: usize,
    retired: bool,
    lifetime: usize,
}

fn tick_delta(bpm_1024: u32) -> u64 {
    let ticks = (bpm_1024 as f32 * 256.0) * (1.0_f32 / 40960000.0);
    (ticks * 65536.0) as u64
}

/// Render a bounded cold-start diagnostic window, preserving voices and effects
/// across loops. Field playback uses `stream::Stream` with running clocks.
/// Voice stealing, sustain pedal and additional controllers fail explicitly.
pub fn render_preview(
    bank: &Resources,
    song: &Score,
    tables: &Tables,
    reverbs: [[f32; 5]; 2],
    frames: u32,
) -> Result<Preview> {
    render_preview_with_volume(bank, song, tables, reverbs, frames, |_| 1.0)
}

/// Group automation is a presentation input, independent of the reusable score.
/// Its callback supplies each millisecond's value before voice controls run.
pub fn render_preview_with_volume(
    bank: &Resources,
    song: &Score,
    tables: &Tables,
    reverbs: [[f32; 5]; 2],
    frames: u32,
    mut group_volume: impl FnMut(u32) -> f32,
) -> Result<Preview> {
    ensure!(
        (1..=32000 * 120).contains(&frames),
        "music preview exceeds 120 seconds"
    );
    let mut pcm = Vec::with_capacity(frames as usize * 2);
    let mut studio = Studio::new(reverbs)?;
    let mut kernel = kernel::Kernel::new(bank, song, tables, Some(frames), true, ClockStart::Cold)?;
    let mut live_controls = |frame| LiveControls {
        volume: group_volume(frame as u32),
        ..Default::default()
    };
    while let Some(block) = kernel.next_block(&mut live_controls)? {
        for buses in block {
            pcm.extend(
                studio
                    .process(*buses)
                    .map(|sample| sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16),
            );
        }
    }
    let mut result = kernel.finish().unwrap();
    result.pcm = pcm;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_retains_per_millisecond_fractional_truncation() {
        assert_eq!(tick_delta(140 * 1024), 58720);
        let mut time = 0;
        let mut note_milliseconds = Vec::new();
        for ms in 0..10 {
            if [7, 8].contains(&(time >> 16)) {
                note_milliseconds.push(ms);
            }
            time += tick_delta(140 * 1024);
        }
        assert_eq!(note_milliseconds, [8, 9]);
        assert_eq!((tick_delta(140 * 1024) * 1000) >> 16, 895);
    }
}
