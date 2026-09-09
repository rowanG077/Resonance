//! Playback transport and mixing. No device, engine, codec, or filesystem APIs.
mod clock;
mod mixer;
mod output;
mod pcm;
mod source;
pub use clock::{Clock, NativeFrame, OutputFrame};
pub use mixer::{Control, Handle, Mixer, Offline, State};
pub use output::{Callback, Converter, Diagnostics, OUTPUT_BLOCK, Output, Suspended, Worker};
pub use pcm::{Pcm, PcmSource};
pub use source::{ChannelCount, Decodable, SampleRate, Source};
pub const SOURCE_RATE: u32 = 32028;
pub const SOURCE_BLOCK: usize = 160;
#[cfg(test)]
mod allocation_tests;
#[cfg(test)]
mod tests;
