//! Summons retain their arrival point after the cinematic restores the fighters.
use crate::{
    ActionId, ActorId, Battle, Cue, EffectAppearance, EffectRequest, ProjectileDefinition, Sound,
    SummonEntryDefinition,
};
use anyhow::{Result, ensure};
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub enum SummonOrigin {
    World,
    TargetGround {
        height: f32,
        nudge: f32,
        direction_threshold: f32,
    },
    TargetDirection {
        height: f32,
        distance: f32,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum SummonPoint {
    World,
    /// Offset in the arrival frame, rotated by its captured heading.
    Arrival([f32; 3]),
}

#[derive(Debug, Clone, Copy)]
pub enum SummonHeading {
    Captured(f32),
    Caster,
}

#[derive(Debug, Clone)]
pub struct SummonEffect {
    pub at: u16,
    pub appearance: EffectAppearance,
    pub point: SummonPoint,
    pub heading: SummonHeading,
}

#[derive(Debug, Clone)]
pub struct SummonContact {
    pub at: u16,
    pub projectile: Arc<ProjectileDefinition>,
    pub point: SummonPoint,
    pub heading: SummonHeading,
}

#[derive(Debug, Clone, Copy)]
pub enum SummonBlessing {
    Attack(u8),
    Defense(u8),
    Accuracy(u8),
    Magic(u8),
    AttackDefense(u8),
    Speed,
    PhysicalProtection,
    MagicalProtection,
    Heal {
        percent: u8,
        effect: EffectAppearance,
    },
}

#[derive(Debug, Clone)]
pub struct PreparedSummon {
    pub entry: SummonEntryDefinition,
    pub origin: SummonOrigin,
    /// All schedule times include the initialization update at zero.
    pub effects: Vec<SummonEffect>,
    pub contacts: Vec<SummonContact>,
    pub voices: Vec<(u16, Sound)>,
    pub sounds: Vec<(u16, Sound)>,
    pub scale: f32,
    pub title: String,
    pub focus_distance: f32,
    pub focus_ticks: u16,
    pub focus_world: bool,
    pub blessing: SummonBlessing,
    pub blessing_radius: f32,
}

impl PreparedSummon {
    pub(crate) fn validate(&self, duration: u16) -> Result<()> {
        ensure!(
            duration > 81 && !self.contacts.is_empty(),
            "empty summon schedule"
        );
        ensure!(
            self.scale.is_finite()
                && self.scale > 0.
                && self.blessing_radius.is_finite()
                && self.blessing_radius > 0.
                && self.focus_distance.is_finite()
                && self.focus_distance > 0.
                && self.focus_ticks > 0
                && self.focus_ticks < duration,
            "invalid summon presentation"
        );
        ensure!(
            match self.origin {
                SummonOrigin::World => true,
                SummonOrigin::TargetGround {
                    height,
                    nudge,
                    direction_threshold,
                } =>
                    height.is_finite()
                        && nudge.is_finite()
                        && nudge >= 0.
                        && direction_threshold.is_finite()
                        && direction_threshold > 0.,
                SummonOrigin::TargetDirection { height, distance } =>
                    height.is_finite() && distance.is_finite(),
            },
            "invalid summon arrival"
        );
        for (at, point, heading) in self
            .effects
            .iter()
            .map(|e| (e.at, e.point, e.heading))
            .chain(self.contacts.iter().map(|e| (e.at, e.point, e.heading)))
        {
            ensure!(
                at < duration
                    && match point {
                        SummonPoint::World => true,
                        SummonPoint::Arrival(offset) => offset.into_iter().all(f32::is_finite),
                    }
                    && match heading {
                        SummonHeading::Caster => true,
                        SummonHeading::Captured(offset) => offset.is_finite(),
                    },
                "invalid summon emission"
            );
        }
        ensure!(
            self.voices
                .iter()
                .chain(&self.sounds)
                .all(|(at, _)| *at < duration),
            "invalid summon sound schedule"
        );
        for contact in &self.contacts {
            contact.projectile.validate()?;
        }
        Ok(())
    }
}

pub(crate) struct ActiveSummon {
    arrival: [f32; 3],
    heading: f32,
    blessed: crate::ActorSet,
}

impl ActiveSummon {
    pub(crate) fn begin(
        definition: &PreparedSummon,
        battle: &mut Battle,
        owner: ActorId,
        target: &mut ActorId,
    ) -> Self {
        let recipient = &battle.actors[target.index()];
        if !recipient.available()
            && let Some(index) = battle
                .actors
                .iter()
                .position(|a| a.side == recipient.side && a.available())
        {
            *target = ActorId(index as u8);
        }
        let actor = &battle.actors[owner.index()];
        let recipient = &battle.actors[target.index()];
        let mut arrival = recipient.position;
        match definition.origin {
            SummonOrigin::World => arrival = [0.; 3],
            SummonOrigin::TargetGround {
                height,
                nudge,
                direction_threshold,
            } => {
                arrival[1] = height;
                let dx = actor.position[0] - arrival[0];
                let dz = actor.position[2] - arrival[2];
                let distance = dx.hypot(dz);
                if distance >= direction_threshold {
                    arrival[0] += dx / distance * nudge;
                    arrival[2] += dz / distance * nudge;
                }
            }
            SummonOrigin::TargetDirection { height, distance } => {
                arrival[1] = height;
                for (value, direction) in arrival.iter_mut().zip(actor.facing_direction) {
                    *value += direction * distance;
                }
            }
        }
        let heading = actor.heading;
        if let Some(camera) = &mut battle.camera {
            camera.request_summon_focus(
                if definition.focus_world {
                    [0.; 3]
                } else {
                    arrival
                },
                definition.focus_distance,
                definition.focus_ticks,
            );
        }
        Self {
            arrival,
            heading,
            blessed: Default::default(),
        }
    }

    fn point(&self, point: SummonPoint) -> [f32; 3] {
        match point {
            SummonPoint::World => [0.; 3],
            SummonPoint::Arrival([x, y, z]) => {
                let (sin, cos) = self.heading.to_radians().sin_cos();
                [
                    self.arrival[0] + cos * x + sin * z,
                    self.arrival[1] + y,
                    self.arrival[2] - sin * x + cos * z,
                ]
            }
        }
    }

    fn heading(&self, heading: SummonHeading, caster: f32) -> f32 {
        match heading {
            SummonHeading::Captured(offset) => self.heading + offset,
            SummonHeading::Caster => caster,
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Summon scheduling uses the current action and actor identities without a separate context"
    )]
    pub(crate) fn step(
        &mut self,
        definition: &PreparedSummon,
        battle: &mut Battle,
        id: ActionId,
        owner: ActorId,
        target: ActorId,
        age: u32,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let caster_heading = battle.actors[owner.index()].heading;
        for effect in definition.effects.iter().filter(|e| u32::from(e.at) == age) {
            cues.push(Cue::DeferredEffect(EffectRequest {
                owner,
                target,
                appearance: effect.appearance,
                origin: self.point(effect.point),
                heading: self.heading(effect.heading, caster_heading),
                follow: None,
                scale: definition.scale,
                tint: Default::default(),
            }));
        }
        for contact in definition
            .contacts
            .iter()
            .filter(|e| u32::from(e.at) == age)
        {
            battle.emit_at(
                Arc::clone(&contact.projectile),
                id,
                owner,
                target,
                self.point(contact.point),
                self.heading(contact.heading, caster_heading),
            )?;
        }
        for &(_, sound) in definition
            .voices
            .iter()
            .filter(|(at, _)| u32::from(*at) == age)
        {
            battle.stop_voice(owner, cues);
            cues.push(Cue::Voice {
                actor: owner,
                sound,
                priority: crate::VoicePriority::Announcement,
                position: battle.actors[owner.index()].audio_position(),
                centered: true,
            });
        }
        for &(_, sound) in definition
            .sounds
            .iter()
            .filter(|(at, _)| u32::from(*at) == age)
        {
            cues.push(Cue::Sound {
                actor: owner,
                sound,
                position: battle.actors[owner.index()].position,
                priority: 2,
            });
        }
        // Active callback age zero follows the initialization update.
        if (31..=81).contains(&age) {
            self.bless(definition, battle, owner, cues)?;
        }
        Ok(())
    }

    fn bless(
        &mut self,
        definition: &PreparedSummon,
        battle: &mut Battle,
        owner: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        use crate::conditions::{Buff, Condition};
        let side = battle.actors[owner.index()].side;
        for index in 0..battle.actors.len() {
            let actor = &battle.actors[index];
            let target = ActorId(index as u8);
            if actor.side != side
                || !actor.available()
                || actor.hp <= 0
                || self.blessed.contains(target)
                || actor.position[0].hypot(actor.position[2]) > definition.blessing_radius
            {
                continue;
            }
            self.blessed.insert(target);
            if let SummonBlessing::Heal { percent, effect } = definition.blessing {
                cues.push(Cue::DeferredEffect(EffectRequest {
                    owner: target,
                    target,
                    appearance: effect,
                    origin: actor.effect_origin(),
                    heading: actor.heading,
                    follow: Some(crate::EffectFollow::Center(target)),
                    scale: actor.effect_scale,
                    tint: Default::default(),
                }));
                battle.recover(target, i16::from(percent), cues)?;
                continue;
            }
            let actor = &mut battle.actors[index];
            let label = match definition.blessing {
                SummonBlessing::Attack(amount) => {
                    actor.conditions.bless_stat(Condition::AttackUp, amount);
                    "ATTACK"
                }
                SummonBlessing::Defense(amount) => {
                    actor.conditions.bless_stat(Condition::DefenseUp, amount);
                    "DEFENSE"
                }
                SummonBlessing::Accuracy(amount) => {
                    actor.conditions.bless_stat(Condition::AccuracyUp, amount);
                    "ACC"
                }
                SummonBlessing::Magic(amount) => {
                    actor
                        .conditions
                        .bless_stat(Condition::MagicAttackUp, amount);
                    "MAGIC"
                }
                SummonBlessing::AttackDefense(amount) => {
                    actor.conditions.bless_stat(Condition::AttackUp, amount);
                    actor.conditions.bless_stat(Condition::DefenseUp, amount);
                    "STATUS"
                }
                SummonBlessing::Speed => {
                    actor.conditions.bless_speed();
                    "SPEED"
                }
                SummonBlessing::PhysicalProtection | SummonBlessing::MagicalProtection => {
                    let buff = if matches!(definition.blessing, SummonBlessing::PhysicalProtection)
                    {
                        Buff::PhysicalBarrier { persistent: true }
                    } else {
                        Buff::MagicalBarrier { persistent: true }
                    };
                    actor.conditions.prepare_buff(buff, false).commit(actor);
                    "STATUS"
                }
                SummonBlessing::Heal { .. } => unreachable!(),
            };
            cues.push(Cue::ActorFlash {
                actor: target,
                color: [112, 40, 40],
            });
            cues.push(Cue::CustomLabel {
                actor: target,
                position: actor.condition_label_origin(),
                left: label.into(),
                right: if matches!(
                    definition.blessing,
                    SummonBlessing::PhysicalProtection | SummonBlessing::MagicalProtection
                ) {
                    "SET!"
                } else {
                    "UP"
                }
                .into(),
                duration: 45,
            });
        }
        Ok(())
    }
}
