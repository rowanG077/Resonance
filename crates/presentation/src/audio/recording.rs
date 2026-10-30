//! Shared song recordings, captured once with their effective mod-selected files.
use anyhow::{Context, Result, ensure};
use resonance_audio::recording::{Player, Prepared};
use resonance_content::{library::Snapshot, music};
use std::sync::Arc;

pub(crate) struct Recording {
    main: Arc<Prepared>,
    pub(crate) gains: [f32; 128],
}

impl Recording {
    #[cfg(test)]
    pub(crate) fn test(
        pcm: &[[i16; 2]],
        repeat: Option<std::ops::Range<u32>>,
    ) -> Result<(Arc<Self>, std::fs::File)> {
        let mut file = tempfile::tempfile()?;
        let mut wave = hound::WavWriter::new(
            &mut file,
            hound::WavSpec {
                channels: 2,
                sample_rate: resonance_audio::SOURCE_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for &sample in pcm.iter().flatten() {
            wave.write_sample(sample)?;
        }
        wave.finalize()?;
        if let Some(repeat) = repeat {
            resonance_asset_writer::wav::append_loop(
                &mut file,
                resonance_audio::SOURCE_RATE,
                repeat,
            )?;
        }
        let main = Prepared::new(file.try_clone()?)?;
        Ok((
            Arc::new(Self {
                main,
                gains: std::array::from_fn(|i| i as f32 / 127.),
            }),
            file,
        ))
    }

    pub(crate) fn capture(
        snapshot: &mut Snapshot,
        path: &str,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<Self>> {
        let captured = music::capture(snapshot, path, cancelled)
            .with_context(|| format!("music recording {path}"))?;
        let main = Prepared::new(captured).with_context(|| format!("music WAV {path}"))?;
        // Baking removes the synthesizer that used to apply this curve. Keep
        // the user's music slider on the same scale as the live effect mixer.
        #[derive(serde::Deserialize)]
        struct Tables {
            mix: resonance_audio::mix::Tables,
        }
        let tables: Tables =
            snapshot.json(resonance_audio::effect::recipe::TABLES, 4 * 1024 * 1024)?;
        tables.mix.validate()?;
        let gains = std::array::from_fn(|i| tables.mix.volume[i]);
        ensure!(!cancelled(), "music preparation cancelled");
        Ok(Arc::new(Self { main, gains }))
    }

    pub(crate) fn start(&self) -> Result<Player> {
        self.main.start(resonance_audio::SOURCE_RATE)
    }
}
