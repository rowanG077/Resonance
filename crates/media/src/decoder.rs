use crate::{AudioChunk, DecodeResult, VideoFrame, VideoReader, container};
use anyhow::{Context, Result, ensure};
use matroska_demuxer::TrackType;
use resonance_content::MovieAsset;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::Duration,
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_FLAC, CodecParameters, DecoderOptions},
    formats::Packet,
};

pub(super) fn audio(
    path: &Path,
    asset: &MovieAsset,
    cancelled: &AtomicBool,
    sender: &SyncSender<DecodeResult<AudioChunk>>,
) -> Result<()> {
    if cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut input = container::open(path)?;
    let scale = input.info().timestamp_scale().get();
    let audio_track = input
        .tracks()
        .iter()
        .filter(|track| track.track_type() == TrackType::Audio)
        .nth(usize::from(asset.audio_track))
        .with_context(|| format!("movie has no audio track {}", asset.audio_track))?;
    ensure!(
        audio_track.content_encodings().is_none(),
        "encoded container tracks are unsupported"
    );
    let audio_id = audio_track.track_number().get();
    let info = audio_track.audio().context("missing movie audio format")?;
    ensure!(
        audio_track.codec_id() == "A_FLAC"
            && info.sampling_frequency() == f64::from(asset.sample_rate)
            && info.channels().get() == u64::from(asset.channels),
        "movie audio format disagrees with the cooked manifest"
    );
    let private = audio_track
        .codec_private()
        .context("missing FLAC configuration")?;
    ensure!(
        private.len() >= 42
            && &private[..4] == b"fLaC"
            && private[4] & 0x7f == 0
            && private[5..8] == [0, 0, 34],
        "invalid FLAC stream info"
    );
    let mut parameters = CodecParameters::new();
    parameters.codec = CODEC_TYPE_FLAC;
    parameters.with_extra_data(private[8..42].into());
    let mut audio =
        symphonia::default::get_codecs().make(&parameters, &DecoderOptions::default())?;
    ensure!(
        audio.codec_params().sample_rate == Some(asset.sample_rate)
            && audio
                .codec_params()
                .channels
                .is_some_and(|c| c.count() == usize::from(asset.channels)),
        "FLAC format disagrees with manifest"
    );
    let mut audio_frames = 0u64;
    let mut packet = matroska_demuxer::Frame::default();
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        if !input.next_frame(&mut packet)? {
            break;
        }
        if packet.track == audio_id {
            let pts = container::timestamp(packet.timestamp, scale)?;
            let expected =
                Duration::from_secs_f64(audio_frames as f64 / f64::from(asset.sample_rate));
            ensure!(
                pts.abs_diff(expected) < Duration::from_millis(2),
                "discontinuous movie audio timestamps"
            );
            let decoded =
                audio.decode(&Packet::new_from_slice(0, audio_frames, 0, &packet.data))?;
            ensure!(
                decoded.spec().rate == asset.sample_rate
                    && decoded.spec().channels.count() == 2
                    && (1..=65536).contains(&decoded.frames()),
                "unexpected movie audio block format"
            );
            let count = decoded.frames() as u64;
            let mut samples = SampleBuffer::<f32>::new(count, *decoded.spec());
            samples.copy_interleaved_ref(decoded);
            let start_frame = audio_frames;
            audio_frames += count;
            ensure!(
                audio_frames <= asset.audio_frames,
                "movie audio exceeds cooked frame count"
            );
            sender
                .send(Ok(Some(AudioChunk {
                    start_frame,
                    timestamp: pts,
                    samples: samples.samples().to_vec(),
                })))
                .context("movie consumer closed")?;
        }
    }
    ensure!(
        audio_frames == asset.audio_frames,
        "movie audio ended early: {audio_frames}/{} frames",
        asset.audio_frames
    );
    Ok(())
}

/// Video has its own reader and worker. Sharing a demux loop would let a full
/// video packet queue block delivery of the following audio packets again.
pub(super) fn video(
    path: &Path,
    asset: &MovieAsset,
    cancelled: &AtomicBool,
    sender: &SyncSender<DecodeResult<VideoFrame>>,
) -> Result<()> {
    if cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut input = VideoReader::open(path)?;
    ensure!(
        input.dimensions() == (asset.width, asset.height),
        "movie video format disagrees with the cooked manifest"
    );
    let mut frames = 0;
    while !cancelled.load(Ordering::Acquire) {
        let Some(frame) = input.next_frame()? else {
            ensure!(
                frames == asset.frames,
                "movie video ended early: {frames}/{} frames",
                asset.frames
            );
            return Ok(());
        };
        ensure!(frames < asset.frames, "unexpected movie video frame");
        sender
            .send(Ok(Some(frame)))
            .context("movie consumer closed")?;
        frames += 1;
    }
    Ok(())
}
