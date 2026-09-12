use super::{
    Tool, Workspace, be_u32, hash_file, json_file, process, valid_asset, wav_frames, write_json,
};
use crate::dol::slice as dol_slice;
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{
    fs,
    io::{BufReader, Read, Write},
    path::Path,
    time::Duration,
};

struct Header {
    width: usize,
    height: usize,
    frames: u32,
    frame_micros: u32,
    sample_rate: u32,
}

impl Header {
    fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() >= 68 && &bytes[..16] == b"HVQM4 1.5\0\0\0\0\0\0\0",
            "expected HVQM4 1.5 movie"
        );
        let header = Self {
            width: usize::from(u16::from_be_bytes(bytes[0x34..0x36].try_into()?)),
            height: usize::from(u16::from_be_bytes(bytes[0x36..0x38].try_into()?)),
            frames: be_u32(bytes, 0x1c)?,
            frame_micros: be_u32(bytes, 0x24)?,
            sample_rate: be_u32(bytes, 0x40)?,
        };
        ensure!(
            be_u32(bytes, 0x10)? == 68
                && bytes[0x38..0x3a] == [2, 2]
                && bytes[0x3c..0x3f] == [2, 16, 0]
                && (2..=640).contains(&header.width)
                && (2..=480).contains(&header.height)
                && header.width.is_multiple_of(2)
                && header.height.is_multiple_of(2)
                && (1..=100_000).contains(&header.frames)
                && header.frame_micros == 33366
                && header.sample_rate == 32028,
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
        // Use the cooked fixed-point YUV coefficients, shared chroma, and rounding.
        let bytes = dol_slice(dol, 0x802a3178, 0x1000)?;
        let mut components = [[0; 256]; 5];
        for (index, value) in components.iter_mut().flatten().enumerate() {
            *value = i32::from(i16::from_be_bytes(
                bytes[index * 2..index * 2 + 2].try_into()?,
            ));
        }
        Ok(Self {
            components,
            // Original instructions 800F06A0..800F06B0 add the signed
            // coefficients to the table base, then load at offset 0xA00.
            // Opening frame 1137 contains one negative index (-11): the
            // executable reads the preceding coefficient byte (217). Retain
            // that lookup in this bounded offline slice rather than indexing
            // a Rust clamp array out of bounds or changing the source pixel.
            table: bytes.to_vec(),
            clamp_offset: 0xa00,
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

#[derive(Clone, Copy)]
pub enum MovieSource {
    Opening,
    StoryIntroduction,
}
impl MovieSource {
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Opening => ("op", "intro"),
            Self::StoryIntroduction => ("s01", "story-intro"),
        }
    }
}

/// Convert a source movie to a standard, device-independent cooked asset.
pub fn cook_movie(
    extracted: &Path,
    output: &Path,
    video_decoder: &Path,
    audio_decoder: &Path,
    ffmpeg: &Path,
    audio_stream: u8,
    source: MovieSource,
) -> Result<()> {
    let (source_name, name) = source.names();
    let relative = format!("movies/{name}.mkv");
    ensure!(
        (1..=2).contains(&audio_stream),
        "movie audio stream must be 1 or 2"
    );
    let workspace = Workspace::open(extracted, output)?;
    let video_decoder = Tool::resolve(video_decoder)?;
    let audio_decoder = Tool::resolve(audio_decoder)?;
    let ffmpeg = Tool::resolve(ffmpeg)?;
    let movie = workspace
        .extracted
        .join(format!("files/MOV/{source_name}.h4m"));
    let mut header_bytes = [0u8; 68];
    fs::File::open(&movie)?.read_exact(&mut header_bytes)?;
    let header = Header::parse(&header_bytes)?;
    let dol = workspace.extracted.join("sys/main.dol");
    let colors = Colors::from_dol(&fs::read(&dol)?)?;
    let recipe = json!({"version": 2, "movie_sha256": hash_file(&movie)?,
        "dol_sha256": hash_file(&dol)?, "video_decoder_sha256": video_decoder.hash,
        "audio_decoder_sha256": audio_decoder.hash, "ffmpeg_sha256": ffmpeg.hash,
        "audio_stream": audio_stream, "video_codec": "ffv1", "audio_codec": "flac",
        "color_conversion": "gqseaf-yuv-table-v2", "audio_channel_order": "swap_lr"});
    let metadata = workspace.output.join(format!("{name}.json"));
    if let Some(previous) = json_file(&metadata)
        && previous["version"] == 1
        && previous["recipe"] == recipe
        && previous["path"] == relative
        && valid_asset(&workspace.output, &previous)
        && serde_json::from_value::<resonance_content::MovieAsset>(previous)
            .is_ok_and(|movie| movie.validate().is_ok())
    {
        println!("{name} movie is current");
        if matches!(source, MovieSource::StoryIntroduction) {
            crate::field::refresh_preloads(&workspace.output)?;
        }
        return Ok(());
    }
    let directory = workspace.intermediate(name)?;
    let yuv = directory.join("video.yuv");
    let decode_metadata = directory.join("video-recipe.json");
    let decode_recipe = json!({"movie_sha256": recipe["movie_sha256"], "video_decoder_sha256": recipe["video_decoder_sha256"]});
    let expected = header.frame_bytes() as u64 * u64::from(header.frames);
    if !(json_file(&decode_metadata).is_some_and(|value| {
        value["recipe"] == decode_recipe
            && value["sha256"]
                .as_str()
                .is_some_and(|hash| hash_file(&yuv).is_ok_and(|actual| actual == hash))
    }) && fs::metadata(&yuv).is_ok_and(|metadata| metadata.len() == expected))
    {
        let temporary = yuv.with_extension("partial.yuv");
        process::pipe(
            video_decoder
                .command(&directory)
                .arg(&movie)
                .arg(&temporary),
            &directory.join("decode.log"),
            Duration::from_secs(600),
            |_| Ok(()),
        )?;
        ensure!(
            fs::metadata(&temporary)?.len() == expected,
            "decoded video frame count mismatch"
        );
        fs::rename(&temporary, &yuv)?;
        write_json(
            &decode_metadata,
            &json!({"recipe": decode_recipe, "sha256": hash_file(&yuv)?}),
        )?;
    }
    let audio = directory.join(format!("audio-{audio_stream}.wav"));
    process::run(
        audio_decoder
            .command(&directory)
            .args(["-i", "-s", &audio_stream.to_string(), "-o"])
            .arg(&audio)
            .arg(&movie),
        &directory.join("audio.log"),
        b"",
    )?;
    let audio_frames = wav_frames(&audio, header.sample_rate, header.sample_rate * 3600)?;
    ensure!(
        !matches!(source, MovieSource::Opening) || audio_frames == 3_879_328,
        "unexpected opening audio frame count"
    );
    // The game's movie DMA output has the opposite channel order to this
    // vgmstream decoder. Stereo stream 1, swapped here, matches recorded
    // Dolphin PCM exactly in steady playback windows. Normalize it offline;
    // the runtime receives ordinary left/right stereo audio.
    let normalized_audio = directory.join(format!("audio-{audio_stream}-stereo.wav"));
    normalize_channels(&audio, &normalized_audio)?;
    let destination = workspace.output.join(&relative);
    fs::create_dir_all(
        destination
            .parent()
            .context("movie destination has no parent")?,
    )?;
    let temporary = destination.with_extension("partial.mkv");
    process::pipe(
        ffmpeg
            .command(&directory)
            .args([
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgb24",
                "-video_size",
            ])
            .arg(format!("{}x{}", header.width, header.height))
            .arg("-framerate")
            .arg(format!("1000000/{}", header.frame_micros))
            .args(["-i", "pipe:0", "-i"])
            .arg(&normalized_audio)
            .args([
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "ffv1",
                "-level",
                "3",
                "-pix_fmt",
                "bgr0",
                "-threads",
                "4",
                "-c:a",
                "flac",
                "-map_metadata",
                "-1",
                "-color_range",
                "pc",
                "-colorspace",
                "rgb",
            ])
            .arg(&temporary),
        &directory.join("encode.log"),
        Duration::from_secs(1200),
        |stdin| {
            let mut source = BufReader::new(fs::File::open(&yuv)?);
            let mut raw = vec![0; header.frame_bytes()];
            let mut rgb = vec![0; header.width * header.height * 3];
            for frame in 0..header.frames {
                source.read_exact(&mut raw)?;
                colors
                    .convert(&raw, header.width, header.height, &mut rgb)
                    .with_context(|| format!("convert {name} frame {frame}"))?;
                stdin.write_all(&rgb)?;
                if frame % 300 == 0 {
                    println!("Converted {name} frame {frame}/{}", header.frames);
                }
            }
            Ok(())
        },
    )?;
    fs::rename(&temporary, &destination)?;
    write_json(
        &metadata,
        &json!({"version": 1, "path": relative,
        "sha256": hash_file(&destination)?, "width": header.width, "height": header.height,
        "frames": header.frames, "frame_micros": header.frame_micros,
        "sample_rate": header.sample_rate, "channels": 2, "audio_frames": audio_frames, "recipe": recipe}),
    )?;
    println!(
        "Cooked {name}: {} video frames, {audio_frames} audio frames",
        header.frames
    );
    if matches!(source, MovieSource::StoryIntroduction) {
        crate::field::refresh_preloads(&workspace.output)?;
    }
    Ok(())
}

fn normalize_channels(source: &Path, destination: &Path) -> Result<()> {
    let mut source = hound::WavReader::open(source)?;
    let spec = source.spec();
    ensure!(
        spec.channels == 2
            && spec.bits_per_sample == 16
            && spec.sample_format == hound::SampleFormat::Int,
        "expected stereo PCM16 movie intermediate"
    );
    let temporary = destination.with_extension("partial.wav");
    let mut output = hound::WavWriter::create(&temporary, spec)?;
    let mut samples = source.samples::<i16>();
    while let Some(left) = samples.next() {
        let left = left?;
        let right = samples.next().context("incomplete stereo movie frame")??;
        output.write_sample(right)?;
        output.write_sample(left)?;
    }
    output.finalize()?;
    fs::rename(&temporary, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
