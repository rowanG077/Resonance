//! Shared song recordings, captured once with their effective mod-selected files.
use anyhow::{Context as ContextExt, Result, ensure};
use resonance_audio::recording::{Player, Prepared};
use resonance_content::{field_audio::mix, library::Snapshot, music, preparation::JSON_LIMIT};
use std::{
    collections::BTreeMap,
    fs::{File, Metadata},
    sync::{Arc, Weak},
    time::SystemTime,
};

#[derive(PartialEq, Eq)]
struct Identity {
    length: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    #[cfg(unix)]
    file: (u64, u64),
}

impl Identity {
    fn new(metadata: &Metadata) -> Option<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        let created = metadata.created().ok();
        #[cfg(not(unix))]
        if created.is_none() {
            return None;
        }
        Some(Self {
            length: metadata.len(),
            modified: metadata.modified().ok()?,
            created,
            #[cfg(unix)]
            file: (metadata.dev(), metadata.ino()),
        })
    }
}

struct Entry {
    identity: Identity,
    mix_bytes: Arc<[u8]>,
    recording: Weak<Recording>,
}

/// Only startup or live audio owns recordings. Reuse never accumulates whole songs.
#[derive(Default)]
pub(crate) struct Cache(BTreeMap<String, Entry>);

impl Cache {
    pub(crate) fn capture(
        &mut self,
        snapshot: &mut Snapshot,
        path: &str,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<Recording>> {
        let file = music::capture(snapshot, path, cancelled)?;
        let identity = Identity::new(&file.metadata()?);
        let mix_bytes = snapshot.read(mix::PATH, JSON_LIMIT)?;
        self.0.retain(|_, entry| entry.recording.strong_count() > 0);
        if let Some(entry) = self.0.get(path)
            && identity.as_ref() == Some(&entry.identity)
            && resonance_content::same_bytes(&mix_bytes, &entry.mix_bytes)
            && let Some(recording) = entry.recording.upgrade()
        {
            ensure!(!cancelled(), "music preparation cancelled");
            return Ok(recording);
        }
        let recording = Recording::prepare(file, &mix_bytes, path, cancelled)?;
        if let Some(identity) = identity {
            self.0.insert(
                path.into(),
                Entry {
                    identity,
                    mix_bytes,
                    recording: Arc::downgrade(&recording),
                },
            );
        }
        Ok(recording)
    }

    pub(crate) fn adopt(&mut self, other: Self) {
        self.0.extend(other.0);
    }
}

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
        let mix_bytes = snapshot.read(mix::PATH, JSON_LIMIT)?;
        Self::prepare(captured, &mix_bytes, path, cancelled)
    }

    fn prepare(
        captured: File,
        mix_bytes: &[u8],
        path: &str,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<Self>> {
        let main = Prepared::new(captured).with_context(|| format!("music WAV {path}"))?;
        let mix = serde_json::from_slice::<mix::Mix>(mix_bytes)
            .with_context(|| format!("decode {}", mix::PATH))?;
        let gains = mix
            .music_gains
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid music volume curve"))?;
        ensure!(!cancelled(), "music preparation cancelled");
        Ok(Arc::new(Self { main, gains }))
    }

    pub(crate) fn start(&self) -> Result<Player> {
        self.main.start(resonance_audio::SOURCE_RATE)
    }
}
