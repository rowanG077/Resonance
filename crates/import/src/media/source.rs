//! Native audio inputs owned by a cook. Compiled data goes directly to the
//! renderer or editable program writer; it is never serialized for reloading.
use super::{library::Pools, music_library, sound_library};
use anyhow::{Context, Result, ensure};
use resonance_audio::{music_voice::Tables, package::Loaded};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct Source {
    extracted: PathBuf,
    executable: Vec<u8>,
    pools: Pools,
    tables: Tables,
    selection: super::field_audio::resources::Catalogue,
}

impl Source {
    pub(crate) fn extracted(&self) -> &Path {
        &self.extracted
    }

    pub(crate) fn unavailable_dependency(
        &self,
        error: &anyhow::Error,
    ) -> Result<Option<resonance_audio_cook::bank::MissingObject>> {
        match error.downcast_ref::<resonance_audio_cook::bank::MissingObject>() {
            Some(dependency) if self.pools.absent(dependency)? => Ok(Some(*dependency)),
            _ => Ok(None),
        }
    }

    pub(crate) fn read(extracted: &Path) -> Result<Self> {
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let tables = super::synthesis_tables(&executable)?;
        Ok(Self {
            selection: super::field_audio::resources::Catalogue::read(extracted, &executable)?,
            extracted: extracted.into(),
            executable,
            pools: Pools::read(extracted)?,
            tables,
        })
    }

    pub(crate) fn selection(
        &self,
        script: &[u8],
    ) -> Result<resonance_content::battle_audio::Selection> {
        let resources = self.selection.resources(script)?;
        Ok(resonance_content::battle_audio::Selection {
            music: resources.music,
            sounds: resources
                .banks
                .into_iter()
                .flat_map(|(_, ids)| ids)
                .collect(),
            voices: resources.voices,
            streams: Default::default(),
        })
    }

    pub(crate) fn music_reverbs(&self) -> Result<resonance_content::field_audio::MusicReverbs> {
        super::music::music_reverbs(&self.executable)
    }

    pub(crate) fn world(
        &self,
    ) -> Result<(
        resonance_content::battle_audio::Selection,
        BTreeMap<u32, Vec<u8>>,
    )> {
        let files = self.extracted.join("files");
        let module = crate::field_resources::resolve_path(&files, "US_r_Top2field.rel")?;
        let (sounds, archives) =
            crate::all_assets::world_audio_sources(&crate::rel::Rel::read(&files.join(module))?)?;
        let sounds = sounds
            .into_iter()
            .map(u16::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let script = crate::field_resources::resolve_path(&files, "field_sil.so")?;
        let resources =
            self.selection
                .with_native(&fs::read(files.join(script))?, &[2, 3, 4, 5], &sounds)?;
        let mut selection = resonance_content::battle_audio::Selection {
            music: resources.music,
            sounds: resources
                .banks
                .into_iter()
                .flat_map(|(_, ids)| ids)
                .collect(),
            voices: resources.voices,
            streams: Default::default(),
        };
        let mut voices = BTreeMap::new();
        for declaration in archives {
            let source = crate::field_resources::resolve_path(&files, &declaration.source)?;
            let bytes = crate::compression::payload(fs::read(files.join(source))?)?;
            let members = crate::field::sections(&bytes)?;
            let range = members
                .get(declaration.member)
                .and_then(Option::as_ref)
                .context("world cinematic voice archive missing")?;
            let archive = &bytes[range.clone()];
            let members = crate::field::sections(archive)?;
            ensure!(
                !members.is_empty() && members.len() <= 256,
                "invalid world cinematic voice count"
            );
            for (index, range) in members.into_iter().enumerate() {
                let id = declaration
                    .first_resource
                    .checked_add(u32::try_from(index)?)
                    .context("world cinematic voice ID overflow")?;
                let data = archive[range.context("world cinematic voice missing")?].to_vec();
                if let Some(previous) = voices.insert(id, data.clone()) {
                    ensure!(
                        previous == data,
                        "conflicting original cinematic voice {id}"
                    );
                }
                selection.voices.insert(id);
            }
        }
        Ok((selection, voices))
    }

    pub(crate) fn voice_gains(&self) -> Result<Vec<f32>> {
        super::field_audio::voice_gains(&self.executable)
    }

    pub(crate) fn battle_spatial(&self) -> Result<[[f32; 5]; 2]> {
        let files = self.extracted.join("files");
        let path = crate::field_resources::resolve_path(&files, "US_r_Top2Btl.rel")?;
        let module = crate::rel::Rel::read(&files.join(path))?;
        let mut parameters = [[0.; 5]; 2];
        for (values, offset) in parameters.iter_mut().zip([0x4d8, 0x4668]) {
            for (index, value) in values.iter_mut().enumerate() {
                *value = crate::read::f32(module.at((4, offset))?, index * 4)?;
            }
        }
        Ok(parameters)
    }

    pub(crate) fn music(&self, id: u16, environment: Option<[[f32; 5]; 2]>) -> Result<Loaded> {
        music_library::prepare(
            &self.extracted,
            &self.executable,
            &self.pools,
            id,
            environment,
        )
    }

    pub(crate) fn music_source(&self, id: u16) -> Result<String> {
        let path = super::music_path(&self.executable, id)?;
        crate::field_resources::resolve_path(&self.extracted.join("files"), &path)
    }

    pub(crate) fn music_inherits_environment(&self, id: u16) -> Result<bool> {
        Ok(matches!(
            super::song_reverb_change(&self.executable, id)?,
            super::music::ReverbChange::Keep
        ))
    }

    pub(crate) fn sound(
        &self,
        bytes: &[u8],
        payload: Option<&[u8]>,
        id: u16,
        reverbs: [[f32; 5]; 2],
    ) -> Result<Loaded> {
        let mut bank = self.pools.bank(bytes)?;
        if let Some(payload) = payload {
            bank = bank.with_sample_payload(payload);
        }
        let (resources, score) = sound_library::sound(&bank, id, &self.pools.sustains)?;
        Loaded::new(resources, score, self.tables.clone(), reverbs)
    }

    pub(crate) fn menu(&self) -> Result<Vec<(&'static str, Loaded)>> {
        let [_, common] =
            crate::all_assets::roles::resident_banks(&self.extracted, &self.executable)?;
        let bytes = fs::read(self.extracted.join("files").join(common))?;
        let bank = self.pools.bank(&bytes)?;
        let reverbs = super::music::title_reverbs(&self.executable)?;
        [("navigate", 1), ("confirm", 2), ("back", 3), ("error", 4)]
            .into_iter()
            .map(|(name, id)| {
                let (resources, score) = sound_library::sound(&bank, id, &self.pools.sustains)?;
                Ok((
                    name,
                    Loaded::new(resources, score, self.tables.clone(), reverbs)?,
                ))
            })
            .collect()
    }
}
