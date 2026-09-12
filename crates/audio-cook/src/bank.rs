//! Checked SND readers adapted from the author's `tales-script-lang/src/snd.rs`.
//! Pool IDs are scoped by object type, as in the original linked tables.
use crate::{dsp, read};
use anyhow::{Context, Result, ensure};
pub use resonance_audio::sample::Sample;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug)]
pub enum ObjectKind {
    Macro,
    Table,
    Keymap,
    Layer,
}

#[derive(Clone, Copy, Debug)]
pub struct Sound {
    pub object: u16,
    pub volume: u8,
    pub pan: u8,
    pub key: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub object: u16,
    pub priority: u8,
    pub max_voices: u8,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Channel {
    pub program: u8,
    pub volume: u8,
    pub pan: u8,
    pub reverb: u8,
    pub chorus: u8,
}

pub struct MusicSetup {
    pub normal: BTreeMap<u8, Page>,
    pub drums: BTreeMap<u8, Page>,
    pub channels: [Channel; 16],
}

impl MusicSetup {
    pub fn page(&self, channel: u8, program: u8) -> Option<Page> {
        if channel == 9 {
            &self.drums
        } else {
            &self.normal
        }
        .get(&program)
        .copied()
    }
}

pub struct Bank<'a> {
    sections: [&'a [u8]; 4],
    sample_banks: Vec<[&'a [u8]; 2]>,
    objects: [BTreeMap<u16, &'a [u8]>; 4],
    sounds: BTreeMap<u16, Sound>,
    music_groups: BTreeMap<u16, usize>,
}

impl<'a> Bank<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        ensure!(read::u32(data, 0)? == 4, "expected four SND sections");
        let mut sections = [&[][..]; 4];
        let mut ranges = Vec::new();
        for (index, section) in sections.iter_mut().enumerate() {
            let start = read::u32(data, 4 + index * 8)? as usize;
            let size = read::u32(data, 8 + index * 8)? as usize;
            *section = read::slice(data, start, size)?;
            let end = start + size;
            ensure!(start >= 36 && size > 0, "invalid SND section {index}");
            ensure!(
                !ranges.iter().any(|&(a, b)| start < b && end > a),
                "overlapping SND sections"
            );
            ranges.push((start, end));
        }
        let mut objects: [BTreeMap<u16, &'a [u8]>; 4] = Default::default();
        for (kind, table) in objects.iter_mut().enumerate() {
            let mut at = read::u32(sections[1], kind * 4)? as usize;
            loop {
                let stride = read::u32(sections[1], at)?;
                if stride == u32::MAX {
                    break;
                }
                ensure!(stride >= 8, "invalid SND pool stride");
                let object = read::slice(sections[1], at, stride as usize)?;
                let id = read::u16(object, 4)?;
                ensure!(
                    table.insert(id, &object[8..]).is_none(),
                    "duplicate pool ID {id} in kind {kind}"
                );
                at += stride as usize;
            }
        }
        let mut sounds = BTreeMap::new();
        let mut music_groups = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut at = 0;
        loop {
            ensure!(seen.insert(at), "cyclic SND descriptor chain");
            let next = read::u32(sections[0], at)?;
            if next == u32::MAX && !seen.is_empty() && at != 0 {
                break;
            }
            let descriptor = read::slice(sections[0], at, 32)?;
            if read::u16(descriptor, 6)? == 0 {
                let id = read::u16(descriptor, 4)?;
                ensure!(
                    music_groups.insert(id, at).is_none(),
                    "duplicate music group {id}"
                );
            }
            if read::u16(descriptor, 6)? == 1 {
                let extra = read::u32(descriptor, 28)? as usize;
                let count = usize::from(read::u16(sections[0], extra)?);
                let records = read::slice(sections[0], extra + 4, count * 10)?;
                for record in records.chunks_exact(10) {
                    let id = read::u16(record, 0)?;
                    let sound = Sound {
                        object: read::u16(record, 2)?,
                        volume: record[6],
                        pan: record[7],
                        key: record[8],
                    };
                    ensure!(
                        sound.volume <= 127 && sound.pan <= 127 && sound.key <= 127,
                        "invalid sound controls for {id}"
                    );
                    ensure!(
                        sounds.insert(id, sound).is_none(),
                        "ambiguous public sound ID {id}"
                    );
                }
            }
            if next == u32::MAX {
                break;
            }
            at = next as usize;
        }
        Ok(Self {
            sections,
            sample_banks: Vec::new(),
            objects,
            sounds,
            music_groups,
        })
    }

    pub fn sound(&self, id: u16) -> Result<Sound> {
        self.sounds
            .get(&id)
            .copied()
            .with_context(|| format!("sound {id} is missing"))
    }

    pub fn sound_ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.sounds.keys().copied()
    }

    /// Event sound banks can reference envelope tables from the resident common
    /// bank. Resolve those references offline, retaining local overrides.
    pub fn inherit_tables(&mut self, common: &Bank<'a>) {
        for (&id, &bytes) in &common.objects[ObjectKind::Table as usize] {
            self.objects[ObjectKind::Table as usize]
                .entry(id)
                .or_insert(bytes);
        }
    }
    /// Resident common and instrument samples are visible to event macros too.
    /// Keep each directory paired with its own payload and retain local IDs.
    pub fn inherit_samples(&mut self, resident: &Bank<'a>) {
        self.sample_banks
            .push([resident.sections[2], resident.sections[3]]);
        self.sample_banks.extend_from_slice(&resident.sample_banks);
    }

    pub fn object(&self, kind: ObjectKind, id: u16) -> Result<&'a [u8]> {
        self.objects[kind as usize]
            .get(&id)
            .copied()
            .with_context(|| format!("{kind:?} {id} is missing"))
    }

    /// Resolve a music group and setup into program pages and sixteen channel states.
    pub fn music_setup(&self, group: u16, setup: u16) -> Result<MusicSetup> {
        let at = *self
            .music_groups
            .get(&group)
            .with_context(|| format!("music group {group} is missing"))?;
        let project = self.sections[0];
        let descriptor = read::slice(project, at, 40)?;
        let pages = |at| -> Result<BTreeMap<u8, Page>> {
            let mut at = at;
            let mut result = BTreeMap::new();
            loop {
                let entry = read::slice(project, at, 6)?;
                let program = entry[4];
                if program == 255 {
                    break;
                }
                ensure!(program < 128, "invalid instrument program");
                let page = Page {
                    object: read::u16(entry, 0)?,
                    priority: entry[2],
                    max_voices: entry[3],
                };
                ensure!(
                    result.insert(program, page).is_none(),
                    "duplicate instrument program {program}"
                );
                at += 6;
            }
            Ok(result)
        };
        let normal = pages(read::u32(descriptor, 28)? as usize)?;
        let drums = pages(read::u32(descriptor, 32)? as usize)?;
        let mut at = read::u32(descriptor, 36)? as usize;
        let record = loop {
            let id = read::u16(project, at)?;
            ensure!(
                id != u16::MAX,
                "music setup {setup} is missing in group {group}"
            );
            let record = read::slice(project, at, 84)?;
            if id == setup {
                break record;
            }
            at += 84;
        };
        let mut channels = [Channel::default(); 16];
        for (channel, bytes) in channels.iter_mut().zip(record[4..].chunks_exact(5)) {
            ensure!(
                bytes.iter().all(|v| *v <= 127),
                "invalid music setup control"
            );
            *channel = Channel {
                program: bytes[0],
                volume: bytes[1],
                pan: bytes[2],
                reverb: bytes[3],
                chorus: bytes[4],
            };
        }
        Ok(MusicSetup {
            normal,
            drums,
            channels,
        })
    }

    pub fn sample(&self, id: u16) -> Result<Sample> {
        let mut found = None;
        for [directory, payload] in std::iter::once([self.sections[2], self.sections[3]])
            .chain(self.sample_banks.iter().copied())
        {
            let mut at = 0;
            while read::u32(directory, at)? != u32::MAX {
                let entry = read::slice(directory, at, 32)?;
                if read::u16(entry, 0)? == id {
                    found = Some((directory, payload, entry));
                    break;
                }
                at += 32;
            }
            if found.is_some() {
                break;
            }
        }
        let (directory, payload, entry) =
            found.with_context(|| format!("sample {id} is missing"))?;
        let format_count = read::u32(entry, 16)?;
        ensure!(format_count >> 24 == 0, "sample {id}: unsupported encoding");
        let count = format_count & 0x00ff_ffff;
        let metadata = read::slice(directory, read::u32(entry, 28)? as usize, 40)?;
        let mut coefficients = [[0i16; 2]; 8];
        for (index, pair) in coefficients.iter_mut().enumerate() {
            pair[0] = read::u16(metadata, 8 + index * 4)? as i16;
            pair[1] = read::u16(metadata, 10 + index * 4)? as i16;
        }
        let payload = read::slice(
            payload,
            read::u32(entry, 4)? as usize,
            dsp::encoded_size(count),
        )?;
        let rate_key = read::u32(entry, 12)?;
        let loop_start = read::u32(entry, 20)?;
        let loop_length = read::u32(entry, 24)?;
        ensure!(
            u64::from(loop_start) + u64::from(loop_length) <= u64::from(count),
            "sample loop exceeds payload"
        );
        Ok(Sample {
            key: (rate_key >> 24) as u8,
            rate: rate_key as u16,
            loop_start,
            loop_length,
            pcm: dsp::decode_range(
                payload,
                0..count,
                coefficients,
                dsp::State {
                    predictor_scale: metadata[2],
                    history: [0; 2],
                },
            )?,
            loop_pcm: if loop_length != 0 {
                dsp::decode_range(
                    payload,
                    loop_start..loop_start + loop_length,
                    coefficients,
                    dsp::State {
                        predictor_scale: metadata[3],
                        history: [
                            read::u16(metadata, 6)? as i16,
                            read::u16(metadata, 4)? as i16,
                        ],
                    },
                )?
            } else {
                Vec::new()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; 140];
        let word =
            |b: &mut [u8], at, value: u32| b[at..at + 4].copy_from_slice(&value.to_be_bytes());
        word(&mut bytes, 0, 4);
        for (i, (start, size)) in [(36, 32), (68, 48), (116, 4), (120, 20)]
            .into_iter()
            .enumerate()
        {
            word(&mut bytes, 4 + i * 8, start);
            word(&mut bytes, 8 + i * 8, size);
        }
        word(&mut bytes, 36, u32::MAX);
        for (i, start) in [16, 32, 44, 44].into_iter().enumerate() {
            word(&mut bytes, 68 + i * 4, start);
        }
        // Macro 88 and table 88 are different resources, not a duplicate.
        word(&mut bytes, 84, 12);
        word(&mut bytes, 88, 88 << 16);
        word(&mut bytes, 92, 0x1234);
        word(&mut bytes, 96, u32::MAX);
        word(&mut bytes, 100, 8);
        word(&mut bytes, 104, 88 << 16);
        word(&mut bytes, 108, u32::MAX);
        word(&mut bytes, 112, u32::MAX);
        word(&mut bytes, 116, u32::MAX);
        bytes
    }

    #[test]
    fn preserves_pool_namespaces_and_rejects_bad_ranges() {
        let mut bytes = fixture();
        let bank = Bank::parse(&bytes).unwrap();
        assert_eq!(
            bank.object(ObjectKind::Macro, 88).unwrap(),
            0x1234u32.to_be_bytes()
        );
        assert!(bank.object(ObjectKind::Table, 88).unwrap().is_empty());
        assert!(Bank::parse(&bytes[..139]).is_err());
        bytes[12..16].copy_from_slice(&36u32.to_be_bytes());
        assert!(Bank::parse(&bytes).is_err());
    }

    #[test]
    fn auxiliary_selectors_preserve_the_bus_and_signed_midpoint() {
        // Cue 153 selects A; eraser cue 236 selects B. Both zero-scale signed
        // selectors produce 0x2000, retaining their distinct effect sends.
        for (opcode, expected_bus) in [(0x4b, 0), (0x4c, 1)] {
            let macro_bytes = [0, 0, 0x1e, opcode, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let bank = Bank {
                sections: [&[]; 4],
                sample_banks: Vec::new(),
                objects: [
                    BTreeMap::from([(436, macro_bytes.as_slice())]),
                    BTreeMap::new(),
                    BTreeMap::new(),
                    BTreeMap::new(),
                ],
                sounds: BTreeMap::new(),
                music_groups: BTreeMap::new(),
            };
            let resources = crate::compile::programs(&bank, [436]).unwrap();
            let resonance_audio::data::Command::Auxiliary { bus, value } =
                resources.programs[&436][0]
            else {
                panic!("macro lost its auxiliary selector");
            };
            assert_eq!(bus, expected_bus);
            assert_eq!(u16::from(value) << 7, 0x2000);
        }
    }

    #[test]
    fn resident_samples_keep_their_own_payload_and_local_ids_take_priority() {
        fn directory(records: &[(u16, u32)]) -> Vec<u8> {
            let metadata = records.len() * 32 + 4;
            let mut bytes = vec![0; metadata + 40];
            for (index, &(id, offset)) in records.iter().enumerate() {
                let row = &mut bytes[index * 32..][..32];
                row[..2].copy_from_slice(&id.to_be_bytes());
                row[4..8].copy_from_slice(&offset.to_be_bytes());
                row[12..16].copy_from_slice(&((60u32 << 24) | 32000).to_be_bytes());
                row[16..20].copy_from_slice(&14u32.to_be_bytes());
                row[28..32].copy_from_slice(&(metadata as u32).to_be_bytes());
            }
            bytes[metadata - 4..metadata].copy_from_slice(&u32::MAX.to_be_bytes());
            bytes
        }
        let local_directory = directory(&[(7, 0)]);
        let resident_directory = directory(&[(7, 0), (9, 8)]);
        let local_payload = [0, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11];
        let resident_payload = [
            0, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff,
        ];
        let make = |directory, payload| Bank {
            sections: [&[], &[], directory, payload],
            sample_banks: Vec::new(),
            objects: Default::default(),
            sounds: Default::default(),
            music_groups: Default::default(),
        };
        let mut local = make(&local_directory, &local_payload);
        let resident = make(&resident_directory, &resident_payload);
        assert!(local.sample(9).is_err());
        local.inherit_samples(&resident);
        assert_eq!(local.sample(7).unwrap().pcm, vec![1; 14]);
        let inherited = local.sample(9).unwrap();
        assert_eq!((inherited.key, inherited.rate), (60, 32000));
        assert_eq!(inherited.pcm, vec![-1; 14]);
        assert!(local.sample(10).is_err());
    }

    #[test]
    #[ignore = "requires locally extracted GQSEAF sound banks; no audio device"]
    fn event_cue_425_uses_the_resident_common_sample() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/S");
        let common_bytes = std::fs::read(root.join("se.snd")).unwrap();
        let instrument_bytes = std::fs::read(root.join("inst.snd")).unwrap();
        let event_bytes = std::fs::read(root.join("se_ev00.snd")).unwrap();
        let common = Bank::parse(&common_bytes).unwrap();
        let instruments = Bank::parse(&instrument_bytes).unwrap();
        let mut event = Bank::parse(&event_bytes).unwrap();
        event.inherit_tables(&common);
        event.inherit_tables(&instruments);
        event.inherit_samples(&instruments);
        let sound = event.sound(425).unwrap();
        let layers = crate::instrument::resolve(
            &event,
            Page {
                object: sound.object,
                priority: 64,
                max_voices: 255,
            },
            sound.key,
            sound.volume,
            sound.pan,
        )
        .unwrap();
        let roots = || layers.iter().map(|layer| layer.macro_id);
        let error = crate::compile::programs(&event, roots()).err().unwrap();
        assert_eq!(error.to_string(), "sample 135 is missing");

        event.inherit_samples(&common);
        let resources = crate::compile::programs(&event, roots()).unwrap();
        let actual = &resources.samples[&135];
        let expected = common.sample(135).unwrap();
        assert_eq!((actual.key, actual.rate), (expected.key, expected.rate));
        assert_eq!(
            (actual.loop_start, actual.loop_length),
            (expected.loop_start, expected.loop_length)
        );
        assert_eq!(actual.pcm, expected.pcm);
        assert_eq!(actual.loop_pcm, expected.loop_pcm);
        assert!(actual.pcm.iter().any(|&sample| sample != 0));
    }

    #[test]
    fn music_pages_and_sixteen_channel_setup_are_bounded() {
        let mut project = vec![0; 150];
        for (at, value) in [(28, 40u32), (32, 52), (36, 64)] {
            project[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        project[40..52].copy_from_slice(&[0, 7, 10, 8, 31, 0, 255, 255, 0, 0, 255, 0]);
        project[52..64].copy_from_slice(&[0, 8, 20, 4, 0, 0, 255, 255, 0, 0, 255, 0]);
        project[64..66].copy_from_slice(&1u16.to_be_bytes());
        for channel in project[68..148].chunks_exact_mut(5) {
            channel.copy_from_slice(&[31, 127, 64, 25, 0]);
        }
        project[148..150].copy_from_slice(&u16::MAX.to_be_bytes());
        let bank = Bank {
            sections: [&project, &[], &[], &[]],
            sample_banks: Vec::new(),
            objects: Default::default(),
            sounds: Default::default(),
            music_groups: BTreeMap::from([(0, 0)]),
        };
        let setup = bank.music_setup(0, 1).unwrap();
        assert_eq!(setup.page(0, 31).unwrap().object, 7);
        assert_eq!(setup.page(9, 0).unwrap().object, 8);
        assert!(setup.page(0, 0).is_none());
        assert!(
            setup
                .channels
                .iter()
                .all(|c| (c.program, c.volume, c.pan, c.reverb, c.chorus) == (31, 127, 64, 25, 0))
        );
        assert!(bank.music_setup(0, 2).is_err());
        assert!(bank.music_setup(1, 1).is_err());
        drop(bank);
        project[69] = 128;
        let bank = Bank {
            sections: [&project, &[], &[], &[]],
            sample_banks: Vec::new(),
            objects: Default::default(),
            sounds: Default::default(),
            music_groups: BTreeMap::from([(0, 0)]),
        };
        assert!(bank.music_setup(0, 1).is_err());
    }

    #[test]
    fn layered_instruments_transform_controls_and_reject_cycles() {
        let mut layers = vec![0, 0, 0, 2];
        layers.extend([0, 1, 0, 127, 192, 64, 0, 10, 127, 0, 0, 0]);
        layers.extend([0, 2, 0, 127, 127, 127, 255, 236, 0, 0, 0, 0]);
        let mut bank = Bank {
            sections: [&[], &[], &[], &[]],
            sample_banks: Vec::new(),
            objects: Default::default(),
            sounds: Default::default(),
            music_groups: Default::default(),
        };
        bank.objects[0].insert(1, &[0; 8]);
        bank.objects[0].insert(2, &[0; 8]);
        bank.objects[3].insert(0x8000, &layers);
        let page = Page {
            object: 0x8000,
            priority: 100,
            max_voices: 8,
        };
        let voices = crate::instrument::resolve(&bank, page, 60, 100, 64).unwrap();
        assert_eq!(voices.len(), 2);
        assert_eq!(
            (
                voices[0].macro_id,
                voices[0].key,
                voices[0].velocity,
                voices[0].pan,
                voices[0].priority
            ),
            (1, 0, 50, 127, 110)
        );
        assert_eq!(
            (
                voices[1].macro_id,
                voices[1].key,
                voices[1].velocity,
                voices[1].pan,
                voices[1].priority
            ),
            (2, 127, 100, 0, 90)
        );
        let cycle = [0, 0, 0, 1, 128, 1, 0, 127, 0, 127, 0, 0, 64, 0, 0, 0];
        bank.objects[3].insert(0x8001, &cycle);
        assert!(
            crate::instrument::resolve(
                &bank,
                Page {
                    object: 0x8001,
                    ..page
                },
                60,
                100,
                64
            )
            .is_err()
        );
    }
}
