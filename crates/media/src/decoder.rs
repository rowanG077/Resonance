use crate::{AudioChunk, DecodeResult, MovieEvent, VideoFrame};
use anyhow::{Context, Result, ensure};
use ffmpeg_next as ffmpeg;
use resonance_content::MovieAsset;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::Duration,
};

fn timestamp(value: Option<i64>, time_base: ffmpeg::Rational) -> Result<Duration> {
    let value = value.context("decoded movie frame lacks a timestamp")?;
    ensure!(
        value >= 0 && time_base.numerator() > 0 && time_base.denominator() > 0,
        "invalid movie timestamp or clock"
    );
    let nanos = value as u128 * time_base.numerator() as u128 * 1_000_000_000
        / time_base.denominator() as u128;
    Ok(Duration::from_nanos(u64::try_from(nanos)?))
}

fn needs_packet(error: ffmpeg::Error) -> bool {
    error == ffmpeg::Error::Eof
        || error
            == (ffmpeg::Error::Other {
                errno: ffmpeg::error::EAGAIN,
            })
}

pub(super) fn decode(
    path: &Path,
    asset: &MovieAsset,
    cancelled: &Arc<AtomicBool>,
    sender: &SyncSender<DecodeResult>,
) -> Result<()> {
    ffmpeg::init()?;
    let interrupt = cancelled.clone();
    let mut options = ffmpeg::Dictionary::new();
    options.set("protocol_whitelist", "file");
    options.set("format_whitelist", "matroska,webm");
    let mut input = ffmpeg::format::input_with_interrupt_and_dictionary(
        path,
        move || interrupt.load(Ordering::Acquire),
        options,
    )?;
    // FFmpeg can return a partially probed input after the interrupt callback
    // fires. Do not construct conversion contexts from incomplete parameters.
    if cancelled.load(Ordering::Acquire) {
        return Ok(());
    }
    let video = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .context("movie has no video stream")?;
    let video_index = video.index();
    let video_clock = video.time_base();
    let mut video = ffmpeg::codec::context::Context::from_parameters(video.parameters())?
        .decoder()
        .video()?;
    ensure!(
        video.id() == ffmpeg::codec::Id::FFV1
            && video.format() != ffmpeg::format::Pixel::None
            && video.width() == asset.width
            && video.height() == asset.height,
        "movie video format disagrees with the cooked manifest"
    );
    let mut scale = ffmpeg::software::scaling::Context::get(
        video.format(),
        video.width(),
        video.height(),
        ffmpeg::format::Pixel::RGBA,
        asset.width,
        asset.height,
        ffmpeg::software::scaling::Flags::POINT,
    )?;
    let audio = input
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .context("movie has no audio stream")?;
    let audio_index = audio.index();
    let audio_clock = audio.time_base();
    let mut audio = ffmpeg::codec::context::Context::from_parameters(audio.parameters())?
        .decoder()
        .audio()?;
    ensure!(
        audio.id() == ffmpeg::codec::Id::FLAC
            && audio.format() != ffmpeg::format::Sample::None
            && audio.rate() == asset.sample_rate
            && audio.channels() == asset.channels,
        "movie audio format disagrees with the cooked manifest"
    );
    let mut resample = ffmpeg::software::resampling::Context::get(
        audio.format(),
        audio.channel_layout(),
        audio.rate(),
        ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
        ffmpeg::ChannelLayout::STEREO,
        asset.sample_rate,
    )?;
    let mut video_frames = 0;
    let mut audio_frames = 0;
    let mut previous_video_pts = None;
    let mut receive_video = |video: &mut ffmpeg::decoder::Video| -> Result<()> {
        let mut decoded = ffmpeg::frame::Video::empty();
        loop {
            match video.receive_frame(&mut decoded) {
                Ok(()) => {}
                Err(error) if needs_packet(error) => break,
                Err(error) => return Err(error.into()),
            }
            ensure!(
                decoded.width() == asset.width
                    && decoded.height() == asset.height
                    && video_frames < asset.frames,
                "unexpected movie video dimensions or frame count"
            );
            let pts = timestamp(decoded.timestamp(), video_clock)?;
            ensure!(
                previous_video_pts.is_none_or(|previous| pts > previous),
                "nonmonotonic movie video timestamps"
            );
            previous_video_pts = Some(pts);
            let mut rgba = ffmpeg::frame::Video::empty();
            scale.run(&decoded, &mut rgba)?;
            let row_bytes = asset.width as usize * 4;
            let mut pixels = Vec::with_capacity(row_bytes * asset.height as usize);
            for row in 0..asset.height as usize {
                let start = row * rgba.stride(0);
                pixels.extend_from_slice(
                    rgba.data(0)
                        .get(start..start + row_bytes)
                        .context("truncated decoded image plane")?,
                );
            }
            sender
                .send(Ok(MovieEvent::Video(VideoFrame {
                    index: video_frames,
                    timestamp: pts,
                    width: asset.width,
                    height: asset.height,
                    rgba: pixels,
                })))
                .context("movie consumer closed")?;
            video_frames += 1;
        }
        Ok(())
    };
    let mut receive_audio = |audio: &mut ffmpeg::decoder::Audio| -> Result<()> {
        let mut decoded = ffmpeg::frame::Audio::empty();
        loop {
            match audio.receive_frame(&mut decoded) {
                Ok(()) => {}
                Err(error) if needs_packet(error) => break,
                Err(error) => return Err(error.into()),
            }
            ensure!(
                decoded.rate() == asset.sample_rate
                    && decoded.channels() == asset.channels
                    && (1..=65_536).contains(&decoded.samples()),
                "unexpected movie audio block format"
            );
            let pts = timestamp(decoded.timestamp(), audio_clock)?;
            let expected =
                Duration::from_secs_f64(audio_frames as f64 / f64::from(asset.sample_rate));
            ensure!(
                pts.abs_diff(expected) < Duration::from_millis(2),
                "discontinuous movie audio timestamps"
            );
            let mut output = ffmpeg::frame::Audio::empty();
            ensure!(
                resample.run(&decoded, &mut output)?.is_none(),
                "unexpected delay while converting movie sample format"
            );
            ensure!(
                output.samples() == decoded.samples(),
                "movie sample format conversion changed frame count"
            );
            let bytes = output
                .data(0)
                .get(..output.samples() * usize::from(asset.channels) * 4)
                .context("truncated decoded audio plane")?;
            let samples: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|sample| f32::from_ne_bytes(sample.try_into().expect("four bytes")))
                .collect();
            ensure!(
                samples.iter().all(|sample| sample.is_finite()),
                "nonfinite movie audio samples"
            );
            let start_frame = audio_frames;
            audio_frames += output.samples() as u64;
            ensure!(
                audio_frames <= asset.audio_frames,
                "movie audio exceeds cooked frame count"
            );
            sender
                .send(Ok(MovieEvent::Audio(AudioChunk {
                    start_frame,
                    timestamp: pts,
                    samples,
                })))
                .context("movie consumer closed")?;
        }
        Ok(())
    };
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut input) {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => break,
            Err(error) => return Err(error.into()),
        }
        if packet.stream() == video_index {
            video.send_packet(&packet)?;
            receive_video(&mut video)?;
        } else if packet.stream() == audio_index {
            audio.send_packet(&packet)?;
            receive_audio(&mut audio)?;
        }
    }
    video.send_eof()?;
    receive_video(&mut video)?;
    audio.send_eof()?;
    receive_audio(&mut audio)?;
    ensure!(
        video_frames == asset.frames && audio_frames == asset.audio_frames,
        "movie ended early: {video_frames}/{} video frames, {audio_frames}/{} audio frames",
        asset.frames,
        asset.audio_frames
    );
    sender
        .send(Ok(MovieEvent::End))
        .context("movie consumer closed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_container_timing_and_rejects_invalid_clocks() {
        assert_eq!(
            timestamp(Some(1001), (1, 1000).into()).unwrap(),
            Duration::from_millis(1001)
        );
        assert!(timestamp(None, (1, 1000).into()).is_err());
        assert!(timestamp(Some(-1), (1, 1000).into()).is_err());
        assert!(timestamp(Some(10), (1, 0).into()).is_err());
    }
}
