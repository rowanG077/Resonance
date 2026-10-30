//! The cooked Matroska profile and the RGB FFV1 reader shared with the oracle.
use crate::VideoFrame;
use anyhow::{Context, Result, ensure};
use ffv1::decoder::Decoder;
use matroska_demuxer::{MatroskaFile, TrackEntry, TrackType};
use std::{fs::File, io::BufReader, path::Path, time::Duration};

pub(crate) type Input = MatroskaFile<BufReader<crate::StreamReader>>;
pub(crate) fn open(path: &Path) -> Result<Input> {
    from_source(crate::StreamReader::new(File::open(path)?)?)
}

pub(crate) fn from_source(source: crate::StreamReader) -> Result<Input> {
    Ok(MatroskaFile::open(BufReader::new(source))?)
}

pub(crate) fn track(input: &Input, kind: TrackType) -> Result<&TrackEntry> {
    let mut tracks = input.tracks().iter().filter(|t| t.track_type() == kind);
    let track = tracks.next().context("movie is missing a required track")?;
    ensure!(tracks.next().is_none(), "ambiguous movie tracks");
    ensure!(
        track.content_encodings().is_none(),
        "encoded container tracks are unsupported"
    );
    Ok(track)
}

pub(crate) fn timestamp(ticks: u64, scale: u64) -> Result<Duration> {
    ensure!(scale > 0, "invalid container timestamp scale");
    Ok(Duration::from_nanos(
        ticks
            .checked_mul(scale)
            .context("movie timestamp overflow")?,
    ))
}

pub(crate) struct RgbDecoder {
    decoder: Decoder,
    pub width: u32,
    pub height: u32,
}
impl RgbDecoder {
    pub fn new(track: &TrackEntry) -> Result<Self> {
        let video = track.video().context("missing movie video dimensions")?;
        let width = u32::try_from(video.pixel_width().get())?;
        let height = u32::try_from(video.pixel_height().get())?;
        ensure!(
            (1..=1920).contains(&width) && (1..=1080).contains(&height),
            "invalid movie dimensions"
        );
        ensure!(track.codec_id() == "V_FFV1", "expected an FFV1 movie track");
        let config = track
            .codec_private()
            .context("expected FFV1 version 3 with a configuration record")?;
        let decoder = Decoder::new(config, width, height)?;
        let record = decoder.config_record();
        ensure!(
            record.colorspace_type == 1
                && record.bits_per_raw_sample == 8
                && record.chroma_planes
                && !record.extra_plane
                && record.log2_h_chroma_subsample == 0
                && record.log2_v_chroma_subsample == 0,
            "expected lossless 8-bit RGB FFV1"
        );
        // rust-av allocates entropy contexts by table count but RGB uses
        // two plane groups. Reject this unsupported profile before decoding.
        ensure!(
            record.quant_table_set_count >= 2,
            "RGB FFV1 requires two quantization tables with this decoder"
        );
        Ok(Self {
            decoder,
            width,
            height,
        })
    }
    pub fn decode(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        let frame = self.decoder.decode_frame(bytes)?;
        let pixels = self.width as usize * self.height as usize;
        ensure!(
            frame.buf.len() == 3 && frame.buf.iter().all(|p| p.len() == pixels),
            "invalid decoded RGB planes"
        );
        let mut rgba = Vec::with_capacity(pixels * 4);
        for i in 0..pixels {
            for plane in [2, 0, 1] {
                rgba.push(frame.buf[plane][i]);
            }
            rgba.push(255);
        }
        Ok(rgba)
    }
}

/// Streaming RGB reader. Every coded frame is decoded, including unselected
/// frames, to retain FFV1 entropy state between keyframes.
pub struct VideoReader {
    input: Input,
    decoder: RgbDecoder,
    track: u64,
    scale: u64,
    packet: matroska_demuxer::Frame,
    index: u32,
    previous: Option<Duration>,
    frame_duration: Option<Duration>,
}
impl VideoReader {
    pub fn open(path: &Path) -> Result<Self> {
        Self::from_input(open(path)?)
    }
    pub(crate) fn from_input(input: Input) -> Result<Self> {
        let video = track(&input, TrackType::Video)?;
        let decoder = RgbDecoder::new(video)?;
        let track = video.track_number().get();
        let frame_duration = video
            .default_duration()
            .map(|value| Duration::from_nanos(value.get()));
        let scale = input.info().timestamp_scale().get();
        Ok(Self {
            input,
            decoder,
            track,
            scale,
            packet: Default::default(),
            index: 0,
            previous: None,
            frame_duration,
        })
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.decoder.width, self.decoder.height)
    }
    pub fn frame_duration(&self) -> Option<Duration> {
        self.frame_duration
    }
    pub fn next_frame(&mut self) -> Result<Option<VideoFrame>> {
        while self.input.next_frame(&mut self.packet)? {
            if self.packet.track != self.track {
                continue;
            }
            ensure!(
                !self.packet.is_invisible,
                "invisible video frame is unsupported"
            );
            let timestamp = timestamp(self.packet.timestamp, self.scale)?;
            ensure!(
                self.previous.is_none_or(|p| timestamp > p),
                "nonmonotonic movie video timestamps"
            );
            let rgba = self.decoder.decode(&self.packet.data)?;
            let frame = VideoFrame {
                index: self.index,
                timestamp,
                width: self.decoder.width,
                height: self.decoder.height,
                rgba,
            };
            self.index = self.index.checked_add(1).context("too many video frames")?;
            self.previous = Some(timestamp);
            return Ok(Some(frame));
        }
        Ok(None)
    }
}
