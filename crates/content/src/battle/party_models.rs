//! Validate actor-specific commands on every concrete body/CAB binding.
use super::{
    BattleCatalog,
    actions::{
        Action, ActionCommand, AnimationCommand, AnimationProgram, BattleActions, CastRecipe,
        HitAttachment, HitEmission, TechniqueProgram, TimedCommand,
    },
    visual::{MotionSlot, PartyModel, PartyVisuals, VictoryMotion, VictoryStyle},
};
use anyhow::{Context, Result, ensure};

impl PartyModel {
    /// Validate a source-owner-resolved normal or martial phase on this body/CAB.
    pub fn validate_action(&self, authored: &Action) -> Result<()> {
        action(self, authored)
    }

    pub fn validate_animation(&self, command: AnimationCommand) -> Result<()> {
        animation(self, command, None)
    }
}

impl BattleCatalog {
    pub(super) fn validate_party_models(&self) -> Result<()> {
        self.actions.validate_party_models(&self.visuals.party)
    }
}

impl BattleActions {
    /// Validate motion requests without loading or cooking renderer resources.
    pub fn validate_party_models(
        &self,
        models: &std::collections::BTreeMap<u8, PartyVisuals>,
    ) -> Result<()> {
        for owner in &self.party {
            let variants = models
                .get(&owner.character)
                .context("missing normal-action actor model")?;
            for (costume, model) in variants.concrete() {
                (|| -> Result<()> {
                    for normal in &owner.normal {
                        action(model, &normal.action)?;
                        if let Some(clip) = normal.recovery.animation {
                            model.visual.motion_slot(u16::from(clip))?;
                        }
                    }
                    ensure!(
                        model.visual.victory.keys().copied().eq(VictoryStyle::ALL),
                        "incomplete party victory programs"
                    );
                    for victory in model.visual.victory.values() {
                        ensure!(
                            victory.source_sha256.len() == 64
                                && victory.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                                && model.visual.rig.motions.contains_key(&victory.clip)
                                && victory.animations.commands().any(|command| matches!(
                                    command,
                                    AnimationCommand::Play {
                                        clip: VictoryMotion::NATIVE_CLIP,
                                        ..
                                    }
                                )),
                            "missing external victory motion"
                        );
                        for command in victory.animations.commands() {
                            let relocated = matches!(
                                command,
                                AnimationCommand::Play {
                                    clip: VictoryMotion::NATIVE_CLIP,
                                    ..
                                }
                            )
                            .then_some(victory.clip);
                            animation(model, command, relocated)?;
                        }
                    }
                    Ok(())
                })()
                .with_context(|| format!("party actions {}:{costume:?}", owner.character))?;
            }
        }
        for technique in &self.techniques {
            let casters = match &technique.program {
                TechniqueProgram::AcidRain { .. } => continue,
                // Native martial variants do not encode their character owner.
                // Source admission validates their owner/dispatch mapping before cooking.
                TechniqueProgram::Martial { .. } => continue,
                TechniqueProgram::FireBall {
                    casting,
                    cast_commands,
                    release,
                    ..
                } => {
                    if let Some(variants) = models.get(&3) {
                        for (costume, model) in variants.concrete() {
                            animations(model, casting)
                                .and_then(|_| commands(model, cast_commands))
                                .and_then(|_| animation(model, *release, None))
                                .with_context(|| format!("Fire Ball caster 3:{costume:?}"))?;
                        }
                    }
                    continue;
                }
                TechniqueProgram::Lightning { casters, .. }
                | TechniqueProgram::Icicle { casters, .. }
                | TechniqueProgram::StoneBlast { casters, .. }
                | TechniqueProgram::WindBlade { casters, .. }
                | TechniqueProgram::AirThrust { casters, .. }
                | TechniqueProgram::WindField { casters, .. }
                | TechniqueProgram::AquaEdge { casters, .. }
                | TechniqueProgram::Water { casters, .. }
                | TechniqueProgram::Spread { casters, .. }
                | TechniqueProgram::IceTornado { casters, .. }
                | TechniqueProgram::FreezeLancer { casters, .. }
                | TechniqueProgram::ThunderArrow { casters, .. }
                | TechniqueProgram::Orb { casters, .. }
                | TechniqueProgram::Lance { casters, .. }
                | TechniqueProgram::Ray { casters, .. }
                | TechniqueProgram::PrismSword { casters, .. }
                | TechniqueProgram::GroundPulse { casters, .. }
                | TechniqueProgram::Absolute { casters, .. }
                | TechniqueProgram::EarthBite { casters, .. }
                | TechniqueProgram::MeteorStorm { casters, .. }
                | TechniqueProgram::SpiralFlare { casters, .. }
                | TechniqueProgram::EarthField { casters, .. }
                | TechniqueProgram::FireField { casters, .. }
                | TechniqueProgram::GroundSummon { casters, .. }
                | TechniqueProgram::Summon { casters, .. }
                | TechniqueProgram::Nurse { casters, .. }
                | TechniqueProgram::RecoverySpell { casters, .. }
                | TechniqueProgram::StoredRecoverySpell { casters, .. } => casters,
            };
            for caster in casters {
                let Some(variants) = models.get(&caster.character) else {
                    continue;
                };
                for (costume, model) in variants.concrete() {
                    cast(model, caster)
                        .and_then(|_| {
                            if let Some(settings) = technique.program.stored() {
                                let resume = settings
                                    .party_resume
                                    .context("missing stored party concluding motion")?;
                                animation(model, resume, None)?;
                            }
                            Ok(())
                        })
                        .with_context(|| {
                            format!(
                                "technique {} caster {}:{costume:?}",
                                technique.technique, caster.character
                            )
                        })?;
                }
            }
        }
        Ok(())
    }
}

fn cast(model: &PartyModel, recipe: &CastRecipe) -> Result<()> {
    animations(model, &recipe.animations)?;
    commands(model, &recipe.commands)?;
    for command in recipe.release.into_iter().chain(recipe.recovery_pose) {
        animation(model, command, None)?;
    }
    ensure!(
        model.visual.rig.motions.contains_key(&0),
        "missing caster idle recovery"
    );
    Ok(())
}

fn action(model: &PartyModel, action: &Action) -> Result<()> {
    animations(model, &action.animations)?;
    commands(model, &action.commands)?;
    for hit in &action.hits {
        if let HitEmission::Contact {
            attachment: HitAttachment::Groups(groups),
            ..
        } = &hit.emission
        {
            for &group in groups {
                ensure!(
                    model.visual.rig.attack_groups.contains_key(&group)
                        || model
                            .visual
                            .attachments
                            .iter()
                            .any(|(&slot, attachment)| slot == group
                                || attachment
                                    .rig
                                    .attack_groups
                                    .keys()
                                    .any(|offset| slot.checked_add(*offset) == Some(group)))
                        || model
                            .weapon_motions
                            .values()
                            .any(|bank| bank.rigs.iter().any(|(&slot, rig)| slot == group
                                || rig
                                    .attack_groups
                                    .keys()
                                    .any(|offset| slot.checked_add(*offset) == Some(group)))),
                    "missing action attack group {group}"
                );
            }
        }
        let bone = match &hit.emission {
            HitEmission::Contact {
                attachment: HitAttachment::BodyBone(bone),
                ..
            }
            | HitEmission::Effect {
                bone: Some(bone), ..
            } => Some(*bone),
            _ => None,
        };
        if let Some(bone) = bone {
            ensure!(
                usize::from(bone) < model.visual.rig.skeleton.bones.len(),
                "missing action body bone {bone}"
            );
        }
    }
    Ok(())
}

fn animations(model: &PartyModel, program: &AnimationProgram) -> Result<()> {
    for command in program.commands() {
        animation(model, command, None)?;
    }
    Ok(())
}

fn commands(model: &PartyModel, commands: &[TimedCommand]) -> Result<()> {
    for command in commands {
        if let ActionCommand::TextureLayers(frames) = command.command {
            textures(model, frames)?;
        }
    }
    Ok(())
}

fn textures(model: &PartyModel, frames: impl IntoIterator<Item = u8>) -> Result<()> {
    ensure!(
        model
            .visual
            .texture_layers
            .iter()
            .zip(frames)
            .all(|(layer, frame)| frame < layer.frames),
        "missing party action texture frame"
    );
    Ok(())
}

fn animation(model: &PartyModel, command: AnimationCommand, relocated: Option<u16>) -> Result<()> {
    match command {
        AnimationCommand::Play {
            clip,
            blend,
            start,
            end,
            layer,
            mirror,
            resource,
            rate,
            ..
        } => {
            ensure!(
                matches!(layer, 6 | 8)
                    && !mirror
                    && resource == -1
                    && rate.is_finite()
                    && rate != 0.,
                "unsupported party motion binding"
            );
            let MotionSlot::Present(motion) = model
                .visual
                .motion_slot(relocated.unwrap_or(u16::from(clip)))?
            else {
                return Ok(());
            };
            interval(start, end, rate.abs(), motion.duration_frames)?;
            for bank in model.weapon_motions.values() {
                for rig in bank.rigs.values() {
                    bank.link.playback(rig, clip, start, end, blend)?;
                }
            }
        }
        AnimationCommand::Texture { layers } => textures(model, layers)?,
        // Loop targets are checked by the authored program; they do not bind a rig.
        AnimationCommand::Loop { .. } => {}
        AnimationCommand::Rate { rate } => ensure!(rate.is_finite(), "invalid party motion rate"),
    }
    Ok(())
}

fn interval(start: u8, end: Option<u8>, scale: f32, duration: f32) -> Result<()> {
    let start = f32::from(start) * scale;
    let end = end.map_or(duration, |end| f32::from(end) * scale);
    ensure!(
        start <= end && end <= duration,
        "party motion playback interval exceeds selected clip"
    );
    Ok(())
}
