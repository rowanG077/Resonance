//! Offline diagnostic for pitched instrument PCM, before envelope and studio gain.
use super::{Workspace, hash_file, write_json};
use anyhow::{Result, ensure};
use resonance_asset_writer::wav::write_pcm16;
use resonance_audio_cook::{bank::Bank, pitch, render, resample};
use serde_json::json;
use std::{fs, path::Path};

pub struct PitchedSampleOptions<'a> {
    pub extracted: &'a Path,
    pub bank: &'a Path,
    pub coefficients: &'a Path,
    pub output: &'a Path,
    pub id: u16,
    pub key: Option<u8>,
    pub cents: i8,
    pub frames: u32,
    pub coefficient_set: u8,
}

pub(super) fn pitch_tables(bytes: &[u8]) -> Result<pitch::Tables> {
    let float = |address| -> Result<f32> {
        Ok(f32::from_be_bytes(
            crate::dol::slice(bytes, address, 4)?.try_into()?,
        ))
    };
    let mut up = [0.; 128];
    let mut down = [0.; 128];
    for (values, base) in [(&mut up, 0x802a_84a0), (&mut down, 0x802a_86a0)] {
        for (i, value) in values.iter_mut().enumerate() {
            *value = float(base + i as u32 * 4)?;
        }
    }
    let tables = pitch::Tables {
        up,
        down,
        semitone: float(0x8035_e250)?,
    };
    tables.validate()?;
    Ok(tables)
}

pub fn render_pitched_sample(options: PitchedSampleOptions<'_>) -> Result<()> {
    ensure!(
        (1..=render::SYNTHESIS_RATE * 10).contains(&options.frames),
        "sample render limit must be 1 sample to 10 seconds"
    );
    ensure!(options.coefficient_set < 4, "invalid coefficient set");
    let workspace = Workspace::open(options.extracted, options.output)?;
    let _publications = crate::publication::Session::start_if_needed(options.output)?;
    let executable_bytes = fs::read(workspace.extracted.join("sys/main.dol"))?;
    let bank_bytes = fs::read(options.bank)?;
    let coefficient_bytes = fs::read(options.coefficients)?;
    let coefficients = resample::Coefficients::from_be_bytes(&coefficient_bytes)?;
    let bank = Bank::parse(&bank_bytes)?;
    let sample = bank.sample(options.id)?;
    let key = options.key.unwrap_or(sample.key);
    let pitch = (i32::from(key) << 16) + (i32::from(options.cents) << 16) / 100;
    let ratio = pitch_tables(&executable_bytes)?.ratio(pitch, &sample)?;
    let mut cursor = resample::SampleCursor::new(&sample)?;
    let mut source = resample::Resampler::new(
        resample::Mode::Polyphase(&coefficients.0[usize::from(options.coefficient_set)]),
        ratio,
    )?;
    let name = format!(
        "sample-{}-key-{key}-cents-{}.wav",
        options.id, options.cents
    );
    let path = workspace.output.join(&name);
    let temporary = crate::temporary_path(&path);
    let stereo = (0..options.frames).flat_map(|_| [source.next_sample(|| cursor.next_sample()); 2]);
    write_pcm16(&temporary, 2, render::PLAYBACK_RATE, stereo)?;
    crate::publication::install(&temporary, &path, &hash_file(&temporary)?)?;
    write_json(
        &path.with_extension("json"),
        &json!({
        "version":1,"renderer":"resonance-audio-cook","audio_device":false,
        "renderer_sha256":hash_file(&std::env::current_exe()?)?,"oracle_accepted":false,
            "bank_sha256":crate::digest(&bank_bytes),
            "executable_sha256":crate::digest(&executable_bytes),
            "coefficients_sha256":crate::digest(&coefficient_bytes),
            "sample_id":options.id,"key":key,"cents":options.cents,
            "source_key":sample.key,"source_rate":sample.rate,
            "source_mode":0,"coefficient_set":options.coefficient_set,"ratio_16_16":ratio,
            "loop_start":sample.loop_start,"loop_frames":sample.loop_length,
            "envelope_applied":false,"studio_effects_applied":false,
            "asset":{"path":name,"sha256":hash_file(&path)?,"frames":options.frames,
                "channels":2,"sample_rate":render::PLAYBACK_RATE},
        }),
    )?;
    println!(
        "Rendered sample {} at key {key} to {} without playback",
        options.id,
        path.display()
    );
    Ok(())
}
