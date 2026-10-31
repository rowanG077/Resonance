//! GameCube arrangements directly into shared, named PCM music.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_audio::{SOURCE_RATE, music_voice, package::Loaded, sequence::recording};
use resonance_audio_cook::{bank::Bank, compile, song::Song};
use resonance_content::library::Mappings;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::BufWriter,
    path::{Path, PathBuf},
    sync::Arc,
};
use symphonia_constants::MusicId;

const SONGS: u32 = 0x801f_982c;
const REVERB_SELECTORS: u32 = 0x8021_08b0;

pub fn cook_music(root: &Path, library: &Path, selected: Option<u16>) -> Result<()> {
    let disc = root.join("disc1");
    let exe = fs::read(disc.join("sys/main.dol"))?;
    let songs = catalogue(&disc, &exe, selected)?;
    let instruments = fs::read(disc.join("files/S/inst.snd"))?;
    let bank = Bank::parse(&instruments)?;
    let sustains = sustains(&exe)?;
    let tables = tables(&exe)?;
    let field_reverbs = reverbs(&exe, 1)?;
    let output = library.join("audio/music");
    fs::create_dir_all(&output)?;
    let mut rendered = BTreeSet::new();
    let mut mappings = BTreeMap::new();
    let mut jobs = Vec::new();
    for (id, (name, source)) in songs {
        if rendered.insert(name.clone()) {
            jobs.push((id, name.clone(), source));
        }
        mappings.insert(u32::from(id), format!("{name}.wav"));
    }
    crate::parallel::for_each(&jobs, |index, (id, name, source)| {
        let target = output.join(format!("{name}.wav"));
        eprintln!("  [{}/{} jobs] Music: {name} ({id})", index + 1, jobs.len());
        let setup = bank.music_setup(0, *id)?;
        let preset = dol::slice(&exe, REVERB_SELECTORS + u32::from(*id), 1)?[0];
        let effects = match preset {
            0 => field_reverbs,
            1 | 2 => reverbs(&exe, preset)?,
            _ => anyhow::bail!("unsupported music reverb preset {preset}"),
        };
        render_song(source, &target, &bank, &setup, &sustains, &tables, effects)
            .with_context(|| format!("render music {id} ({name})"))?;
        Ok(())
    })?;
    if selected.is_none() {
        cook_unmapped(&disc, &output, &bank, &sustains, &tables)?;
    }
    let mapping = output.join("mappings.json");
    crate::output::json(&mapping, &Mappings::new(mappings)?)
}

/// Preserve arrangements absent from the native ID table without inventing IDs.
pub fn cook_extra_music(root: &Path, library: &Path) -> Result<()> {
    let disc = root.join("disc1");
    let exe = fs::read(disc.join("sys/main.dol"))?;
    let instruments = fs::read(disc.join("files/S/inst.snd"))?;
    let bank = Bank::parse(&instruments)?;
    cook_unmapped(
        &disc,
        &library.join("audio/music"),
        &bank,
        &sustains(&exe)?,
        &tables(&exe)?,
    )
}

fn cook_unmapped(
    disc: &Path,
    output: &Path,
    bank: &Bank<'_>,
    sustains: &resonance_audio_cook::parameters::Sustains,
    tables: &music_voice::Tables,
) -> Result<()> {
    let output = output.join("unmapped");
    fs::create_dir_all(&output)?;
    // All 112 group-0 setup records have identical channels and share these pages.
    let setup = bank.music_setup(0, 0)?;
    // No native music ID supplies a reverb selector for these arrangements.
    let dry = [[0., 0., 0.01, 0., 0.]; 2];
    let arrangements = [
        ("bgm_etc004.song", "bgm-etc004.wav"),
        ("temp2.song", "temp2.wav"),
        ("temp3.song", "temp3.wav"),
        ("temp4.song", "temp4.wav"),
    ];
    for (index, &(source, name)) in arrangements.iter().enumerate() {
        let target = output.join(name);
        eprintln!(
            "  [{}/{} jobs] Unmapped music: {name}",
            index + 1,
            arrangements.len()
        );
        render_song(
            &disc.join("files/S").join(source),
            &target,
            bank,
            &setup,
            sustains,
            tables,
            dry,
        )
        .with_context(|| format!("render unmapped music {source}"))?;
    }
    Ok(())
}

fn render_song(
    source: &Path,
    target: &Path,
    bank: &Bank<'_>,
    setup: &resonance_audio_cook::bank::MusicSetup,
    sustains: &resonance_audio_cook::parameters::Sustains,
    tables: &music_voice::Tables,
    effects: [[f32; 5]; 2],
) -> Result<()> {
    let song = Song::parse(&fs::read(source)?)?;
    let (resources, score) = compile::music(bank, &song, setup, sustains)
        .with_context(|| format!("compile music from {}", source.display()))?;
    write_song(
        target,
        Arc::new(Loaded::new(resources, score, tables.clone(), effects)?),
        effects,
    )
}

fn catalogue(
    disc: &Path,
    exe: &[u8],
    selected: Option<u16>,
) -> Result<BTreeMap<u16, (String, PathBuf)>> {
    let mut songs = BTreeMap::new();
    for row in dol::slice(exe, SONGS, 113 * 8)?.chunks_exact(8) {
        let id = dol::u16(row, 0)?;
        if id == u16::MAX {
            break;
        }
        if selected.is_some_and(|wanted| wanted != id) {
            continue;
        }
        let name = MusicId::try_from(id)?
            .resource_name()
            .with_context(|| format!("music {id} has no maintained name"))?
            .to_owned();
        let source = text::string(exe, dol::u32(row, 4)?)?;
        let (directory, file) = source
            .split_once('/')
            .context("music source has no directory")?;
        ensure!(
            directory.eq_ignore_ascii_case("s")
                && !file.contains(['/', '\\'])
                && file.ends_with(".song"),
            "invalid music source {source:?}"
        );
        let path = disc.join("files/S").join(file);
        ensure!(
            path.is_file(),
            "music {id} source is missing: {}",
            path.display()
        );
        ensure!(
            songs.insert(id, (name, path)).is_none(),
            "duplicate music ID {id}"
        );
    }
    ensure!(!songs.is_empty(), "no requested native music");
    if selected.is_none() {
        ensure!(songs.len() == 112, "incomplete native music catalogue");
    }
    Ok(songs)
}

fn write_song(path: &Path, loaded: Arc<Loaded>, effects: [[f32; 5]; 2]) -> Result<()> {
    let parent = path.parent().context("music output has no parent")?;
    fs::create_dir_all(parent)?;
    let mut output = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    let mut writer = hound::WavWriter::new(
        BufWriter::new(&mut output),
        hound::WavSpec {
            channels: 2,
            sample_rate: SOURCE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    let mut scratch = tempfile::tempfile()?;
    let rendered = recording::render(loaded, effects, &mut writer, &mut scratch, || false)?;
    writer.finalize()?;
    if let Some(repeat) = rendered.repeat {
        resonance_asset_writer::wav::append_loop(
            &mut output,
            SOURCE_RATE,
            u32::try_from(repeat.start)?..u32::try_from(repeat.end)?,
        )?;
    }
    let wave = hound::WavReader::open(path)?;
    ensure!(
        u64::from(wave.duration()) == rendered.frames,
        "music frame count changed"
    );
    Ok(())
}

fn float(exe: &[u8], address: u32) -> Result<f32> {
    Ok(f32::from_be_bytes(dol::slice(exe, address, 4)?.try_into()?))
}

pub(crate) fn reverbs(exe: &[u8], preset: u8) -> Result<[[f32; 5]; 2]> {
    let main = match preset {
        1 => [
            0x8035_c47c,
            0x8035_c480,
            0x8035_c470,
            0x8035_c478,
            0x8035_c474,
        ],
        2 => [
            0x8035_c480,
            0x8035_c480,
            0x8035_c484,
            0x8035_c478,
            0x8035_c488,
        ],
        _ => anyhow::bail!("invalid music reverb preset {preset}"),
    };
    let auxiliary = [
        0x8035_c490,
        0x8035_c4ac,
        0x8035_c490,
        0x8035_c47c,
        0x8035_c4a8,
    ];
    let mut buses = [[0.; 5]; 2];
    for (bus, addresses) in buses.iter_mut().zip([main, auxiliary]) {
        for (value, address) in bus.iter_mut().zip(addresses) {
            *value = float(exe, address)?;
        }
    }
    Ok(buses)
}

pub(crate) fn sustains(exe: &[u8]) -> Result<resonance_audio_cook::parameters::Sustains> {
    let inverse = dol::slice(exe, 0x802a_8a24, 1024)?.try_into()?;
    let mut curve = [0.; 129];
    for (index, value) in curve.iter_mut().enumerate() {
        *value = float(exe, 0x802a_8e24 + index as u32 * 4)?;
    }
    resonance_audio_cook::parameters::Sustains::new(inverse, curve)
}

pub(crate) fn tables(exe: &[u8]) -> Result<music_voice::Tables> {
    let mut volume = [0.; 129];
    let mut alternate_volume = [0.; 129];
    let mut pan = [0.; 4];
    let mut attenuation = [0; 194];
    let mut sine = [0; 1024];
    let mut tremolo = [0.; 5];
    let mut left_delay = [0; 128];
    for (i, value) in volume.iter_mut().enumerate() {
        *value = float(exe, 0x802a_aa98 + i as u32 * 4)?;
    }
    for (i, value) in alternate_volume.iter_mut().enumerate() {
        *value = float(exe, 0x802a_8e24 + i as u32 * 4)?;
    }
    for (i, value) in pan.iter_mut().enumerate() {
        *value = float(exe, 0x802a_aa98 + (129 + i) as u32 * 4)?;
    }
    for (i, value) in attenuation.iter_mut().enumerate() {
        *value = dol::u16(dol::slice(exe, 0x802a_88a0 + i as u32 * 2, 2)?, 0)?;
    }
    for (i, value) in sine.iter_mut().enumerate() {
        *value = dol::u16(dol::slice(exe, 0x802a_ad50 + i as u32 * 2, 2)?, 0)? as i16;
    }
    for (i, value) in tremolo.iter_mut().enumerate() {
        *value = float(exe, 0x8035_e1d8 + i as u32 * 4)?;
    }
    for (i, value) in left_delay.iter_mut().enumerate() {
        *value = u8::try_from(dol::u16(
            dol::slice(exe, 0x801e_0618 + i as u32 * 2, 2)?,
            0,
        )?)?;
    }
    Ok(music_voice::Tables {
        mix: resonance_audio::mix::Tables {
            volume,
            alternate_volume,
            pan,
            volume_16_scale: float(exe, 0x8035_e1d4)?,
            controller_14_scale: float(exe, 0x8035_e1ec)?,
            pan_16_scale: float(exe, 0x8035_e2a8)?,
            spatial: Some(resonance_audio::mix::Spatial {
                pan_scale: float(exe, 0x8035_e2bc)?,
                left_delay,
            }),
        },
        dls: resonance_audio::dls::Tables { attenuation },
        modulation: resonance_audio::modulation::Tables { sine, tremolo },
        coefficients: resonance_audio::resample::Coefficients::from_be_bytes(
            &resonance_audio_cook::interpolation::coefficients(),
        )?,
    })
}
