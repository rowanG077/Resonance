//! Shared battle cues select the native recordings through ordinary mappings.
use crate::{audio_effects, output, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_audio::{document, spatial_controls},
    library::Mappings,
};
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) const CUES: [(u16, u16); 20] = [
    (38, 0x01f),
    (46, 0x115),
    (54, 0x11d),
    (65, 0x104),
    (66, 0x105),
    (69, 0x106),
    (72, 0x107),
    (73, 0x108),
    (76, 0x143),
    (77, 0x136),
    (80, 0x01b),
    (81, 0x109),
    (95, 0x11f),
    (108, 0x01c),
    (109, 0x12e),
    (122, 0x130),
    (123, 0x12f),
    (126, 0x12c),
    (137, 0x13b),
    (138, 0x123),
];

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    // Verify selectors in the native contact, casting and movement consumers.
    for (address, opcode, expected) in [
        (0x46595a, 0xbf, 0x115),
        (0x465a06, 0xbf, 0x11d),
        (0x465992, 0xbf, 0x104),
        (0x465948, 0xbf, 0x105),
        (0x4080d5, 0xba, 0x106),
        (0x425bf6, 0xb9, 0x107),
        (0x46503d, 0xba, 0x108),
        (0x4322f2, 0xba, 0x136),
        (0x410802, 0xbf, 0x109),
        (0x412133, 0xbf, 0x11f),
        (0x4260d4, 0xba, 0x12e),
        (0x426de4, 0xba, 0x130),
        (0x427822, 0xba, 0x12f),
        (0x41c778, 0xbf, 0x12c),
        (0x407ce6, 0xbf, 0x123),
    ] {
        let code = image.slice(address, 5)?;
        ensure!(
            code[0] == opcode && pe::word(code, 1)? == expected,
            "battle sound selector changed at {address:#x}"
        );
    }
    let path = library.join("audio/mappings.json");
    let existing: Mappings = serde_json::from_slice(&fs::read(&path)?)?;
    let mut mappings = existing
        .iter()
        .map(|(id, path)| (id, path.to_owned()))
        .collect::<BTreeMap<_, _>>();
    for (cue, packed) in CUES {
        let recording = existing
            .target(u32::from(audio_effects::id(packed)?))?
            .to_owned();
        if let Some(previous) = mappings.insert(u32::from(cue), recording.clone()) {
            ensure!(
                previous == recording,
                "battle cue {cue} conflicts with another service"
            );
        }
    }
    output::json(&path, &Mappings::new(mappings)?)?;
    let double = |address| -> Result<f32> {
        let value = f64::from_le_bytes(image.slice(address, 8)?.try_into()?) as f32;
        ensure!(value.is_finite(), "non-finite battle audio constant");
        Ok(value)
    };
    let scalar =
        |address| -> Result<f32> { Ok(f32::from_bits(pe::word(image.slice(address, 4)?, 0)?)) };
    let origin = double(0x883270)?;
    let source_voice_origin = double(0x882d48)?;
    let source_voice_scale = double(0x882950)?;
    ensure!(
        source_voice_origin > 0. && source_voice_scale > 0.,
        "invalid voice projection"
    );
    let definition = document::Definition {
        speech_rates: Vec::new(),
        reverbs: None,
        effect_spatial: spatial_controls([
            origin,
            double(0x882fb0)?,
            double(0x882db8)?,
            0.,
            double(0x8829c0)?,
        ])?,
        // Normalize the port's voice projection to the shared 640-wide space.
        voice_spatial: spatial_controls([
            origin,
            double(0x882dc0)? * origin / source_voice_origin / source_voice_scale,
            double(0x882db8)?,
            scalar(0x882db0)?,
            scalar(0x882da4)?,
        ])?,
    };
    output::json(&library.join(document::PATH), &definition).context("battle audio settings")
}
