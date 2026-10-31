//! GameCube voice archives directly into named WAVs and silence clocks.
//! Skit voices are authored as silence, so their headers suffice for timing.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::field_audio::{
    document::SPEECH,
    speech::{Clock, Mappings, Source},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const DIRECTORY: u32 = 0x802a30a0;
const ARCHIVES: [(&str, &str); 3] = [
    ("cv/cht_common.afs", "cht_common.afs"),
    ("cv/cht_disc_1.afs", "cht_disc_1.afs"),
    ("cv/cht_disc_2.afs", "cht_disc_2.afs"),
];
const WORLD_SPEAKERS: [&str; 9] = [
    "lloyd", "sheena", "raine", "sheena", "sheena", "genis", "sheena", "lloyd", "genis",
];

#[derive(Deserialize)]
struct Binding {
    skit: u32,
    speakers: BTreeMap<u32, String>,
}

#[derive(Deserialize)]
struct Scene {
    name: String,
    lines: BTreeMap<String, String>,
}

struct Member {
    name: String,
    offset: u64,
    size: usize,
}

fn afs_index(file: &mut fs::File) -> Result<Vec<Member>> {
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(0))?;
    let mut header = [0; 8];
    file.read_exact(&mut header)?;
    ensure!(&header[..4] == b"AFS\0", "invalid speech AFS header");
    let count = u32::from_le_bytes(header[4..8].try_into()?) as usize;
    ensure!((1..=65536).contains(&count), "invalid speech AFS count");
    let mut table = vec![0; count * 8 + 8];
    file.read_exact(&mut table)?;
    let word = |at: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(table[at..at + 4].try_into()?) as usize)
    };
    let names_at = word(count * 8)?;
    let names_size = word(count * 8 + 4)?;
    ensure!(
        names_at >= 16 + count * 8
            && names_size >= count * 48
            && (names_at as u64) + (names_size as u64) <= length,
        "invalid speech AFS names"
    );
    file.seek(SeekFrom::Start(names_at as u64))?;
    let mut names = vec![0; count * 48];
    file.read_exact(&mut names)?;
    let mut members = Vec::with_capacity(count);
    for member in 0..count {
        let offset = word(member * 8)?;
        let size = word(member * 8 + 4)?;
        ensure!(
            offset >= 16 + count * 8 && size >= 20 && (offset as u64) + (size as u64) <= length,
            "invalid speech AFS member {member}"
        );
        let name = &names[member * 48..member * 48 + 32];
        let end = name
            .iter()
            .position(|&byte| byte == 0)
            .context("unterminated speech name")?;
        let name = std::str::from_utf8(&name[..end])?.to_owned();
        members.push(Member {
            name,
            offset: offset as u64,
            size,
        });
    }
    Ok(members)
}

fn read_member(file: &mut fs::File, member: &Member) -> Result<Vec<u8>> {
    ensure!(
        member.size <= 128 * 1024 * 1024,
        "speech member is too large"
    );
    file.seek(SeekFrom::Start(member.offset))?;
    let mut bytes = vec![0; member.size];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn names() -> Result<(BTreeMap<u32, Binding>, BTreeMap<u32, String>)> {
    Ok((
        serde_json::from_str(include_str!("skit-speech.json"))?,
        serde_json::from_str(include_str!("skit-titles.json"))?,
    ))
}

fn line_name(
    source: &str,
    bindings: &BTreeMap<u32, Binding>,
    titles: &BTreeMap<u32, String>,
) -> Result<String> {
    let stem = source
        .strip_suffix(".ahx")
        .context("skit member is not an AHX stream")?;
    let parts = stem.split('_').collect::<Vec<_>>();
    ensure!(
        parts.len() == 4 && parts[0] == "cht" && parts[3] == "a",
        "unrecognized skit member {source}"
    );
    let group = parts[1].parse::<u32>()?;
    let line = parts[2].parse::<u32>()?;
    let Some(binding) = bindings.get(&group) else {
        let kind = if [
            68, 91, 101, 114, 167, 168, 169, 172, 324, 350, 428, 429, 430,
        ]
        .contains(&group)
        {
            "silence"
        } else {
            "line"
        };
        return Ok(format!("skits/unassigned/{kind}-{group:04}-{line:03}"));
    };
    let title = titles
        .get(&binding.skit)
        .cloned()
        .unwrap_or_else(|| format!("skit-{:04}", binding.skit));
    let speaker = binding
        .speakers
        .get(&line)
        .map(String::as_str)
        .unwrap_or("line");
    Ok(format!("skits/{title}/{speaker}-{line:03}"))
}

fn archive_bases(exe: &[u8]) -> Result<BTreeMap<String, u32>> {
    let mut found = BTreeMap::new();
    for row in dol::slice(exe, DIRECTORY, 18 * 12)?.chunks_exact(12) {
        let pointer = dol::u32(row, 0)?;
        if pointer == 0 {
            break;
        }
        let source = text::string(exe, pointer)?.to_ascii_lowercase();
        if ARCHIVES.iter().any(|(name, _)| *name == source) {
            let base = dol::u32(row, 4)?;
            ensure!(base & 0xffff == 0, "unaligned skit voice base");
            ensure!(
                found.insert(source, base).is_none(),
                "duplicate skit voice archive"
            );
        }
    }
    ensure!(found.len() == ARCHIVES.len(), "missing skit voice archive");
    Ok(found)
}

/// Return only the skit entries for the single shared speech mapping. The
/// caller merges these with event, battle and world speech before publication.
pub fn skit_mappings(root: &Path, selected: Option<u32>) -> Result<Mappings> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let bases = archive_bases(&exe)?;
    let (bindings, titles) = names()?;
    let mut mappings = BTreeMap::new();
    for (source, filename) in ARCHIVES {
        let base = bases[source];
        if selected.is_some_and(|id| id & 0xffff0000 != base) {
            continue;
        }
        let path = root.join("disc1/files/CV").join(filename);
        let mut file = fs::File::open(&path)
            .with_context(|| format!("missing GameCube skit archive {filename}"))?;
        let members = afs_index(&mut file)?;
        for (member, entry) in members.iter().enumerate() {
            let id = base | u32::try_from(member)?;
            if selected.is_some_and(|wanted| wanted != id) {
                continue;
            }
            let name = line_name(&entry.name, &bindings, &titles)?;
            file.seek(SeekFrom::Start(entry.offset))?;
            let mut stream = [0; 20];
            file.read_exact(&mut stream)?;
            ensure!(
                stream[..2] == [0x80, 0] && matches!(stream[4], 0x10 | 0x11),
                "unsupported skit AHX member {member}"
            );
            let silence = Clock {
                channels: u16::from(stream[7]),
                sample_rate: u32::from_be_bytes(stream[8..12].try_into()?),
                frames: u32::from_be_bytes(stream[12..16].try_into()?),
            };
            silence.validate()?;
            let source = Source::Silence { name, silence };
            source.resolve()?;
            ensure!(
                mappings.insert(id, source).is_none(),
                "duplicate skit voice {id}"
            );
        }
    }
    if let Some(id) = selected {
        ensure!(mappings.contains_key(&id), "skit voice {id} was not found");
    }
    Ok(mappings)
}

fn event_name(source: &str, scenes: &BTreeMap<u32, Scene>) -> Result<String> {
    let stem = source
        .strip_suffix(".ahx")
        .context("event member is not an AHX stream")?;
    let parts = stem.split('_').collect::<Vec<_>>();
    ensure!(
        parts.len() == 4 && parts[0] == "sce",
        "unrecognized event member {source}"
    );
    let scene_id = parts[1].parse::<u32>()?;
    let line = parts[2].parse::<usize>()?;
    if scene_id == 48 && (1..=9).contains(&line) {
        return Ok(format!("world/{line:02}-{}", WORLD_SPEAKERS[line - 1]));
    }
    let scene = scenes
        .get(&scene_id)
        .with_context(|| format!("event {scene_id} has no maintained name"))?;
    let key = format!("{}-{}", parts[2], parts[3]);
    let speaker = scene
        .lines
        .get(&key)
        .with_context(|| format!("event {scene_id} line {key} has no maintained speaker"))?;
    Ok(format!("events/{}/{speaker}", scene.name))
}

fn decode_ahx(bytes: &[u8]) -> Result<(Clock, Vec<i16>)> {
    let mut decoder = ahx_rs::SliceDecoder::new(bytes)?;
    let metadata = decoder.metadata();
    let clock = Clock {
        frames: metadata.samples(),
        sample_rate: metadata.sample_rate(),
        channels: metadata.channels(),
    };
    clock.validate()?;
    let mut samples = Vec::with_capacity(clock.frames as usize);
    while let Some(block) = decoder.next_block()? {
        samples.extend_from_slice(block);
    }
    ensure!(
        samples.len() == clock.frames as usize,
        "AHX frame count changed"
    );
    Ok((clock, samples))
}

fn decode_adx(bytes: &[u8]) -> Result<(Clock, Vec<i16>)> {
    ensure!(
        bytes.len() >= 20 && dol::u16(bytes, 0)? == 0x8000,
        "invalid ADX header"
    );
    let start = usize::from(dol::u16(bytes, 2)?) + 4;
    let clock = Clock {
        channels: u16::from(bytes[7]),
        sample_rate: dol::u32(bytes, 8)?,
        frames: dol::u32(bytes, 12)?,
    };
    clock.validate()?;
    let cutoff = dol::u16(bytes, 16)?;
    let version = dol::u16(bytes, 18)?;
    ensure!(
        (2..=4).contains(&bytes[4])
            && bytes[5..7] == [18, 4]
            && matches!(version, 0x0300 | 0x0400 | 0x0500)
            && u32::from(cutoff) < clock.sample_rate / 2,
        "unsupported ADX profile"
    );
    let minimum = if version == 0x0400 { 0x20 } else { 0x14 };
    ensure!(
        start >= minimum + 6 && bytes.get(start - 6..start) == Some(b"(c)CRI"),
        "invalid ADX data offset"
    );
    let channels = usize::from(clock.channels);
    let blocks = (clock.frames as usize).div_ceil(32);
    let data = bytes.get(start..).context("truncated ADX header")?;
    ensure!(data.len() >= blocks * channels * 18, "truncated ADX audio");
    let z = (std::f32::consts::TAU * f32::from(cutoff) / clock.sample_rate as f32).cos();
    let a = std::f32::consts::SQRT_2 - z;
    let b = std::f32::consts::SQRT_2 - 1.;
    let c = (a - ((a + b) * (a - b)).sqrt()) / b;
    let coefficients = [(c * 8192.) as i16 as i32, (c * c * -4096.) as i16 as i32];
    let mut history = [[0i32; 2]; 2];
    if version == 0x0400 {
        for (channel, values) in history.iter_mut().take(channels).enumerate() {
            for (index, value) in values.iter_mut().enumerate() {
                *value = i32::from(dol::u16(bytes, 0x18 + channel * 4 + index * 2)? as i16);
            }
        }
    }
    let mut samples = Vec::with_capacity(clock.frames as usize * channels);
    for block in 0..blocks {
        let count = (clock.frames as usize - block * 32).min(32);
        let mut pcm = [0i16; 64];
        for channel in 0..channels {
            let at = (block * channels + channel) * 18;
            let frame = &data[at..at + 18];
            let header = dol::u16(frame, 0)?;
            let (scale, [c1, c2]) = match bytes[4] {
                2 => {
                    const FILTERS: [[i32; 2]; 4] =
                        [[0, 0], [0x0f00, 0], [0x1cc0, -0x0d00], [0x1880, -0x0dc0]];
                    let filter = *FILTERS
                        .get(usize::from(header >> 13))
                        .context("invalid ADX predictor")?;
                    (i32::from(header & 0x1fff) + 1, filter)
                }
                4 => {
                    ensure!(header <= 12, "invalid ADX scale");
                    (1 << (12 - header), coefficients)
                }
                _ => {
                    ensure!(header < 0x8000, "early ADX end marker");
                    (i32::from(header) + 1, coefficients)
                }
            };
            let [mut h1, mut h2] = history[channel];
            for index in 0..count {
                let nibble = (frame[2 + index / 2] >> (4 * (1 - index % 2))) & 15;
                let sample = i32::from((nibble << 4) as i8) >> 4;
                let prediction = (c1 * h1 + c2 * h2) >> 12;
                let sample =
                    (sample * scale + prediction).clamp(i32::from(i16::MIN), i32::from(i16::MAX));
                pcm[index * channels + channel] = sample as i16;
                h2 = h1;
                h1 = sample;
            }
            history[channel] = [h1, h2];
        }
        samples.extend_from_slice(&pcm[..count * channels]);
    }
    Ok((clock, samples))
}

fn write_wave(path: &Path, clock: Clock, samples: &[i16]) -> Result<()> {
    fs::create_dir_all(path.parent().context("speech wave has no directory")?)?;
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: clock.channels,
            sample_rate: clock.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

pub(crate) fn write_adx_wave(bytes: &[u8], path: &Path) -> Result<()> {
    let (clock, samples) = decode_adx(bytes)?;
    write_wave(path, clock, &samples)
}

fn publish_voice(
    bytes: &[u8],
    name: String,
    library: &Path,
    owners: &mut BTreeMap<String, Source>,
) -> Result<Source> {
    let target = library.join("audio/speech").join(format!("{name}.wav"));
    let (clock, samples) = match bytes.get(4) {
        Some(0x10 | 0x11) => decode_ahx(bytes)?,
        Some(2..=4) => decode_adx(bytes)?,
        _ => anyhow::bail!("unsupported native speech encoding"),
    };
    let source = if samples.iter().all(|&sample| sample == 0) {
        Source::Silence {
            name: name.clone(),
            silence: clock,
        }
    } else {
        Source::Wave(format!("{name}.wav"))
    };
    source.resolve()?;
    if let Some(previous) = owners.get(&name) {
        ensure!(
            *previous == source,
            "voice owner {name} has conflicting definitions"
        );
        if matches!(&source, Source::Wave(_)) {
            let mut existing = hound::WavReader::open(&target)?;
            ensure!(
                existing.spec().sample_rate == clock.sample_rate
                    && existing.spec().channels == clock.channels
                    && existing
                        .samples::<i16>()
                        .collect::<std::result::Result<Vec<_>, _>>()?
                        == samples,
                "voice owner {name} has different audio"
            );
        }
    } else {
        if matches!(&source, Source::Wave(_)) {
            write_wave(&target, clock, &samples)
                .with_context(|| format!("write speech wave {}", target.display()))?;
        }
        owners.insert(name, source.clone());
    }
    Ok(source)
}

/// Convert event dialogue and the nine named overworld lines. Returns the
/// entries for the shared speech mapping; WAVs are written to their final
/// human-readable owners. A selected ID cooks only one line for quick review.
pub fn cook_event_speech(root: &Path, library: &Path, selected: Option<u32>) -> Result<Mappings> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let scenes: BTreeMap<u32, Scene> = serde_json::from_str(include_str!("event-speech.json"))?;
    let mut mappings = BTreeMap::new();
    let mut owners = BTreeMap::<String, Source>::new();
    let mut archives = Vec::new();
    for row in dol::slice(&exe, DIRECTORY, 18 * 12)?.chunks_exact(12) {
        let pointer = dol::u32(row, 0)?;
        if pointer == 0 {
            break;
        }
        let archive = text::string(&exe, pointer)?.to_ascii_lowercase();
        let Some(filename) = archive.strip_prefix("ev/") else {
            continue;
        };
        ensure!(
            filename.starts_with("sce_")
                && filename.ends_with(".afs")
                && !filename.contains(['/', '\\']),
            "unrecognized event archive {archive}"
        );
        let base = dol::u32(row, 4)?;
        ensure!(base & 0xffff == 0, "unaligned event voice base");
        if selected.is_some_and(|id| id & 0xffff0000 != base) {
            continue;
        }
        archives.push((base, filename.to_owned()));
    }
    let total = archives.len();
    for (job, (base, filename)) in archives.into_iter().enumerate() {
        let first = root.join("disc1/files/EV").join(&filename);
        let path = if first.is_file() {
            first
        } else {
            root.join("disc2/files/EV").join(&filename)
        };
        let mut file = fs::File::open(&path)
            .with_context(|| format!("missing GameCube event archive {filename}"))?;
        let members = afs_index(&mut file)?;
        eprintln!(
            "  [{}/{} jobs] Event speech: {filename} ({} lines)",
            job + 1,
            total,
            members.len()
        );
        for (member, entry) in members.iter().enumerate() {
            if selected.is_none() && member % 100 == 0 {
                eprintln!(
                    "    [{}/{} lines] {filename}: {}",
                    member + 1,
                    members.len(),
                    entry.name
                );
            }
            let id = base | u32::try_from(member)?;
            if selected.is_some_and(|wanted| wanted != id) {
                continue;
            }
            let name = event_name(&entry.name, &scenes)?;
            let bytes = read_member(&mut file, entry)?;
            let source = publish_voice(&bytes, name, library, &mut owners)
                .with_context(|| format!("decode event voice {id} ({})", entry.name))?;
            ensure!(
                mappings.insert(id, source).is_none(),
                "duplicate event voice {id}"
            );
        }
    }
    if let Some(id) = selected {
        ensure!(mappings.contains_key(&id), "event voice {id} was not found");
    }
    Ok(mappings)
}

/// Battle streams have both their short playback ID and their full voice ID.
/// Both map to one named recording (or one explicit silence clock).
pub fn cook_battle_speech(root: &Path, library: &Path, selected: Option<u32>) -> Result<Mappings> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let mut declaration = None;
    for row in dol::slice(&exe, DIRECTORY, 18 * 12)?.chunks_exact(12) {
        let pointer = dol::u32(row, 0)?;
        if pointer == 0 {
            break;
        }
        if dol::u16(row, 8)? == 0x78 {
            ensure!(
                text::string(&exe, pointer)?.eq_ignore_ascii_case("BTL/BTLvoice.afs"),
                "battle voice archive has an unexpected source"
            );
            ensure!(
                declaration.replace(dol::u32(row, 4)?).is_none(),
                "duplicate battle voice archive"
            );
        }
    }
    let base = declaration.context("missing battle voice archive")?;
    ensure!(base & 0xffff == 0 && base != 0, "invalid battle voice base");
    if let Some(id) = selected {
        ensure!(
            id < 0x10000 || id & 0xffff0000 == base,
            "voice {id} is not a battle stream"
        );
    }
    let names: BTreeMap<u32, String> = serde_json::from_str(include_str!("battle-speech.json"))?;
    let mut file = fs::File::open(root.join("disc1/files/BTL/btlvoice.afs"))?;
    let members = afs_index(&mut file)?;
    ensure!(
        names.len() == members.len(),
        "battle voice names do not cover the archive"
    );
    let mut owners = BTreeMap::new();
    let mut mappings = BTreeMap::new();
    eprintln!("  [1/1 jobs] Battle speech ({} lines)", members.len());
    for (member, entry) in members.iter().enumerate() {
        if selected.is_none() && member % 100 == 0 {
            eprintln!(
                "    [{}/{} lines] Battle speech: {}",
                member + 1,
                members.len(),
                entry.name
            );
        }
        let short = u32::try_from(member)?;
        let full = base | short;
        if selected.is_some_and(|wanted| wanted != short && wanted != full) {
            continue;
        }
        let name = names
            .get(&short)
            .with_context(|| format!("battle voice {short} lacks a maintained name"))?;
        let name = format!("battle/{name}");
        let bytes = read_member(&mut file, entry)?;
        let source = publish_voice(&bytes, name, library, &mut owners)
            .with_context(|| format!("decode battle voice {short} ({})", entry.name))?;
        ensure!(
            mappings.insert(short, source.clone()).is_none(),
            "duplicate short battle voice {short}"
        );
        ensure!(
            mappings.insert(full, source).is_none(),
            "duplicate full battle voice {full}"
        );
    }
    if let Some(id) = selected {
        ensure!(
            mappings.contains_key(&id),
            "battle voice {id} was not found"
        );
    }
    Ok(mappings)
}

/// One publication point for the complete human-named speech library.
pub fn cook_speech(root: &Path, library: &Path) -> Result<()> {
    let target = library.join(SPEECH);
    let mut mappings = skit_mappings(root, None)?;
    for (id, source) in cook_event_speech(root, library, None)?
        .into_iter()
        .chain(cook_battle_speech(root, library, None)?)
    {
        ensure!(
            mappings.insert(id, source).is_none(),
            "duplicate speech ID {id}"
        );
    }
    for path in mappings
        .values()
        .filter_map(Source::path)
        .collect::<BTreeSet<_>>()
    {
        let path = library.join("audio/speech").join(path);
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("missing speech wave {}", path.display()))?;
        ensure!(
            metadata.file_type().is_file() && metadata.len() >= 44,
            "incomplete speech wave: {}",
            path.display()
        );
    }
    crate::output::json(&target, &mappings)
}

fn native_voice_archives(exe: &[u8]) -> Result<(BTreeMap<u32, String>, u32)> {
    let mut archives = BTreeMap::new();
    let mut battle_base = None;
    for row in dol::slice(exe, DIRECTORY, 18 * 12)?.chunks_exact(12) {
        let pointer = dol::u32(row, 0)?;
        if pointer == 0 {
            break;
        }
        let base = dol::u32(row, 4)?;
        let name = text::string(exe, pointer)?.to_ascii_lowercase();
        ensure!(base & 0xffff == 0, "unaligned native voice base");
        if dol::u16(row, 8)? == 0x78 {
            ensure!(
                battle_base.replace(base).is_none(),
                "duplicate battle voice archive"
            );
        }
        ensure!(
            archives.insert(base, name).is_none(),
            "duplicate native voice base {base}"
        );
    }
    Ok((
        archives,
        battle_base.context("missing native battle voice archive")?,
    ))
}

fn native_voice_file(root: &Path, name: &str) -> Result<PathBuf> {
    let (directory, filename) = name.split_once('/').context("invalid native voice path")?;
    ensure!(
        matches!(directory, "ev" | "cv" | "btl")
            && !filename.contains(['/', '\\'])
            && filename.ends_with(".afs"),
        "invalid native voice path {name}"
    );
    let directory = directory.to_ascii_uppercase();
    let first = root.join("disc1/files").join(&directory).join(filename);
    Ok(if first.is_file() {
        first
    } else {
        root.join("disc2/files").join(&directory).join(filename)
    })
}

/// Enumerate playable native voices without decoding or publishing any media.
/// Battle streams have both short and full IDs.
pub fn speech_ids(root: &Path) -> Result<BTreeSet<u32>> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let (archives, battle_base) = native_voice_archives(&exe)?;
    let mut ids = BTreeSet::new();
    for (base, name) in archives {
        let mut file = fs::File::open(native_voice_file(root, &name)?)?;
        for (member, entry) in afs_index(&mut file)?.iter().enumerate() {
            file.seek(SeekFrom::Start(entry.offset))?;
            let mut header = [0; 16];
            file.read_exact(&mut header)?;
            ensure!(header[..2] == [0x80, 0], "invalid native voice in {name}");
            if dol::u32(&header, 12)? == 0 {
                continue;
            }
            let member = u32::try_from(member)?;
            ids.insert(base | member);
            if base == battle_base {
                ids.insert(member);
            }
        }
    }
    Ok(ids)
}

/// Native 32 kHz AHX speech plays at the GameCube DAC's 32,028 Hz clock.
/// Return only IDs needing that override; ordinary voices use their WAV rate.
pub fn speech_playback_rates(
    root: &Path,
    ids: impl IntoIterator<Item = u32>,
) -> Result<BTreeMap<u32, u32>> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let (archives, battle_base) = native_voice_archives(&exe)?;
    let mut selected = BTreeMap::<u32, Vec<(u32, usize)>>::new();
    for id in ids {
        let base = if id < 0x10000 {
            battle_base
        } else {
            id & 0xffff0000
        };
        ensure!(
            archives.contains_key(&base),
            "voice {id} has no native archive"
        );
        selected
            .entry(base)
            .or_default()
            .push((id, (id & 0xffff) as usize));
    }
    let mut rates = BTreeMap::new();
    for (base, ids) in selected {
        let name = &archives[&base];
        let mut file = fs::File::open(native_voice_file(root, name)?)
            .with_context(|| format!("missing native voice archive {name}"))?;
        let members = afs_index(&mut file)?;
        for (id, member) in ids {
            let entry = members
                .get(member)
                .with_context(|| format!("voice {id} is absent from {name}"))?;
            file.seek(SeekFrom::Start(entry.offset))?;
            let mut header = [0; 20];
            file.read_exact(&mut header)?;
            ensure!(header[..2] == [0x80, 0], "invalid native voice {id}");
            if matches!(header[4], 0x10 | 0x11) && dol::u32(&header, 8)? == 32_000 {
                rates.insert(id, 32_028);
            }
        }
    }
    Ok(rates)
}
