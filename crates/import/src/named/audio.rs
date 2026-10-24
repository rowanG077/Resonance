use super::*;
use resonance_content::field_audio::document::{self, Playback, Selection, Speech};
use std::sync::Arc;
mod battle;
mod music;
#[cfg(test)]
mod original_voice_tests;
#[cfg(test)]
mod source_tests;
mod speech;
mod startup;
#[cfg(test)]
mod tests;

/// Source identities determine ownership. Package digests never choose a song,
/// bank, archive, member name or instrument scope.
pub struct AudioSource {
    disc: u8,
    additional: Option<Box<AudioSource>>,
    pub(super) original: crate::media::source::Source,
    music: BTreeMap<u32, String>,
    effects: BTreeMap<u32, Arc<EffectBank>>,
    speech: BTreeMap<u32, speech::Archive>,
    field_reverbs: [[f32; 5]; 2],
    battle_reverbs: [[f32; 5]; 2],
    streams: Option<u32>,
    fields: BTreeMap<u32, (u8, std::path::PathBuf)>,
}

struct EffectBank {
    name: String,
    bytes: Vec<u8>,
    payload: Option<Vec<u8>>,
}

impl AudioSource {
    pub(super) fn field_selection(
        &self,
        id: u32,
    ) -> Result<resonance_content::battle_audio::Selection> {
        let (disc, path) = self
            .fields
            .get(&id)
            .with_context(|| format!("field {id} has no original archive"))?;
        let archive = crate::field::MapArchive::open(path)?;
        self.for_disc(*disc)?
            .original
            .selection(archive.section(6)?)
    }

    pub(super) fn for_disc(&self, disc: u8) -> Result<&Self> {
        if self.disc == disc {
            Ok(self)
        } else {
            self.additional
                .as_deref()
                .filter(|source| source.disc == disc)
                .with_context(|| format!("audio source disc {disc} was not supplied"))
        }
    }

    #[cfg(test)]
    pub(super) fn field_source(&self, id: u32) -> Result<&Self> {
        let (disc, _) = self
            .fields
            .get(&id)
            .with_context(|| format!("field {id} has no original archive"))?;
        self.for_disc(*disc)
    }

    fn music_ids(&self) -> BTreeSet<u32> {
        self.music
            .keys()
            .chain(
                self.additional
                    .iter()
                    .flat_map(|source| source.music.keys()),
            )
            .copied()
            .collect()
    }

    #[cfg(test)]
    pub(super) fn speech_source(&self, id: u32) -> Result<(Vec<u8>, Playback)> {
        let archive = self
            .speech
            .get(&(id & 0xffff_0000))
            .context("missing native speech archive")?;
        Ok((
            archive.read(id as u16)?,
            archive.members[usize::from(id as u16)].playback.clone(),
        ))
    }

    #[cfg(test)]
    pub(super) fn prepare_music(
        &self,
        id: u16,
        environment: [[f32; 5]; 2],
    ) -> Result<resonance_audio::package::Loaded> {
        self.original.music(id, Some(environment))
    }

    pub(super) fn prepare_sound(
        &self,
        id: u16,
        environment: [[f32; 5]; 2],
    ) -> Result<resonance_audio::package::Loaded> {
        let bank = self
            .effects
            .get(&u32::from(id))
            .with_context(|| format!("sound {id} has no source bank"))?;
        self.original
            .sound(&bank.bytes, bank.payload.as_deref(), id, environment)
            .with_context(|| format!("sound {id} from {}", bank.name))
    }

    pub fn read(extracted: &Path, additional: Option<&Path>) -> Result<Self> {
        let disc = crate::disc_number(extracted)?;
        if let Some(other) = additional {
            let other_disc = crate::disc_number(other)?;
            ensure!(disc != other_disc, "audio needs distinct original discs");
            if other_disc < disc {
                return Self::read(other, Some(extracted));
            }
        }
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let mut names = Self {
            disc,
            additional: additional
                .map(|root| Self::read(root, None).map(Box::new))
                .transpose()?,
            original: crate::media::source::Source::read(extracted)?,
            field_reverbs: crate::media::music::song_reverbs(&executable, 1)?,
            battle_reverbs: crate::media::music::battle_reverbs(&executable)?,
            music: BTreeMap::new(),
            effects: BTreeMap::new(),
            speech: BTreeMap::new(),
            streams: None,
            fields: BTreeMap::new(),
        };
        for entry in crate::music_directory::Directory::read(&executable)?.active() {
            let id = u32::try_from(entry.id)?;
            if let std::collections::btree_map::Entry::Vacant(slot) = names.music.entry(id) {
                slot.insert(format!("{}-{id:03}", source_name(&entry.source_path()?)?));
            }
        }
        let [_, common] = crate::all_assets::roles::resident_banks(extracted, &executable)?;
        let banks = crate::event_bank_directory::Directory::read(&executable)?
            .entries
            .iter()
            .map(|entry| entry.source_path())
            .chain([Ok(common)])
            .collect::<Result<BTreeSet<_>>>()?;
        let mut owners = BTreeMap::new();
        let mut groups = BTreeMap::<u16, Arc<EffectBank>>::new();
        let mut sample_archive = None;
        // Retain established field/common owners, then account for every other
        // native bank. Group identity determines aliasing; equal bytes alone do
        // not merge independent editable banks.
        for source in banks
            .into_iter()
            .chain(crate::media::library::bank_sources(extracted, &executable)?)
        {
            let path = crate::field_resources::resolve_path(&extracted.join("files"), &source)?;
            let bytes = fs::read(extracted.join("files").join(&path))?;
            let bank = resonance_audio_cook::bank::Bank::parse(&bytes)?;
            let group = bank.group()?;
            if let Some(previous) = groups.get(&group) {
                ensure!(
                    previous.bytes == bytes,
                    "conflicting source sound group {group}"
                );
                continue;
            }
            let name = source_name(&source)?;
            if let Some(previous) = owners.insert(name.clone(), path.clone()) {
                ensure!(previous == path, "ambiguous audio bank name {name}");
            }
            let ids: Vec<_> = bank.sound_ids().collect();
            // Battle banks keep their PCM in a separate native archive. Bind
            // the declared group payload before compiling its instrument closure.
            let payload = if group >= 19 {
                if sample_archive.is_none() {
                    let sources = crate::source_assets::Sources::read_with(extracted, &executable)?;
                    let voice_bank = crate::all_assets::roles::voice_bank_path(extracted)?;
                    sample_archive = Some(crate::media::library::bank_archive(
                        extracted,
                        &voice_bank,
                        &sources.usual,
                    )?);
                }
                let (archive, offsets) = sample_archive.as_mut().unwrap();
                let index = usize::from(group - 19);
                let range = offsets
                    .get(index..index + 2)
                    .context("missing native effect payload")?;
                Some(crate::media::library::payload(archive, range[0], range[1])?)
            } else {
                None
            };
            let bank = Arc::new(EffectBank {
                name,
                bytes,
                payload,
            });
            for id in ids {
                ensure!(
                    names.effects.insert(u32::from(id), bank.clone()).is_none(),
                    "ambiguous source ownership for sound {id}"
                );
            }
            groups.insert(group, bank);
        }
        let roots: Vec<_> = [Some(extracted), additional]
            .into_iter()
            .flatten()
            .collect();
        (names.speech, names.streams) = speech::catalogue(&roots)?;
        let mut field_sources = BTreeMap::<u32, String>::new();
        for &root in &roots {
            let disc = crate::disc_number(root)?;
            let executable = fs::read(root.join("sys/main.dol"))?;
            for field in crate::field_catalogue::read(&executable)?.records {
                let id = u32::try_from(field.id)?;
                if let Some(name) = field.resource {
                    if let Some(previous) = field_sources.get(&id) {
                        ensure!(
                            previous.eq_ignore_ascii_case(&name),
                            "conflicting field {id} source bindings: {previous} and {name}"
                        );
                    } else {
                        field_sources.insert(id, name.clone());
                    }
                    let files = root.join("files");
                    if let Some(path) =
                        crate::field_resources::find_path(&files, &format!("MAP/{name}"))?
                    {
                        let path = files.join(path);
                        if let Some((previous_disc, previous)) = names.fields.get(&id) {
                            ensure!(
                                crate::media::hash_file(previous)?
                                    == crate::media::hash_file(&path)?,
                                "conflicting source definitions for field {id}"
                            );
                            let first = names.for_disc(*previous_disc)?;
                            let current = names.for_disc(disc)?;
                            ensure!(
                                first.field_reverbs == current.field_reverbs
                                    && first.original.music_reverbs()?
                                        == current.original.music_reverbs()?
                                    && first.original.voice_gains()?
                                        == current.original.voice_gains()?,
                                "conflicting audio environments for field {id}"
                            );
                        } else {
                            names.fields.insert(id, (disc, path));
                        }
                    }
                }
            }
        }
        Ok(names)
    }
}

pub(crate) fn source_name(source: &str) -> Result<String> {
    library::validate_path(source)?;
    let stem = source.rsplit_once('.').map_or(source, |(stem, _)| stem);
    let name = stem
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_ascii_lowercase();
    ensure!(
        !name.is_empty(),
        "audio source has no portable name: {source}"
    );
    Ok(name)
}

impl Publisher<'_> {
    /// Emit an editable cue and its ordinary named instrument WAVs directly
    /// from compiled native resources, with no intermediate package format.
    pub(crate) fn native_program(
        &mut self,
        loaded: &resonance_audio::package::Loaded,
        owner: &str,
        instruments: &str,
        inherit_environment: bool,
    ) -> Result<()> {
        use resonance_audio::package::editable::{self, Program};
        let mut samples = BTreeMap::new();
        for (&id, sample) in &loaded.resources().samples {
            samples.insert(
                id,
                self.native_sample(sample, &format!("{instruments}/sample-{id:05}.wav"))?,
            );
        }
        self.json(
            &format!("{owner}/sound.json"),
            &resonance_audio::effect::Definition::Synthesized(Program {
                version: editable::VERSION,
                programs: loaded.resources().programs.clone(),
                samples,
                score: loaded.score().clone(),
                tables: loaded.tables().clone(),
                reverbs: (!inherit_environment).then(|| loaded.reverbs()),
            }),
        )
    }

    pub(crate) fn native_sample(
        &mut self,
        sample: &resonance_audio::sample::Sample,
        path: &str,
    ) -> Result<resonance_audio::package::editable::Sample> {
        ensure!(
            sample.key < 128
                && sample.rate > 0
                && !sample.pcm.is_empty()
                && sample.pcm.len().saturating_add(sample.loop_pcm.len()) <= 32_000_000,
            "invalid instrument sample"
        );
        resonance_audio::resample::SampleCursor::new(sample)?;
        self.check_spelling(path)?;
        let destination = self.output.join(path);
        let directory = destination.parent().context("instrument has no owner")?;
        fs::create_dir_all(directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        let mut writer = hound::WavWriter::new(
            std::io::BufWriter::new(temporary.as_file_mut()),
            hound::WavSpec {
                channels: 1,
                sample_rate: u32::from(sample.rate),
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for &value in sample.pcm.iter().chain(&sample.loop_pcm) {
            writer.write_sample(value)?;
        }
        writer.finalize()?;
        let hash = crate::media::hash_file(temporary.path())?;
        if !self.already_published(path, &hash)? {
            crate::publication::install(temporary.path(), &destination, &hash)?;
            self.remember(path, hash)?;
        }
        Ok(resonance_audio::package::editable::Sample {
            path: path.into(),
            key: sample.key,
            loop_start: sample.loop_start,
            loop_length: sample.loop_length,
        })
    }

    #[cfg(test)]
    pub(super) fn field_audio(&mut self, id: u32, destination: &str) -> Result<()> {
        let definition = self.field_audio_definition(id)?;
        self.json(destination, &definition)
    }

    pub(super) fn field_audio_definition(&mut self, id: u32) -> Result<document::Definition> {
        let selection = self.audio_source()?.field_selection(id)?;
        let disc = self.audio_source()?.fields[&id].0;
        self.selected_audio(selection, disc)
    }

    fn selected_audio(
        &mut self,
        selection: resonance_content::battle_audio::Selection,
        disc: u8,
    ) -> Result<document::Definition> {
        ensure!(
            selection.streams.is_empty(),
            "field selection contains a battle stream alias"
        );
        self.music_recordings(selection.music.iter().copied(), &|| false)?;
        self.sound_programs_for_disc(selection.sounds.iter().copied(), disc)?;
        self.speech_lines(selection.voices.iter().copied())?;
        let source = self.audio_source()?;
        let mut speech = BTreeMap::<u32, BTreeMap<u16, Playback>>::new();
        for id in selection.voices {
            let base = id & 0xffff_0000;
            let member = id as u16;
            let archive = source
                .speech
                .get(&base)
                .context("speech archive was not published")?;
            speech.entry(base).or_default().insert(
                member,
                archive.members[usize::from(member)].playback.clone(),
            );
        }
        let selected = |mapping: &str, ids: BTreeSet<u16>| -> Vec<Selection> {
            if ids.is_empty() {
                vec![]
            } else {
                vec![Selection {
                    mapping: mapping.into(),
                    ids: ids.into_iter().map(u32::from).collect(),
                }]
            }
        };
        let native = source.for_disc(disc)?;
        Ok(document::Definition {
            version: document::VERSION,
            music: selected("audio/music/mappings.json", selection.music),
            effects: selected("audio/effects/mappings.json", selection.sounds),
            speech: speech
                .into_iter()
                .map(|(archive, members)| Speech { archive, members })
                .collect(),
            music_reverbs: native.original.music_reverbs()?,
            voice_gains: native.original.voice_gains()?,
            reverbs: Some(native.field_reverbs),
        })
    }

    pub(super) fn world_audio(&mut self, destination: &str) -> Result<()> {
        let (selection, embedded) = self.audio_source()?.original.world()?;
        let disc = self.audio_source()?.disc;
        let mut definition = self.selected_audio(selection, disc)?;
        for (id, bytes) in embedded {
            let base = id & 0xffff_0000;
            let member = id as u16;
            let archive = &self.audio_source()?.speech[&base];
            let path = format!(
                "audio/speech/{}/{}",
                archive.name,
                archive.members[usize::from(member)].name
            );
            // The cinematic embeds the same spoken line as the native event
            // archive, but uses its native playback clock. Confirm the alias
            // against original PCM while retaining one editable WAV owner.
            let temporary = tempfile::NamedTempFile::new()?;
            let format = crate::media::library::stream_format(&bytes)
                .context("unsupported cinematic voice encoding")?;
            crate::media::field_audio::decode_voice_wave(temporary.path(), &bytes, format)?;
            ensure!(
                self.published.get(&path) == Some(&crate::media::hash_file(temporary.path())?),
                "cinematic voice {id} differs from its shared speech owner"
            );
            definition
                .speech
                .iter_mut()
                .find(|speech| speech.archive == base)
                .and_then(|speech| speech.members.get_mut(&member))
                .context("cinematic voice missing from world selection")?
                .sample_rate = None;
        }
        self.json(destination, &definition)
    }

    /// Publish effects directly from their native banks. The current field or
    /// battle supplies the shared studio at playback, so one program is enough.
    pub fn sound_programs(&mut self, ids: impl IntoIterator<Item = u16>) -> Result<()> {
        let disc = self.audio_source()?.disc;
        self.sound_programs_for_disc(ids, disc)
    }

    fn sound_programs_for_disc(
        &mut self,
        ids: impl IntoIterator<Item = u16>,
        disc: u8,
    ) -> Result<()> {
        for id in ids.into_iter().collect::<BTreeSet<_>>() {
            if self.published_effects.contains(&(disc, id)) {
                continue;
            }
            let source = self.audio_source()?.for_disc(disc)?;
            let bank = source
                .effects
                .get(&u32::from(id))
                .with_context(|| format!("sound {id} has no source bank"))?
                .clone();
            let cue = crate::naming::sound(u32::from(id));
            let owner = format!("{}/{cue}", bank.name);
            let loaded = source.prepare_sound(id, source.field_reverbs)?;
            // Repeated source declarations publish the same editable owner.
            // Ordinary publication rejects conflicting programs or samples.
            self.native_program(
                &loaded,
                &format!("audio/effects/{owner}"),
                &format!("audio/instruments/effects/{}", bank.name),
                true,
            )
            .with_context(|| format!("publish sound {id} from source disc {disc}"))?;
            self.map("audio/effects/mappings.json", u32::from(id), &owner)?;
            self.published_effects.insert((disc, id));
        }
        Ok(())
    }

    /// Account for every original declaration. Only dependencies proven absent
    /// from the source banks become unavailable-source reports; decode errors
    /// still fail the cook. A selected unavailable sound always fails preparation.
    pub fn sound_catalogue(&mut self) -> Result<()> {
        let source = self.audio_source()?;
        let discs = std::iter::once(source)
            .chain(source.additional.as_deref())
            .map(|source| source.disc)
            .collect::<Vec<_>>();
        for disc in discs {
            self.sound_catalogue_for_disc(disc)?;
        }
        Ok(())
    }

    fn sound_catalogue_for_disc(&mut self, disc: u8) -> Result<()> {
        let ids = self
            .audio_source()?
            .for_disc(disc)?
            .effects
            .keys()
            .copied()
            .map(u16::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let mut unavailable = BTreeMap::new();
        for &id in &ids {
            if let Err(error) = self.sound_programs_for_disc([id], disc) {
                let source = self.audio_source()?.for_disc(disc)?;
                let Some(dependency) = source.original.unavailable_dependency(&error)? else {
                    return Err(error);
                };
                unavailable.insert(
                    id,
                    serde_json::json!({
                        "bank": source.effects[&u32::from(id)].name,
                        "dependency": dependency,
                    }),
                );
            }
        }
        let report = if disc == self.audio_source()?.disc {
            "reports/audio-effects.json".into()
        } else {
            format!("reports/audio-effects/disc-{disc}.json")
        };
        self.json(
            &report,
            &serde_json::json!({
                "disc": disc,
                "declared": ids.len(),
                "published": ids.len() - unavailable.len(),
                "unavailable_source_dependencies": unavailable,
            }),
        )
    }

    fn map(&mut self, path: &str, id: u32, name: &str) -> Result<()> {
        library::validate_path(name)?;
        let entries = self.mappings.entry(path.into()).or_default();
        if let Some(previous) = entries.insert(id, name.into()) {
            ensure!(
                previous == name,
                "conflicting audio binding in {path} for {id}"
            );
        }
        Ok(())
    }
}
