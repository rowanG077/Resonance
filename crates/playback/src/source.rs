use std::{
    num::{NonZeroU16, NonZeroU32},
    time::Duration,
};
pub type ChannelCount = NonZeroU16;
pub type SampleRate = NonZeroU32;

/// Native PCM providers run only on the mixer worker or the offline driver.
pub trait Source: Iterator<Item = f32> + Send + 'static {
    fn channels(&self) -> ChannelCount;
    fn sample_rate(&self) -> SampleRate;
    /// True when the last `next()` returned `None` awaiting more PCM rather
    /// than EOF. The mixer retries next block without advancing this source's
    /// timeline. Streaming sources must yield complete stereo frames.
    fn is_pending(&self) -> bool {
        false
    }
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
pub trait Decodable {
    type Decoder: Source;
    fn decoder(&self) -> Self::Decoder;
}
