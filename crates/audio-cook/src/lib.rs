//! Offline compilation of the original MusyX resources.
//!
//! This crate compiles resources directly in Rust. Its outputs are typed
//! musical data and decoded PCM for the importer. Playback shares
//! resonance-audio's synthesis core and reads only the cooked package.
//! The renderer exposes voice buses and a standard-reverb studio mix for oracle
//! diagnosis; it rejects unsupported macro commands instead of dropping them.
pub mod bank;
pub use resonance_audio::{
    control, dls, envelope, mix, modulation, music_voice, pitch, resample, reverb, sequence, volume,
};
pub mod compile;
pub mod decode;
pub mod dsp;
pub mod instrument;
pub mod parameters;
pub mod pool;
pub mod render;
pub mod song;

mod read {
    use anyhow::{Context, Result};

    pub fn slice(data: &[u8], at: usize, size: usize) -> Result<&[u8]> {
        data.get(at..at.checked_add(size).context("sound range overflow")?)
            .context("sound resource is truncated")
    }
    pub fn u16(data: &[u8], at: usize) -> Result<u16> {
        Ok(u16::from_be_bytes(slice(data, at, 2)?.try_into()?))
    }
    pub fn u32(data: &[u8], at: usize) -> Result<u32> {
        Ok(u32::from_be_bytes(slice(data, at, 4)?.try_into()?))
    }
}
