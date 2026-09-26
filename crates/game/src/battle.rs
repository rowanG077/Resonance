//! Battle sequence preparation. Filesystem integrity and rendering resource
//! preparation belong to the caller, before this batch can be activated.
pub mod action;
pub mod ai;
pub mod audio;
pub mod casting;
pub mod command;
pub mod companion;
pub mod contact_audio;
pub mod contact_feedback;
pub mod control;
pub mod death;
pub mod effect_program;
pub mod effect_timeline;
pub mod encounter;
pub mod enemy;
pub mod entry;
pub mod entry_transition;
pub mod fire_ball;
pub mod lifecycle;
pub mod martial;
pub mod melee;
pub mod model;
pub mod normal;
pub mod party;
pub mod profile;
pub mod projectile;
pub mod recoil;
pub mod results;
pub mod rewards;
pub mod stage;
pub mod trail;
pub mod victory;
pub mod voice;
pub mod weapon;
pub mod weapon_flight;
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActionDefinition, ActionPhase, Actor, PreparedBattle, ResourceBinding};
use resonance_content::prepared::Files;
use std::sync::Arc;
use symphonia_script_compiler::ScriptKind;
use symphonia_script_tools::PreparationCache;

/// Bindings are data; they never contain compiled instructions or source recipes.
#[derive(Debug, Clone)]
pub struct ActionBinding {
    pub id: u16,
    pub phase: ActionPhase,
    pub module: String,
    pub entry: String,
    pub duration: u16,
    pub tp_cost: u16,
}

/// Original source members needed by the encounter and their ready presentation
/// resource generation. Executable modules are prepared by the game loader.
#[derive(Debug, Clone)]
pub struct EffectResource {
    pub source: String,
    pub resource: u32,
    pub members: Vec<u16>,
    /// Stored scene and presentation IDs for its independently bound model slots.
    pub scene: Option<SceneResources>,
    /// Verified ordinary model templates. Each model particle owns its playback.
    pub models: std::collections::BTreeMap<u8, resonance_battle::PreparedEffectModel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneResources {
    pub technique: u16,
    pub models: std::collections::BTreeMap<u8, u32>,
}

pub struct ProjectileResource {
    pub source: String,
    pub member: u16,
    /// Selects the caller's original hit descriptor in the verified action table.
    pub hit: HitResource,
    /// Caller-selected banks, including the overrides in the original allocator.
    pub birth: Option<EffectResource>,
    pub clash: Option<EffectResource>,
    pub trail: Option<EffectResource>,
    pub ground: Option<EffectResource>,
    pub impact: Option<EffectResource>,
}

#[derive(Debug, Clone, Copy)]
pub enum HitSelection {
    Technique { member: u16, phase: u8 },
    Enemy,
}

#[derive(Debug, Clone)]
pub struct HitResource {
    pub source: String,
    pub selection: HitSelection,
    /// Relative to a technique phase's root, or an enemy's shared rule pool.
    pub rule: u8,
}

#[derive(Debug, Clone, Copy)]
pub enum MeleeSelection {
    Normal {
        /// Zero-based character group in the original normal table.
        character: u8,
        selection: u8,
    },
    Enemy {
        action: u8,
    },
}

/// An attached contact and the model/weapon groups resolved to pose anchors.
pub struct MeleeResource {
    pub source: String,
    pub selection: MeleeSelection,
    /// Relative to the selected action's hit-stream root.
    pub row: u8,
    pub anchor_groups: Vec<Vec<u16>>,
    pub impact: Option<EffectResource>,
}

pub struct WeaponFlightResource {
    pub source: String,
    pub character: u8,
    pub selection: u8,
    pub row: u8,
    pub impact: Option<EffectResource>,
}

/// The resource service prepares rendering and audio dependencies before returning their
/// IDs. Original effect commands are loaded from verified files by `prepare`.
pub trait BattleResources {
    /// Core-owned feedback may need effects without an authored asset reference.
    fn effects(&mut self) -> Vec<EffectResource> {
        vec![]
    }
    fn casting(&mut self, path: &str) -> Result<casting::CastingResource>;
    fn voice(&mut self, _path: &str) -> Result<Vec<Option<resonance_battle::VoiceLine>>> {
        anyhow::bail!("actor voices are not prepared")
    }
    fn sound(&mut self, path: &str) -> Result<resonance_battle::SoundBinding>;
    fn effect(&mut self, path: &str) -> Result<EffectResource>;
    fn particle(&mut self, path: &str) -> Result<Arc<resonance_battle::ParticleDefinition>>;
    /// Selects the source row, caller hit descriptor and ready effect resources.
    /// `prepare` reads the verified row and compiles its effect dependencies.
    fn projectile(&mut self, path: &str) -> Result<ProjectileResource>;
    fn melee(&mut self, path: &str) -> Result<MeleeResource>;
    fn weapon_flight(&mut self, _path: &str) -> Result<WeaponFlightResource> {
        anyhow::bail!("detached weapon flights are not prepared")
    }
    fn motion(&mut self, path: &str) -> Result<resonance_battle::MotionBinding>;
    fn optional_motion(
        &mut self,
        _path: &str,
    ) -> Result<Vec<Option<resonance_battle::MotionBinding>>> {
        anyhow::bail!("optional actor motions are not prepared")
    }
    fn spell(&mut self, path: &str) -> Result<u16>;
}

/// Compile from an immutable verified snapshot and bind the complete set before
/// returning anything that can be activated. Failure cannot mutate a live battle,
/// the persistent party, or the suspended field VM.
pub fn prepare(
    cache: &mut PreparationCache,
    files: &Files,
    bindings: &[ActionBinding],
    actors: Vec<Actor>,
    random_seed: u32,
    resources: &mut impl BattleResources,
    models: Vec<Option<Arc<resonance_battle::ModelDefinition>>>,
) -> Result<Arc<PreparedBattle>> {
    let sources = files.script_sources()?;
    let generation = cache.prepare(
        bindings.iter().map(|b| b.module.as_str()),
        &sources,
        &resonance_battle::native_declarations(),
    )?;
    let mut actions = Vec::with_capacity(bindings.len());
    let mut effects = std::collections::BTreeMap::new();
    for request in resources.effects() {
        require_effect(&mut effects, request)?;
    }
    for binding in bindings {
        let module = generation
            .module(&binding.module)
            .context("missing battle module")?;
        ensure!(
            module.kind == ScriptKind::Battle,
            "battle action requires `script battle;`"
        );
        let name = format!("{}::{}", binding.module, binding.entry);
        let function = module
            .program
            .authored()
            .unwrap()
            .functions
            .iter()
            .find(|f| f.name == name)
            .with_context(|| format!("missing battle entry {name}"))?;
        let mut motions = Vec::new();
        let mut assets: Vec<_> = module
            .assets
            .iter()
            .enumerate()
            .map(|(index, asset)| {
                ensure!(
                    asset.index as usize == index,
                    "invalid battle resource binding {}",
                    asset.path
                );
                resonance_content::validate_asset_path(&asset.path)?;
                match asset.kind.as_str() {
                    "battle::ActorTints" => {
                        let table: resonance_content::battle_effect::Tints =
                            files.json(&asset.path)?;
                        Ok(ResourceBinding::ActorTints(table.actors))
                    }
                    "battle::Casting" => {
                        let request = resources.casting(&asset.path)?;
                        Ok(ResourceBinding::Casting(Arc::new(casting::load(
                            files,
                            request,
                            module.assets.len(),
                            &mut motions,
                        )?)))
                    }
                    "battle::Voice" => resources.voice(&asset.path).map(ResourceBinding::Voice),
                    "battle::Sound" => resources.sound(&asset.path).map(ResourceBinding::Sound),
                    "battle::ParticleTemplate" => resources
                        .particle(&asset.path)
                        .map(ResourceBinding::Particle),
                    "battle::Effect" => {
                        let request = resources.effect(&asset.path)?;
                        let resource = request.resource;
                        require_effect(&mut effects, request)?;
                        Ok(ResourceBinding::Effect(resource))
                    }
                    "battle::Projectile" => {
                        let request = resources.projectile(&asset.path)?;
                        let definition = projectile::load(files, &request)?;
                        for effect in request
                            .birth
                            .into_iter()
                            .chain(request.clash)
                            .chain(request.trail)
                            .chain(request.ground)
                            .chain(request.impact)
                        {
                            require_effect(&mut effects, effect)?;
                        }
                        Ok(ResourceBinding::Projectile(Arc::new(definition)))
                    }
                    "battle::Melee" => {
                        let request = resources.melee(&asset.path)?;
                        let definition = melee::load(files, &request)?;
                        if let Some(effect) = request.impact {
                            require_effect(&mut effects, effect)?;
                        }
                        Ok(ResourceBinding::Melee(Arc::new(definition)))
                    }
                    "battle::WeaponFlight" => {
                        let request = resources.weapon_flight(&asset.path)?;
                        let definition = weapon_flight::load(files, &request)?;
                        if let Some(effect) = request.impact {
                            require_effect(&mut effects, effect)?;
                        }
                        Ok(ResourceBinding::WeaponFlight(Arc::new(definition)))
                    }
                    "battle::Motion" => resources.motion(&asset.path).map(ResourceBinding::Motion),
                    "battle::OptionalMotion" => resources
                        .optional_motion(&asset.path)
                        .map(ResourceBinding::OptionalMotion),
                    "battle::Spell" => resources.spell(&asset.path).map(ResourceBinding::Spell),
                    _ => anyhow::bail!("unknown battle resource kind {}", asset.kind),
                }
            })
            .collect::<Result<_>>()?;
        assets.extend(motions);
        actions.push(ActionDefinition {
            id: binding.id,
            phase: binding.phase,
            program: module.program.clone(),
            entry: function.entry,
            duration: binding.duration,
            tp_cost: binding.tp_cost,
            resources: assets,
        });
    }
    Ok(Arc::new(PreparedBattle::new(
        actors,
        actions,
        random_seed,
        models,
        effects
            .into_values()
            .map(|request| {
                let mut bank = effect_program::load(
                    files,
                    &request.source,
                    request.resource,
                    &request.members,
                    &mut |id| resources.sound(&format!("battle/sounds/common/{id}")),
                )?;
                bank.models = request.models;
                if let Some(scene) = request.scene {
                    ensure!(
                        bank.models.is_empty(),
                        "stored scene also binds ordinary models"
                    );
                    let definition: resonance_content::battle_scene::Scene =
                        files.json(&resonance_content::battle_scene::path(scene.technique))?;
                    ensure!(
                        definition.effects == request.source,
                        "scene effect source differs"
                    );
                    bank.models = model::scene(files, &definition, &scene.models)?;
                }
                Ok(bank)
            })
            .collect::<Result<_>>()?,
    )?))
}

fn require_effect(
    effects: &mut std::collections::BTreeMap<u32, EffectResource>,
    mut request: EffectResource,
) -> Result<()> {
    if let Some(previous) = effects.get_mut(&request.resource) {
        ensure!(
            previous.source == request.source
                && previous.scene == request.scene
                && previous
                    .models
                    .iter()
                    .map(|(slot, model)| (slot, model.resource()))
                    .eq(request
                        .models
                        .iter()
                        .map(|(slot, model)| (slot, model.resource()))),
            "inconsistent effect resource {}",
            request.resource
        );
        previous.members.append(&mut request.members);
        previous.members.sort_unstable();
        previous.members.dedup();
    } else {
        effects.insert(request.resource, request);
    }
    Ok(())
}
