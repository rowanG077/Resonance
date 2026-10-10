//! Device-independent musical synthesis shared by cooking and playback.
/// Rate of synthesized PCM presented to the playback transport.
pub const SOURCE_RATE: u32 = 32028;
/// Synthesizer transport block and its control update interval, in PCM frames.
pub const BLOCK_FRAMES: usize = 160;
pub const CONTROL_FRAMES: usize = 32;
pub const CONTROLS_PER_BLOCK: usize = BLOCK_FRAMES / CONTROL_FRAMES;

pub mod control;
pub mod cue;
pub mod data;
pub mod dls;
pub mod envelope;
pub mod mix;
pub mod modulation;
pub mod music_voice;
pub mod package;
pub mod pitch;
pub mod resample;
pub mod reverb;
pub mod sample;
pub mod sequence;
pub mod volume;

mod release;
pub use release::RELEASE_FRAMES;
