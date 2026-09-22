//! Convert physical sound banks, arrangements and voice files without a selection.
use crate::{
    all_assets::{
        MemberResults,
        roles::{self, Role},
    },
    media::{self, VoiceFormat, Workspace},
    source_assets::Sources,
};
use anyhow::{Context, Result, ensure};
use resonance_audio::data::Score;
use resonance_audio_cook::{
    bank::{Bank, MissingObject, ObjectKind, Sound},
    decode::{self, Resources},
    instrument, pool,
    song::{EventKind, Song},
};
use resonance_content::field_audio::archive::VoiceArchive;
use serde::Serialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

pub(crate) fn is_stream(bytes: &[u8]) -> bool {
    stream_format(bytes).is_some()
}

/// Undeclared audio still needs a complete structural parse before admission.
pub(crate) fn structural_role(bytes: &[u8]) -> Option<Role> {
    match audio_header(bytes, bytes.len() as u64)? {
        Role::SoundBank if Bank::parse(bytes).is_ok() => Some(Role::SoundBank),
        Role::Song if Song::parse(bytes).is_ok() => Some(Role::Song),
        _ => None,
    }
}

fn audio_header(bytes: &[u8], length: u64) -> Option<Role> {
    let word = |at| crate::read::u32(bytes, at).ok().map(u64::from);
    if length <= 128 * 1024 * 1024 && word(0)? == 4 {
        let mut ranges = Vec::new();
        for index in 0..4 {
            let start = word(4 + index * 8)?;
            let size = word(8 + index * 8)?;
            if index == 1 && start == 0 && size == 0 {
                continue;
            }
            let end = start.checked_add(size)?;
            if start < 36
                || size == 0
                || end > length
                || ranges.iter().any(|&(a, b)| start < b && end > a)
            {
                return None;
            }
            ranges.push((start, end));
        }
        return Some(Role::SoundBank);
    }
    if length > 16 * 1024 * 1024 {
        return None;
    }
    for (field, size) in [(0, 256), (4, 4), (8, 64), (12, 4)] {
        let offset = word(field)?;
        if field == 12 && offset == 0 {
            continue;
        }
        if offset < 24 || offset.checked_add(size)? > length {
            return None;
        }
    }
    Some(Role::Song)
}

pub(crate) fn file_role(path: &Path) -> Result<Option<Role>> {
    let mut file = fs::File::open(path)?;
    let mut header = [0; 36];
    let length = file.metadata()?.len();
    if length < header.len() as u64 {
        return Ok(None);
    }
    file.read_exact(&mut header)?;
    if audio_header(&header, length).is_none() {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(structural_role(&bytes))
}

pub(crate) fn bank_sources(extracted: &Path, executable: &[u8]) -> Result<Vec<String>> {
    let files = extracted.join("files");
    let mut sources: BTreeSet<_> = roles::audio_paths(extracted, executable)?
        .into_iter()
        .filter_map(|(path, role)| (role == Role::SoundBank).then_some(path))
        .collect();
    let mut directories = vec![files.clone()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                directories.push(path);
            } else if entry.file_type()?.is_file() && file_role(&path)? == Some(Role::SoundBank) {
                sources.insert(
                    path.strip_prefix(&files)?
                        .to_str()
                        .context("non-UTF8 audio source")?
                        .into(),
                );
            }
        }
    }
    Ok(sources.into_iter().collect())
}

fn song_setups(
    extracted: &Path,
    directory: &crate::music_directory::Directory,
    files: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, Vec<u16>>> {
    let mut setups = BTreeMap::<_, Vec<_>>::new();
    let mut seen = BTreeSet::new();
    for entry in directory.active().filter(|entry| seen.insert(entry.id)) {
        let source =
            crate::field_resources::resolve_path(&extracted.join("files"), &entry.source_path()?)?;
        let hash = files
            .get(&source)
            .with_context(|| format!("declared song absent from inventory: {source}"))?;
        setups
            .entry(hash.clone())
            .or_default()
            .push(entry.id as u16);
    }
    for ids in setups.values_mut() {
        ids.sort_unstable();
        ids.dedup();
    }
    Ok(setups)
}

fn stream_format(bytes: &[u8]) -> Option<VoiceFormat> {
    if !bytes.starts_with(&[0x80, 0]) {
        return None;
    }
    match bytes.get(4)? {
        0x10 | 0x11 => Some(VoiceFormat::Ahx),
        2..=4 => Some(VoiceFormat::Adx),
        _ => None,
    }
}

fn resource_key(environment: &str, source: &str, setups: &[u16]) -> String {
    crate::digest(format!("{environment}:{source}:{setups:?}").as_bytes())
}

pub(crate) struct ArchiveMember {
    pub label: String,
    archive: Arc<PathBuf>,
    directory: Arc<String>,
    id: usize,
    entry: crate::afs::Entry,
}

pub(crate) struct Cooker {
    workspace: Workspace,
    executable: Vec<u8>,
    coefficients: Vec<u8>,
    song_setups: BTreeMap<String, Vec<u16>>,
    pools: Arc<Pools>,
    environment: String,
}

#[derive(Serialize)]
struct NoteBinding {
    event: usize,
    voices: Vec<resonance_audio::data::Note>,
}

impl Cooker {
    pub(crate) fn new(
        extracted: &Path,
        session: &Arc<media::OutputSession>,
        coefficients: &Path,
        files: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        // Identical arrangements may be named by different instrument setups.
        // Gather all aliases before the file queue deduplicates their bytes.
        let song_setups = song_setups(
            extracted,
            &crate::music_directory::Directory::read(&executable)?,
            files,
        )?;
        let coefficients = fs::read(coefficients)?;
        let pools = Arc::new(Pools::read(extracted)?);
        let battle_sources = pools.sources()?;
        let mut dependencies = vec![
            crate::digest(&executable),
            crate::digest(&coefficients),
            pools.fingerprint()?,
        ];
        for source in [
            "US_r_Top2Btl.rel".into(),
            battle_sources.usual.clone(),
            battle_sources.enemy.clone(),
            roles::voice_bank_path(extracted)?,
        ] {
            let source = crate::field_resources::resolve_path(&extracted.join("files"), &source)?;
            dependencies.push(match files.get(&source) {
                Some(hash) => hash.clone(),
                None => media::hash_file(&extracted.join("files").join(source))?,
            });
        }
        Ok(Self {
            workspace: session.workspace(extracted)?,
            executable,
            coefficients,
            song_setups,
            pools,
            environment: crate::digest(&serde_json::to_vec(&dependencies)?),
        })
    }

    pub(crate) fn resource_key(&self, hash: &str) -> String {
        resource_key(
            &self.environment,
            hash,
            self.song_setups.get(hash).map(Vec::as_slice).unwrap_or(&[]),
        )
    }

    fn source(&self, source: &str) -> Result<PathBuf> {
        resonance_content::validate_asset_path(source)?;
        Ok(self.workspace.extracted.join("files").join(source))
    }

    /// Compile every local sample, macro and sound, retaining per-member errors.
    /// A physical bank has no intrinsic field/battle reverb environment.
    pub(crate) fn cook_bank(&self, source: &str) -> Result<MemberResults> {
        let bytes = fs::read(self.source(source)?)?;
        let directory = format!("audio/banks/{}", self.resource_key(&crate::digest(&bytes)));
        cook_bank_data(
            &self.workspace.output,
            &directory,
            Bank::parse(&bytes)?,
            &self.pools,
        )
    }

    pub(crate) fn cook_bank_archive(&self, source: &str) -> Result<MemberResults> {
        let path = self.source(source)?;
        let namespace = format!(
            "audio/bank-archives/{}",
            self.resource_key(&media::hash_file(&path)?)
        );
        let (mut archive, directory) = bank_archive(
            &self.workspace.extracted,
            source,
            &self.pools.sources()?.usual,
        )?;
        let banks = self
            .pools
            .others
            .iter()
            .map(|bytes| Bank::parse(bytes))
            .collect::<Result<Vec<_>>>()?;
        let groups = banks.iter().map(Bank::group).collect::<Result<Vec<_>>>()?;
        let mut results = Vec::new();
        for (index, range) in directory.windows(2).enumerate() {
            let group = u16::try_from(index)?
                .checked_add(19)
                .context("sample group overflow")?;
            let result = (|| -> Result<MemberResults> {
                if range[0] == range[1] {
                    return Ok(vec![(
                        "empty".into(),
                        json_file(
                            &self.workspace.output,
                            &format!("{namespace}/{group}.json"),
                            &json!({"group":group,"samples":[]}),
                        )
                        .map(|path| vec![path]),
                    )]);
                }
                let index = groups
                    .iter()
                    .position(|&id| id == group)
                    .with_context(|| format!("sample group {group} has no source directory"))?;
                let payload = payload(&mut archive, range[0], range[1])?;
                let bank = Bank::parse(&self.pools.others[index])?.with_sample_payload(&payload);
                cook_bank_data(
                    &self.workspace.output,
                    &format!("{namespace}/{group}"),
                    bank,
                    &self.pools,
                )
            })();
            match result {
                Ok(members) => results.extend(
                    members
                        .into_iter()
                        .map(|(name, result)| (format!("group/{group}/{name}"), result)),
                ),
                Err(error) => results.push((format!("group/{group}"), Err(error))),
            }
        }
        Ok(results)
    }

    pub(crate) fn cook_song(&self, source: &str) -> Result<MemberResults> {
        let bytes = fs::read(self.source(source)?)?;
        let hash = crate::digest(&bytes);
        let directory = format!("audio/songs/{}", self.resource_key(&hash));
        let song = Song::parse(&bytes)?;
        let mut results = vec![(
            "arrangement".into(),
            self.json(&format!("{directory}/arrangement.json"), &song)
                .map(|path| vec![path]),
        )];
        let ids = self
            .song_setups
            .get(&hash)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let bank = self.pools.instruments();
        let mut playback = Vec::new();
        for &id in ids {
            let path = format!("{directory}/setup-{id}.json");
            let result = (|| -> Result<Vec<String>> {
                let bank = bank
                    .as_ref()
                    .map_err(|error| anyhow::anyhow!("{error:#}"))?;
                let setup = bank.music_setup(0, id)?;
                let mut roots = std::collections::BTreeSet::new();
                let mut programs = setup.channels.map(|channel| channel.program);
                let first = song.events();
                let first_event_count = first.len();
                let mut notes = Vec::new();
                for (index, event) in first.into_iter().chain(song.loop_events()?).enumerate() {
                    let channel = usize::from(event.channel);
                    match event.kind {
                        EventKind::Pattern {
                            program: Some(program),
                            ..
                        }
                        | EventKind::Command {
                            command: 0,
                            value: program,
                        } => {
                            if setup.page(event.channel, program).is_some() {
                                programs[channel] = program;
                            }
                        }
                        EventKind::Note { key, velocity, .. } => {
                            if let Some(page) = setup.page(event.channel, programs[channel]) {
                                let voices = instrument::resolve(bank, page, key, velocity, 64)?;
                                roots.extend(voices.iter().map(|note| note.macro_id));
                                notes.push(NoteBinding {
                                    event: index,
                                    voices,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                let resources = decode::programs(bank, roots)?;
                let samples = samples(&self.workspace.output, &resources)?;
                let mut files: Vec<_> =
                    samples.values().map(|sample| sample.path.clone()).collect();
                self.json(&path, &json!({"version":1, "setup":setup, "note_bindings":notes, "first_event_count":first_event_count, "programs":resources.programs,
                "samples":samples, "tables":media::synthesis_tables(&self.executable, &self.coefficients)?,
                "reverb":media::song_reverb_change(&self.executable, id)?}))?;
                files.push(path.clone());
                files.sort();
                files.dedup();
                Ok(files)
            })();
            if result.is_ok() {
                playback.push(path);
            }
            results.push((format!("setup-{id}"), result));
        }
        // Unbound arrangements are still converted; no guessed instruments or reverb.
        results.push((
            "playback".into(),
            self.json(&format!("{directory}/playback.json"), &playback)
                .map(|path| vec![path]),
        ));
        Ok(results)
    }

    pub(crate) fn archive_members(
        &self,
        source: &str,
        sha256: &str,
    ) -> Result<(String, Vec<ArchiveMember>)> {
        let path = Arc::new(self.source(source)?);
        let directory = Arc::new(format!("audio/archives/{sha256}"));
        let mut file = fs::File::open(&*path)?;
        let (index, entries) = media::voice_library::directory(&mut file, sha256)?;
        let index_path = self.json(&VoiceArchive::path(sha256)?, &index)?;
        let members = entries
            .into_iter()
            .enumerate()
            .map(|(id, entry)| ArchiveMember {
                label: format!("{source}/{id}/{}", entry.name),
                archive: path.clone(),
                directory: directory.clone(),
                id,
                entry,
            })
            .collect();
        Ok((index_path, members))
    }

    pub(crate) fn cook_member(&self, member: &ArchiveMember) -> Result<Vec<String>> {
        ensure!(
            member.entry.size <= 128 * 1024 * 1024,
            "voice member exceeds encoded read budget"
        );
        let mut file = fs::File::open(&*member.archive)?;
        file.seek(SeekFrom::Start(member.entry.offset))?;
        let mut bytes = vec![0; member.entry.size];
        file.read_exact(&mut bytes)?;
        self.voice(
            &format!("{}/{}.json", member.directory, member.id),
            &crate::afs::Member {
                name: &member.entry.name,
                data: &bytes,
            },
        )
    }

    pub(crate) fn cook_stream(&self, source: &str) -> Result<Vec<String>> {
        let path = self.source(source)?;
        ensure!(
            fs::metadata(&path)?.len() <= 128 * 1024 * 1024,
            "voice file exceeds encoded read budget"
        );
        let bytes = fs::read(path)?;
        let name = Path::new(source)
            .file_name()
            .and_then(|name| name.to_str())
            .context("invalid voice filename")?;
        self.voice(
            &format!("audio/streams/{}.json", crate::digest(&bytes)),
            &crate::afs::Member { name, data: &bytes },
        )
    }

    /// Nested containers do not always supply filenames. Keep each source label
    /// in its own metadata record while sharing byte-identical decoded PCM.
    pub(crate) fn cook_embedded_stream(&self, bytes: &[u8], name: &str) -> Result<Vec<String>> {
        ensure!(
            bytes.len() <= 128 * 1024 * 1024,
            "embedded voice exceeds encoded read budget"
        );
        self.voice(
            &format!(
                "audio/embedded-streams/{}/{}.json",
                crate::digest(bytes),
                crate::digest(name.as_bytes())
            ),
            &crate::afs::Member { name, data: bytes },
        )
        .with_context(|| format!("decode embedded voice {name}"))
    }

    fn voice(&self, metadata: &str, member: &crate::afs::Member<'_>) -> Result<Vec<String>> {
        let format = stream_format(member.data)
            .context("invalid signature or unsupported CRI voice encoding")?;
        let frames = crate::read::u32(member.data, 12)?;
        if frames == 0 {
            return Ok(vec![self.json(
                metadata,
                &json!({"version":1, "silence":true, "frames":0,
                    "source_name":member.name, "sample_rate":crate::read::u32(member.data, 8)?,
                    "channels":member.data.get(7).context("truncated voice header")?}),
            )?]);
        }
        let path = format!("audio/streams/{}.wav", crate::digest(member.data));
        let voice = media::decode_voice_to(&self.workspace, &path, member, format)?;
        ensure!(
            voice.frames == frames,
            "decoded voice length differs from source"
        );
        Ok(vec![path, self.json(metadata, &voice)?])
    }

    fn json(&self, path: &str, data: &impl Serialize) -> Result<String> {
        json_file(&self.workspace.output, path, data)
    }
}

pub(crate) struct Pools {
    extracted: PathBuf,
    common: Vec<u8>,
    instruments: Vec<u8>,
    others: Vec<Vec<u8>>,
    isolated: Vec<Vec<u8>>,
    sources: OnceLock<std::result::Result<Sources, String>>,
    available: OnceLock<std::result::Result<BTreeSet<(ObjectKind, u16)>, String>>,
}

impl Pools {
    pub(crate) fn read(extracted: &Path) -> Result<Self> {
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let [instruments, common] = roles::resident_banks(extracted, &executable)?;
        let sources = bank_sources(extracted, &executable)?;
        let isolated = roles::party_banks(extracted, &executable)?;
        Self::from_sources(extracted, &instruments, &common, &sources, &isolated)
    }

    fn from_sources(
        extracted: &Path,
        instruments: &str,
        common: &str,
        sources: &[String],
        isolated: &[String],
    ) -> Result<Self> {
        let read = |source: &str| -> Result<Vec<u8>> {
            let bytes = fs::read(extracted.join("files").join(source))?;
            Bank::parse(&bytes).with_context(|| format!("sound bank {source}"))?;
            Ok(bytes)
        };
        let mut others = Vec::new();
        let mut local = Vec::new();
        for source in sources
            .iter()
            .filter(|source| source.as_str() != instruments && source.as_str() != common)
        {
            let target = if isolated.contains(source) {
                &mut local
            } else {
                &mut others
            };
            target.push(read(source)?);
        }
        Ok(Self {
            extracted: extracted.into(),
            common: read(common)?,
            instruments: read(instruments)?,
            others,
            isolated: local,
            sources: OnceLock::new(),
            available: OnceLock::new(),
        })
    }

    fn sources(&self) -> Result<&Sources> {
        self.sources
            .get_or_init(|| Sources::read(&self.extracted).map_err(|error| format!("{error:#}")))
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))
    }

    pub(crate) fn fingerprint(&self) -> Result<String> {
        let hashes = |banks: &[Vec<u8>]| {
            let mut hashes: Vec<_> = banks.iter().map(|bytes| crate::digest(bytes)).collect();
            hashes.sort();
            hashes.dedup();
            hashes
        };
        Ok(crate::digest(&serde_json::to_vec(&(
            crate::digest(&self.common),
            crate::digest(&self.instruments),
            hashes(&self.others),
            hashes(&self.isolated),
        ))?))
    }

    pub(crate) fn instruments(&self) -> Result<Bank<'_>> {
        self.bank(&self.instruments)
    }

    pub(crate) fn bank<'a>(&'a self, bytes: &'a [u8]) -> Result<Bank<'a>> {
        let mut bank = Bank::parse(bytes)?;
        self.inherit(&mut bank)?;
        Ok(bank)
    }

    fn inherit<'a>(&'a self, bank: &mut Bank<'a>) -> Result<()> {
        let common = Bank::parse(&self.common)?;
        let instruments = Bank::parse(&self.instruments)?;
        bank.inherit_objects(&common);
        bank.inherit_objects(&instruments);
        // Party groups are loaded separately; their local objects are not fallback definitions.
        bank.inherit_unique_objects(
            &self
                .others
                .iter()
                .map(|bytes| Bank::parse(bytes))
                .collect::<Result<Vec<_>>>()?,
        );
        bank.inherit_samples(&instruments);
        bank.inherit_samples(&common);
        Ok(())
    }

    fn absent(&self, dependency: &MissingObject) -> Result<bool> {
        let available = self
            .available
            .get_or_init(|| available_objects(self).map_err(|error| format!("{error:#}")))
            .as_ref()
            .map_err(|error| {
                anyhow::anyhow!("cannot establish source dependency availability: {error}")
            })?;
        Ok(!available.contains(&(dependency.kind, dependency.id)))
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum SoundDeclaration {
    UnavailableSourceDependency {
        source: Sound,
        dependency: MissingObject,
    },
}

fn cook_bank_data<'a>(
    output: &Path,
    directory: &str,
    mut bank: Bank<'a>,
    pools: &'a Pools,
) -> Result<MemberResults> {
    let sample_ids = bank.sample_ids()?;
    let programs: Vec<_> = bank.object_ids(ObjectKind::Macro).collect();
    let mappings: Vec<_> = [ObjectKind::Keymap, ObjectKind::Layer, ObjectKind::Table]
        .into_iter()
        .flat_map(|kind| bank.object_ids(kind).map(move |id| (kind, id)))
        .collect();
    let sounds: Vec<_> = bank.sound_ids().collect();
    let definitions = (|| {
        let sounds = sounds
            .iter()
            .map(|&id| Ok((id, bank.sound(id)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        json_file(
            output,
            &format!("{directory}/definitions.json"),
            &json!({"group":bank.group()?, "sounds":sounds, "music_setups":bank.music_setups()?}),
        )
        .map(|path| vec![path])
    })();
    pools.inherit(&mut bank)?;
    let mut results = vec![("definitions".into(), definitions)];
    let table_uses = pool::table_uses(&bank);
    for (kind, id) in mappings {
        let name = match kind {
            ObjectKind::Keymap => "keymap",
            ObjectKind::Layer => "layer",
            ObjectKind::Table => "table",
            ObjectKind::Macro => unreachable!(),
        };
        let path = format!("{directory}/{name}-{id}.json");
        let result = (|| {
            let path = match kind {
                ObjectKind::Keymap => json_file(output, &path, &pool::keymap(&bank, id)?)?,
                ObjectKind::Layer => json_file(output, &path, &pool::layer(&bank, id)?)?,
                ObjectKind::Table => {
                    let uses = table_uses
                        .as_ref()
                        .map_err(|error| anyhow::anyhow!("{error:#}"))?;
                    json_file(output, &path, &pool::table(&bank, id, uses)?)?
                }
                ObjectKind::Macro => unreachable!(),
            };
            Ok(vec![path])
        })();
        results.push((format!("{name}/{id}"), result));
    }
    for id in sample_ids {
        results.push((
            format!("sample/{id}"),
            (|| {
                let asset = media::write_shared_sample(output, &bank.sample(id)?)?;
                let path = json_file(output, &format!("{directory}/sample-{id}.json"), &asset)?;
                Ok(vec![asset.path, path])
            })(),
        ));
    }
    for id in programs {
        let result = decode::programs(&bank, [id]).and_then(|resources| {
            resource_file(
                output,
                &format!("{directory}/program-{id}.json"),
                resources,
                None,
            )
        });
        results.push((format!("program/{id}"), result));
    }
    for id in sounds {
        let path = format!("{directory}/sound-{id}.json");
        let result = (|| {
            let (resources, score) = media::sound_library::sound(&bank, id)?;
            resource_file(output, &path, resources, Some(score))
        })();
        let result = result.or_else(|error| {
            if let Some(&dependency) = error.downcast_ref::<MissingObject>()
                && pools.absent(&dependency)?
            {
                let declaration = SoundDeclaration::UnavailableSourceDependency {
                    source: bank.sound(id)?,
                    dependency,
                };
                json_file(output, &path, &declaration).map(|path| vec![path])
            } else {
                Err(error)
            }
        });
        results.push((format!("sound/{id}"), result));
    }
    Ok(results)
}

fn samples(
    output: &Path,
    resources: &Resources,
) -> Result<BTreeMap<u16, resonance_audio::package::SampleAsset>> {
    resources
        .samples
        .iter()
        .map(|(&id, sample)| Ok((id, media::write_shared_sample(output, sample)?)))
        .collect()
}

fn resource_file(
    output: &Path,
    path: &str,
    resources: Resources,
    score: Option<Score>,
) -> Result<Vec<String>> {
    let samples = samples(output, &resources)?;
    let mut files: Vec<_> = samples.values().map(|sample| sample.path.clone()).collect();
    files.push(json_file(
        output,
        path,
        &media::sound_library::Resources {
            version: 1,
            programs: resources.programs,
            samples,
            score,
        },
    )?);
    Ok(files)
}

fn json_file(output: &Path, path: &str, data: &impl Serialize) -> Result<String> {
    crate::write_atomic(&output.join(path), &serde_json::to_vec_pretty(data)?)?;
    Ok(path.into())
}

fn embedded_sections<'a>(enemy: &'a [u8], samples: &'a [u8]) -> Result<[&'a [u8]; 4]> {
    use crate::read::u32 as word;
    let at = word(enemy, 0x1e4)? as usize;
    ensure!(at != 0, "enemy has no embedded sound bank");
    let bytes = enemy
        .get(at..)
        .context("embedded bank offset exceeds enemy package")?;
    let starts = [
        word(bytes, 0)? as usize,
        word(bytes, 4)? as usize,
        word(bytes, 8)? as usize,
    ];
    let mut sections = [&[][..]; 4];
    for (index, &start) in starts.iter().enumerate() {
        if index == 1 && start == 0 {
            continue;
        }
        ensure!(
            start >= 12 && start < bytes.len(),
            "invalid embedded bank section {index}"
        );
        let end = starts
            .iter()
            .copied()
            .filter(|&other| other > start)
            .min()
            .unwrap_or(bytes.len());
        sections[index] = &bytes[start..end];
    }
    sections[3] = samples;
    Ok(sections)
}

/// Prove absence across physical banks and every embedded enemy pool. This is
/// lazy and cached: successful sound conversion does not scan the enemy archive.
fn available_objects(pools: &Pools) -> Result<BTreeSet<(ObjectKind, u16)>> {
    let extracted = &pools.extracted;
    let mut objects = BTreeSet::new();
    let mut collect = |bank: Bank<'_>| {
        for kind in [
            ObjectKind::Macro,
            ObjectKind::Table,
            ObjectKind::Keymap,
            ObjectKind::Layer,
        ] {
            objects.extend(bank.object_ids(kind).map(|id| (kind, id)));
        }
    };
    for bytes in [&pools.common, &pools.instruments]
        .into_iter()
        .chain(&pools.others)
        .chain(&pools.isolated)
    {
        collect(Bank::parse(bytes)?);
    }
    let sources = pools.sources()?;
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let offsets = crate::source_assets::section(&usual, 10)?
        .chunks_exact(4)
        .map(|bytes| crate::read::u32(bytes, 0))
        .collect::<Result<Vec<_>>>()?;
    let mut archive = fs::File::open(extracted.join("files").join(&sources.enemy))?;
    for (id, range) in
        crate::source_assets::physical_ranges(&offsets, 0, archive.metadata()?.len())?
    {
        let bytes = crate::compression::decode(&payload(
            &mut archive,
            range.start.try_into()?,
            range.end.try_into()?,
        )?)
        .with_context(|| format!("scan enemy {id} sound pool"))?;
        ensure!(
            bytes.starts_with(b"em8\0"),
            "invalid enemy {id} package during sound-pool scan"
        );
        if crate::read::u32(&bytes, 0x1e4)? != 0 {
            collect(Bank::from_sections(embedded_sections(&bytes, &[])?)?);
        }
    }
    Ok(objects)
}

fn bank_archive(extracted: &Path, source: &str, usual: &str) -> Result<(fs::File, Vec<u32>)> {
    resonance_content::validate_asset_path(source)?;
    let archive = fs::File::open(extracted.join("files").join(source))?;
    let directory = payload_directory(
        &extracted.join("files").join(usual),
        archive.metadata()?.len(),
    )?;
    Ok((archive, directory))
}

fn payload_directory(source: &Path, length: u64) -> Result<Vec<u32>> {
    use crate::read::u32 as word;
    let mut usual = fs::File::open(source)?;
    let mut header = [0; 64];
    usual.read_exact(&mut header)?;
    let count = word(&header, 0)?;
    ensure!(count > 13, "missing sample payload directory");
    let start = u64::from(word(&header, 56)?);
    let end = if count == 14 {
        usual.metadata()?.len()
    } else {
        u64::from(word(&header, 60)?)
    };
    ensure!(
        end > start && end - start <= 65536 && (end - start).is_multiple_of(4),
        "invalid sample payload directory range"
    );
    usual.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; (end - start) as usize];
    usual.read_exact(&mut bytes)?;
    let mut offsets = bytes
        .chunks_exact(4)
        .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    let length = u32::try_from(length)?;
    let last = offsets
        .iter()
        .position(|&offset| offset == length)
        .context("sample payload directory does not cover archive")?;
    ensure!(
        last > 0
            && offsets[0] == 0
            && offsets[..=last].windows(2).all(|pair| pair[0] <= pair[1])
            && offsets[last + 1..]
                .iter()
                .all(|&value| matches!(value, 0 | 0xfefefefe)),
        "invalid sample payload offsets or padding"
    );
    offsets.truncate(last + 1);
    Ok(offsets)
}

fn payload(archive: &mut fs::File, start: u32, end: u32) -> Result<Vec<u8>> {
    ensure!(
        end >= start
            && end - start <= 128 * 1024 * 1024
            && u64::from(end) <= archive.metadata()?.len(),
        "invalid sample payload range"
    );
    archive.seek(SeekFrom::Start(u64::from(start)))?;
    let mut bytes = vec![0; (end - start) as usize];
    archive.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renamed_banks_and_songs_keep_their_roles_aliases_and_publications() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("renamed-audio"));
        let result = (|| -> Result<()> {
            fs::create_dir_all(root.join("files"))?;
            fs::create_dir_all(root.join("sys"))?;
            fs::write(root.join("sys/boot.bin"), b"GQSEAF\0\0")?;
            let mut bank = vec![0; 93];
            for (at, value) in [
                (0, 4u32),
                (4, 36),
                (8, 36),
                (12, 72),
                (16, 16),
                (20, 88),
                (24, 4),
                (28, 92),
                (32, 1),
                (36, u32::MAX),
                (64, 32),
                (88, u32::MAX),
            ] {
                bank[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            bank[40..44].copy_from_slice(&[0, 19, 0, 1]);
            fs::write(root.join("files/Instruments.song"), &bank)?;
            fs::write(root.join("files/Common.payload"), &bank)?;
            let mut song = vec![0; 360];
            for (at, value) in [(0, 24u32), (4, 356), (8, 280), (16, 120), (24, 344)] {
                song[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            song[352..354].fill(255);
            fs::write(root.join("files/Music.snd"), &song)?;
            assert_eq!(structural_role(&bank), Some(Role::SoundBank));
            assert_eq!(file_role(&root.join("files/Music.snd"))?, Some(Role::Song));
            assert_eq!(structural_role(&bank[..92]), None);
            let mut damaged = song.clone();
            damaged[280] = 16;
            assert_eq!(structural_role(&damaged), None);
            let files = BTreeMap::from([("Music.snd".into(), crate::digest(&song))]);
            let entry = |id, file: &str| crate::music_directory::Entry {
                id,
                buffer: crate::music_directory::Buffer::Resident,
                file: Some(file.into()),
            };
            let directory = crate::music_directory::Directory {
                entries: vec![
                    entry(7, "music.SND"),
                    entry(8, "Music.snd"),
                    entry(7, "missing.song"),
                    entry(-1, ""),
                ],
            };
            assert_eq!(
                song_setups(&root, &directory, &files)?[&crate::digest(&song)],
                [7, 8]
            );
            fs::write(root.join("files/Party.resource"), &bank)?;
            fs::write(root.join("files/Event.resource"), &bank)?;
            let pools = Pools::from_sources(
                &root,
                "Instruments.song",
                "Common.payload",
                &["Party.resource".into(), "Event.resource".into()],
                &["Party.resource".into()],
            )?;
            assert_eq!(pools.others.len(), 1);
            assert_eq!(pools.isolated.len(), 1);
            let output = root.join("cooked");
            let cooker = Cooker {
                workspace: Workspace::open(&root, &output)?,
                executable: vec![],
                coefficients: vec![],
                song_setups: BTreeMap::new(),
                pools: Arc::new(pools),
                environment: "synthetic".into(),
            };
            for members in [
                cooker.cook_bank("Instruments.song")?,
                cooker.cook_song("Music.snd")?,
            ] {
                for (_, paths) in members {
                    for path in paths? {
                        assert!(output.join(path).is_file());
                    }
                }
            }
            let playback: Vec<String> = serde_json::from_slice(&fs::read(output.join(format!(
                "audio/songs/{}/playback.json",
                cooker.resource_key(&crate::digest(&song))
            )))?)?;
            assert!(
                playback.is_empty(),
                "unused songs must not invent an instrument setup"
            );
            fs::write(root.join("files/Common.payload"), [0; 40])?;
            assert!(
                Pools::from_sources(&root, "Instruments.song", "Common.payload", &[], &[]).is_err()
            );
            Ok(())
        })();
        if root.exists() {
            fs::remove_dir_all(root)?;
        }
        result
    }

    #[test]
    fn compiled_audio_keys_include_dependencies_and_setup_bindings() {
        let key = resource_key("resident-a", "source", &[3, 7]);
        assert_eq!(key, resource_key("resident-a", "source", &[3, 7]));
        assert_ne!(key, resource_key("resident-b", "source", &[3, 7]));
        assert_ne!(key, resource_key("resident-a", "other", &[3, 7]));
        assert_ne!(key, resource_key("resident-a", "source", &[3]));
    }

    #[test]
    #[ignore = "requires both original extracted discs; parses audio without conversion or playback"]
    fn original_audio_structures_cover_declared_and_undeclared_resources() -> Result<()> {
        for disc in [1, 2] {
            let extracted = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            let declared = roles::audio_paths(&extracted, &executable)?;
            let files = extracted.join("files");
            let mut directories = vec![files.clone()];
            let mut discovered = BTreeMap::new();
            while let Some(directory) = directories.pop() {
                for entry in fs::read_dir(directory)? {
                    let entry = entry?;
                    let path = entry.path();
                    if entry.file_type()?.is_dir() {
                        directories.push(path);
                    } else if entry.file_type()?.is_file() {
                        let role = file_role(&path)?;
                        let extension = path.extension().and_then(|s| s.to_str());
                        if matches!(extension, Some("snd" | "song")) {
                            assert_eq!(
                                role,
                                Some(if extension == Some("snd") {
                                    Role::SoundBank
                                } else {
                                    Role::Song
                                }),
                                "{}",
                                path.display()
                            );
                        }
                        if let Some(role) = role {
                            discovered.insert(
                                path.strip_prefix(&files)?
                                    .to_str()
                                    .context("source path")?
                                    .to_owned(),
                                role,
                            );
                        }
                    }
                }
            }
            for (path, role) in &declared {
                assert_eq!(discovered.get(path), Some(role), "{path}");
            }
            assert_eq!(
                discovered
                    .values()
                    .filter(|&&role| role == Role::SoundBank)
                    .count(),
                89
            );
            assert_eq!(
                discovered
                    .values()
                    .filter(|&&role| role == Role::Song)
                    .count(),
                118
            );
            assert_eq!(
                discovered
                    .iter()
                    .filter(
                        |(path, role)| **role == Role::SoundBank && !declared.contains_key(*path)
                    )
                    .count(),
                70
            );
            assert_eq!(
                discovered
                    .iter()
                    .filter(|(path, role)| **role == Role::Song && !declared.contains_key(*path))
                    .count(),
                6
            );
            let party = roles::party_banks(&extracted, &executable)?;
            assert_eq!(party.len(), 9);
            for (source, role) in &discovered {
                if *role == Role::SoundBank {
                    assert_eq!(
                        party.contains(source),
                        source.starts_with("BTL/"),
                        "{source}"
                    );
                }
            }
            assert_eq!(
                bank_sources(&extracted, &executable)?,
                discovered
                    .into_iter()
                    .filter_map(|(path, role)| (role == Role::SoundBank).then_some(path))
                    .collect::<Vec<_>>()
            );
        }
        Ok(())
    }

    #[test]
    fn renamed_bank_archive_keeps_empty_members_and_rejects_bad_ranges() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("voice-bank-ranges"));
        let result = (|| -> Result<()> {
            fs::create_dir_all(root.join("files"))?;
            fs::write(root.join("files/renamed.payload"), [1, 2, 3, 4])?;
            let directory = |offsets: [u32; 4]| -> Result<()> {
                let mut bytes = vec![0; 64];
                bytes[..4].copy_from_slice(&14u32.to_be_bytes());
                bytes[56..60].copy_from_slice(&64u32.to_be_bytes());
                bytes.extend(offsets.into_iter().flat_map(u32::to_be_bytes));
                fs::write(root.join("files/renamed.directory"), bytes)?;
                Ok(())
            };
            directory([0, 0, 4, 0xfefefefe])?;
            let (mut archive, offsets) =
                bank_archive(&root, "renamed.payload", "renamed.directory")?;
            assert_eq!(offsets, [0, 0, 4]);
            assert!(payload(&mut archive, offsets[0], offsets[1])?.is_empty());
            assert_eq!(payload(&mut archive, offsets[1], offsets[2])?, [1, 2, 3, 4]);
            assert!(bank_archive(&root, "missing.payload", "renamed.directory").is_err());
            assert!(bank_archive(&root, "../renamed.payload", "renamed.directory").is_err());
            for offsets in [[0, 3, 2, 4], [0, 0, 5, 0], [0, 0, 4, 1]] {
                directory(offsets)?;
                assert!(bank_archive(&root, "renamed.payload", "renamed.directory").is_err());
            }
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(root);
        result?;
        cleanup?;
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; reads every voice-bank member without decoding audio"]
    fn original_voice_bank_members_follow_native_directory() -> Result<()> {
        for disc in [1, 2] {
            let extracted = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            let source = roles::voice_bank_path(&extracted)?;
            let sources = Sources::read(&extracted)?;
            let (mut archive, offsets) = bank_archive(&extracted, &source, &sources.usual)?;
            let usual = fs::read(extracted.join("files").join(&sources.usual))?;
            let authored = crate::source_assets::section(&usual, 13)?;
            let pools = Pools::read(&extracted)?;
            let banks = pools
                .others
                .iter()
                .map(|bytes| {
                    let bank = Bank::parse(bytes)?;
                    Ok((bank.group()?, bank))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            let (mut nonempty, mut samples, mut bytes) = (0, 0, 0u64);
            for (index, &offset) in offsets.iter().enumerate() {
                assert_eq!(offset, crate::read::u32(authored, index * 4)?);
            }
            for (index, range) in offsets.windows(2).enumerate() {
                let member = payload(&mut archive, range[0], range[1])?;
                bytes += member.len() as u64;
                if !member.is_empty() {
                    let group = u16::try_from(index + 19)?;
                    let bank = banks
                        .get(&group)
                        .context("member has no voice-bank tables")?;
                    samples += bank.sample_ids()?.len();
                    nonempty += 1;
                }
            }
            assert_eq!(bytes, archive.metadata()?.len());
            assert!(nonempty > 0 && samples > 0);
            eprintln!(
                "disc{disc}: {} members, {nonempty} nonempty, {samples} sample declarations",
                offsets.len() - 1
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs and RESONANCE_DSP_COEFFICIENTS; no audio playback"]
    fn original_song_aliases_keep_all_setups_and_isolate_member_failures() -> Result<()> {
        let output = crate::temporary_path(&std::env::temp_dir().join("resonance-song-aliases"));
        let session = media::OutputSession::open(&output)?;
        let coefficients = PathBuf::from(std::env::var("RESONANCE_DSP_COEFFICIENTS")?);
        let mut cookers = Vec::new();
        for disc in [1, 2] {
            let extracted = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            let files = fs::read_dir(extracted.join("files/S"))?
                .map(|entry| -> Result<_> {
                    let path = entry?.path();
                    Ok((
                        format!("S/{}", path.file_name().unwrap().to_str().unwrap()),
                        path,
                    ))
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .filter(|(name, _)| name.ends_with(".song"))
                .map(|(name, path)| Ok((name, media::hash_file(&path)?)))
                .collect::<Result<BTreeMap<_, _>>>()?;
            let cooker = Cooker::new(&extracted, &session, &coefficients, &files)?;
            cookers.push((cooker, files));
        }
        for (mut cooker, files) in cookers {
            let source = "S/bgm_c016.song";
            let ids = cooker.song_setups.get_mut(&files[source]).unwrap();
            assert_eq!(ids, &[0, 60, 62, 64, 67, 84, 99, 102, 111]);
            ids.push(u16::MAX);
            let mut accepted = Vec::new();
            let mut failures = Vec::new();
            for (member, result) in cooker.cook_song(source)? {
                match result {
                    Ok(paths) => {
                        assert!(paths.iter().all(|path| output.join(path).is_file()));
                        accepted.push(member);
                    }
                    Err(_) => failures.push(member),
                }
            }
            assert_eq!(accepted.len(), 11); // Arrangement, nine setups and their index.
            assert_eq!(failures, ["setup-65535"]);
        }
        drop(session);
        fs::remove_dir_all(output)?;
        Ok(())
    }
}
