//! Device-independent musical synthesis shared by cooking and playback.
//! Original resource readers and executable lookup extraction live in audio-cook.
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
mod voice_order;
pub mod volume;
