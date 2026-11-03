//! Immutable playable battle inputs. Scene snapshots share their startup bytes;
//! no dependency adoption or encounter state is stored here.
use super::*;
#[path = "common_audio.rs"]
mod audio;
use crate::battle::model;
use anyhow::{Context, Result, ensure};
use resonance_content::{
    ScenePart,
    animation::Motion,
    battle_effect::{SourceBank, document as effects},
    battle_model::{self, document as models},
    battle_scene::{
        self,
        document::{self as scenes, Kind},
    },
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::RwLock,
};

pub struct Body {
    pub character: u8,
    pub costume: u8,
    pub files: Arc<Files>,
    inputs: Vec<String>,
    pub(super) victory_motions: Arc<crate::battle::victory::PreparedMotions>,
    pub source: battle_model::Party,
    pub selection: resonance_content::battle_victory::document::Selection,
    pub motions: BTreeMap<u16, Arc<Motion>>,
}

/// One equipped item's captured inputs; retired when no party member selects it.
pub struct Equipment {
    pub item: u16,
    pub files: Arc<Files>,
    pub attachment: battle_model::Attachment,
    inputs: Vec<String>,
}

impl Equipment {
    fn capture(common: &Files, item: u16, cancelled: &impl Fn() -> bool) -> Result<Arc<Self>> {
        let mut snapshot = common
            .library()
            .context("common equipment library missing")?
            .snapshot_with(common)?;
        let attachment =
            models::capture_equipment(&mut snapshot, item, common.diagnostics(), cancelled)?;
        let files = Arc::new(Files::from_snapshot(
            snapshot,
            cancelled,
            common.diagnostics().clone(),
        )?);
        let inputs = files
            .keys()
            .filter(|path| !common.contains_key(path))
            .cloned()
            .collect();
        Ok(Arc::new(Self {
            item,
            files,
            attachment,
            inputs,
        }))
    }

    pub fn parts(&self) -> impl Iterator<Item = &ScenePart> {
        let weapon = match &self.attachment {
            battle_model::Attachment::Weapon(weapon) | battle_model::Attachment::Shield(weapon) => {
                Some(weapon)
            }
            battle_model::Attachment::Nonvisual => None,
        };
        weapon
            .into_iter()
            .flat_map(|weapon| weapon.parts.values())
            .flat_map(|part| &part.layers)
            .map(|layer| &layer.scene)
    }

    pub(super) fn adopt(
        &self,
        snapshot: &mut Snapshot,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        adopt_inputs(snapshot, &self.files, &self.inputs, "equipment", cancelled)
    }
}

fn adopt_inputs(
    snapshot: &mut Snapshot,
    files: &Arc<Files>,
    inputs: &[String],
    scope: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    snapshot.reuse(files.clone())?;
    for path in inputs {
        ensure!(!cancelled(), "{scope} adoption cancelled");
        snapshot.read(path, ASSET_LIMIT)?;
    }
    Ok(())
}

/// Keep startup material preparation scoped to the model's live renderer.
#[derive(Clone, Copy)]
pub enum ModelKind {
    Body,
    Wings,
    Weapon,
    Particle,
}

pub struct Common {
    pub files: Arc<Files>,
    pub ui: resonance_content::battle_ui::Art,
    pub bodies: BTreeMap<u8, Arc<Body>>,
    alternates: RwLock<BTreeMap<u8, Arc<Body>>>,
    equipment: RwLock<BTreeMap<u16, Arc<Equipment>>>,
    pub banks: BTreeMap<String, Arc<SourceBank>>,
    pub scenes: BTreeMap<(u8, u16), (battle_scene::Scene, Arc<SourceBank>)>,
    pub rules: resonance_content::battle_rules::Captured<resonance_content::arte::Catalogue>,
    pub audio: Arc<resonance_content::battle_audio::document::Catalogue>,
    pub actions: BTreeMap<u16, resonance_content::battle_martial::AuthoredAction>,
    extra_sounds: BTreeSet<resonance_content::battle_voice::Sound>,
    pub(super) effect_models: Arc<model::EffectCache>,
    pub(super) effect_programs: Arc<crate::battle::effect_program::cache::Cache>,
}

impl Common {
    pub fn common_music(&self) -> impl Iterator<Item = u16> {
        crate::battle::entry::COMMON_MUSIC
            .into_iter()
            .chain(std::iter::once(crate::battle::lifecycle::VICTORY_MUSIC))
    }

    /// Fixed player/common cues, including recorded cues used by player voices.
    /// Enemy/arena-only audio remains encounter-selected.
    pub fn body(&self, character: u8, costume: u8) -> Option<Arc<Body>> {
        if costume == 0 {
            self.bodies.get(&character).cloned()
        } else {
            self.alternates
                .read()
                .unwrap()
                .get(&character)
                .filter(|body| body.costume == costume)
                .cloned()
        }
    }

    pub fn prepare_costume(
        &self,
        character: u8,
        costume: u8,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<Body>> {
        let mut snapshot = self
            .files
            .library()
            .context("common battle library missing")?
            .snapshot_with(&self.files)?;
        let captured = BodyInputs::capture(
            &mut snapshot,
            character,
            costume,
            &self.rules.techniques,
            &self.actions,
            self.files.diagnostics(),
            cancelled,
        )?;
        let victory = resonance_content::battle_victory::document::capture(
            &mut snapshot,
            [(character, costume, &captured.selection)],
            self.files.diagnostics(),
            cancelled,
        )?;
        let files = Arc::new(Files::from_snapshot(
            snapshot,
            cancelled,
            self.files.diagnostics().clone(),
        )?);
        captured.prepare(files, &victory, Some(&self.files), cancelled)
    }

    pub fn install_costume(&self, body: Arc<Body>) {
        self.alternates
            .write()
            .unwrap()
            .insert(body.character, body);
    }
    pub fn retire_costume(&self, character: u8) {
        self.alternates.write().unwrap().remove(&character);
    }

    pub fn equipment(&self, item: u16) -> Option<Arc<Equipment>> {
        self.equipment.read().unwrap().get(&item).cloned()
    }

    pub fn prepare_equipment(
        &self,
        item: u16,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<Equipment>> {
        Equipment::capture(&self.files, item, cancelled)
    }

    pub fn install_equipment(&self, equipment: Arc<Equipment>) {
        self.equipment
            .write()
            .unwrap()
            .insert(equipment.item, equipment);
    }

    pub fn retire_equipment(&self, item: u16) {
        self.equipment.write().unwrap().remove(&item);
    }

    pub(super) fn adopt_body(
        &self,
        snapshot: &mut Snapshot,
        body: &Body,
        cancelled: &impl Fn() -> bool,
    ) -> Result<()> {
        adopt_inputs(snapshot, &body.files, &body.inputs, "costume", cancelled)
    }

    pub fn effect_sounds(&self) -> BTreeSet<u16> {
        let mut sounds: BTreeSet<u16> = self
            .banks
            .values()
            .chain(self.scenes.values().map(|scene| &scene.1))
            .flat_map(|bank| bank.programs.iter().flatten())
            .filter_map(|event| match event.operation {
                resonance_content::battle_effect::EffectOperation::Sound { id, .. } if id != 0 => {
                    Some(id)
                }
                _ => None,
            })
            .collect();
        sounds.extend(
            audio::player_sounds(self)
                .into_iter()
                .filter_map(audio::cue),
        );
        sounds
    }

    /// Player speech, including victory groups, before live speaker selection.
    pub fn voice_streams(&self) -> BTreeSet<u16> {
        audio::player_sounds(self)
            .into_iter()
            .filter_map(|sound| match sound {
                resonance_content::battle_voice::Sound::Stream(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    /// The same source geometry can have an additive layer recipe (notably
    /// Colette's wings). Preserve that recipe for renderer preparation too.
    pub fn render_layers(&self) -> Vec<(&ScenePart, bool, ModelKind)> {
        self.bodies
            .values()
            .flat_map(|body| {
                body.source
                    .parts
                    .iter()
                    .map(|part| (part, false, ModelKind::Body))
                    .chain(
                        body.source
                            .wings
                            .iter()
                            .map(|part| (part, true, ModelKind::Wings)),
                    )
            })
            .chain(self.model_parts().flat_map(|(model, kind)| {
                model
                    .layers
                    .iter()
                    .map(move |layer| (&layer.scene, layer.additive, kind))
            }))
            .collect()
    }

    fn model_parts(&self) -> impl Iterator<Item = (&battle_model::ModelPart, ModelKind)> {
        let weapons = self
            .scenes
            .values()
            .filter_map(|scene| scene.0.weapon.as_ref());
        self.effect_banks()
            .flat_map(|bank| bank.art.iter().flat_map(|art| art.models.values()))
            .map(|part| (part, ModelKind::Particle))
            .chain(
                weapons
                    .flat_map(|weapon| weapon.parts.values())
                    .map(|part| (part, ModelKind::Weapon)),
            )
    }

    pub fn render_images(&self) -> Vec<&str> {
        self.banks
            .values()
            .chain(self.scenes.values().map(|scene| &scene.1))
            .flat_map(|bank| bank.art.iter())
            .flat_map(|art| art.textures.values().flatten())
            .flat_map(|texture| &texture.images)
            .map(|image| image.path.as_str())
            .chain(std::iter::once(
                resonance_content::model_preview::document::TOON_RAMP,
            ))
            .collect()
    }

    pub fn effect_banks(&self) -> impl Iterator<Item = &Arc<SourceBank>> {
        self.banks
            .values()
            .chain(self.scenes.values().map(|(_, bank)| bank))
    }

    /// The checkpoint prepares all common declarations without a disposable encounter.
    /// Unsupported unused art remains subject to normal selected-encounter diagnostics.
    pub fn render_effects(&self) -> Vec<super::RenderEffect> {
        let diagnostics = Diagnostics::default();
        let tints = self
            .files
            .json(resonance_content::battle_effect::TINTS_PATH)
            .ok();
        self.banks
            .iter()
            .map(|(path, bank)| (bank, path == effects::COMMON || path == effects::TECHNIQUES))
            .chain(self.scenes.values().map(|(_, bank)| (bank, false)))
            .enumerate()
            .filter_map(|(index, (bank, tinted))| {
                let programs = (0..bank.programs.len())
                    .map(u16::try_from)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .ok()?;
                let mut render = super::render::effects(
                    resonance_battle::ResourceId(u32::try_from(index + 1).ok()?),
                    bank,
                    &programs,
                    tints.as_ref().filter(|_| tinted),
                    &diagnostics,
                )
                .ok()?;
                if bank.document.as_deref() == Some(effects::COMMON)
                    && let Ok((member, _)) = bank.named_particle(effects::STUN_BIRD)
                {
                    render.members.insert(u16::from(member));
                }
                Some(render)
            })
            .collect()
    }

    #[cfg(feature = "test-support")]
    pub fn factory_reuse(&self) -> (u64, u64, u64) {
        (
            self.effect_models.hits(),
            self.effect_programs.hits(),
            self.effect_programs.validations(),
        )
    }

    pub fn capture(
        snapshot: &mut Snapshot,
        diagnostics: &Diagnostics,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self> {
        let ui = resonance_content::battle_ui::document::capture(snapshot, diagnostics, cancelled)?;
        let mut rules = resonance_content::battle_rules::capture(snapshot, cancelled)?;
        let audio = Arc::new(
            resonance_content::battle_audio::document::Catalogue::capture(snapshot, cancelled)?,
        );
        let mut banks = BTreeMap::new();
        for path in [effects::COMMON, effects::TECHNIQUES] {
            if let Some(bank) = diagnostics.preload(path, cancelled, || {
                effects::capture(snapshot, path, diagnostics, cancelled)
            })? {
                banks.insert(path.to_owned(), Arc::new(bank));
            }
        }
        let actions = capture_actions(snapshot, &mut rules, &mut banks, diagnostics, cancelled)?;
        let mut inputs = Vec::new();
        for character in Character::ALL {
            let character = character as u8;
            let captured = diagnostics.preload(
                &format!("common battle character {character}"),
                cancelled,
                || {
                    BodyInputs::capture(
                        snapshot,
                        character,
                        0,
                        &rules.techniques,
                        &actions,
                        diagnostics,
                        cancelled,
                    )
                },
            )?;
            if let Some(body) = captured {
                inputs.push(body);
            }
        }
        // Scene discovery depends on character identities, not on whether their
        // optional model preload succeeded.
        let (spell_ids, skill_ids) = assets::choreography_ids(
            &rules.techniques,
            &rules.party,
            &rules.unison,
            Character::ALL,
            &BTreeMap::new(),
        )?;
        let scenes = capture_scenes(
            snapshot,
            [(Kind::Technique, spell_ids), (Kind::Skill, skill_ids)],
            banks.get(effects::COMMON).map(Arc::as_ref),
            diagnostics,
            cancelled,
        )?;
        let victory = resonance_content::battle_victory::document::capture(
            snapshot,
            inputs
                .iter()
                .map(|body| (body.character, body.costume, &body.selection)),
            diagnostics,
            cancelled,
        )?;
        snapshot.read(resonance_content::battle_effect::TINTS_PATH, JSON_LIMIT)?;
        snapshot.read(
            resonance_content::model_preview::document::TOON_RAMP,
            ASSET_LIMIT,
        )?;
        let files = Arc::new(Files::capture_snapshot(
            snapshot,
            cancelled,
            diagnostics.clone(),
        )?);
        let mut bodies = BTreeMap::new();
        for input in inputs {
            let character = input.character;
            if let Some(body) = diagnostics.preload(
                &format!("common battle character {character}"),
                cancelled,
                || input.prepare(files.clone(), &victory, None, cancelled),
            )? {
                bodies.insert(character, body);
            }
        }
        let extra_sounds = actions
            .values()
            .flat_map(|action| action.attack.sounds())
            .chain(victory.groups.iter().filter_map(|group| group.voice))
            .collect();
        ensure!(!cancelled(), "common battle preparation cancelled");
        let mut common = Self {
            files,
            ui,
            bodies,
            alternates: Default::default(),
            equipment: Default::default(),
            banks,
            scenes,
            rules,
            audio,
            actions,
            extra_sounds,
            effect_models: Default::default(),
            effect_programs: Default::default(),
        };
        common.prepare_effects(cancelled)?;
        Ok(common)
    }

    fn prepare_effects(&mut self, cancelled: &impl Fn() -> bool) -> Result<()> {
        let mut models = model::EffectCache::default();
        for (part, kind) in self.model_parts() {
            ensure!(!cancelled(), "common effect model preparation cancelled");
            models.warm(&self.files, part, false);
            if matches!(kind, ModelKind::Weapon) {
                models.warm(&self.files, part, true);
            }
        }
        let models = Arc::new(models);
        let mut programs = crate::battle::effect_program::cache::Cache::default();
        for bank in self.effect_banks() {
            ensure!(!cancelled(), "common effect program preparation cancelled");
            let factories = bank
                .art
                .iter()
                .flat_map(|art| &art.models)
                .filter_map(|(&slot, part)| {
                    models
                        .prepare(&self.files, part, resonance_battle::ResourceId(0), false)
                        .ok()
                        .map(|model| (slot, model))
                })
                .collect();
            programs.warm(bank, &factories, cancelled);
        }
        ensure!(!cancelled(), "common effect program preparation cancelled");
        self.effect_models = models;
        self.effect_programs = Arc::new(programs);
        Ok(())
    }
}

/// Capture and bind default and alternate bodies through the same path. A body
/// is reusable only when all of its authored motion slots have been captured.
struct BodyInputs {
    character: u8,
    costume: u8,
    source: battle_model::Party,
    selection: resonance_content::battle_victory::document::Selection,
}

impl BodyInputs {
    fn capture(
        snapshot: &mut Snapshot,
        character: u8,
        costume: u8,
        techniques: &resonance_content::arte::Catalogue,
        actions: &BTreeMap<u16, resonance_content::battle_martial::AuthoredAction>,
        diagnostics: &Diagnostics,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self> {
        let (selection, mut source) =
            models::capture_party(snapshot, character, costume, diagnostics, cancelled)?;
        for &id in techniques.learned_by(character)? {
            if techniques.definition(usize::from(id))?.action.is_some() {
                let action = actions
                    .get(&id)
                    .with_context(|| format!("unprepared authored arte {id}"))?;
                append_motions(snapshot, &mut source, action, cancelled)?;
            }
        }
        Ok(Self {
            character,
            costume,
            source,
            selection,
        })
    }

    fn prepare(
        self,
        files: Arc<Files>,
        victory: &resonance_content::battle_victory::Performances,
        common: Option<&Files>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Arc<Body>> {
        let motions = model::motions(
            &files,
            self.source.parts.first().context("missing battle body")?,
            &self.source.body.skeleton,
        )?;
        let victory_motions = Arc::new(crate::battle::victory::prepare_motions(
            &files,
            victory,
            self.character,
            self.costume,
            &self.source.body.skeleton,
            cancelled,
        )?);
        let inputs = common.map_or_else(Vec::new, |common| {
            files
                .keys()
                .filter(|path| !common.contains_key(path))
                .cloned()
                .collect()
        });
        ensure!(!cancelled(), "battle body preparation cancelled");
        Ok(Arc::new(Body {
            character: self.character,
            costume: self.costume,
            source: self.source,
            selection: self.selection,
            files,
            inputs,
            motions,
            victory_motions,
        }))
    }
}

fn capture_actions(
    snapshot: &mut Snapshot,
    rules: &mut resonance_content::battle_rules::Captured<resonance_content::arte::Catalogue>,
    banks: &mut BTreeMap<String, Arc<SourceBank>>,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<BTreeMap<u16, resonance_content::battle_martial::AuthoredAction>> {
    let mut ids = BTreeSet::new();
    for character in 1..=9 {
        ids.extend(rules.techniques.learned_by(character)?.iter().copied());
    }
    let mut actions = BTreeMap::new();
    for id in ids {
        let Some(path) = &rules.techniques.definition(usize::from(id))?.action else {
            continue;
        };
        let captured = diagnostics.preload(
            &format!("common battle action {id}"),
            cancelled,
            || -> Result<resonance_content::battle_martial::AuthoredAction> {
                let action: resonance_content::battle_martial::AuthoredAction =
                    snapshot.json(path, JSON_LIMIT)?;
                if let Some(path) = &action.effects {
                    if !banks.contains_key(path) {
                        let bank = effects::capture(snapshot, path, diagnostics, cancelled)?;
                        banks.insert(path.clone(), Arc::new(bank));
                    }
                }
                Ok(action)
            },
        )?;
        if let Some(action) = captured {
            rules.unison.placements.insert(id, action.unison);
            actions.insert(id, action);
        }
    }
    Ok(actions)
}

fn capture_scenes(
    snapshot: &mut Snapshot,
    selections: impl IntoIterator<Item = (Kind, BTreeSet<u16>)>,
    common: Option<&SourceBank>,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<BTreeMap<(u8, u16), (battle_scene::Scene, Arc<SourceBank>)>> {
    let mut scenes = BTreeMap::new();
    for (kind, ids) in selections {
        for id in ids {
            // A failed speculative preload remains a miss. Selected encounters
            // still capture it normally, with their existing error policy.
            if let Some((scene, mut bank)) = diagnostics.preload(
                &format!("common battle scene {}:{id}", kind.mapping()),
                cancelled,
                || scenes::capture(snapshot, kind, id, diagnostics, cancelled),
            )? {
                inherit_textures(&mut bank, common);
                scenes.insert((kind_key(kind), id), (scene, Arc::new(bank)));
            }
        }
    }
    Ok(scenes)
}

pub(super) fn kind_key(kind: Kind) -> u8 {
    match kind {
        Kind::Technique => 0,
        Kind::Skill => 1,
    }
}

pub(super) fn inherit_textures(bank: &mut SourceBank, common: Option<&SourceBank>) {
    if let (Some(art), Some(common)) = (&mut bank.art, common.and_then(|bank| bank.art.as_ref())) {
        for (&slot, textures) in &common.textures {
            art.textures.entry(slot).or_insert_with(|| textures.clone());
        }
    }
}

pub(super) fn append_motions(
    snapshot: &mut Snapshot,
    source: &mut battle_model::Party,
    action: &resonance_content::battle_martial::AuthoredAction,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let scene = source
        .parts
        .first_mut()
        .context("missing authored arte body")?;
    for (&slot, path) in &action.motions {
        ensure!(!cancelled(), "common battle preparation cancelled");
        ensure!(
            !scene.clips.iter().any(|clip| clip.resource_slot == slot),
            "authored arte conflicts with motion slot {slot}"
        );
        snapshot.read(path, ASSET_LIMIT)?;
        scene.clips.push(resonance_content::SceneClip {
            resource_slot: slot,
            motion: path.clone(),
            joints: Default::default(),
            duration_seconds: 0.,
            animation_resource: None,
            pose_chains: Vec::new(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> Result<(tempfile::TempDir, resonance_content::library::Library)> {
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join("content.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": resonance_content::library::VERSION,
                "game": resonance_content::library::GAME,
            }))?,
        )?;
        let library = resonance_content::library::Library::open(root.path(), &[])?;
        Ok((root, library))
    }

    // `Diagnostics::preload` owns the tolerant/strict/cancelled policy and its
    // tests. These check only that each warmup is wired through it.
    #[test]
    fn warmups_skip_bad_entries_unless_strict() -> Result<()> {
        let (root, _) = library()?;
        let scenes = root.path().join("battle/scenes");
        std::fs::create_dir_all(scenes.join("broken"))?;
        std::fs::create_dir(scenes.join("healthy"))?;
        std::fs::write(
            scenes.join("mappings.json"),
            br#"{"1":"missing","2":"broken","3":"healthy"}"#,
        )?;
        std::fs::write(scenes.join("broken/scene.json"), b"invalid scene")?;
        std::fs::write(
            scenes.join("healthy/scene.json"),
            br#"{"effects":"effects.json","duration":1}"#,
        )?;
        std::fs::write(root.path().join("effects.json"), b"{}")?;
        let library = resonance_content::library::Library::open(root.path(), &[])?;
        let selections = || [(Kind::Technique, [1, 2, 3].into())];

        let diagnostics = Diagnostics::default();
        let warmed = capture_scenes(
            &mut library.snapshot(),
            selections(),
            None,
            &diagnostics,
            &|| false,
        )?;
        assert_eq!(warmed.keys().copied().collect::<Vec<_>>(), [(0, 3)]);
        assert_eq!(diagnostics.entries().len(), 2);

        let strict = Diagnostics::new(true);
        assert!(
            capture_scenes(
                &mut library.snapshot(),
                selections(),
                None,
                &strict,
                &|| false
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn equipped_items_capture_only_the_selected_inputs_and_release_their_owner() -> Result<()> {
        let (root, _) = library()?;
        let directory = root.path().join("meshes/equipment");
        std::fs::create_dir_all(directory.join("selected"))?;
        std::fs::create_dir(directory.join("broken"))?;
        std::fs::write(
            directory.join("mappings.json"),
            br#"{"1":"selected","2":"broken","3":null}"#,
        )?;
        std::fs::write(
            directory.join("broken/equipment.json"),
            b"broken unused equipment",
        )?;
        let mesh = "meshes/equipment/selected/body.glb";
        std::fs::write(
            root.path().join(mesh),
            resonance_asset_writer::gltf::pack_glb(
                &serde_json::json!({"asset":{"version":"2.0"},"nodes":[{"name":"root","extras":{"resonance_bone":0}}]}),
                &[],
            )?,
        )?;
        std::fs::write(
            directory.join("selected/equipment.json"),
            serde_json::to_vec(&serde_json::json!({
                "kind":"weapon", "parts":{"0":{"layers":[{"mesh":mesh}]}}
            }))?,
        )?;
        let library = resonance_content::library::Library::open(root.path(), &[])?;
        let common = Files::from_snapshot(library.snapshot(), || false, Diagnostics::new(true))?;
        let equipment = Equipment::capture(&common, 1, &|| false)?;
        assert_eq!(equipment.parts().count(), 1);
        assert!(equipment.files.contains_key(mesh));
        assert!(
            !equipment
                .files
                .contains_key("meshes/equipment/broken/equipment.json")
        );
        assert!(
            !common.contains_key(models::EQUIPMENT),
            "startup must not retain the equipment catalogue"
        );
        assert!(Equipment::capture(&common, 2, &|| false).is_err());
        assert!(Equipment::capture(&common, 1, &|| true).is_err());
        assert!(matches!(
            Equipment::capture(&common, 3, &|| false)?.attachment,
            battle_model::Attachment::Nonvisual
        ));

        let owner = Arc::downgrade(&equipment);
        let bytes = equipment.files.read(mesh)?;
        let mut snapshot = library.snapshot_with(&common)?;
        equipment.adopt(&mut snapshot, &|| false)?;
        let encounter = Files::from_snapshot(snapshot, || false, Diagnostics::new(true))?;
        drop(equipment);
        assert!(
            owner.upgrade().is_none(),
            "encounter retained the equipment preload owner"
        );
        assert!(Arc::ptr_eq(&bytes, &encounter.read(mesh)?));
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS; compares all nine playable characters with uncached capture"]
    fn common_inputs_match_fresh_encounters_and_share_curves() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?;
        let library = resonance_content::library::Library::open(std::path::Path::new(&root), &[])?;
        let diagnostics = Diagnostics::new(true);
        let mut snapshot = library.snapshot();
        resonance_content::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
        resonance_content::preparation::ui(&mut snapshot, &diagnostics, &|| false)?;
        let common = Common::capture(&mut snapshot, &diagnostics, &|| false)?;
        assert_eq!(common.bodies.len(), 9);
        assert!(
            common
                .files
                .keys()
                .filter(|path| path.ends_with(".glb"))
                .all(|path| !path.starts_with("battle/arenas/")
                    && (!path.starts_with("battle/enemies/") || path.contains("-animation-")))
        );
        let data = resonance_content::session::SessionData::load(&common.files)?;
        let menus = data.rules.as_ref().context("menu rules missing")?;
        let mut party = resonance_events::party::Party::new(&data, Default::default())?;
        let alternate = common.prepare_costume(1, 1, &|| false)?;
        assert!(!alternate.inputs.is_empty());
        let alternate_owner = Arc::downgrade(&alternate);
        common.install_costume(alternate);
        for (formation, costume) in [
            (vec![1, 2, 3, 4], 0),
            (vec![5, 6, 7, 8], 0),
            (vec![9, 2, 3, 4], 0),
            (vec![1, 2, 3, 4], 1),
        ] {
            party.members[0].costume = costume;
            party.formation = formation;
            let setup = resonance_events::battle::Setup {
                encounter: resonance_events::battle::Encounter::Formation(1),
                arena: 13,
                route: [0; 5],
                music: None,
                defeat: resonance_events::battle::DefeatPolicy::GameOver,
            };
            let fresh = Assets::capture(
                &mut library.snapshot_with(&common.files)?,
                menus,
                &data,
                &party,
                setup.clone(),
                diagnostics.clone(),
                &|| false,
            )?;
            let cached = Assets::capture_with_common(
                &mut library.snapshot_with(&common.files)?,
                menus,
                &data,
                &party,
                setup,
                diagnostics.clone(),
                &|| false,
                &common,
            )?;
            assert_eq!(fresh.party_actors.len(), cached.party_actors.len());
            for (fresh, cached) in fresh.party_actors.iter().zip(&cached.party_actors) {
                assert_eq!(fresh.character, cached.character);
                assert_eq!(fresh.motions, cached.motions);
                assert_eq!(
                    serde_json::to_value(&fresh.source)?,
                    serde_json::to_value(&cached.source)?
                );
                let body = common
                    .body(
                        cached.character as u8,
                        party.members[cached.character as usize - 1].costume,
                    )
                    .unwrap();
                assert!(Arc::ptr_eq(cached.prepared_body.as_ref().unwrap(), &body));
                for (&clip, motion) in &cached.motions {
                    assert!(Arc::ptr_eq(motion, &body.motions[&clip]));
                }
            }
            assert_eq!(
                serde_json::to_value(&fresh.stage)?,
                serde_json::to_value(&cached.stage)?
            );
            for (kind, fresh, cached) in [
                (Kind::Technique, &fresh.spell_scenes, &cached.spell_scenes),
                (Kind::Skill, &fresh.skill_scenes, &cached.skill_scenes),
            ] {
                for (&id, bank) in cached {
                    if let Some((_, shared)) = common.scenes.get(&(kind_key(kind), id)) {
                        assert!(Arc::ptr_eq(bank, shared));
                    }
                    assert_eq!(
                        serde_json::to_value(fresh[&id].as_ref())?,
                        serde_json::to_value(bank.as_ref())?
                    );
                }
            }
            for (path, bytes) in fresh.files.iter() {
                assert_eq!(
                    bytes.as_ref(),
                    cached.files.read(path)?.as_ref(),
                    "changed input {path}"
                );
            }
        }
        common.retire_costume(1);
        assert!(common.body(1, 1).is_none());
        assert!(
            alternate_owner.upgrade().is_none(),
            "retired costume body leaked"
        );
        assert!(common.body(1, 0).is_some(), "default body was evicted");
        let overlay = tempfile::tempdir()?;
        let reopen = || {
            resonance_content::library::Library::open_paths(
                std::path::Path::new(&root),
                &[overlay.path().to_owned()],
            )
        };
        let different = reopen()?;
        let setup = resonance_events::battle::Setup {
            encounter: resonance_events::battle::Encounter::Formation(1),
            arena: 13,
            route: [0; 5],
            music: None,
            defeat: resonance_events::battle::DefeatPolicy::ResumeEvent,
        };
        assert!(
            Assets::capture_with_common(
                &mut different.snapshot(),
                menus,
                &data,
                &party,
                setup,
                diagnostics.clone(),
                &|| false,
                &common
            )
            .is_err(),
            "reused inputs from another mod namespace"
        );
        assert!(!diagnostics.has_errors());

        // An unused broken character must remain a cache miss. A healthy
        // encounter still prepares, while selecting the broken character fails.
        let broken = models::party_path(&mut library.snapshot(), 9, 0)?;
        let write = |path: &str, bytes: &[u8]| -> Result<()> {
            let path = overlay.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap())?;
            Ok(std::fs::write(path, bytes)?)
        };
        write(&broken, b"invalid unused party")?;
        let different = reopen()?;
        let tolerant = Diagnostics::default();
        let warmed = Common::capture(&mut different.snapshot(), &tolerant, &|| false)?;
        assert_eq!(warmed.bodies.len(), 8);
        assert!(warmed.body(9, 0).is_none());
        let setup = resonance_events::battle::Setup {
            encounter: resonance_events::battle::Encounter::Formation(1),
            arena: 13,
            route: [0; 5],
            music: None,
            defeat: resonance_events::battle::DefeatPolicy::ResumeEvent,
        };
        party.members[0].costume = 0;
        party.formation = vec![1, 2, 3, 4];
        Assets::capture_with_common(
            &mut different.snapshot_with(&warmed.files)?,
            menus,
            &data,
            &party,
            setup.clone(),
            tolerant.clone(),
            &|| false,
            &warmed,
        )?;
        party.formation = vec![9];
        assert!(
            Assets::capture_with_common(
                &mut different.snapshot_with(&warmed.files)?,
                menus,
                &data,
                &party,
                setup,
                tolerant,
                &|| false,
                &warmed,
            )
            .is_err()
        );

        // A failed action preload must also prevent incomplete body reuse,
        // including alternate costumes prepared after startup.
        std::fs::remove_file(overlay.path().join(&broken))?;
        let &id = common
            .rules
            .techniques
            .learned_by(1)?
            .first()
            .context("party technique missing")?;
        let techniques: resonance_content::arte::document::Document =
            common.files.json(resonance_content::arte::PATH)?;
        let name = techniques.mappings.target(u32::from(id))?;
        let mut rules = techniques.techniques[name].rules.clone();
        let action = "battle/artes/broken-test/action.json";
        rules.action = Some(action.into());
        write(
            &format!("game/techniques/techniques/{name}/rules.json"),
            &serde_json::to_vec(&rules)?,
        )?;
        write(action, b"invalid unused action")?;
        let different = reopen()?;
        let warmed = Common::capture(&mut different.snapshot(), &Diagnostics::default(), &|| {
            false
        })?;
        assert!(!warmed.actions.contains_key(&id));
        for character in Character::ALL {
            let character = character as u8;
            if warmed.rules.techniques.learned_by(character)?.contains(&id) {
                assert!(warmed.body(character, 0).is_none());
                assert!(warmed.prepare_costume(character, 1, &|| false).is_err());
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS; compares every selected common recipe with ordinary preparation"]
    fn common_effect_libraries_match_uncached_selected_recipes() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?;
        let library = resonance_content::library::Library::open(std::path::Path::new(&root), &[])?;
        let common = Common::capture(&mut library.snapshot(), &Diagnostics::new(true), &|| false)?;
        let mut compared = 0;
        for source in common.effect_banks() {
            let models: BTreeMap<_, _> = source
                .art
                .iter()
                .flat_map(|art| &art.models)
                .filter_map(|(&slot, part)| {
                    common
                        .effect_models
                        .prepare(
                            &common.files,
                            part,
                            resonance_battle::ResourceId(1000 + u32::from(slot)),
                            false,
                        )
                        .ok()
                        .map(|model| (slot, model))
                })
                .collect();
            for member in 0..source.programs.len() {
                let resource = resonance_battle::ResourceId(77);
                let mut requests = crate::battle::ActionResources::default();
                requests.effect(crate::battle::EffectResource {
                    bank: Some(source.clone()),
                    resource,
                    members: vec![u16::try_from(member)?],
                    models: models.clone(),
                })?;
                let selected = &requests.effects[&resource].members;
                let outputs = [false, true].map(|cached| {
                    let mut calls = Vec::new();
                    let mut sound = |id| {
                        calls.push(id);
                        Ok((id % 3 != 0).then_some(resonance_battle::Sound::Stream(id)))
                    };
                    let result = if cached {
                        common.effect_programs.prepare(
                            source,
                            resource,
                            selected,
                            models.clone(),
                            &mut sound,
                            &Diagnostics::new(true),
                        )
                    } else {
                        crate::battle::effect_program::prepare(
                            source,
                            resource,
                            selected,
                            models.clone(),
                            &mut sound,
                            &Diagnostics::new(true),
                        )
                    };
                    (result, calls)
                });
                let [(fresh, fresh_calls), (cached, cached_calls)] = outputs;
                assert_eq!(
                    fresh.is_ok(),
                    cached.is_ok(),
                    "{:?}:{member}",
                    source.document
                );
                assert_eq!(fresh_calls, cached_calls, "sound resolution order changed");
                if let (Ok(fresh), Ok(cached)) = (fresh, cached) {
                    for &member in selected {
                        assert_eq!(
                            fresh.member(member).unwrap().events,
                            cached.member(member).unwrap().events
                        );
                        for event in source.program(usize::from(member))? {
                            if let resonance_content::battle_effect::EffectOperation::Sound {
                                id,
                                ..
                            } = event.operation
                            {
                                assert_eq!(fresh.sound(id), cached.sound(id));
                            }
                        }
                        for (&slot, particle) in &fresh.member(member).unwrap().particles {
                            let shared = &cached.member(member).unwrap().particles[&slot];
                            assert_eq!(particle.member, shared.member);
                            assert_eq!(
                                serde_json::to_value((&particle.model, &particle.data))?,
                                serde_json::to_value((&shared.model, &shared.data))?
                            );
                        }
                    }
                }
                compared += 1;
            }
        }
        assert!(compared > 100 && common.effect_programs.hits() > 0);
        eprintln!(
            "Compared {compared} selected common recipes, including child closure, model validation, missing sounds and sound order"
        );
        Ok(())
    }
}
