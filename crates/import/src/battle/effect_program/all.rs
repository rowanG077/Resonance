//! Decode every authored effect root without requiring runtime rendering support.
use super::*;
use crate::battle::{
    all::{Archive, Sources},
    archive_directories,
};
use crate::field::MapArchive;
use crate::read::{Storage, unreferenced_storage};
use resonance_content::battle::effect_inventory::EffectSourceBank;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RootId {
    Program { id: u16 },
    Actor { id: u16 },
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureStage {
    Binding,
    Source,
    Decode,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Failure {
    pub stage: FailureStage,
    pub error: String,
}

impl Failure {
    fn new(stage: FailureStage, error: impl std::fmt::Display) -> Self {
        Self {
            stage,
            error: format!("{error:#}"),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum RootStatus {
    Cooked,
    Unsupported { failure: Failure },
}

#[derive(Debug, Serialize)]
pub(crate) struct RootOutcome {
    pub root: RootId,
    #[serde(flatten)]
    pub status: RootStatus,
}

/// Cooked source records, independent of runtime admission and allocator bindings.
#[derive(Debug, Serialize)]
pub(crate) struct BankCook {
    pub bank: EffectSourceBank,
    pub sha256: Option<String>,
    pub roots: Vec<RootOutcome>,
    pub failure: Option<Failure>,
    pub source_size: usize,
    pub header: Option<Header>,
    pub actors: BTreeMap<u16, actor::Record>,
    pub program_roots: Vec<i16>,
    pub program_records: BTreeMap<u16, events::Record>,
    pub uv_table: Vec<u16>,
    pub uv_roots: BTreeSet<u16>,
    pub uv_rows: BTreeMap<u16, uv::Record>,
    pub modifier_roots: BTreeSet<u16>,
    pub modifier_records: BTreeMap<u16, std::result::Result<modifiers::Record, String>>,
    pub storage: Vec<Storage>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct Header {
    program_count: u8,
    uv_count: u8,
    storage: u16,
    actors: u16,
    events: u16,
    modifiers: u16,
    uv: u16,
    program_table: u16,
    uv_table: u16,
}

impl Header {
    fn read(bytes: &[u8]) -> Result<Self> {
        bank_bytes(bytes)?;
        Ok(Self {
            program_count: bytes[4],
            uv_count: bytes[5],
            storage: half(bytes, 6)?,
            actors: half(bytes, 8)?,
            events: half(bytes, 10)?,
            modifiers: half(bytes, 12)?,
            uv: half(bytes, 14)?,
            program_table: half(bytes, 16)?,
            uv_table: half(bytes, 18)?,
        })
    }

    fn pool_end(self, start: u16) -> usize {
        if start == self.program_table {
            return usize::from(start);
        }
        [
            self.actors,
            self.events,
            self.modifiers,
            self.uv,
            self.program_table,
            self.uv_table,
        ]
        .into_iter()
        .filter(|&offset| offset > start)
        .min()
        .map(usize::from)
        .unwrap_or(usize::from(self.uv_table) + usize::from(self.uv_count) * 2)
    }
}

impl BankCook {
    fn new(bank: EffectSourceBank) -> Self {
        Self {
            bank,
            sha256: None,
            roots: Vec::new(),
            failure: None,
            source_size: 0,
            header: None,
            actors: BTreeMap::new(),
            program_roots: Vec::new(),
            program_records: BTreeMap::new(),
            uv_table: Vec::new(),
            uv_roots: BTreeSet::new(),
            uv_rows: BTreeMap::new(),
            modifier_roots: BTreeSet::new(),
            modifier_records: BTreeMap::new(),
            storage: Vec::new(),
        }
    }
}

pub(crate) struct Cooker {
    usual: Vec<u8>,
    enemy: PathBuf,
    magic: MagicArchive,
    extra: ExtraArchives,
}

impl Cooker {
    pub fn read(extracted: &Path) -> Result<Self> {
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
        let sources = crate::battle::all::Sources::read(extracted)?;
        Ok(Self {
            usual: fs::read(extracted.join("files").join(&sources.usual))?,
            enemy: extracted.join("files").join(&sources.enemy),
            magic: MagicArchive::with_sources(extracted, &rel, &sources)?,
            extra: ExtraArchives::with_sources(extracted, &rel, &sources)?,
        })
    }

    pub fn cook_bank(&self, bank: EffectSourceBank) -> BankCook {
        let mut result = BankCook::new(bank);
        let binding = match binding(bank) {
            Ok(binding) => binding,
            Err(error) => {
                result.failure = Some(Failure::new(FailureStage::Binding, error));
                return result;
            }
        };
        match self.load(binding) {
            Ok(bytes) => {
                if let Err(error) = decode_bank(&bytes, &mut result) {
                    result.failure = Some(Failure::new(FailureStage::Source, error));
                }
            }
            Err(error) => result.failure = Some(Failure::new(FailureStage::Source, error)),
        }
        result
    }

    fn load(&self, bank: EffectBank) -> Result<Vec<u8>> {
        Ok(match bank {
            EffectBank::Common => member(&self.usual, 2)?.to_vec(),
            EffectBank::Techniques => member(&self.usual, 3)?.to_vec(),
            EffectBank::Magic(id) => magic_member(self.magic.package(id)?, 4)?
                .context("magic package has no effect bank")?
                .to_vec(),
            EffectBank::Enemy(id) => {
                let package =
                    archive_directories::enemy_package(&self.enemy, &self.usual, u16::from(id))?;
                let start = word(&package, 0x1cc)? as usize;
                ensure!(start != 0, "enemy package has no effect bank");
                crate::battle::enemy_inventory::offset_section(&package, start)?.to_vec()
            }
            EffectBank::Skill(id) => magic_member(&self.extra.skill.package(id)?, 4)?
                .context("skill package has no effect bank")?
                .to_vec(),
            EffectBank::Arena(id) => {
                let package = MapArchive::decode(&self.extra.arena.package(id)?)?;
                let range = package
                    .sections
                    .get(9)
                    .and_then(Option::as_ref)
                    .context("arena has no effect bank")?;
                package.bytes[range.clone()].to_vec()
            }
        })
    }
}

fn decode_bank(bytes: &[u8], result: &mut BankCook) -> Result<()> {
    result.sha256 = Some(format!("{:x}", Sha256::digest(bytes)));
    result.source_size = bytes.len();
    let header = Header::read(bytes)?;
    result.header = Some(header);
    let original = bytes;
    let bytes = bank_bytes(bytes)?;
    let mut covered = vec![0..20];
    let roots = roots(bytes)?;
    let uv_pool_error = match uv::rows(bytes) {
        Ok(rows) => {
            result.uv_rows = rows;
            None
        }
        Err(error) => Some(error),
    };
    result.program_roots = (0..usize::from(header.program_count))
        .map(|index| half(bytes, usize::from(header.program_table) + index * 2).map(|v| v as i16))
        .collect::<Result<_>>()?;
    result.uv_table = (0..usize::from(header.uv_count))
        .map(|index| half(bytes, usize::from(header.uv_table) + index * 2))
        .collect::<Result<_>>()?;
    covered.push(usize::from(header.program_table)..bytes.len());
    for &offset in result.uv_rows.keys() {
        let at = usize::from(header.uv) + usize::from(offset);
        covered.push(at..at + 10);
    }
    (result.uv_roots, result.modifier_roots) = stream_offsets(bytes);
    // The pool can retain instructions after the final executable stream.
    // Decode physical records independently of stream entrypoints and terminators.
    let mut at = usize::from(half(bytes, 12)?);
    let end = [10, 14, 16, 18]
        .into_iter()
        .map(|field| half(bytes, field).map(usize::from))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|&offset| offset >= at)
        .min()
        .context("unbounded effect modifier pool")?;
    let modifier_pool = bytes
        .get(..end)
        .context("effect modifier pool exceeds bank")?;
    while at < end {
        match modifiers::read(modifier_pool, at) {
            Ok((record, next)) => {
                covered.push(at..next);
                let terminated = matches!(&record, modifiers::Record::End);
                result.modifier_records.insert(at.try_into()?, Ok(record));
                at = if terminated {
                    next.next_multiple_of(4)
                } else {
                    next
                };
            }
            Err(error) => {
                result
                    .modifier_records
                    .insert(at.try_into()?, Err(format!("{error:#}")));
                break;
            }
        }
    }
    // Retain dormant rows after terminal events. Partial trailing storage is
    // preserved below; native stream addresses may also precede this base.
    let start = usize::from(header.events);
    let end = header.pool_end(header.events);
    let pool = bytes
        .get(start..end)
        .context("effect event pool exceeds bank")?;
    for (index, row) in pool.chunks_exact(events::RECORD_BYTES).enumerate() {
        let at = start + index * events::RECORD_BYTES;
        result
            .program_records
            .insert(at.try_into()?, events::Record::read(row)?);
        covered.push(at..at + events::RECORD_BYTES);
    }
    let actor_start = usize::from(half(bytes, 8)?);
    for root in roots {
        let decoded = match root {
            RootId::Program { id } => (|| -> Result<()> {
                let timeline = program_timeline(bytes, id as u8)?;
                let mut at = bytes.len() - timeline.len();
                let mut payload = false;
                loop {
                    let row = events::Record::read(&bytes[at..])?;
                    result.program_records.insert(at.try_into()?, row);
                    covered.push(at..at + events::RECORD_BYTES);
                    at += events::RECORD_BYTES;
                    if !payload && matches!(row.command, events::Command::End { .. }) {
                        break;
                    }
                    payload = !payload && matches!(row.command, events::Command::Repeat { .. });
                }
                Ok(())
            })(),
            RootId::Actor { id } => {
                let at = actor_start + usize::from(id) * ACTOR_BYTES;
                actor::Record::read(&bytes[at..at + ACTOR_BYTES]).map(|actor| {
                    covered.push(at..at + ACTOR_BYTES);
                    result.actors.insert(id, actor);
                })
            }
        };
        let status = match decoded {
            Ok(()) => RootStatus::Cooked,
            Err(error) => RootStatus::Unsupported {
                failure: Failure::new(FailureStage::Decode, error),
            },
        };
        result.roots.push(RootOutcome { root, status });
    }
    result.storage = unreferenced_storage(original, covered);
    let failures = result
        .modifier_records
        .values()
        .filter(|value| value.is_err())
        .count();
    if let Some(error) = uv_pool_error {
        result.failure = Some(Failure::new(FailureStage::Source, error));
    } else if failures > 0 {
        result.failure = Some(Failure::new(
            FailureStage::Decode,
            format!(
                "{failures} authored effect records could not be decoded; see modifier_records"
            ),
        ));
    }
    Ok(())
}

/// Archive offsets stay resident; packages are read and decoded individually.
struct IndexedArchive {
    path: PathBuf,
    directory: archive_directories::Directory,
}

impl IndexedArchive {
    fn read(extracted: &Path, rel: &Rel, sources: &Sources, archive: Archive) -> Result<Self> {
        let file = sources.archive(archive);
        let path = extracted.join("files").join(file);
        let directory = archive_directories::read(
            rel,
            crate::battle::embedded::Layout::RETAIL.archives,
            archive,
            file,
            path.metadata()?.len(),
        )?;
        Ok(Self { path, directory })
    }

    fn package(&self, id: u16) -> Result<Vec<u8>> {
        crate::battle::all::read_range(&self.path, self.directory.load_range(id)?)
    }
}

pub(crate) struct SkillArchive(IndexedArchive);

impl SkillArchive {
    pub fn read(extracted: &Path) -> Result<Self> {
        Self::with_sources(
            extracted,
            &Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?,
            &Sources::read(extracted)?,
        )
    }

    fn with_sources(extracted: &Path, rel: &Rel, sources: &Sources) -> Result<Self> {
        Ok(Self(IndexedArchive::read(
            extracted,
            rel,
            sources,
            Archive::Skill,
        )?))
    }

    pub fn package(&self, id: u16) -> Result<Vec<u8>> {
        self.0.package(id)
    }
}

pub(super) struct ExtraArchives {
    skill: SkillArchive,
    arena: IndexedArchive,
}

impl ExtraArchives {
    pub fn read(extracted: &Path, rel: &Rel) -> Result<Self> {
        Self::with_sources(extracted, rel, &Sources::read(extracted)?)
    }

    fn with_sources(extracted: &Path, rel: &Rel, sources: &Sources) -> Result<Self> {
        Ok(Self {
            skill: SkillArchive::with_sources(extracted, rel, sources)?,
            arena: IndexedArchive::read(extracted, rel, sources, Archive::Arena)?,
        })
    }

    pub fn bank(&self, bank: EffectBank) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
        match bank {
            EffectBank::Skill(id) => {
                let package = self.skill.package(id)?;
                Ok((
                    magic_member(&package, 4)?
                        .context("skill package has no effect bank")?
                        .to_vec(),
                    magic_member(&package, 8)?.map(<[u8]>::to_vec),
                ))
            }
            EffectBank::Arena(id) => {
                let package = MapArchive::decode(&self.arena.package(id)?)?;
                let member = |index: usize| {
                    package
                        .sections
                        .get(index)
                        .and_then(Option::as_ref)
                        .map(|range| package.bytes[range.clone()].to_vec())
                };
                Ok((member(9).context("arena has no effect bank")?, member(10)))
            }
            _ => bail!("not a skill or arena resource bank"),
        }
    }
}

#[cfg(test)]
pub(crate) fn read_range(path: &Path, start: u32, end: u32) -> Result<Vec<u8>> {
    crate::battle::all::read_range(path, start as usize..end as usize)
}

fn binding(bank: EffectSourceBank) -> Result<EffectBank> {
    Ok(match bank {
        EffectSourceBank::Common => EffectBank::Common,
        EffectSourceBank::Techniques => EffectBank::Techniques,
        EffectSourceBank::Enemy { monster } => EffectBank::Enemy(monster.try_into()?),
        EffectSourceBank::Magic { package } => EffectBank::Magic(package),
        EffectSourceBank::Skill { package } => EffectBank::Skill(package),
        EffectSourceBank::Arena { arena } => EffectBank::Arena(arena),
        _ => bail!("effect source bank {bank:?} has no implemented resource binding"),
    })
}

fn bank_bytes(bytes: &[u8]) -> Result<&[u8]> {
    ensure!(
        bytes.len() >= 20 && bytes.starts_with(b"ef1\0"),
        "invalid battle effect bank"
    );
    let programs = usize::from(half(bytes, 16)?);
    let uv = usize::from(half(bytes, 18)?);
    ensure!(
        programs >= 20 && uv == programs + usize::from(bytes[4]) * 2,
        "invalid effect program table extent"
    );
    bytes
        .get(..uv + usize::from(bytes[5]) * 2)
        .context("effect UV table exceeds bank")
}

fn roots(bytes: &[u8]) -> Result<Vec<RootId>> {
    let bytes = bank_bytes(bytes)?;
    let start = usize::from(half(bytes, 8)?);
    let end = usize::from(half(bytes, 12)?);
    ensure!(
        start >= 20
            && end >= start
            && end <= bytes.len()
            && (end - start).is_multiple_of(ACTOR_BYTES),
        "invalid effect actor region"
    );
    Ok((0..u16::from(bytes[4]))
        .map(|id| RootId::Program { id })
        .chain((0..u16::try_from((end - start) / ACTOR_BYTES)?).map(|id| RootId::Actor { id }))
        .collect())
}

fn stream_offsets(bytes: &[u8]) -> (BTreeSet<u16>, BTreeSet<u16>) {
    let uv_table = usize::from(half(bytes, 18).unwrap());
    let mut uv = (0..usize::from(bytes[5]))
        .map(|index| half(bytes, uv_table + index * 2).unwrap())
        .collect::<BTreeSet<_>>();
    let actor_start = usize::from(half(bytes, 8).unwrap());
    let actor_end = usize::from(half(bytes, 12).unwrap());
    for row in bytes[actor_start..actor_end].chunks_exact(ACTOR_BYTES) {
        if !matches!(row[0], 22..=25) && row[0x32] != 255 {
            uv.insert(u16::from(row[0x32]) * 10);
        }
    }
    let mut modifiers = BTreeSet::new();
    for id in 0..bytes[4] {
        // Invalid timelines already produce their own failed program outcome.
        let Ok(timeline) = program_timeline(bytes, id) else {
            continue;
        };
        let mut payload = false;
        for row in timeline.chunks_exact(events::RECORD_BYTES) {
            let command = events::Record::read(row)
                .expect("complete event record")
                .command;
            match command {
                events::Command::End { .. } if !payload => break,
                events::Command::Emit {
                    modifier: offset, ..
                }
                | events::Command::ModifyRetained {
                    modifier: offset, ..
                } if offset != 0 => {
                    modifiers.insert(offset);
                }
                _ => (),
            }
            payload = !payload && matches!(command, events::Command::Repeat { .. });
        }
    }
    (uv, modifiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reconstructed(published: &serde_json::Value) -> Result<Vec<u8>> {
        ensure!(
            published["failure"].is_null(),
            "published bank has a failure"
        );
        ensure!(
            published["roots"]
                .as_array()
                .context("missing roots")?
                .iter()
                .all(|root| root["status"] == "cooked"),
            "published bank has unsupported roots"
        );
        let header: Header = serde_json::from_value(published["header"].clone())?;
        let mut bytes = vec![
            0;
            published["source_size"]
                .as_u64()
                .context("missing source size")? as usize
        ];
        let mut written = vec![false; bytes.len()];
        let mut put = |at: usize, source: &[u8]| -> Result<()> {
            let end = at
                .checked_add(source.len())
                .context("reconstruction offset overflow")?;
            let target = bytes
                .get_mut(at..end)
                .context("record exceeds source extent")?;
            for (index, (&value, previous)) in source.iter().zip(target.iter_mut()).enumerate() {
                ensure!(
                    !written[at + index] || *previous == value,
                    "overlapping records disagree at {:#x}",
                    at + index
                );
                *previous = value;
                written[at + index] = true;
            }
            Ok(())
        };
        put(0, b"ef1\0")?;
        put(4, &[header.program_count, header.uv_count])?;
        for (at, value) in [
            (6, header.storage),
            (8, header.actors),
            (10, header.events),
            (12, header.modifiers),
            (14, header.uv),
            (16, header.program_table),
            (18, header.uv_table),
        ] {
            put(at, &value.to_be_bytes())?;
        }
        let actors: BTreeMap<u16, actor::Record> =
            serde_json::from_value(published["actors"].clone())?;
        for (&id, actor) in &actors {
            put(
                usize::from(header.actors) + usize::from(id) * ACTOR_BYTES,
                &actor.source_bytes()?,
            )?;
        }
        let events: BTreeMap<u16, events::Record> =
            serde_json::from_value(published["program_records"].clone())?;
        for (&offset, record) in &events {
            put(usize::from(offset), &record.source_bytes())?;
        }
        let program_roots: Vec<i16> = serde_json::from_value(published["program_roots"].clone())?;
        ensure!(
            program_roots.len() == usize::from(header.program_count),
            "program root count changed"
        );
        for (index, &relative) in program_roots.iter().enumerate() {
            put(
                usize::from(header.program_table) + index * 2,
                &relative.to_be_bytes(),
            )?;
            let start = u16::try_from(i32::from(header.events) + i32::from(relative))?;
            ensure!(
                events.contains_key(&start),
                "program root {index} has no physical row"
            );
        }
        let modifier_records: BTreeMap<u16, std::result::Result<modifiers::Record, String>> =
            serde_json::from_value(published["modifier_records"].clone())?;
        for (offset, record) in modifier_records {
            put(
                usize::from(offset),
                &record.map_err(anyhow::Error::msg)?.bytes(),
            )?;
        }
        let uv: BTreeMap<u16, uv::Record> = serde_json::from_value(published["uv_rows"].clone())?;
        for (offset, record) in uv {
            put(
                usize::from(header.uv) + usize::from(offset),
                &record.bytes(),
            )?;
        }
        let uv_table: Vec<u16> = serde_json::from_value(published["uv_table"].clone())?;
        ensure!(
            uv_table.len() == usize::from(header.uv_count),
            "UV root count changed"
        );
        for (index, relative) in uv_table.into_iter().enumerate() {
            put(
                usize::from(header.uv_table) + index * 2,
                &relative.to_be_bytes(),
            )?;
        }
        for storage in serde_json::from_value::<Vec<Storage>>(published["storage"].clone())? {
            put(storage.offset, &storage.bytes)?;
        }
        Ok(bytes)
    }

    #[test]
    #[ignore = "requires both extracted discs and cook-all; no codecs or devices"]
    fn original_effect_banks_reconstruct_every_published_byte() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let output = local.join("all-assets");
        let sources: BTreeMap<String, Vec<String>> =
            serde_json::from_slice(&fs::read(output.join("sources.json"))?)?;
        for disc in [1, 2] {
            let cooker = Cooker::read(&local.join(format!("extracted/disc{disc}")))?;
            let directories: BTreeSet<_> = sources
                .iter()
                .filter(|(source, _)| source.starts_with(&format!("disc{disc}/")))
                .flat_map(|(_, paths)| paths)
                .collect();
            let (mut banks, mut rows, mut storage, mut wide_palettes, mut zero_duration) =
                (0, 0, 0, 0, 0);
            for directory in directories {
                let directory = output.join(directory).join("battle/all/effects");
                if !directory.is_dir() {
                    continue;
                }
                for file in fs::read_dir(directory)? {
                    let published: serde_json::Value =
                        serde_json::from_slice(&fs::read(file?.path())?)?;
                    let bank: EffectSourceBank = serde_json::from_value(published["bank"].clone())?;
                    let bytes = cooker.load(binding(bank)?)?;
                    let restored = reconstructed(&published)
                        .with_context(|| format!("disc{disc} {bank:?}"))?;
                    ensure!(
                        restored == bytes,
                        "disc{disc} {bank:?} full bank differs at {:?}",
                        restored.iter().zip(&bytes).position(|(a, b)| a != b)
                    );
                    assert_eq!(published["sha256"], format!("{:x}", Sha256::digest(&bytes)));
                    let bytes = bank_bytes(&bytes)?;
                    let pool = uv::rows(bytes)?;
                    let restored: BTreeMap<u16, uv::Record> =
                        serde_json::from_value(published["uv_rows"].clone())?;
                    assert_eq!(
                        restored.keys().collect::<Vec<_>>(),
                        pool.keys().collect::<Vec<_>>()
                    );
                    assert_eq!(
                        published["uv_roots"],
                        serde_json::to_value(stream_offsets(bytes).0)?
                    );
                    let start = usize::from(half(bytes, 14)?);
                    for (offset, record) in restored {
                        let at = start + usize::from(offset);
                        assert_eq!(
                            record.bytes(),
                            &bytes[at..at + 10],
                            "{bank:?} UV offset {offset}"
                        );
                        storage += usize::from(record.control != 0 && record.timing != 255);
                        wide_palettes += usize::from(
                            record.timing < 128
                                && record.values[0] == -32000
                                && !(0..=255).contains(&record.values[1]),
                        );
                        zero_duration += usize::from(record.timing == 0);
                        rows += 1;
                    }
                    banks += 1;
                }
            }
            assert_eq!(banks, 419, "published effect-bank coverage");
            println!(
                "disc{disc}: {banks} complete banks, {rows} UV rows, {storage} stored control bytes, {wide_palettes} wide palettes, {zero_duration} zero durations"
            );
        }
        Ok(())
    }

    #[test]
    fn authored_roots_and_streams_are_independent_of_runtime_support() -> Result<()> {
        let decode = |bytes: &[u8]| -> Result<BankCook> {
            let mut result = BankCook::new(EffectSourceBank::Common);
            decode_bank(bytes, &mut result)?;
            let published = serde_json::to_value(&result)?;
            ensure!(
                reconstructed(&published)? == bytes,
                "synthetic bank reconstruction changed bytes"
            );
            Ok(result)
        };
        let mut bytes = vec![0; 418];
        bytes[..6].copy_from_slice(b"ef1\0\x01\x01");
        for (field, value) in [
            (8, 20u16),
            (10, 402),
            (12, 372),
            (14, 382),
            (16, 414),
            (18, 416),
        ] {
            bytes[field..field + 2].copy_from_slice(&value.to_be_bytes());
        }
        let row = &mut bytes[20..372];
        row[0] = 3;
        row[2] = 8; // A caller-supplied model slot, not a fixed package alias.
        row[0x32] = 255;
        row[0x91] = 8;
        row[0x92] = 7;
        row[0x93] = 3;
        row[0x14..0x18].copy_from_slice(&0x80000000_u32.to_be_bytes());
        bytes[372..382].copy_from_slice(&[0, 11, 0x7f, 0xf8, 0, 10, 0x7f, 0xf8, 255, 255]);
        bytes[382..392].copy_from_slice(&[1, 0, 0x83, 0, 0, 7, 0, 0, 0, 0]);
        bytes[392] = 254;
        bytes[402..414].copy_from_slice(&[0, 0, 0, 0, 1, 116, 0, 1, 254, 0, 0, 0]);
        let result = decode(&bytes)?;
        assert_eq!(result.program_roots, [0]);
        assert_eq!(
            result.program_records.keys().copied().collect::<Vec<_>>(),
            [402, 408]
        );
        assert!(result.uv_roots.contains(&0) && result.modifier_roots.contains(&372));
        assert_eq!(result.uv_rows[&10].timing, 254);
        assert!(matches!(
            &result.modifier_records[&380],
            Ok(modifiers::Record::End)
        ));
        let prefix = &result.actors[&0].prefix;
        assert_eq!(prefix.resource_slot, 8);
        assert_eq!(
            (prefix.secondary.periodic_program, prefix.secondary.period),
            (7, 3)
        );
        assert!(
            super::super::test_cooker()
                .program(
                    &bytes,
                    EffectId {
                        bank: EffectBank::Common,
                        id: 0
                    }
                )
                .is_err()
        );
        bytes[20] = 255;
        let result = decode(&bytes)?;
        assert!(result.program_records.contains_key(&402));
        assert_eq!(result.actors[&0].prefix.kind, 255);

        // Aliased negative roots coexist with a dormant event suffix, partial
        // pool padding, modifier-End alignment, and bytes beyond both directories.
        let mut bytes = vec![0; 80];
        bytes[..6].copy_from_slice(b"ef1\0\x02\0");
        for (at, value) in [
            (6, 0xabcd_u16),
            (8, 32),
            (10, 40),
            (12, 32),
            (14, 68),
            (16, 68),
            (18, 72),
        ] {
            bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
        }
        bytes[20..32].copy_from_slice(&[0, 1, 252, 99, 0x12, 0x34, 0, 2, 254, 9, 0xab, 0xcd]);
        bytes[32..40].copy_from_slice(&[255, 255, 1, 2, 255, 255, 3, 4]);
        bytes[40..64].copy_from_slice(&[
            0, 3, 252, 1, 0, 0, 0, 4, 254, 8, 0x12, 0x34, 0, 5, 255, 3, 0, 8, 0x80, 1, 252, 87,
            0xab, 0xcd,
        ]);
        bytes[64..68].copy_from_slice(&[5, 6, 7, 8]);
        bytes[68..72].copy_from_slice(&[(-20i16).to_be_bytes(); 2].concat());
        bytes[72..].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        let result = decode(&bytes)?;
        assert_eq!(result.header.unwrap().storage, 0xabcd);
        assert_eq!(result.program_roots, [-20, -20]);
        assert_eq!(
            result.program_records.keys().copied().collect::<Vec<_>>(),
            [20, 26, 40, 46, 52, 58]
        );
        assert_eq!(
            result
                .storage
                .iter()
                .map(|storage| storage.offset)
                .collect::<Vec<_>>(),
            [34, 38, 64, 72]
        );

        let mut malformed = vec![0; 400];
        malformed[..6].copy_from_slice(b"ef1\0\x01\x02");
        for (at, value) in [
            (8, 46u16),
            (10, 500),
            (12, 398),
            (14, 500),
            (16, 394),
            (18, 396),
        ] {
            malformed[at..at + 2].copy_from_slice(&value.to_be_bytes());
        }
        assert!(decode_bank(&malformed, &mut BankCook::new(EffectSourceBank::Common)).is_err());
        Ok(())
    }
}
