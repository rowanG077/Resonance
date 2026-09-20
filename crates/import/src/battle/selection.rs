//! Explicit cook inputs; a selected bundle is not an exhaustive coverage claim.
use anyhow::{Context, Result, ensure};
use resonance_content::battle::{
    actions::{
        Action, ActionCommand, BattleActions, CastRecipe, HitEmission, TechniqueProgram,
        TimedCommand, thunder_arrow::ThunderArrowRecipe,
    },
    effect_program::{BattleEffectPrograms, EffectCommand, ModelRef},
    effects::{BattleEffects, EffectBank, EffectId, ProjectileRecipe},
    projectile_modifiers::ProjectileModifiers,
    visual::VisualAssets,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Encounter {
    pub formation: u16,
    pub arena: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CookSelection {
    pub encounters: Vec<Encounter>,
    pub party: Vec<u8>,
    /// Menu arte IDs. Native routine IDs are resolved from the source table.
    pub artes: Vec<u16>,
    pub weapons: Vec<u16>,
    /// Additional authored effect programs, including programs without visual emissions.
    #[serde(default)]
    pub effects: Vec<EffectId>,
    /// Particle recipes created directly by combat controllers, without a program.
    #[serde(default)]
    pub effect_actors: Vec<EffectId>,
}

impl Default for CookSelection {
    fn default() -> Self {
        Self {
            encounters: [1, 2]
                .map(|formation| Encounter {
                    formation,
                    arena: 13,
                })
                .into(),
            party: vec![1, 2, 3],
            artes: vec![1, 35, 66],
            weapons: vec![135, 159, 175],
            effects: Vec::new(),
            effect_actors: Vec::new(),
        }
    }
}

impl CookSelection {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.encounters.is_empty() && !self.party.is_empty(),
            "battle selection needs encounters and party actors"
        );
        unique(&self.encounters, "encounter")?;
        unique(&self.party, "party actor")?;
        unique(&self.artes, "arte")?;
        unique(&self.weapons, "weapon")?;
        unique(&self.effects, "effect program")?;
        unique(&self.effect_actors, "effect actor")?;
        ensure!(
            self.party.iter().all(|id| (1..=9).contains(id)),
            "invalid selected party actor"
        );
        ensure!(
            self.artes.iter().all(
                |&id| id > 0 && usize::from(id) < resonance_content::menu_data::TECHNIQUE_COUNT
            ),
            "invalid selected arte ID"
        );
        ensure!(
            self.weapons.iter().all(|&id| id > 0),
            "invalid selected weapon ID"
        );
        Ok(())
    }
}

fn unique<T: Ord>(values: &[T], kind: &str) -> Result<()> {
    ensure!(
        values.iter().collect::<BTreeSet<_>>().len() == values.len(),
        "duplicate selected {kind}"
    );
    Ok(())
}

#[test]
fn optional_effect_selection_accepts_zero_ids_and_distinguishes_banks() {
    let mut json = serde_json::to_value(CookSelection::default()).unwrap();
    json.as_object_mut().unwrap().remove("effects");
    json.as_object_mut().unwrap().remove("effect_actors");
    let mut selection: CookSelection = serde_json::from_value(json).unwrap();
    assert!(selection.effects.is_empty());
    assert!(selection.effect_actors.is_empty());
    selection.effects = [
        EffectBank::Common,
        EffectBank::Techniques,
        EffectBank::Magic(38),
        EffectBank::Magic(57),
    ]
    .map(|bank| EffectId { bank, id: 0 })
    .into();
    selection.validate().unwrap();
    selection.effect_actors = selection.effects.clone();
    selection.validate().unwrap(); // Actor and program IDs belong to separate tables.
    selection.effects.push(selection.effects[0]);
    assert_eq!(
        selection.validate().unwrap_err().to_string(),
        "duplicate selected effect program"
    );
}

pub(super) struct Dependencies {
    pub projectiles: BTreeSet<EffectId>,
    pub programs: BTreeSet<EffectId>,
}

impl Dependencies {
    pub fn unison(
        &mut self,
        unison: &resonance_content::battle::unison::UnisonData,
        party: &[u8],
    ) -> Result<()> {
        use resonance_content::battle::unison::CombinedPair;
        for id in [13, 14] {
            self.program(EffectBank::Common, id)?;
        }
        for (kind, program) in &unison.strikes {
            if !kind.selected(party) {
                continue;
            }
            self.programs
                .extend(kind.programs().map(|id| kind.effect(id)));
            self.projectiles.insert(kind.effect(1));
            for phase in &program.phases {
                self.action(kind.bank(), &phase.action);
                for hit in &phase.action.hits {
                    self.programs.extend(
                        hit.rule.impact_program_from(
                            EffectBank::Techniques,
                            Some(kind.native() - 200),
                        )?,
                    );
                }
            }
            self.programs.extend(
                program
                    .projectile_rule
                    .impact_program_from(EffectBank::Techniques, Some(kind.native() - 200))?,
            );
        }
        for kind in CombinedPair::ALL {
            let Some(program) = unison.pair_program(kind).filter(|_| kind.selected(party)) else {
                continue;
            };
            self.programs
                .extend(kind.programs().map(|id| kind.effect(id)));
            self.projectiles.extend(
                program
                    .projectiles
                    .iter()
                    .flatten()
                    .flat_map(|p| p.ids().map(|id| kind.effect(id))),
            );
            for phase in &program.phases {
                self.action(EffectBank::Magic(kind.native() - 200), &phase.action);
            }
            for rule in program
                .phases
                .iter()
                .flat_map(|phase| &phase.action.hits)
                .map(|hit| hit.rule)
                .chain(program.projectiles.iter().flatten().map(|p| p.rule))
            {
                self.programs.extend(
                    rule.impact_program_from(EffectBank::Techniques, Some(kind.native() - 200))?,
                );
            }
        }
        for &character in party {
            let opener = character
                .checked_sub(1)
                .and_then(|index| unison.opener.get(usize::from(index)))
                .context("invalid Unison opener character")?;
            let action = &opener.action;
            self.action(EffectBank::Techniques, action);
            self.programs
                .extend(action.impact_programs(EffectBank::Techniques)?);
        }
        Ok(())
    }

    pub fn actions(actions: &BattleActions) -> Result<Self> {
        let mut result = Self {
            projectiles: BTreeSet::new(),
            programs: actions
                .impact_programs()?
                .into_iter()
                .chain(actions.chain_effects())
                .collect(),
        };
        if !actions.party.is_empty() || !actions.enemies.is_empty() {
            result.program(EffectBank::Common, 17)?;
        }
        for party in &actions.party {
            for normal in &party.normal {
                result.action(EffectBank::Techniques, &normal.action);
                if let Some(id) = normal.effect {
                    result.program(EffectBank::Common, id)?;
                }
            }
        }
        for enemy in &actions.enemies {
            for binding in enemy.native_spells() {
                anyhow::ensure!(
                    actions
                        .techniques
                        .iter()
                        .any(|technique| binding.matches(technique)),
                    "enemy {} requires uncooked native spell binding {binding:?}",
                    enemy.monster
                );
                result.program(EffectBank::Common, 7)?;
            }
            if let Some(effect) = enemy.locomotion.and_then(|recipe| recipe.effect) {
                result.program(EffectBank::Enemy(enemy.monster), u16::from(effect.id))?;
            }
            for cast in enemy.casting.values() {
                for id in [6, cast.pulse, cast.release_effect] {
                    result.program(EffectBank::Common, u16::from(id))?;
                }
            }
            for action in &enemy.actions {
                result.action(EffectBank::Enemy(enemy.monster), &action.action);
                if let Some(id) = action.effect {
                    result.program(EffectBank::Enemy(enemy.monster), u16::from(id))?;
                }
            }
        }
        for arte in &actions.techniques {
            match &arte.program {
                TechniqueProgram::Martial { variants } => {
                    for phase in variants {
                        if let Some(alternate) = &phase.alternate {
                            result.programs.insert(alternate.effect);
                        }
                        result.action(EffectBank::Techniques, &phase.action);
                        if let Some(callback) = phase.callback {
                            result.projectiles.extend(callback.projectiles());
                            if let resonance_content::battle::actions::MartialCallback::Steal {
                                recipe,
                            } = callback
                            {
                                for operation in recipe.overrides {
                                    if let resonance_content::battle::projectile_modifiers::ProjectileOverride::BirthEffect { id } = operation {
                                        result.program(recipe.projectile.bank, u16::from(id))?;
                                    }
                                }
                            }
                            if let resonance_content::battle::actions::MartialCallback::ContactArea { followup, .. } = callback {
                                result.program(followup.effect.bank, u16::from(followup.effect.id))?;
    }
                            if let resonance_content::battle::actions::MartialCallback::Seal {
                                recipe,
                            } = callback
                            {
                                result.programs.insert(recipe.impact.effect);
                            }
                            if let resonance_content::battle::actions::MartialCallback::ContactProjectile { overrides, followup, .. } = callback {
                                for operation in overrides {
                                    if let resonance_content::battle::projectile_modifiers::ProjectileOverride::BirthEffect { id } = operation {
                                        result.program(EffectBank::Techniques, u16::from(id))?;
                                    }
                                }
                                if let Some(followup) = followup {
                                    result.program(followup.effect.bank, u16::from(followup.effect.id))?;
                                }
                            }
                            if let resonance_content::battle::actions::MartialCallback::HammerVolley { birth_effects, .. } = callback {
                                for id in birth_effects { result.program(EffectBank::Techniques, u16::from(id))?; }
                            }
                            if let resonance_content::battle::actions::MartialCallback::TigerBlade { overrides, .. } = callback {
                                for operation in overrides {
                                    if let resonance_content::battle::projectile_modifiers::ProjectileOverride::BirthEffect { id } = operation {
                                        result.program(EffectBank::Techniques, u16::from(id))?;
                                    }
                                }
                            }
                        }
                        if let Some(id) = phase.effect {
                            result.program(EffectBank::Techniques, id)?;
                        }
                    }
                }
                TechniqueProgram::FireBall {
                    effect,
                    cast_pulse,
                    cast_commands,
                    emissions,
                    ..
                } => {
                    result.commands(cast_commands);
                    result.program(EffectBank::Techniques, u16::from(*effect))?;
                    for id in [*cast_pulse, 6, 7] {
                        result.program(EffectBank::Common, u16::from(id))?;
                    }
                    result
                        .projectiles
                        .extend(emissions.iter().map(|shot| EffectId {
                            bank: EffectBank::Techniques,
                            id: shot.effect,
                        }));
                }
                TechniqueProgram::Lightning { casters, recipe } => {
                    result.casting(casters)?;
                    result
                        .projectiles
                        .extend(recipe.pulses.iter().map(|pulse| pulse.projectile));
                    if let Some(stored) = &recipe.stored {
                        result.program(EffectBank::Common, 37)?;
                        result.programs.insert(stored.effect);
                    }
                }
                TechniqueProgram::Icicle { casters, recipe } => {
                    result.casting(casters)?;
                    result.programs.insert(recipe.effect);
                    result.projectiles.extend(recipe.contact.id);
                }
                TechniqueProgram::StoneBlast { casters, recipe } => {
                    result.casting(casters)?;
                    result.programs.insert(recipe.effect);
                    result.projectiles.extend(recipe.contact.id);
                }
                TechniqueProgram::WindBlade { casters, recipe } => {
                    result.casting(casters)?;
                    result.programs.insert(recipe.effect);
                    result.projectiles.extend(recipe.contact.id);
                }
                TechniqueProgram::IceTornado { casters, .. } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    let effect =
                        resonance_content::battle::actions::ice_tornado::IceTornadoRecipe::EFFECT;
                    result.programs.insert(effect);
                    result.projectiles.insert(effect);
                }
                TechniqueProgram::FreezeLancer { casters, .. } => {
                    use resonance_content::battle::actions::freeze_lancer::FreezeLancerRecipe;
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.insert(FreezeLancerRecipe::effect(1));
                    result.projectiles.insert(FreezeLancerRecipe::effect(1));
                }
                TechniqueProgram::AcidRain { recipe } => {
                    result.program(EffectBank::Common, 37)?;
                    result.programs.extend([
                        recipe.effect,
                        resonance_content::battle::actions::acid_rain::AcidRainRecipe::effect(2),
                    ]);
                }
                TechniqueProgram::Absolute { casters, .. } => {
                    use resonance_content::battle::actions::absolute::AbsoluteRecipe;
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.extend((1..=3).map(AbsoluteRecipe::effect));
                    result
                        .projectiles
                        .extend((1..=2).map(AbsoluteRecipe::effect));
                }
                TechniqueProgram::EarthBite { casters, .. } => {
                    use resonance_content::battle::actions::earth_bite::EarthBiteRecipe;
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.extend((1..=1).map(EarthBiteRecipe::effect));
                    result
                        .projectiles
                        .extend((1..=2).map(EarthBiteRecipe::effect));
                }
                TechniqueProgram::MeteorStorm { casters, .. } => {
                    use resonance_content::battle::actions::meteor_storm::MeteorStormRecipe;
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result
                        .programs
                        .extend((1..=3).map(MeteorStormRecipe::effect));
                    result
                        .projectiles
                        .extend((1..=1).map(MeteorStormRecipe::effect));
                }
                TechniqueProgram::SpiralFlare { casters, .. } => {
                    use resonance_content::battle::actions::spiral_flare::SpiralFlareRecipe;
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result
                        .programs
                        .extend((1..=3).map(SpiralFlareRecipe::effect));
                    result.projectiles.insert(SpiralFlareRecipe::effect(1));
                }
                TechniqueProgram::GroundPulse {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.insert(recipe.kind.effect(1));
                    result.projectiles.insert(recipe.kind.effect(1));
                }
                TechniqueProgram::PrismSword { casters, .. } => {
                    use resonance_content::battle::actions::prism::PrismRecipe;
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.extend((1..=10).map(PrismRecipe::effect));
                    result
                        .projectiles
                        .extend([PrismRecipe::effect(1), PrismRecipe::effect(2)]);
                }
                TechniqueProgram::Ray { casters, .. } => {
                    use resonance_content::battle::actions::ray::RayRecipe;
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result
                        .programs
                        .extend([RayRecipe::effect(1), RayRecipe::effect(2)]);
                    result.projectiles.insert(RayRecipe::effect(1));
                }
                TechniqueProgram::Lance {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result
                        .programs
                        .extend((1..=6).map(|id| recipe.kind.effect(id)));
                    result
                        .projectiles
                        .extend([recipe.kind.effect(1), recipe.kind.effect(2)]);
                }
                TechniqueProgram::Orb {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.insert(recipe.kind.effect(1));
                    result
                        .projectiles
                        .extend([recipe.kind.effect(1), recipe.kind.effect(2)]);
                }
                TechniqueProgram::ThunderArrow { casters, .. } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    for id in [1, 2] {
                        result.programs.insert(ThunderArrowRecipe::effect(id));
                    }
                    result.projectiles.insert(ThunderArrowRecipe::effect(1));
                }
                TechniqueProgram::EarthField {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.insert(recipe.kind.effect(1));
                    result
                        .projectiles
                        .extend(recipe.pulses.iter().map(|pulse| pulse.projectile));
                }
                TechniqueProgram::FireField {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.insert(recipe.effect);
                    result
                        .projectiles
                        .extend(recipe.pulses.iter().map(|pulse| pulse.projectile));
                }
                TechniqueProgram::WindField {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.insert(recipe.kind.effect(1));
                    result.projectiles.insert(recipe.kind.effect(1));
                }
                TechniqueProgram::AirThrust {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.insert(recipe.effect);
                    result.projectiles.insert(recipe.projectile);
                }
                TechniqueProgram::AquaEdge { casters, .. } => {
                    result.casting(casters)?;
                    result.projectiles.insert(EffectId {
                        bank: EffectBank::Techniques,
                        id: 8,
                    });
                }
                TechniqueProgram::Water {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.insert(recipe.kind.effect(1));
                    result.projectiles.insert(recipe.kind.effect(1));
                }
                TechniqueProgram::Spread {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.insert(recipe.effect);
                    result.projectiles.insert(recipe.projectile);
                }
                TechniqueProgram::GroundSummon {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 36)?;
                    if recipe.kind == resonance_content::battle::actions::ground_summon::GroundSummonKind::Undine {
                        result.program(EffectBank::Techniques, 20)?;
                    }
                    result
                        .programs
                        .extend([recipe.kind.effect(1), recipe.kind.effect(2)]);
                    result
                        .projectiles
                        .extend(recipe.pulses.iter().map(|p| p.projectile));
                }
                TechniqueProgram::Summon {
                    casters, recipe, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 36)?;
                    result
                        .programs
                        .extend([recipe.kind.effect(1), recipe.kind.strike_effect()]);
                    result.projectiles.insert(recipe.kind.effect(1));
                }
                TechniqueProgram::Nurse { casters, .. } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.extend(
                        (1..=6).map(resonance_content::battle::actions::nurse::NurseRecipe::effect),
                    );
                }
                TechniqueProgram::RecoverySpell {
                    casters, pulses, ..
                } => {
                    result.casting(casters)?;
                    result
                        .programs
                        .extend(pulses.iter().map(|pulse| pulse.effect));
                }
                TechniqueProgram::StoredRecoverySpell {
                    casters, effect, ..
                } => {
                    result.casting(casters)?;
                    result.program(EffectBank::Common, 37)?;
                    result.programs.insert(*effect);
                }
            }
        }
        Ok(result)
    }

    fn casting(&mut self, casters: &[CastRecipe]) -> Result<()> {
        self.program(EffectBank::Common, 6)?;
        for cast in casters {
            self.commands(&cast.commands);
            self.program(EffectBank::Common, u16::from(cast.pulse))?;
            self.program(EffectBank::Common, u16::from(cast.release_effect))?;
        }
        Ok(())
    }

    fn action(&mut self, bank: EffectBank, action: &Action) {
        self.commands(&action.commands);
        self.projectiles
            .extend(action.hits.iter().filter_map(|hit| {
                if let HitEmission::Effect { effect, .. } = hit.emission {
                    Some(EffectId { bank, id: effect })
                } else {
                    None
                }
            }));
    }

    fn commands(&mut self, commands: &[TimedCommand]) {
        self.programs
            .extend(commands.iter().filter_map(|step| match step.command {
                ActionCommand::ImpactFlash => Some(EffectId {
                    bank: EffectBank::Common,
                    id: 13,
                }),
                ActionCommand::CommonEffect { id, .. } => Some(EffectId {
                    bank: EffectBank::Common,
                    id,
                }),
                _ => None,
            }));
    }

    fn program(&mut self, bank: EffectBank, id: u16) -> Result<()> {
        self.programs.insert(EffectId {
            bank,
            id: id
                .try_into()
                .context("effect program ID exceeds its bank")?,
        });
        Ok(())
    }

    pub fn projectiles(
        &mut self,
        effects: &BattleEffects,
        modifiers: &ProjectileModifiers,
    ) -> Result<()> {
        for &id in &self.projectiles {
            let recipe = effects
                .projectile(id)
                .with_context(|| format!("missing selected projectile {id:?}"))?;
            self.programs.extend(programs(recipe));
        }
        for usage in &modifiers.uses {
            let mut recipe = effects
                .projectile(usage.projectile)
                .context("modifier requires an uncooked projectile")?
                .clone();
            let program = modifiers
                .programs
                .iter()
                .find(|program| program.id == usage.modifier)
                .context("missing selected projectile modifier")?;
            program.apply(&mut recipe)?;
            self.programs.extend(programs(&recipe));
        }
        Ok(())
    }

    pub fn validate(&self, effects: &BattleEffectPrograms, visuals: &VisualAssets) -> Result<()> {
        for &id in &self.programs {
            let program = effects
                .program(id)
                .with_context(|| format!("missing selected effect program {id:?}"))?;
            for emission in &program.emissions {
                if let EffectCommand::Particle { actor, .. } = emission.command {
                    let actor = effects
                        .actor(actor)
                        .context("missing selected effect actor")?;
                    for model in effects.models_for(actor.id) {
                        // Equipped items are chosen at battle entry; this checks
                        // that the bundle can supply the dynamic weapon binding.
                        let available = if model == ModelRef::ColetteWeapon {
                            visuals.party.contains_key(&2)
                                && visuals.weapons.values().any(|w| w.slots.contains_key(&0))
                        } else {
                            visuals.effect_models.iter().any(|v| v.binding == model)
                        };
                        ensure!(
                            available,
                            "selected effect {id:?} requires an uncooked model {model:?}"
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

fn programs(recipe: &ProjectileRecipe) -> impl Iterator<Item = EffectId> {
    [
        recipe.spawn_effect,
        recipe.trail_effect,
        recipe.ground_effect,
    ]
    .into_iter()
    .flatten()
}
