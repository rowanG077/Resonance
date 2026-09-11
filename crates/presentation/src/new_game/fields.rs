//! Prepared field packages are immutable and shared by transitions and reloads.
use super::super::loading::Cache;
use super::*;
use resonance_content::prepared::Files;

pub(super) const PLAYABLE_FIELDS: [u32; 3] = [330, 332, 340];

pub(crate) fn manifest_path(map: u32) -> Result<String> {
    Ok(match map {
        5 => "fields/new-game-setup.preload.json".into(),
        340 => "fields/iselia-classroom.preload.json".into(),
        330 | 332 => format!("fields/map-{map}.preload.json"),
        _ => anyhow::bail!("field {map} is not available in this build"),
    })
}

pub(crate) struct FieldPackage {
    pub assets: FieldAssets,
    pub script: Arc<[u8]>,
    messages: Arc<[u8]>,
    pub audio: Arc<super::super::field_audio::Assets>,
    pub files: Arc<Files>,
}
impl FieldPackage {
    pub fn load(
        root: &Path,
        files: Arc<Files>,
        map: u32,
        cache: &mut super::super::field_audio::Cache,
    ) -> Result<Self> {
        let manifest = files
            .manifests
            .get(&map)
            .context("field inventory is missing")?;
        let assets: FieldAssets = files.json(&manifest.inputs.field)?;
        assets.validate()?;
        ensure!(
            assets.map_id == map,
            "field inventory has the wrong map binding"
        );
        for (path, hash) in &assets.files {
            ensure!(
                manifest.files.get(path).is_some_and(|f| f.sha256 == *hash),
                "field dependency differs from its preparation inventory: {path}"
            );
        }
        ensure!(
            manifest.inputs.audio.len() == 1,
            "field needs one combined audio bank"
        );
        let audio = cache.load(root, manifest.inputs.audio.first().unwrap(), &files)?;
        Ok(Self {
            script: files.read(&assets.script.path)?,
            messages: files.read(&assets.messages)?,
            assets,
            audio,
            files,
        })
    }

    pub fn prepare(
        root: &Path,
        map: u32,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let files = Arc::new(Files::load(
            root,
            &[&manifest_path(map)?],
            &mut cache.bytes,
            cancelled,
        )?);
        Self::load(root, files, map, &mut cache.audio)
    }

    pub fn enter(&self, mut entry: FieldEntry) -> Result<FieldSession> {
        let menu: resonance_content::menu_data::MenuData =
            self.files.json("game/menu-data.json")?;
        menu.validate()?;
        entry.menu_data = Some(Arc::new(menu));
        entry.text = Arc::new(self.files.json("game/text.json")?);
        let mut field = FieldSession::enter(
            &self.script,
            serde_json::from_slice(&self.messages)?,
            &self.assets,
            entry,
        )?;
        field.voice_durations = self.audio.voice_durations();
        field.prepare_skits(&self.files)?;
        Ok(field)
    }
}
