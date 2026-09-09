//! Decoded instrument PCM; no original codec headers or bank offsets.
#[derive(Clone, Debug)]
pub struct Sample {
    pub key: u8,
    pub rate: u16,
    pub loop_start: u32,
    pub loop_length: u32,
    pub pcm: Vec<i16>,
    /// Each repeat may have a different predictor history from the first pass.
    pub loop_pcm: Vec<i16>,
}
