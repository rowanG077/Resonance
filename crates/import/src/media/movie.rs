mod binding;
pub(crate) use binding::{bind_all_movies, cook_directory};

use super::{Workspace, be_u32, hash_file, write_json};
use crate::dol::slice as dol_slice;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const SIGNATURE: &[u8; 16] = b"HVQM4 1.5\0\0\0\0\0\0\0";

pub(crate) fn is_movie(bytes: &[u8]) -> bool {
    bytes.starts_with(SIGNATURE)
}

struct Header {
    width: usize,
    height: usize,
    frames: u32,
    frame_micros: u32,
    sample_rate: u32,
    channels: u16,
    audio_streams: u16,
}

impl Header {
    fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() >= 68 && is_movie(bytes),
            "expected HVQM4 1.5 movie"
        );
        let header = Self {
            width: usize::from(u16::from_be_bytes(bytes[0x34..0x36].try_into()?)),
            height: usize::from(u16::from_be_bytes(bytes[0x36..0x38].try_into()?)),
            frames: be_u32(bytes, 0x1c)?,
            frame_micros: be_u32(bytes, 0x24)?,
            sample_rate: be_u32(bytes, 0x40)?,
            channels: u16::from(bytes[0x3c]),
            audio_streams: if bytes[0x3c] == 0 && be_u32(bytes, 0x40)? == 0 {
                0
            } else {
                u16::from(bytes[0x3f] & 15) + 1
            },
        };
        ensure!(
            be_u32(bytes, 0x10)? == 68
                && bytes[0x38..0x3a] == [2, 2]
                && (2..=640).contains(&header.width)
                && (2..=480).contains(&header.height)
                && header.width.is_multiple_of(2)
                && header.height.is_multiple_of(2)
                && (1..=100_000).contains(&header.frames)
                && (10_000..=100_000).contains(&header.frame_micros)
                && (header.audio_streams != 0) == (be_u32(bytes, 0x20)? != 0)
                && (header.audio_streams == 0
                    || ((1..=2).contains(&header.channels)
                        && bytes[0x3d..0x3f] == [16, 0]
                        && (8_000..=96_000).contains(&header.sample_rate))),
            "unsupported movie dimensions or timing"
        );
        Ok(header)
    }

    fn frame_bytes(&self) -> usize {
        self.width * self.height * 3 / 2
    }
}

struct Colors {
    components: [[i32; 256]; 5],
    table: Vec<u8>,
    clamp_offset: i32,
}

impl Colors {
    fn from_dol(dol: &[u8]) -> Result<Self> {
        const TABLE: u32 = 0x802a3178;
        const COEFFICIENT_BYTES: u32 = 5 * 256 * 2;
        let bytes = dol_slice(dol, TABLE, COEFFICIENT_BYTES as usize)?;
        let mut components = [[0; 256]; 5];
        for (index, value) in components.iter_mut().flatten().enumerate() {
            *value = i32::from(i16::from_be_bytes(
                bytes[index * 2..index * 2 + 2].try_into()?,
            ));
        }
        let bounds = [i32::min, i32::max].map(|choose| {
            let [y, vr, vg, ug, ub] =
                components.map(|channel| channel.into_iter().reduce(choose).unwrap());
            [y + vr, y + vg + ug, y + ub]
                .into_iter()
                .reduce(choose)
                .unwrap()
        });
        let [min, max] = bounds;
        // The converter reads a byte at the signed coefficient sum, without
        // saturating to the declared clamp array. Include the entire reachable
        // interval: vivid colors also read adjacent static data on either side.
        let start = (TABLE + COEFFICIENT_BYTES)
            .checked_add_signed(min)
            .context("movie color lookup address overflow")?;
        Ok(Self {
            components,
            table: dol_slice(dol, start, (max - min + 1) as usize)?.to_vec(),
            clamp_offset: -min,
        })
    }

    fn convert(&self, yuv: &[u8], width: usize, height: usize, rgb: &mut [u8]) -> Result<()> {
        let plane = width * height;
        ensure!(
            width.is_multiple_of(2)
                && height.is_multiple_of(2)
                && yuv.len() == plane * 3 / 2
                && rgb.len() == plane * 3,
            "invalid YUV420 frame size"
        );
        for row in 0..height {
            for col in 0..width {
                let pixel = row * width + col;
                let chroma = (row / 2) * (width / 2) + col / 2;
                let y = usize::from(yuv[pixel]);
                let u = usize::from(yuv[plane + chroma]);
                let v = usize::from(yuv[plane + plane / 4 + chroma]);
                let luma = self.components[0][y];
                let values = [
                    luma + self.components[1][v],
                    luma + self.components[2][v] + self.components[3][u],
                    luma + self.components[4][u],
                ];
                for (channel, index) in values.into_iter().enumerate() {
                    rgb[pixel * 3 + channel] = *self.table.get((self.clamp_offset + index) as usize).with_context(|| format!(
                        "YUV color lookup outside table: ({col},{row}) Y={y} U={u} V={v} channel={channel} index={index}"))?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MovieAudioTrack {
    /// One-based source stream index; audio streams retain this order in the mux.
    pub stream: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub frames: u32,
}

/// A physical movie can contain multiple audio tracks, or no audio at all.
/// Runtime playback metadata is produced separately for a selected track.
#[derive(Debug, Serialize, Deserialize)]
pub struct CookedMovie {
    pub version: u32,
    pub source: String,
    pub source_sha256: String,
    pub path: String,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub frame_micros: u32,
    pub audio_tracks: Vec<MovieAudioTrack>,
}

/// Cook any physical movie, preserving video and all source audio tracks.
/// Runtime metadata binds one audio track separately without copying this file.
pub fn cook_movie_file(extracted: &Path, source: &Path, output: &Path) -> Result<CookedMovie> {
    let workspace = Workspace::open(extracted, output)?;
    let _publications = crate::publication::Session::start_if_needed(output)?;
    let source = source.to_str().context("movie source path is not UTF-8")?;
    resonance_content::validate_asset_path(source)?;
    let files = workspace.extracted.join("files").canonicalize()?;
    let movie = files.join(source).canonicalize()?;
    ensure!(
        movie.starts_with(&files),
        "movie source escapes extracted files"
    );
    let mut bytes = [0; 68];
    fs::File::open(&movie)?.read_exact(&mut bytes)?;
    let header = Header::parse(&bytes)?;
    let dol = workspace.extracted.join("sys/main.dol");
    let colors = Colors::from_dol(&fs::read(&dol)?)?;
    let source_sha256 = hash_file(&movie)?;
    let metadata = workspace.output.join("movie.json");
    let directory = workspace.output.join("intermediate/movie");
    fs::create_dir_all(&directory)?;
    let yuv = directory.join("video.yuv");
    decode_video(&movie, &yuv, &header)?;
    let (audio_tracks, audio_paths) = cook_audio(&movie, &directory, &header)?;
    let destination = workspace.output.join("movie.mkv");
    let temporary = crate::temporary_path(&destination);
    let formats: Vec<_> = audio_tracks
        .iter()
        .map(|track| (track.channels, track.sample_rate, u64::from(track.frames)))
        .collect();
    let mut encoder = resonance_media::encode::MovieWriter::with_audio_tracks(
        BufWriter::new(fs::File::create(&temporary)?),
        header.width as u32,
        header.height as u32,
        header.frame_micros,
        &formats,
    )?;
    let mut waves = audio_paths
        .iter()
        .map(hound::WavReader::open)
        .collect::<Result<Vec<_>, _>>()?;
    let mut video = BufReader::new(fs::File::open(&yuv)?);
    let mut raw = vec![0; header.frame_bytes()];
    let mut rgb = vec![0; header.width * header.height * 3];
    let mut video_index = 0u32;
    let mut audio_indices = vec![0u64; audio_tracks.len()];
    loop {
        let audio = audio_tracks
            .iter()
            .enumerate()
            .filter(|&(index, track)| audio_indices[index] < u64::from(track.frames))
            .min_by_key(|&(index, track)| {
                audio_indices[index] * 1_000_000 / u64::from(track.sample_rate)
            })
            .map(|(index, track)| {
                (
                    index,
                    audio_indices[index] * 1_000_000 / u64::from(track.sample_rate),
                )
            });
        if let Some((index, timestamp)) = audio
            && (video_index == header.frames
                || timestamp <= u64::from(video_index) * u64::from(header.frame_micros))
        {
            let track = &audio_tracks[index];
            let count = (u64::from(track.frames) - audio_indices[index])
                .min(resonance_media::encode::AUDIO_BLOCK as u64) as usize;
            let pcm = waves[index]
                .samples::<i16>()
                .take(count * usize::from(track.channels))
                .collect::<Result<Vec<_>, _>>()?;
            ensure!(
                pcm.len() == count * usize::from(track.channels),
                "truncated normalized movie audio"
            );
            encoder.audio_track(index, &pcm)?;
            audio_indices[index] += count as u64;
        } else if video_index < header.frames {
            video.read_exact(&mut raw)?;
            colors.convert(&raw, header.width, header.height, &mut rgb)?;
            encoder.video(
                &rgb,
                Duration::from_micros(u64::from(video_index) * u64::from(header.frame_micros)),
            )?;
            video_index += 1;
        } else {
            break;
        }
    }
    encoder.finish()?.into_inner()?.sync_all()?;
    let cooked = CookedMovie {
        version: 1,
        source: source.into(),
        source_sha256,
        path: "movie.mkv".into(),
        sha256: hash_file(&temporary)?,
        width: header.width as u32,
        height: header.height as u32,
        frames: header.frames,
        frame_micros: header.frame_micros,
        audio_tracks,
    };
    verify_mux(&temporary, &cooked)?;
    crate::publication::install(&temporary, &destination, &cooked.sha256)?;
    write_json(&metadata, &json!(cooked))?;
    drop(waves);
    drop(video);
    fs::remove_dir_all(directory)?;
    println!(
        "Cooked {source}: {} video frames, {} audio tracks",
        cooked.frames,
        cooked.audio_tracks.len()
    );
    Ok(cooked)
}

fn decode_video(movie: &Path, output: &Path, header: &Header) -> Result<()> {
    let mut decoder = h4m::VideoDecoder::with_limits(
        BufReader::new(fs::File::open(movie)?),
        h4m::VideoLimits {
            max_pixels: 640 * 480,
            max_frame_bytes: 64 * 1024 * 1024,
        },
    )?;
    let mut target = fs::File::create(output)?;
    let mut seen = vec![false; header.frames as usize];
    while let Some(frame) = decoder.next_frame()? {
        let index = frame.display_index() as usize;
        ensure!(
            index < seen.len() && !seen[index],
            "invalid or repeated H4M presentation index"
        );
        ensure!(
            frame.y().data().len() + frame.u().data().len() + frame.v().data().len()
                == header.frame_bytes(),
            "unexpected H4M plane layout"
        );
        // H4M yields decode order, e.g. I0, P3, B1, B2. Place by display index
        // without retaining an unbounded reorder queue in memory.
        target.seek(SeekFrom::Start(index as u64 * header.frame_bytes() as u64))?;
        for plane in frame.planes() {
            target.write_all(plane.data())?;
        }
        seen[index] = true;
    }
    ensure!(
        seen.iter().all(|&seen| seen),
        "H4M movie is missing presentation frames"
    );
    ensure!(
        target.metadata()?.len() == header.frame_bytes() as u64 * u64::from(header.frames),
        "decoded video frame count mismatch"
    );
    target.sync_all()?;
    Ok(())
}

fn cook_audio(
    movie: &Path,
    directory: &Path,
    header: &Header,
) -> Result<(Vec<MovieAudioTrack>, Vec<PathBuf>)> {
    let mut tracks = Vec::new();
    let mut paths = Vec::new();
    for stream in 1..=header.audio_streams {
        let mut decoder = h4m::AudioDecoder::new(BufReader::new(fs::File::open(movie)?), stream)?;
        let info = decoder.metadata();
        ensure!(
            info.channels() == header.channels && info.sample_rate() == header.sample_rate,
            "unexpected movie audio format"
        );
        let path = directory.join(format!("audio-{stream}.wav"));
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: info.channels(),
                sample_rate: info.sample_rate(),
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        let mut frames = 0u32;
        while let Some(pcm) = decoder.next_block()? {
            // Match the game's opposite movie-DMA stereo channel order.
            ensure!(
                pcm.len().is_multiple_of(usize::from(info.channels())),
                "incomplete movie audio frame"
            );
            for frame in pcm.chunks_exact(usize::from(info.channels())) {
                for &sample in frame.iter().rev() {
                    writer.write_sample(sample)?;
                }
                frames = frames
                    .checked_add(1)
                    .context("movie audio frame count overflow")?;
            }
            ensure!(
                frames <= header.sample_rate * 3600,
                "movie audio exceeds one hour"
            );
        }
        writer.finalize()?;
        ensure!(frames > 0, "movie audio track is empty");
        tracks.push(MovieAudioTrack {
            stream,
            channels: info.channels(),
            sample_rate: info.sample_rate(),
            frames,
        });
        paths.push(path);
    }
    Ok((tracks, paths))
}

fn verify_mux(path: &Path, movie: &CookedMovie) -> Result<()> {
    use matroska_demuxer::{MatroskaFile, TrackType};
    let mut input = MatroskaFile::open(BufReader::new(fs::File::open(path)?))?;
    let tracks = input.tracks();
    ensure!(
        tracks.len() == movie.audio_tracks.len() + 1,
        "movie stream count mismatch"
    );
    let video = &tracks[0];
    let format = video.video().context("missing video dimensions")?;
    ensure!(
        video.track_type() == TrackType::Video
            && video.codec_id() == "V_FFV1"
            && video.track_number().get() == 1
            && format.pixel_width().get() == u64::from(movie.width)
            && format.pixel_height().get() == u64::from(movie.height)
            && video.default_duration().map(|value| value.get())
                == Some(u64::from(movie.frame_micros) * 1000),
        "movie video format mismatch"
    );
    for (index, track) in movie.audio_tracks.iter().enumerate() {
        let audio = &tracks[index + 1];
        let format = audio.audio().context("missing audio format")?;
        let private = audio
            .codec_private()
            .context("missing FLAC configuration")?;
        ensure!(
            audio.track_type() == TrackType::Audio
                && audio.codec_id() == "A_FLAC"
                && audio.track_number().get() == index as u64 + 2
                && track.stream as usize == index + 1
                && audio.name() == Some(format!("Source stream {}", track.stream).as_str())
                && format.channels().get() == u64::from(track.channels)
                && format.sampling_frequency() == f64::from(track.sample_rate)
                && private.len() == 42
                && &private[..8] == b"fLaC\x80\x00\x00\x22",
            "movie audio track format or order mismatch"
        );
        let info = &private[8..];
        let packed = u64::from_be_bytes(info[10..18].try_into()?);
        ensure!(
            u16::from_be_bytes(info[..2].try_into()?) as usize
                == resonance_media::encode::AUDIO_BLOCK
                && info[..2] == info[2..4]
                && packed >> 44 == u64::from(track.sample_rate)
                && ((packed >> 41) & 7) + 1 == u64::from(track.channels)
                && ((packed >> 36) & 31) + 1 == 16
                && packed & 0xf_ffff_ffff == u64::from(track.frames),
            "movie FLAC sample count or format mismatch"
        );
    }
    let scale = input.info().timestamp_scale().get();
    let mut counts = vec![0u64; tracks.len()];
    let mut packet = matroska_demuxer::Frame::default();
    while input.next_frame(&mut packet)? {
        let index = usize::try_from(packet.track)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .filter(|&index| index < counts.len())
            .context("unexpected movie track number")?;
        let expected = if index == 0 {
            counts[0] * u64::from(movie.frame_micros)
        } else {
            counts[index] * resonance_media::encode::AUDIO_BLOCK as u64 * 1_000_000
                / u64::from(movie.audio_tracks[index - 1].sample_rate)
        };
        ensure!(
            !packet.is_invisible
                && packet.timestamp.checked_mul(scale) == expected.checked_mul(1000),
            "movie packet timestamp mismatch"
        );
        counts[index] += 1;
    }
    ensure!(
        counts[0] == u64::from(movie.frames),
        "movie frame count mismatch"
    );
    for (index, track) in movie.audio_tracks.iter().enumerate() {
        ensure!(
            counts[index + 1]
                == u64::from(track.frames).div_ceil(resonance_media::encode::AUDIO_BLOCK as u64),
            "movie audio packet count mismatch"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mux_verification_checks_complete_video_and_audio_tracks() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("movie.mkv");
        for count in [0, 1, 2] {
            let formats = vec![(2, 32_000, 32); count];
            let mut writer = resonance_media::encode::MovieWriter::with_audio_tracks(
                fs::File::create(&path)?,
                16,
                16,
                40_000,
                &formats,
            )?;
            for index in 0..count {
                writer.audio_track(index, &[17; 64])?;
            }
            for index in 0..2 {
                writer.video(&[41; 16 * 16 * 3], Duration::from_micros(index * 40_000))?;
            }
            writer.finish()?;
            let mut movie = CookedMovie {
                version: 1,
                source: "example.h4m".into(),
                source_sha256: String::new(),
                path: "movie.mkv".into(),
                sha256: String::new(),
                width: 16,
                height: 16,
                frames: 2,
                frame_micros: 40_000,
                audio_tracks: (0..count)
                    .map(|index| MovieAudioTrack {
                        stream: index as u16 + 1,
                        channels: 2,
                        sample_rate: 32_000,
                        frames: 32,
                    })
                    .collect(),
            };
            verify_mux(&path, &movie)?;
            movie.frames += 1;
            assert!(verify_mux(&path, &movie).is_err());
            movie.frames -= 1;
            if count > 0 {
                movie.audio_tracks[0].frames += 1;
                assert!(verify_mux(&path, &movie).is_err());
                movie.audio_tracks.clear();
                assert!(verify_mux(&path, &movie).is_err());
            }
        }
        Ok(())
    }

    #[test]
    fn header_distinguishes_no_audio_from_one_or_more_tracks() -> Result<()> {
        let mut bytes = [0; 68];
        bytes[..16].copy_from_slice(SIGNATURE);
        for (at, value) in [(0x10, 68u32), (0x1c, 1), (0x24, 40_000)] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[0x34..0x3a].copy_from_slice(&[0, 2, 0, 2, 2, 2]);
        assert_eq!(Header::parse(&bytes)?.audio_streams, 0);
        bytes[0x3c] = 2;
        bytes[0x3d] = 16;
        bytes[0x40..].copy_from_slice(&32_028u32.to_be_bytes());
        assert!(Header::parse(&bytes).is_err()); // Audio format with no packets.
        bytes[0x23] = 1;
        for maximum in [0, 1, 15] {
            bytes[0x3f] = maximum;
            assert_eq!(Header::parse(&bytes)?.audio_streams, u16::from(maximum) + 1);
        }
        bytes[0x3c] = 0;
        assert!(Header::parse(&bytes).is_err()); // Incomplete audio format.
        Ok(())
    }

    #[test]
    fn executable_lookup_covers_both_sides_of_the_declared_clamp_array() {
        let mut dol = vec![0; 0x1500];
        for (at, word) in [(0, 0x100u32), (0x48, 0x802a3178), (0x90, 0x1400)] {
            dol[at..at + 4].copy_from_slice(&word.to_be_bytes());
        }
        let table = &mut dol[0x100..];
        table[0x202..0x204].copy_from_slice(&(-1i16).to_be_bytes());
        table[0x802..0x804].copy_from_slice(&1800i16.to_be_bytes());
        table[0xa00 - 1] = 41;
        table[0xa00 + 1800] = 211;
        let colors = Colors::from_dol(&dol).unwrap();
        let mut rgb = [0; 12];
        colors.convert(&[0, 0, 0, 0, 1, 1], 2, 2, &mut rgb).unwrap();
        assert_eq!(rgb, [41, 0, 211, 41, 0, 211, 41, 0, 211, 41, 0, 211]);
        assert!(Colors::from_dol(&dol[..0x100 + 0xa00 + 1800]).is_err());
    }

    #[test]
    fn shares_chroma_across_a_two_by_two_block_and_checks_sizes() {
        let mut colors = Colors {
            components: [[0; 256]; 5],
            table: (0..=255).collect(),
            clamp_offset: 0,
        };
        colors.components[0][1] = 10;
        colors.components[0][2] = 20;
        colors.components[1][6] = 1;
        colors.components[2][6] = 2;
        colors.components[3][5] = 3;
        colors.components[4][5] = 4;
        let mut rgb = [0; 12];
        colors.convert(&[1, 2, 2, 1, 5, 6], 2, 2, &mut rgb).unwrap();
        assert_eq!(rgb, [11, 15, 14, 21, 25, 24, 21, 25, 24, 11, 15, 14]);
        assert!(colors.convert(&[0; 5], 2, 2, &mut rgb).is_err());
        assert!(Header::parse(&[0; 67]).is_err());
    }

    #[test]
    fn negative_color_index_uses_checked_coefficient_prefix() {
        let mut colors = Colors {
            components: [[0; 256]; 5],
            table: vec![0; 0x1000],
            clamp_offset: 0xa00,
        };
        colors.components[0][34] = 340;
        colors.components[1][73] = -351;
        colors.table[0xa00 - 11] = 217;
        let mut rgb = [0; 12];
        colors
            .convert(&[34, 34, 34, 34, 128, 73], 2, 2, &mut rgb)
            .unwrap();
        assert_eq!(rgb, [217, 0, 0, 217, 0, 0, 217, 0, 0, 217, 0, 0]);
        colors.components[1][73] = -4000;
        assert!(
            colors
                .convert(&[34, 34, 34, 34, 128, 73], 2, 2, &mut rgb)
                .is_err()
        );
    }
}
