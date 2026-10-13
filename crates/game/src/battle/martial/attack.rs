//! Each technique owns its complete native attack and optional feedback.
use super::{ActionDefinition, Sequence, Sound, Technique};
use anyhow::{Result, bail};
use resonance_battle::{AttackEvent as Event, AttackPose, EffectAppearance, PreparedAttack};

impl Technique {
    pub(crate) fn prepare<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
    ) -> Result<ActionDefinition> {
        use Sequence::*;
        let source = crate::battle::model::ModelSource::Party(self.character as u8);
        let pose = |clip, blend| {
            crate::battle::model::motion(model, clip).map(|motion| AttackPose {
                end: None,
                repeat: false,
                motion,
                start: 0.,
                rate: 0.5,
                blend,
            })
        };
        let tp_cost = u16::from(
            resources
                .catalogue
                .definition(usize::from(self.catalogue))?
                .tp_cost,
        );
        match self.sequence {
            Authored | RayThrust | DemonFang => {
                return self.prepare_authored(resources, model, tp_cost);
            }
            Destruction => return self.prepare_destruction(resources, model, tp_cost),
            ItemThief => return self.prepare_steal(resources, model, tp_cost),
            PowHammer => return self.prepare_hammer(resources, model, tp_cost),
            HammerVolley => return self.prepare_hammer_volley(resources, model, tp_cost),
            ColetteProjectiles => {
                return self.prepare_colette_projectiles(resources, model, tp_cost);
            }
            TigerBlade => return self.prepare_tiger_blade(resources, model, tp_cost),
            FierceDemonFang => return self.prepare_fierce_fang(resources, model, tp_cost),
            HurricaneThrust => return self.prepare_hurricane_thrust(resources, model, tp_cost),
            Tempest => return self.prepare_tempest(resources, model, tp_cost),
            LightningBlade => return self.prepare_lightning_blade(resources, model, tp_cost),
            RisingFalcon => return self.prepare_rising_falcon(resources, model, tp_cost),
            FalconsCrest => return self.prepare_falcons_crest(resources, model, tp_cost),
            Seal => return self.prepare_seal(resources, model, tp_cost),
            CycloneSeal => return self.prepare_cyclone_seal(resources, model, tp_cost),
            ForceSeal => return self.prepare_force_seal(resources, model, tp_cost),
            EagleDive => return self.prepare_eagle(resources, model, tp_cost),
            SpecialGuard => {}
            Spell(_) => bail!("technique does not use a direct attack"),
        }
        let guard = super::special_guard(self.character as u8).unwrap();
        resources.effect(true, &[guard.effect])?;
        const STANCE_TICKS: u16 = 60;
        let mut events = vec![
            (0, Event::SpecialGuard),
            (
                0,
                Event::Effect {
                    appearance: EffectAppearance {
                        resource: resources.techniques.resource,
                        member: guard.effect,
                    },
                    tints: Default::default(),
                    centered: true,
                },
            ),
            (0, Event::Notice { duration: 90 }),
            (0, Event::Sound((resources.sound)(Sound::Cue(69))?)),
        ];
        if let Some(line) =
            resources
                .voices
                .absolute(source, Some(guard.voice), &mut resources.sound)?
        {
            events.push((
                0,
                Event::Voice {
                    sound: line,
                    priority: resonance_battle::VoicePriority::Action,
                },
            ));
        }
        events.extend(pose(0, 4).map(|pose| (STANCE_TICKS, Event::Pose(pose))));
        Ok(ActionDefinition {
            normal: None,
            tp_cost,
            execution: crate::battle::ActionExecution::Attack(PreparedAttack {
                contact_recovery: None,
                opening: pose(22, 4),
                events,
                chain_at: None,
                end_at: STANCE_TICKS,
                recovery: 12,
            }),
        })
    }
}
