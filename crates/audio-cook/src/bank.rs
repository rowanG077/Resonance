//! Checked SND readers adapted from the author's `tales-script-lang/src/snd.rs`.
//! Pool IDs are scoped by object type, as in the original linked tables.
use crate::{dsp, read};
use anyhow::{Context, Result, ensure};
pub use resonance_audio::sample::Sample;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Macro,
    Table,
    Keymap,
    Layer,
}

#[derive(Clone, Copy, Debug, Serialize, thiserror::Error)]
#[error("{kind:?} {id} is missing")]
pub struct MissingObject {
    pub kind: ObjectKind,
    pub id: u16,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Sound {
    pub object: u16,
    pub priority: u8,
    pub max_voices: u8,
    pub volume: u8,
    pub pan: u8,
    pub key: u8,
    pub volume_group: u8,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Page {
    pub object: u16,
    pub priority: u8,
    pub max_voices: u8,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Channel {
    pub program: u8,
    pub volume: u8,
    pub pan: u8,
    pub reverb: u8,
    pub chorus: u8,
}

#[derive(Serialize, Deserialize)]
pub struct MusicSetup {
    pub group: u16,
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
    table_spans: BTreeMap<u16, &'a [u8]>,
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
            // Some archive bank descriptors omit the pool; their public IDs
            // still exist, but instruments require separately loaded tables.
            if index == 1 && start == 0 && size == 0 {
                continue;
            }
            *section = read::slice(data, start, size)?;
            let end = start + size;
            ensure!(start >= 36 && size > 0, "invalid SND section {index}");
            ensure!(
                !ranges.iter().any(|&(a, b)| start < b && end > a),
                "overlapping SND sections"
            );
            ranges.push((start, end));
        }
        Self::from_sections(sections)
    }

    /// The same bank tables can be embedded separately from their PCM payload.
    pub fn from_sections(sections: [&'a [u8]; 4]) -> Result<Self> {
        let mut objects: [BTreeMap<u16, &'a [u8]>; 4] = Default::default();
        let mut table_spans = BTreeMap::new();
        for (kind, table) in objects.iter_mut().enumerate() {
            if sections[1].is_empty() {
                break;
            }
            let mut at = read::u32(sections[1], kind * 4)? as usize;
            // An unused list has a zero head. Its descriptor registers no IDs;
            // following zero would misread this directory and the macro list.
            if at == 0 {
                continue;
            }
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
                if kind == ObjectKind::Table as usize {
                    table_spans.insert(id, &sections[1][at + 8..]);
                }
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
                        priority: record[5],
                        max_voices: record[4],
                        volume: record[6],
                        pan: record[7],
                        key: record[8],
                        volume_group: record[9],
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
            table_spans,
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

    pub fn group(&self) -> Result<u16> {
        read::u16(self.sections[0], 4)
    }

    pub fn with_sample_payload(mut self, payload: &'a [u8]) -> Self {
        self.sections[3] = payload;
        self
    }

    pub fn music_setups(&self) -> Result<BTreeMap<u16, BTreeMap<u16, MusicSetup>>> {
        self.music_groups
            .iter()
            .map(|(&group, &at)| {
                let mut cursor = read::u32(self.sections[0], at + 36)? as usize;
                let mut setups = BTreeMap::new();
                loop {
                    let id = read::u16(self.sections[0], cursor)?;
                    if id == u16::MAX {
                        break;
                    }
                    ensure!(
                        setups.insert(id, self.music_setup(group, id)?).is_none(),
                        "duplicate music setup {id}"
                    );
                    cursor += 84;
                }
                Ok((group, setups))
            })
            .collect()
    }

    pub fn sound_ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.sounds.keys().copied()
    }

    pub fn object_ids(&self, kind: ObjectKind) -> impl Iterator<Item = u16> + '_ {
        self.objects[kind as usize].keys().copied()
    }

    /// Local source IDs only; inventory does not decode sample payloads.
    pub fn sample_ids(&self) -> Result<BTreeSet<u16>> {
        let mut ids = BTreeSet::new();
        let mut at = 0;
        while read::u32(self.sections[2], at)? != u32::MAX {
            let entry = read::slice(self.sections[2], at, 32)?;
            ensure!(ids.insert(read::u16(entry, 0)?), "duplicate sample ID");
            at += 32;
        }
        Ok(ids)
    }

    /// Event sound banks can reference envelope tables from the resident common
    /// bank. Resolve those references offline, retaining local overrides.
    pub fn inherit_tables(&mut self, common: &Bank<'a>) {
        for (&id, &bytes) in &common.objects[ObjectKind::Table as usize] {
            self.objects[ObjectKind::Table as usize]
                .entry(id)
                .or_insert(bytes);
        }
        for (&id, &bytes) in &common.table_spans {
            self.table_spans.entry(id).or_insert(bytes);
        }
    }

    /// Loaded instrument pools share their numeric namespace. Preserve local
    /// overrides when adding the resident common and instrument banks.
    pub fn inherit_objects(&mut self, resident: &Bank<'a>) {
        for (local, source) in self.objects.iter_mut().zip(&resident.objects) {
            for (&id, &bytes) in source {
                local.entry(id).or_insert(bytes);
            }
        }
        for (&id, &bytes) in &resident.table_spans {
            self.table_spans.entry(id).or_insert(bytes);
        }
    }

    /// Some physical banks omit their pool. Recover only definitions that are
    /// identical across every available provider; conflicting IDs stay missing.
    pub fn inherit_unique_objects(&mut self, residents: &[Bank<'a>]) {
        for kind in 0..self.objects.len() {
            let local: BTreeSet<_> = self.objects[kind].keys().copied().collect();
            let mut ambiguous = BTreeSet::new();
            for resident in residents {
                for (&id, &bytes) in &resident.objects[kind] {
                    if local.contains(&id) {
                        continue;
                    }
                    if self.objects[kind].get(&id).is_some_and(|old| *old != bytes)
                        || (kind == ObjectKind::Table as usize
                            && self.table_spans.get(&id).is_some_and(|old| {
                                old.get(..128) != resident.table_spans[&id].get(..128)
                            }))
                    {
                        ambiguous.insert(id);
                    } else {
                        self.objects[kind].insert(id, bytes);
                        if kind == ObjectKind::Table as usize {
                            self.table_spans.insert(id, resident.table_spans[&id]);
                        }
                    }
                }
            }
            for id in ambiguous {
                self.objects[kind].remove(&id);
                if kind == ObjectKind::Table as usize {
                    self.table_spans.remove(&id);
                }
            }
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
            .ok_or_else(|| MissingObject { kind, id }.into())
    }

    /// Pitch envelopes read twenty bytes from a table pointer even when the
    /// authored row is shorter. Resolve that layout here, bounded by the pool;
    /// only the decoded envelope parameters enter cooked assets.
    pub fn pitch_envelope(&self, id: u16) -> Result<&'a [u8]> {
        self.table_window(id, 20)
            .with_context(|| format!("pitch envelope {id} exceeds its source pool"))
    }

    /// Curves consume 128 adjacent values even when the named object is an
    /// envelope row. Preserve those authored lookups, bounded by its own pool.
    pub fn volume_curve(&self, id: u16) -> Result<&'a [u8]> {
        self.table_window(id, 128)
            .with_context(|| format!("volume curve {id} exceeds its source pool"))
    }

    fn table_window(&self, id: u16, length: usize) -> Result<&'a [u8]> {
        let bytes = self
            .table_spans
            .get(&id)
            .copied()
            .or_else(|| self.objects[ObjectKind::Table as usize].get(&id).copied())
            .ok_or(MissingObject {
                kind: ObjectKind::Table,
                id,
            })?;
        read::slice(bytes, 0, length)
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
            group,
            normal,
            drums,
            channels,
        })
    }

    fn sample_entry(&self, id: u16) -> Result<(&'a [u8], &'a [u8], &'a [u8])> {
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
        found.with_context(|| format!("sample {id} is missing"))
    }

    pub fn sample_frames(&self, id: u16) -> Result<u32> {
        Ok(read::u32(self.sample_entry(id)?.2, 16)? & 0x00ff_ffff)
    }

    pub fn sample_format(&self, id: u16) -> Result<u8> {
        Ok((read::u32(self.sample_entry(id)?.2, 16)? >> 24) as u8)
    }

    pub fn sample(&self, id: u16) -> Result<Sample> {
        let (directory, payload, entry) = self.sample_entry(id)?;
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
        assert!(bank.pitch_envelope(88).is_err());
        assert!(Bank::parse(&bytes[..139]).is_err());
        bytes[12..16].copy_from_slice(&36u32.to_be_bytes());
        assert!(Bank::parse(&bytes).is_err());
    }

    #[test]
    fn zero_heads_do_not_alias_the_directory_and_macro_records() {
        let mut bytes = fixture();
        bytes[72..84].fill(0);
        let bank = Bank::parse(&bytes).unwrap();
        assert_eq!(bank.object_ids(ObjectKind::Macro).collect::<Vec<_>>(), [88]);
        for kind in [ObjectKind::Table, ObjectKind::Keymap, ObjectKind::Layer] {
            assert_eq!(bank.object_ids(kind).count(), 0);
            assert!(bank.object(kind, 0).is_err());
            assert!(bank.object(kind, 88).is_err());
        }
        assert!(bank.volume_curve(88).is_err());
    }

    #[test]
    fn pitch_envelope_resolves_its_full_read_without_crossing_the_pool() {
        let mut bytes = fixture();
        // Enlarge only the pool; keep the directory and sample payload separate.
        bytes.splice(116..116, [0; 16]);
        bytes[16..20].copy_from_slice(&64u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&132u32.to_be_bytes());
        bytes[28..32].copy_from_slice(&136u32.to_be_bytes());
        // A short table followed by another record; the pitch consumer reads
        // twenty bytes, whereas ordinary table lookup keeps the declared size.
        bytes[76..84].copy_from_slice(&[0, 0, 0, 60, 0, 0, 0, 60]);
        bytes[100..104].copy_from_slice(&16u32.to_be_bytes());
        bytes[108..116].copy_from_slice(&[0xa7, 3, 0xd3, 8, 0, 0, 0xc8, 0]);
        bytes[116..132]
            .copy_from_slice(&[0, 0, 0, 12, 0, 89, 0, 0, 1, 2, 3, 4, 255, 255, 255, 255]);
        let bank = Bank::parse(&bytes).unwrap();
        assert_eq!(bank.object(ObjectKind::Table, 88).unwrap().len(), 8);
        let data = bank.pitch_envelope(88).unwrap();
        assert_eq!(data, &bytes[108..128]);
        let envelope = crate::parameters::dls(data).unwrap();
        assert_eq!(envelope.release_ms, 3072);
        assert_eq!(envelope.attack_velocity_scale, 89 << 8);
        assert!(bank.pitch_envelope(89).is_err());
    }

    #[test]
    fn volume_curve_reads_following_records_but_never_leaves_its_pool() {
        let mut bytes = fixture();
        bytes.splice(116..116, [0; 136]);
        bytes[16..20].copy_from_slice(&184u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&252u32.to_be_bytes());
        bytes[28..32].copy_from_slice(&256u32.to_be_bytes());
        for offset in [76, 80] {
            bytes[offset..offset + 4].copy_from_slice(&48u32.to_be_bytes());
        }
        bytes[100..104].copy_from_slice(&16u32.to_be_bytes());
        bytes[108..116].copy_from_slice(&[184, 11, 136, 19, 153, 9, 244, 1]);
        bytes[116..120].fill(255);
        for (i, value) in bytes[120..252].iter_mut().enumerate() {
            *value = i as u8;
        }
        let bank = Bank::parse(&bytes).unwrap();
        assert_eq!(bank.object(ObjectKind::Table, 88).unwrap().len(), 8);
        assert_eq!(bank.volume_curve(88).unwrap(), &bytes[108..236]);
        bytes[16..20].copy_from_slice(&64u32.to_be_bytes());
        assert!(Bank::parse(&bytes).unwrap().volume_curve(88).is_err());
    }

    #[test]
    fn auxiliary_selectors_preserve_the_bus_and_signed_midpoint() {
        // Cue 153 selects A; eraser cue 236 selects B. Both zero-scale signed
        // selectors produce 0x2000, retaining their distinct effect sends.
        for (opcode, expected_bus) in [(0x4b, 0), (0x4c, 1)] {
            let macro_bytes = [0, 0, 0x1e, opcode, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let bank = macro_bank(&macro_bytes);
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

    fn macro_bank(bytes: &[u8]) -> Bank<'_> {
        Bank {
            table_spans: Default::default(),
            sections: [&[]; 4],
            sample_banks: Vec::new(),
            objects: [
                BTreeMap::from([(436, bytes)]),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
            ],
            sounds: BTreeMap::new(),
            music_groups: BTreeMap::new(),
        }
    }

    #[test]
    fn beat_wait_and_second_sweep_preserve_operands_and_reject_unimplemented_modes() {
        use resonance_audio::data::{Command, SweepSlot};
        let compile = |a: u32, b: u32| {
            let bytes: Vec<_> = [a, b, 0, 0]
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .collect();
            crate::compile::programs(&macro_bank(&bytes), [436])
                .map(|resources| resources.programs[&436][0])
        };
        assert!(matches!(
            compile(0x01000104, 0x01800000).unwrap(),
            Command::BeatWait {
                ticks: Some(384),
                key_off: true,
                sample_end: true,
            }
        ));
        assert!(matches!(
            compile(0x01000104, 0xffff0000).unwrap(),
            Command::BeatWait { ticks: None, .. }
        ));
        assert!(matches!(
            compile(4, 0x01800101).unwrap(),
            Command::Wait {
                milliseconds: Some(384),
                from_start: true,
                ..
            }
        ));
        for upper in [0, 2000, 65535] {
            assert!(matches!(compile(0x00010107, upper << 16).unwrap(),
                Command::RandomWait { upper_ms, key_off: true } if u32::from(upper_ms) == upper));
        }
        assert!(matches!(
            compile(0xfc18021e, 0x000a0100).unwrap(),
            Command::PitchSweep {
                slot: SweepSlot::Second,
                step_hz: -1000,
                period: 2,
                wait_ms: 10,
            }
        ));
        // Resident cue288 disables vibrato with an ignored negative depth byte.
        for b in [0, 0x100] {
            assert!(matches!(
                compile(0x0000fe1c, b).unwrap(),
                Command::Vibrato {
                    period_ms: 0,
                    depth_8: 0,
                    reverse: false,
                    scale_by_modulation: false,
                }
            ));
        }
        for (a, b) in [
            (0x00010004, 0x01800000),
            (0x00010007, 0x01800001),
            (0x01010007, 0x01800000),
            (4, 0x01800001),
            (0x1e, 0x000a0000),
            (0x0000011c, 0x00640000),
        ] {
            assert!(compile(a, b).is_err(), "unsupported macro {a:08x} {b:08x}");
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
            table_spans: Default::default(),
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
        assert!(format!("{error:#}").contains("sample 135 is missing"));

        event.inherit_samples(&common);
        let resources = crate::compile::programs(&event, roots()).unwrap();
        let actual = &resources.samples[&135];
        let expected = common.sample(135).unwrap();
        assert_eq!(event.sample_format(135).unwrap(), 0);
        for mode in [0, 1, 2, 255] {
            let operation =
                crate::decode::command(&event, mode << 24 | 135 << 8 | 0x10, u32::MAX).unwrap();
            assert!(matches!(
                operation.mixer().unwrap(),
                resonance_audio::data::Command::StartSample { sample: 135 }
            ));
        }
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
            table_spans: Default::default(),
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
            table_spans: Default::default(),
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
            table_spans: Default::default(),
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
