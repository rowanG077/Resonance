//! Each technique owns its complete native attack and optional feedback.
use super::{ActionDefinition, PreparedAttack, Sequence, Sound, Technique};
use anyhow::{Result, bail};
use resonance_battle::{AttackEvent as Event, AttackOrigin, EffectAppearance};

impl Technique {
    pub(crate) fn prepare<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
    ) -> Result<ActionDefinition> {
        let tp_cost = u16::from(
            resources
                .catalogue
                .definition(usize::from(self.catalogue))?
                .tp_cost,
        );
        Ok(super::attack_definition(
            self.prepare_attack(resources, model, tp_cost)?,
            tp_cost,
        ))
    }

    fn prepare_attack<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
        tp_cost: u16,
    ) -> Result<PreparedAttack> {
        use Sequence::*;
        let source = crate::battle::model::ModelSource::Party(self.character);
        match self.sequence {
            Elemental => return self.prepare_elemental(resources, model, tp_cost),
            Authored | RayThrust => {
                return self.prepare_authored(resources, model);
            }
            Destruction => return self.prepare_destruction(resources, model),
            ItemThief => return self.prepare_steal(resources, model, tp_cost),
            PowHammer => return self.prepare_hammer(resources, model, tp_cost, false),
            HammerVolley => return self.prepare_hammer_volley(resources, model),
            ColetteProjectiles => {
                return self.prepare_colette_projectiles(resources, model);
            }
            TigerBlade => return self.prepare_tiger_blade(resources, model),
            FierceDemonFang => return self.prepare_fierce_fang(resources, model, tp_cost, false),
            HurricaneThrust => return self.prepare_hurricane_thrust(resources, model),
            Tempest => return self.prepare_tempest(resources, model),
            LightningBlade => return self.prepare_lightning_blade(resources, model),
            RisingFalcon => return self.prepare_rising_falcon(resources, model, tp_cost),
            FalconsCrest => return self.prepare_falcons_crest(resources, model),
            Seal => return self.prepare_seal(resources, model),
            CycloneSeal => return self.prepare_cyclone_seal(resources, model),
            ForceSeal => return self.prepare_force_seal(resources, model),
            EagleDive => return self.prepare_eagle(resources, model),
            SpecialGuard => {}
            Spell(_) => bail!("technique does not use a direct attack"),
        }
        let guard = super::special_guard(self.character);
        resources.effect(true, &[guard.effect])?;
        let authored = resources
            .voices
            .party_table()
            .martial_attacks
            .variant(resonance_content::battle_martial::Binding {
                member: 34,
                variant: 0,
            })?
            .clone();
        let mut definition = self.prepare_authored_attack(resources, model, authored)?;
        if let Some(idle) = model
            .and_then(|model| model.idle_motions[0])
            .and_then(|clip| crate::battle::model::motion(model, clip))
            && let Some((_, Event::Pose(pose))) = definition.events.last_mut()
        {
            pose.motion = idle;
        }
        definition.events.extend([
            (0, Event::SpecialGuard),
            (
                0,
                Event::InitializedEffect {
                    appearance: EffectAppearance {
                        resource: resources.techniques.resource,
                        member: guard.effect,
                    },
                    origin: AttackOrigin::ActorAim,
                },
            ),
            (0, Event::Sound((resources.sound)(Sound::Cue(69))?)),
        ]);
        if let Some(line) =
            resources
                .voices
                .absolute(source, Some(guard.voice), &mut resources.sound)?
        {
            definition.events.push((
                0,
                Event::Voice {
                    sound: line,
                    priority: resonance_battle::VoicePriority::Action,
                },
            ));
        }
        definition.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }
}
