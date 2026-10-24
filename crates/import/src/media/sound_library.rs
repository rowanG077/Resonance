//! Prepare native sound programs in memory for the current editable writer.
use anyhow::Result;
use resonance_audio::data::Score;
use resonance_audio_cook::{
    bank::{Bank, Page},
    decode, instrument,
};
#[cfg(test)]
use std::{fs, path::Path};

pub(crate) fn sound(
    bank: &Bank<'_>,
    id: u16,
    sustains: &resonance_audio_cook::parameters::Sustains,
) -> Result<(decode::Resources, Score)> {
    let sound = bank.sound(id)?;
    let notes = instrument::resolve(
        bank,
        Page {
            object: sound.object,
            priority: sound.priority,
            max_voices: sound.max_voices,
        },
        sound.key,
        sound.volume,
        sound.pan,
    )?;
    let resources = decode::programs(bank, notes.iter().map(|note| note.macro_id), sustains)?;
    Ok((resources, super::sound_score(id, Some(notes))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires both extracted discs; CPU package and PCM validation"]
    fn native_sounds_close_instrument_dependencies_and_preserve_sample_data() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let banks = [("S/se.snd", vec![1, 2, 3, 4]), ("S/se_ev00.snd", vec![425])];
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let output = tempfile::tempdir()?;
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let reverbs = super::super::music::title_reverbs(&executable)?;
            let pools = crate::media::library::Pools::read(&extracted)?;
            for (source, ids) in &banks {
                let bytes = fs::read(extracted.join("files").join(source))?;
                let bank = pools.bank(&bytes)?;
                for &id in ids {
                    let (resources, score) = sound(&bank, id, &pools.sustains)?;
                    let loaded = resonance_audio::package::Loaded::new(
                        resources,
                        score,
                        super::super::synthesis_tables(&executable)?,
                        reverbs,
                    )?;
                    assert_eq!(loaded.reverbs(), reverbs);
                    let native = loaded.resources();
                    assert!(!native.samples.is_empty());
                    for (&sample_id, sample) in &native.samples {
                        let decoded = bank.sample(sample_id)?;
                        assert_eq!(sample.key, decoded.key);
                        assert_eq!(sample.rate, decoded.rate);
                        assert_eq!(sample.loop_start, decoded.loop_start);
                        assert_eq!(sample.loop_length, decoded.loop_length);
                        assert_eq!(
                            sample.pcm, decoded.pcm,
                            "disc{disc} {source} sound {id} sample {sample_id}"
                        );
                        assert_eq!(sample.loop_pcm, decoded.loop_pcm);
                    }
                }
            }
            super::super::sound_buses::render_sound_buses(
                &extracted,
                &extracted.join("files/S/se_ev00.snd"),
                425,
                output.path(),
            )?;
            let mut wave = hound::WavReader::open(output.path().join("sound-425-direct.wav"))?;
            assert!(wave.samples::<i16>().any(|s| s.is_ok_and(|s| s != 0)));
        }
        Ok(())
    }
}
