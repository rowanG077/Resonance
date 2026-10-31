//! Native H4M files to named MKV movies and one machine-ID mapping.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::library::{MOVIES, Mappings};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SIGNATURE: &[u8; 16] = b"HVQM4 1.5\0\0\0\0\0\0\0";
fn movie_name(source: &str) -> Result<&'static str> {
    Ok(match source.to_ascii_lowercase().as_str() {
        "mov/op.h4m" => "opening",
        "mov/s01.h4m" => "story-introduction",
        "mov/s03.h4m" => "tower-of-salvation-reveal",
        "mov/as1.h4m" => "tower-of-mana-activation",
        "mov/s07.h4m" => "celsius-freezes-the-sea",
        "mov/s08.h4m" => "mana-cannon-firing",
        "mov/s09.h4m" => "tower-of-salvation-collapse",
        "mov/as2.h4m" => "worlds-reunited",
        "mov/as3.h4m" => "great-seed-awakening",
        "mov/s10.h4m" => "derris-kharlan-revealed",
        _ => anyhow::bail!("movie source {source} has no maintained name"),
    })
}

fn movie_file(extracted: &Path, source: &str) -> Result<PathBuf> {
    let name = source.to_ascii_lowercase();
    let file = name
        .strip_prefix("mov/")
        .context("movie outside MOV directory")?;
    ensure!(
        !file.contains('/') && !file.contains('\\') && file.ends_with(".h4m"),
        "invalid movie source {source}"
    );
    Ok(extracted.join("files/MOV").join(file))
}

fn source_movie(root: &Path, source: &str) -> Result<Option<PathBuf>> {
    let first = movie_file(&root.join("disc1"), source)?;
    if first.is_file() {
        return Ok(Some(first));
    }
    let second = movie_file(&root.join("disc2"), source)?;
    Ok(second.is_file().then_some(second))
}

fn catalogue(root: &Path, selected: Option<u32>) -> Result<BTreeMap<u32, (PathBuf, String)>> {
    let mut found = BTreeMap::<u32, (PathBuf, String)>::new();
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    for (id, row) in dol::slice(&exe, 0x801f9c80, 19 * 4)?
        .chunks_exact(4)
        .enumerate()
    {
        if selected.is_some_and(|wanted| wanted != id as u32) {
            continue;
        }
        let pointer = dol::u32(row, 0)?;
        if pointer == 0 {
            continue;
        }
        let source = text::string(&exe, pointer)?;
        match source_movie(root, &source)? {
            Some(movie) => {
                found.insert(id as u32, (movie, source));
            }
            None if movie_name(&source).is_ok() => {
                anyhow::bail!("missing movie {source} on both discs");
            }
            None => {} // Unshipped development entries in the executable table.
        }
    }
    if let Some(id) = selected {
        ensure!(
            found.contains_key(&id),
            "movie {id} has no source in the supplied GameCube files"
        );
    }
    ensure!(!found.is_empty(), "no GameCube movies were found");
    if selected.is_none() {
        ensure!(found.len() == 13, "incomplete native movie catalogue");
    }
    Ok(found)
}

pub fn cook_movies(root: &Path, library: &Path, selected: Option<u32>) -> Result<()> {
    let sources = catalogue(root, selected)?;
    let output = library.join("movies");
    let mappings = Mappings::new(
        sources
            .iter()
            .map(|(&id, (_, source))| Ok((id, format!("{}.mkv", movie_name(source)?))))
            .collect::<Result<BTreeMap<_, _>>>()?,
    )?;
    fs::create_dir_all(&output)?;
    let mut converted = BTreeSet::new();
    let mut jobs = Vec::new();
    for (movie, source) in sources.values() {
        let name = movie_name(source)?;
        if converted.insert(name) {
            jobs.push((name, movie, source));
        }
    }
    crate::parallel::for_each(&jobs, |index, &(name, movie, source)| {
        eprintln!("  [{}/{} jobs] Movie: {name}", index + 1, jobs.len());
        convert_movie(
            movie,
            &root.join("disc1/sys/main.dol"),
            &output.join(format!("{name}.mkv")),
        )
        .with_context(|| format!("convert movie {source}"))
    })?;
    crate::output::json(&library.join(MOVIES), &mappings)?;
    Ok(())
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
            bytes.len() >= 68 && bytes.starts_with(SIGNATURE),
            "expected HVQM4 1.5 movie"
        );
        let width = usize::from(dol::u16(bytes, 0x34)?);
        let height = usize::from(dol::u16(bytes, 0x36)?);
        let frames = dol::u32(bytes, 0x1c)?;
        let frame_micros = dol::u32(bytes, 0x24)?;
        let sample_rate = dol::u32(bytes, 0x40)?;
        let channels = u16::from(bytes[0x3c]);
        let audio_streams = if channels == 0 && sample_rate == 0 {
            0
        } else {
            u16::from(bytes[0x3f] & 15) + 1
        };
        ensure!(
            dol::u32(bytes, 0x10)? == 68
                && bytes[0x38..0x3a] == [2, 2]
                && (2..=640).contains(&width)
                && (2..=480).contains(&height)
                && width.is_multiple_of(2)
                && height.is_multiple_of(2)
                && (1..=100_000).contains(&frames)
                && (10_000..=100_000).contains(&frame_micros)
                && (audio_streams != 0) == (dol::u32(bytes, 0x20)? != 0)
                && (audio_streams == 0
                    || ((1..=2).contains(&channels)
                        && bytes[0x3d..0x3f] == [16, 0]
                        && (8_000..=96_000).contains(&sample_rate))),
            "unsupported H4M movie dimensions or timing"
        );
        Ok(Self {
            width,
            height,
            frames,
            frame_micros,
            sample_rate,
            channels,
            audio_streams,
        })
    }
    fn frame_bytes(&self) -> usize {
        self.width * self.height * 3 / 2
    }
}

struct Colors {
    components: [[i32; 256]; 5],
    table: Vec<u8>,
    offset: i32,
}
impl Colors {
    fn read(exe: &[u8]) -> Result<Self> {
        const ADDRESS: u32 = 0x802a3178;
        const LENGTH: usize = 5 * 256 * 2;
        let bytes = dol::slice(exe, ADDRESS, LENGTH)?;
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
        let start = (ADDRESS + LENGTH as u32)
            .checked_add_signed(min)
            .context("movie color lookup address overflow")?;
        Ok(Self {
            components,
            table: dol::slice(exe, start, (max - min + 1) as usize)?.to_vec(),
            offset: -min,
        })
    }
    fn convert(&self, yuv: &[u8], header: &Header, rgb: &mut [u8]) -> Result<()> {
        let plane = header.width * header.height;
        ensure!(
            yuv.len() == plane * 3 / 2 && rgb.len() == plane * 3,
            "invalid YUV420 movie frame size"
        );
        for row in 0..header.height {
            for col in 0..header.width {
                let pixel = row * header.width + col;
                let chroma = row / 2 * (header.width / 2) + col / 2;
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
                    rgb[pixel * 3 + channel] = *self
                        .table
                        .get((self.offset + index) as usize)
                        .context("movie color lookup outside source table")?;
                }
            }
        }
        Ok(())
    }
}

fn decode_video(movie: &Path, output: &Path, header: &Header, name: &str) -> Result<()> {
    let mut decoder = h4m::VideoDecoder::with_limits(
        BufReader::new(fs::File::open(movie)?),
        h4m::VideoLimits {
            max_pixels: 640 * 480,
            max_frame_bytes: 64 * 1024 * 1024,
        },
    )?;
    let mut target = fs::File::create(output)?;
    let mut seen = vec![false; header.frames as usize];
    let mut completed = 0;
    let mut progress = Instant::now();
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
        target.seek(SeekFrom::Start(index as u64 * header.frame_bytes() as u64))?;
        for plane in frame.planes() {
            target.write_all(plane.data())?;
        }
        seen[index] = true;
        completed += 1;
        if progress.elapsed() >= Duration::from_secs(10) || completed == header.frames {
            eprintln!(
                "    [{completed}/{} frames] {name}: decoding",
                header.frames
            );
            progress = Instant::now();
        }
    }
    ensure!(
        seen.iter().all(|&present| present),
        "H4M movie is missing display frames"
    );
    target.sync_all()?;
    Ok(())
}

struct AudioTrack {
    channels: u16,
    sample_rate: u32,
    frames: u32,
}

fn decode_audio(
    movie: &Path,
    scratch: &Path,
    header: &Header,
    name: &str,
) -> Result<(Vec<AudioTrack>, Vec<PathBuf>)> {
    let mut tracks = Vec::new();
    let mut paths = Vec::new();
    for stream in 1..=header.audio_streams {
        eprintln!(
            "    [{stream}/{} audio tracks] {name}",
            header.audio_streams
        );
        let mut decoder = h4m::AudioDecoder::new(BufReader::new(fs::File::open(movie)?), stream)?;
        let info = decoder.metadata();
        ensure!(
            info.channels() == header.channels && info.sample_rate() == header.sample_rate,
            "unexpected H4M audio format"
        );
        let path = scratch.join(format!("audio-{stream}.wav"));
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
            ensure!(
                pcm.len().is_multiple_of(usize::from(info.channels())),
                "incomplete movie audio frame"
            );
            for frame in pcm.chunks_exact(usize::from(info.channels())) {
                // GameCube movie DMA uses the opposite stereo channel order.
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
        tracks.push(AudioTrack {
            channels: info.channels(),
            sample_rate: info.sample_rate(),
            frames,
        });
        paths.push(path);
    }
    Ok((tracks, paths))
}

fn convert_movie(movie: &Path, dol_path: &Path, destination: &Path) -> Result<()> {
    let mut header_bytes = [0; 68];
    fs::File::open(movie)?.read_exact(&mut header_bytes)?;
    let header = Header::parse(&header_bytes)?;
    let name = destination
        .file_stem()
        .context("movie output has no name")?
        .to_string_lossy();
    let colors = Colors::read(&fs::read(dol_path)?)?;
    let scratch =
        tempfile::tempdir_in(destination.parent().context("movie has no output parent")?)?;
    let yuv = scratch.path().join("video.yuv");
    decode_video(movie, &yuv, &header, &name)?;
    let (audio_tracks, audio_paths) = decode_audio(movie, scratch.path(), &header, &name)?;
    let formats: Vec<_> = audio_tracks
        .iter()
        .map(|track| (track.channels, track.sample_rate, u64::from(track.frames)))
        .collect();
    let mut writer = resonance_media::encode::MovieWriter::with_audio_tracks(
        BufWriter::new(fs::File::create(destination)?),
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
    let mut progress = Instant::now();
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
                "truncated decoded movie audio"
            );
            writer.audio_track(index, &pcm)?;
            audio_indices[index] += count as u64;
        } else if video_index < header.frames {
            video.read_exact(&mut raw)?;
            colors.convert(&raw, &header, &mut rgb)?;
            writer.video(
                &rgb,
                Duration::from_micros(u64::from(video_index) * u64::from(header.frame_micros)),
            )?;
            video_index += 1;
            if progress.elapsed() >= Duration::from_secs(10) || video_index == header.frames {
                eprintln!(
                    "    [{video_index}/{} frames] {name}: encoding",
                    header.frames
                );
                progress = Instant::now();
            }
        } else {
            break;
        }
    }
    writer.finish()?.into_inner()?;
    drop(waves);
    drop(video);
    scratch.close()?;
    Ok(())
}
