//! Shared song recordings, captured once with their effective mod-selected files.
use anyhow::{Context, Result, ensure};
use resonance_audio::recording::{Player, Prepared};
use resonance_content::{library::Snapshot, music};
use std::sync::Arc;

pub(crate) struct Recording {
    main: Arc<Prepared>,
    battle: Option<Arc<Prepared>>,
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
        let main = Prepared::new(file.try_clone()?, repeat)?;
        Ok((Arc::new(Self { main, battle: None }), file))
    }

    pub(crate) fn capture(
        snapshot: &mut Snapshot,
        path: &str,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<Self>> {
        let captured = music::capture(snapshot, path, cancelled)
            .with_context(|| format!("music recording {path}"))?;
        let prepare = |file, path: &str, repeat: Option<music::Loop>| {
            Prepared::new(file, repeat.map(|r| r.start_frame..r.end_frame))
                .with_context(|| format!("music WAV {path}"))
        };
        let main = prepare(
            captured.audio,
            &captured.definition.audio,
            captured.definition.repeat,
        )?;
        let battle = captured
            .battle
            .zip(captured.definition.battle)
            .map(|(file, variant)| prepare(file, &variant.audio, variant.repeat))
            .transpose()?;
        ensure!(!cancelled(), "music preparation cancelled");
        Ok(Arc::new(Self { main, battle }))
    }

    pub(crate) fn start(&self, battle: bool) -> Result<Player> {
        let track = if battle {
            self.battle.as_ref().unwrap_or(&self.main)
        } else {
            &self.main
        };
        track.start(resonance_audio::SOURCE_RATE)
    }
}
