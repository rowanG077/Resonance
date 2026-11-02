//! PHD/PBD and streamed effects write the same named recordings as GC cues.
use crate::{audio_streams, output, package::Package, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_audio::effect::{Definition, Loop, Wave};
use resonance_content::{field_audio::ServiceCue, library::Mappings};
use std::{collections::BTreeMap, fs, ops::Range, path::Path};

pub(crate) fn id(native: u16) -> Result<u16> {
    match native {
        0..=0x43ff => Ok(native + 512),
        0x8000..=0x80ff => Ok(0x7000 + (native & 255)),
        _ => anyhow::bail!("unsupported native effect ID {native:#x}"),
    }
}

pub(crate) fn recording(
    library: &Path,
    document: &str,
    audio: &str,
    repeat: Option<Range<u32>>,
    gain: f32,
) -> Result<()> {
    output::json(
        &library.join(document),
        &Definition::Wave(Wave {
            audio: audio.into(),
            repeat: repeat.map(|range| Loop {
                start_frame: range.start,
                end_frame: range.end,
            }),
            gain,
            sends: [0.; 2],
            release_ms: 5,
            reverbs: None,
        }),
    )
}

/// Shared services select the corresponding native recording during import.
/// Rebinding these aliases does not decode or duplicate their WAV data.
pub(crate) fn bind_services(source: &Source, library: &Path) -> Result<()> {
    use ServiceCue::*;
    const CUES: [(ServiceCue, u16); 32] = [
        (Navigate, 28),
        (Confirm, 27),
        (Cancel, 26),
        (Error, 7),
        (TreasureReward, 25),
        (TreasureOpen, 5),
        (TreasureBag, 4),
        (Door, 6),
        (RingPrepare, 9),
        (RingFire, 24),
        (MenuOpen, 8),
        (Page, 31),
        (Recovery, 29),
        (BattleEntry, 32),
        (Remedy, 30),
        (RingElectric, 0x1d16),
        (RingLightning, 0x2a1e),
        (RingIce, 0x2b14),
        (RingShatter, 0x2b0c),
        (RingEarthquake, 0x2c09),
        (RingDarkness, 0x2d0c),
        (RingSound, 0x2e21),
        (RingBubble, 0x3100),
        (RingWind, 0x2230),
        (RingWater, 0x2126),
        (RingBombPlace, 0x3613),
        (RingBombBlast, 0x3605),
        (RingBombDebris, 0x2c0e),
        (RingMana, 0x1c0a),
        (RingRadar, 0x200e),
        (RingSplash, 0x2123),
        (RingShrink, 10),
    ];
    let image = pe::Image::parse(&source.executable)?;
    // Ring mode 3 toggles the size at 0x4cc87c. Only the shrink branch makes
    // the second SYS sound request, selecting row 10 at 0x4cc891.
    ensure!(
        image.slice(0x4cc891, 5)? == [0xb9, 10, 0, 0, 0]
            && image.slice(0x4cc89d, 5)? == [0xe8, 0x0e, 0x1d, 6, 0],
        "changed shrink sound selection"
    );
    let path = library.join("audio/mappings.json");
    let entries: Mappings = serde_json::from_slice(&fs::read(&path)?)?;
    let mut mappings = entries
        .iter()
        .map(|(id, path)| (id, path.to_owned()))
        .collect::<BTreeMap<_, _>>();
    for (service, native) in CUES {
        let recording = entries.target(u32::from(id(native)?))?.to_owned();
        mappings.insert(service as u32, recording);
    }
    ensure!(
        ServiceCue::ALL
            .iter()
            .all(|cue| mappings.contains_key(&(*cue as u32))),
        "unbound field service cue"
    );
    output::json(&path, &Mappings::new(mappings)?)
}

/// Some shipped scripts retain selectors beyond their PHD bank. The native
/// playback guard rejects them without audio; represent that in the common
/// explicit silence recipe rather than inventing a replacement recording.
pub(crate) fn bind_unused(source: &Source, library: &Path, mapped: u32) -> Result<()> {
    let native = mapped
        .checked_sub(512)
        .filter(|id| *id <= 0x43ff)
        .context("missing streamed effect")?;
    let image = pe::Image::parse(&source.executable)?;
    ensure!(
        image.slice(0x580943, 10)? == [0x66, 0x39, 0x78, 8, 0x0f, 0x82, 0x98, 0, 0, 0],
        "changed effect bank range guard"
    );
    let bytes = source.require("allphd.pak")?;
    let package = Package::parse(&bytes)?;
    let bank = (native >> 8) as usize;
    let rejected = if bank >= package.len() {
        true
    } else {
        let header = package
            .member(bank)?
            .context("missing effect bank header")?;
        // The native test is strictly greater than the bank's declared count.
        (native & 255) > u32::from(pe::half(header, 8)?)
    };
    ensure!(
        rejected,
        "mapped effect {mapped} is missing a valid native recording"
    );
    output::json(
        &library.join("audio/effects/no-sound.json"),
        &Definition::Silence(resonance_audio::effect::Silence {
            frames: 1,
            sample_rate: 48_000,
        }),
    )?;
    let path = library.join("audio/mappings.json");
    let entries: Mappings = serde_json::from_slice(&fs::read(&path)?)?;
    let mut mappings = entries
        .iter()
        .map(|(id, path)| (id, path.to_owned()))
        .collect::<BTreeMap<_, _>>();
    mappings.insert(mapped, "effects/no-sound.json".into());
    output::json(&path, &Mappings::new(mappings)?)?;
    eprintln!("  Native unused effect: {native:#06x}");
    Ok(())
}

pub(crate) fn cook(
    source: &Source,
    library: &Path,
    mut mappings: BTreeMap<u32, String>,
) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let bytes = source.require("allphd.pak")?;
    let package = Package::parse(&bytes)?;
    ensure!(package.len() == 67, "unexpected effect bank count");
    for bank in 0..package.len() {
        let name = image.text(pe::word(image.slice(0x8ad5a0 + bank as u32 * 4, 4)?, 0)?)?;
        crate::source::validate_name(name)?;
        let owner = name
            .strip_suffix(".PBD")
            .context("effect bank is not PBD")?
            .to_ascii_lowercase()
            .replace('_', "-");
        let bytes = source.require(name)?;
        let header = package.member(bank)?.context("missing effect header")?;
        ensure!(
            header.get(..4) == Some(b"PHDp") && pe::half(header, 6)? == bank as u16,
            "invalid effect bank header"
        );
        let count = usize::from(pe::half(header, 8)?);
        ensure!(count <= 256, "effect bank exceeds ID range");
        let rows = header
            .get(16..16 + count * 80)
            .context("truncated effect rows")?;
        eprintln!("  Effect bank: {name} ({count} recordings)");
        for (index, row) in rows.chunks_exact(80).enumerate() {
            ensure!(pe::half(row, 0)? == index as u16, "unordered effect rows");
            let start = pe::word(row, 60)? as usize;
            let end = start
                .checked_add(pe::word(row, 56)? as usize)
                .context("effect range overflow")?;
            let bytes = bytes.get(start..end).context("effect exceeds bank")?;
            let path = format!("audio/effects/{owner}/effect-{index:03}");
            let repeat = audio_streams::wave(bytes, &library.join(format!("{path}.wav")))
                .with_context(|| format!("decode {name}:{index}"))?;
            recording(
                library,
                &format!("{path}.json"),
                &format!("{path}.wav"),
                repeat,
                f32::from(row[4]) / 127.,
            )?;
            ensure!(
                mappings
                    .insert(
                        u32::from(id((bank as u16) << 8 | index as u16)?),
                        format!("{}.json", path.strip_prefix("audio/").unwrap())
                    )
                    .is_none(),
                "duplicate effect ID"
            );
        }
    }
    // Native menu dispatch selects these SYS rows. Shared service IDs stay stable.
    for (service, native) in [(1, 28), (2, 27), (3, 26), (4, 7), (24, 1), (25, 0), (26, 2)] {
        let path = mappings
            .get(&u32::from(id(native)?))
            .context("missing service cue")?
            .clone();
        ensure!(
            mappings.insert(service, path).is_none(),
            "service cue collision"
        );
    }
    output::json(
        &library.join("audio/mappings.json"),
        &Mappings::new(mappings)?,
    )
}
