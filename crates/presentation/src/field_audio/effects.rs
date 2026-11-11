//! A weak lookup for immutable, validated cues owned by scenes or startup.
use super::{EffectDefinition, PreparedEffect};
use anyhow::Result;
use resonance_audio::package::SampleCache;
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

type Key = (String, [[u32; 5]; 2]);

struct Input {
    path: String,
    limit: usize,
    bytes: Weak<[u8]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_audio::named_tests::{fixture, wav, write};
    use resonance_audio::effect::{
        Definition,
        recipe::{ProgramSource, SharedProgram},
    };
    use resonance_content::{diagnostics::Diagnostics, library::Library, prepared::Files};

    const PATH: &str = "audio/effects/test-cue.json";
    const SHARED: &str = "audio/programs/shared.json";
    const SAMPLE: &str = "audio/instruments/test.wav";
    const REVERBS: [[f32; 5]; 2] = [[0.5, 0.5, 1., 0.5, 0.]; 2];

    fn prepare(
        cache: &mut Cache,
        library: &Library,
        reverbs: [[f32; 5]; 2],
    ) -> Result<(Arc<PreparedEffect>, Files)> {
        let mut snapshot = library.snapshot();
        let effect = cache.prepare(
            PATH,
            reverbs,
            &mut |path, limit| snapshot.read(path, limit),
            &mut SampleCache::default(),
        )?;
        Ok((
            effect,
            Files::from_snapshot(snapshot, || false, Diagnostics::new(true))?,
        ))
    }

    #[test]
    fn prepared_cues_share_exact_inputs_and_invalidate_nested_edits_and_reverbs() -> Result<()> {
        let root = fixture()?;
        let mut definition: Definition =
            serde_json::from_slice(&std::fs::read(root.path().join(PATH))?)?;
        let Definition::Synthesized(recipe) = &mut definition else {
            unreachable!()
        };
        let ProgramSource::Inline(instructions) = recipe.programs.remove(&1).unwrap() else {
            unreachable!()
        };
        let mut shared = SharedProgram {
            instructions,
            samples: BTreeMap::new(),
        };
        recipe
            .programs
            .insert(1, ProgramSource::Shared(SHARED.into()));
        write(root.path(), PATH, &serde_json::to_vec(&definition)?)?;
        write(root.path(), SHARED, &serde_json::to_vec(&shared)?)?;
        let library = Library::open(root.path(), &[])?;
        let mut cache = Cache::default();
        let mut owners = Vec::new();
        let (mut previous, files) = prepare(&mut cache, &library, REVERBS)?;
        owners.push(files);
        let (same, files) = prepare(&mut cache, &library, REVERBS)?;
        owners.push(files);
        assert!(Arc::ptr_eq(&previous, &same));
        assert_eq!(
            owners[0].keys().collect::<Vec<_>>(),
            owners[1].keys().collect::<Vec<_>>()
        );
        for edit in 0..4 {
            match edit {
                0 => {
                    shared.instructions.insert(
                        0,
                        resonance_audio::data::Command::VolumeControl { value: 16380 },
                    );
                    write(root.path(), SHARED, &serde_json::to_vec(&shared)?)?;
                }
                1 => write(root.path(), SAMPLE, &wav(&[8192, 4096], 22050)?)?,
                2 => {
                    let path = resonance_audio::effect::recipe::TABLES;
                    let mut tables: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(root.path().join(path))?)?;
                    let previous = tables["mix"]["volume_16_scale"].as_f64().unwrap();
                    tables["mix"]["volume_16_scale"] = serde_json::json!(previous * 0.5);
                    write(root.path(), path, &serde_json::to_vec(&tables)?)?;
                }
                3 => {
                    let Definition::Synthesized(recipe) = &mut definition else {
                        unreachable!()
                    };
                    recipe.duration_ticks += 1;
                    write(root.path(), PATH, &serde_json::to_vec(&definition)?)?;
                }
                _ => unreachable!(),
            }
            let (current, files) = prepare(&mut cache, &library, REVERBS)?;
            owners.push(files);
            assert!(
                !Arc::ptr_eq(&previous, &current),
                "edit {edit} reused stale playback data"
            );
            previous = current;
        }
        let mut reverbs = REVERBS;
        reverbs[0][2] = 2.;
        let (changed, files) = prepare(&mut cache, &library, reverbs)?;
        owners.push(files);
        assert!(!Arc::ptr_eq(&previous, &changed));
        assert_eq!(changed.program().unwrap().reverbs(), reverbs);
        std::fs::remove_file(root.path().join(SAMPLE))?;
        assert!(
            prepare(&mut cache, &library, REVERBS).is_err(),
            "warm cue hid a missing sample"
        );
        // Editing the root to remove that sample must not replay obsolete reads.
        write(
            root.path(),
            PATH,
            br#"{"kind":"silence","frames":128,"sample_rate":32000}"#,
        )?;
        let (_, files) = prepare(&mut cache, &library, REVERBS)?;
        assert_eq!(files.keys().map(String::as_str).collect::<Vec<_>>(), [PATH]);
        Ok(())
    }
}
struct Entry {
    inputs: Vec<Input>,
    prepared: Weak<PreparedEffect>,
}

#[derive(Default)]
pub(super) struct Cache {
    entries: BTreeMap<Key, Entry>,
    #[cfg(test)]
    hits: usize,
}

impl Cache {
    pub fn prune(&mut self) {
        self.entries.retain(|_, entry| {
            entry.prepared.strong_count() > 0
                && entry
                    .inputs
                    .iter()
                    .all(|input| input.bytes.strong_count() > 0)
        });
    }

    pub fn adopt(&mut self, other: Self) {
        self.entries.extend(other.entries);
        self.prune();
    }

    pub fn prepare(
        &mut self,
        path: &str,
        reverbs: [[f32; 5]; 2],
        read: &mut impl FnMut(&str, usize) -> Result<Arc<[u8]>>,
        samples: &mut SampleCache,
    ) -> Result<Arc<PreparedEffect>> {
        let key = (path.to_owned(), reverbs.map(|row| row.map(f32::to_bits)));
        if let Some(entry) = self.entries.get(&key)
            && let Some(prepared) = entry.prepared.upgrade()
        {
            let mut matches = true;
            // Replay reads in discovery order. Stop at the first edit: a changed
            // recipe may no longer refer to the old recipe's later dependencies.
            for input in &entry.inputs {
                let Some(previous) = input.bytes.upgrade() else {
                    matches = false;
                    break;
                };
                let current = read(&input.path, input.limit)?;
                if !resonance_content::same_bytes(&previous, &current) {
                    matches = false;
                    break;
                }
            }
            if matches {
                #[cfg(test)]
                {
                    self.hits += 1;
                }
                return Ok(prepared);
            }
        }
        let mut inputs: Vec<Input> = Vec::new();
        let mut tracked = |path: &str, limit| {
            let bytes = read(path, limit)?;
            if let Some(input) = inputs.iter_mut().find(|input| input.path == path) {
                input.limit = input.limit.min(limit);
            } else {
                inputs.push(Input {
                    path: path.to_owned(),
                    limit,
                    bytes: Arc::downgrade(&bytes),
                });
            }
            Ok(bytes)
        };
        let definition = EffectDefinition::read(path, &mut tracked)?;
        let prepared = Arc::new(definition.prepare_in(Some(reverbs), &mut tracked, samples)?);
        {
            self.entries.insert(
                key,
                Entry {
                    inputs,
                    prepared: Arc::downgrade(&prepared),
                },
            );
        }
        Ok(prepared)
    }

    #[cfg(test)]
    pub fn hits(&self) -> usize {
        self.hits
    }
}
