use crate::{AudioChunk, DecodeResult, Format, VideoFrame, VideoReader, container};
use anyhow::{Context, Result, ensure};
use matroska_demuxer::TrackType;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::Duration,
};
use symphonia::core::{
    codecs::audio::{
        AudioCodecParameters, AudioDecoder, AudioDecoderOptions, well_known::CODEC_ID_FLAC,
    },
    packet::PacketRef,
    units::{Duration as MediaDuration, Timestamp},
};

/// Validate durations from actual decoded streams, including containers whose
/// headers omit lengths. Neither worker waits for the other or buffers the movie.
#[derive(Clone, Default)]
pub(crate) struct Completion(Arc<Mutex<[Option<Duration>; 2]>>);
impl Completion {
    fn finish(&self, track: usize, duration: Duration) -> Result<()> {
        let mut values = self.0.lock().expect("movie completion poisoned");
        values[track] = Some(duration);
        if let [Some(video), Some(audio)] = *values {
            ensure!(
                video.abs_diff(audio) < Duration::from_millis(100),
                "movie audio and video durations disagree: {audio:?} audio, {video:?} video"
            );
        }
        Ok(())
    }
}

pub(crate) struct AudioReader {
    input: container::Input,
    decoder: Box<dyn AudioDecoder>,
    id: u64,
    pub sample_rate: u32,
    pub frames: Option<u64>,
}

impl AudioReader {
    pub fn new(input: container::Input, selected: u16) -> Result<Self> {
        let track = input
            .tracks()
            .iter()
            .filter(|track| track.track_type() == TrackType::Audio)
            .nth(usize::from(selected))
            .with_context(|| format!("movie has no audio track {selected}"))?;
        ensure!(
            track.content_encodings().is_none(),
            "encoded container tracks are unsupported"
        );
        ensure!(
            track.codec_id() == "A_FLAC",
            "movie playback requires FLAC audio"
        );
        let id = track.track_number().get();
        let info = track.audio().context("missing movie audio format")?;
        let rate = info.sampling_frequency();
        ensure!(
            rate.is_finite()
                && rate.fract() == 0.
                && (8000. ..=96000.).contains(&rate)
                && info.channels().get() == 2,
            "movie playback requires stereo audio at 8000–96000 Hz"
        );
        let sample_rate = rate as u32;
        let private = track
            .codec_private()
            .context("missing FLAC configuration")?;
        ensure!(
            private.len() >= 42
                && &private[..4] == b"fLaC"
                && private[4] & 0x7f == 0
                && private[5..8] == [0, 0, 34],
            "invalid FLAC stream info"
        );
        let mut parameters = AudioCodecParameters::new();
        parameters.codec = CODEC_ID_FLAC;
        parameters.with_extra_data(private[8..42].into());
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&parameters, &AudioDecoderOptions::default())?;
        ensure!(
            decoder.codec_params().sample_rate == Some(sample_rate)
                && decoder
                    .codec_params()
                    .channels
                    .as_ref()
                    .is_some_and(|c| c.count() == 2),
            "FLAC format disagrees with container track"
        );
        // STREAMINFO's zero count means unspecified, not an empty soundtrack.
        // FLAC STREAMINFO stores the total sample count in the low 36 bits.
        let total = u64::from_be_bytes(private[18..26].try_into().unwrap()) & ((1u64 << 36) - 1);
        let frames = (total != 0).then_some(total);
        ensure!(
            frames.is_none_or(|frames| frames <= u64::from(sample_rate) * 3600),
            "movie audio exceeds one hour"
        );
        Ok(Self {
            input,
            decoder,
            id,
            sample_rate,
            frames,
        })
    }

    pub fn run(
        mut self,
        cancelled: &AtomicBool,
        sender: &SyncSender<DecodeResult<AudioChunk>>,
        completion: Completion,
    ) -> Result<()> {
        let scale = self.input.info().timestamp_scale().get();
        let mut audio_frames = 0u64;
        let mut packet = matroska_demuxer::Frame::default();
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            if !self.input.next_frame(&mut packet)? {
                break;
            }
            if packet.track != self.id {
                continue;
            }
            let pts = container::timestamp(packet.timestamp, scale)?;
            let expected =
                Duration::from_secs_f64(audio_frames as f64 / f64::from(self.sample_rate));
            ensure!(
                pts.abs_diff(expected) < Duration::from_millis(2),
                "discontinuous movie audio timestamps"
            );
            let decoded = self.decoder.decode_ref(&PacketRef::new(
                0,
                Timestamp::new(audio_frames as i64),
                MediaDuration::ZERO,
                &packet.data,
            ))?;
            ensure!(
                decoded.spec().rate() == self.sample_rate
                    && decoded.spec().channels().count() == 2
                    && (1..=65536).contains(&decoded.frames()),
                "unexpected movie audio block format"
            );
            let count = decoded.frames() as u64;
            let mut samples = vec![0.0f32; decoded.samples_interleaved()];
            decoded.copy_to_slice_interleaved(&mut samples);
            let start_frame = audio_frames;
            audio_frames += count;
            ensure!(
                audio_frames <= u64::from(self.sample_rate) * 3600
                    && self.frames.is_none_or(|frames| audio_frames <= frames),
                "movie audio exceeds stream length or one-hour limit"
            );
            sender
                .send(Ok(Some(AudioChunk {
                    start_frame,
                    timestamp: pts,
                    samples,
                })))
                .context("movie consumer closed")?;
        }
        ensure!(
            audio_frames > 0 && self.frames.is_none_or(|frames| frames == audio_frames),
            "movie audio ended early: {audio_frames} frames, stream declares {:?}",
            self.frames
        );
        completion.finish(
            1,
            Duration::from_secs_f64(audio_frames as f64 / f64::from(self.sample_rate)),
        )
    }
}

/// Independent demuxing prevents video backpressure from blocking audio.
pub(super) fn video(
    mut input: VideoReader,
    format: Format,
    cancelled: &AtomicBool,
    sender: &SyncSender<DecodeResult<VideoFrame>>,
    completion: Completion,
) -> Result<()> {
    let mut frames = 0;
    let mut end = Duration::ZERO;
    while !cancelled.load(Ordering::Acquire) {
        let Some(frame) = input.next_frame()? else {
            ensure!(frames > 0, "movie contains no video frames");
            return completion.finish(0, end);
        };
        ensure!(
            frames < 100_000 && frame.timestamp < Duration::from_secs(3600),
            "movie video exceeds playback limits"
        );
        ensure!(
            frames != 0 || frame.timestamp == Duration::ZERO,
            "movie video must start at time zero"
        );
        end = frame.timestamp + format.frame_duration;
        sender
            .send(Ok(Some(frame)))
            .context("movie consumer closed")?;
        frames += 1;
    }
    Ok(())
}
