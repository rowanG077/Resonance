//! Prepare combined techniques as ordinary actor actions with scene-owned placement.
use crate::battle::encounter::resources::Resources;
use crate::battle::party::Character;
use crate::battle::{ActionDefinition, ActionDefinitions, ActionExecution, EffectResource};
use anyhow::{Context, Result};
use resonance_battle::{
    ActorId, AttackEvent, EffectAppearance, ModelDefinition, Sound, UnisonCombination,
    WeaponDefinition,
};
use resonance_content::{battle_scene::Scene, battle_unison::Combination};
use std::{collections::BTreeSet, sync::Arc};

mod action;
mod family;
mod host;
pub(crate) use host::{HostMember, prepare_host};

pub(crate) struct Participant<'a> {
    pub actor: ActorId,
    pub character: Character,
    pub model: &'a ModelDefinition,
    pub null_motions: &'a BTreeSet<u16>,
    pub temporary_weapons: Vec<Arc<WeaponDefinition>>,
}

pub(crate) fn prepare(
    combination: &Combination,
    scene: &Scene,
    bank: &EffectResource,
    participants: &[Participant<'_>],
    resources: &mut Resources<'_>,
    actions: &mut ActionDefinitions,
) -> Result<Option<UnisonCombination>> {
    family::prepare(combination, scene, bank, participants, resources, actions)
}
