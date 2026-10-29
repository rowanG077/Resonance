//! Owned, verified encounter inputs. Loading never changes the retained field.
#[path = "named.rs"]
mod named;
use super::{Enemies, PrepareOptions, Prepared, placement::party_positions};
use crate::battle::{model, party::Character};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    arte,
    battle_audio::Selection,
    battle_effect::{self, SourceBank},
    battle_enemy, battle_model, battle_profile,
    battle_stage::Stage,
    battle_ui::Art,
    menu_data::MenuData,
    prepared::Files,
    session::SessionData,
};
use resonance_events::{battle::Setup, party::Party};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Copy)]
pub(super) enum EquipmentKind {
    Weapon,
    Shield,
}

/// Verified CPU/art inputs can also enumerate required audio during cooking.
/// A live encounter resolves available audio before binding simulation requests.
pub struct Assets {
    pub setup: Setup,
    pub(super) events: Option<crate::battle::event::Plan>,
    event_audio: Selection,
    pub files: Files,
    pub stage: Stage,
    pub ui: Art,
    pub catalogue: Arc<arte::Catalogue>,
    pub unison: Arc<resonance_content::battle_unison::Catalogue>,
    pub(super) projectiles: resonance_content::battle_projectile::Table,
    pub(super) recoil: crate::battle::recoil::Parameters,
    pub(crate) victory: resonance_content::battle_victory::Performances,
    pub game_over: Option<crate::game_over::Assets>,
    pub enemies: Enemies,
    pub(super) party: Party,
    pub(super) party_profiles: battle_profile::Table,
    pub(in crate::battle) defense_system: resonance_content::battle_defense_system::Definition,
    pub(super) party_actors: Vec<PartyInput>,
    pub(super) weapons: BTreeMap<u16, battle_model::Weapon>,
    pub(super) enemy_definitions: BTreeMap<u8, battle_enemy::Definition>,
    pub(super) enemy_models: BTreeMap<
        u8,
        (
            battle_model::Enemy,
            BTreeMap<u16, resonance_content::animation::Motion>,
        ),
    >,
    pub common_effects: Option<Arc<SourceBank>>,
    pub technique_effects: Option<Arc<SourceBank>>,
    pub stage_effects: Option<Arc<SourceBank>>,
    pub enemy_effects: BTreeMap<u8, Arc<SourceBank>>,
    pub spell_scenes: BTreeMap<u16, Arc<SourceBank>>,
    pub skill_scenes: BTreeMap<u16, Arc<SourceBank>>,
}

pub(super) struct PartyInput {
    pub character: Character,
    pub position: [f32; 3],
    pub(super) source: battle_model::Party,
    pub(super) equipment: BTreeMap<u16, EquipmentKind>,
    pub(super) motions: BTreeMap<u16, resonance_content::animation::Motion>,
}

impl Assets {
    /// Replay externally selected choreography without changing ordinary encounter assets.
    #[cfg(feature = "test-support")]
    pub fn bind_fixture_party_motion(
        &mut self,
        actor: usize,
        clip: u16,
        motion: resonance_content::animation::Motion,
    ) -> Result<()> {
        let input = self
            .party_actors
            .get_mut(actor)
            .context("fixture motion party actor is absent")?;
        ensure!(clip >= 100, "fixture motion must use an external slot");
        ensure!(
            !input.motions.contains_key(&clip),
            "fixture motion slot is already bound"
        );
        motion.validate(&input.source.body.skeleton)?;
        input.motions.insert(clip, motion);
        Ok(())
    }
}

fn choreography_ids(
    catalogue: &arte::Catalogue,
    unison: &resonance_content::battle_unison::Catalogue,
    party_actors: &[PartyInput],
    enemy_definitions: &BTreeMap<u8, battle_enemy::Definition>,
) -> Result<(BTreeSet<u16>, BTreeSet<u16>)> {
    let mut scene_ids = BTreeSet::new();
    let mut skill_ids = BTreeSet::new();
    let mut party_artes = BTreeSet::new();
    for actor in party_actors {
        for catalogue in super::resources::party_techniques(catalogue, actor.character)?.into_keys()
        {
            if crate::battle::skill_scene::contains(catalogue) {
                skill_ids.insert(catalogue);
            }
            scene_ids.extend(
                crate::battle::stored_spells::scene(catalogue)
                    .or_else(|| crate::battle::summons::scene(catalogue)),
            );
            if let Some(placement) = unison.placements.get(&catalogue) {
                party_artes.insert(placement.native_id);
            }
        }
    }
    for enemy in enemy_definitions.values() {
        for native in crate::battle::enemy::technique_ids(enemy) {
            if let Some(catalogue) = crate::battle::enemy::catalogue_id(unison, native) {
                scene_ids.extend(
                    crate::battle::stored_spells::scene(catalogue)
                        .or_else(|| crate::battle::summons::scene(catalogue)),
                );
                if crate::battle::skill_scene::contains(catalogue) {
                    skill_ids.insert(catalogue);
                }
            }
        }
    }
    scene_ids.extend(unison.combinations.iter().filter_map(|combination| {
        combination
            .recipes
            .iter()
            .any(|recipe| {
                recipe
                    .iter()
                    .all(|ingredient| party_artes.contains(ingredient))
            })
            .then_some(combination.native_id)
    }));
    Ok((scene_ids, skill_ids))
}
