//! Martial projectiles retain the motion and impact feedback of each authored hit.
use super::{ActionDefinition, Sound, Technique};
use anyhow::{Context, Result, bail};
use resonance_battle::{
    ActionExecution, AttackEvent, AttackPose, EffectAppearance, HitShape, ModelDefinition,
    ProjectileContact, ProjectileContactFeedback, ProjectileDefinition,
};
use resonance_content::{battle_action::ActionHit, battle_martial::ProjectileFeedback};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_lightning_blade<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let timeline = resources
            .voices
            .party_table()
            .martial_attack(self.catalogue, self.character as u8)?
            .clone();
        let hit = timeline
            .callback_hits
            .get(&1)
            .context("missing Lightning Blade follow-up hit")?;
        let follow_up = resources.lightning_blade_follow_up(hit, self.catalogue == 151)?;
        let mut action = self.prepare_authored_attack(resources, model, tp_cost, timeline)?;
        let ActionExecution::Attack(attack) = &mut action.execution else {
            unreachable!("martial preparation produces an attack")
        };
        attack.events.push(follow_up);
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(action)
    }

    pub(super) fn prepare_rising_falcon<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let member = resources
            .voices
            .party_table()
            .martial_attacks
            .bindings
            .get(&self.catalogue)
            .and_then(|characters| characters.get(&(self.character as u8)))
            .context("missing Rising Falcon binding")?
            .member;
        let mut prepare = |variant| -> Result<Arc<ActionDefinition>> {
            let timeline = resources
                .voices
                .party_table()
                .martial_attacks
                .variant(resonance_content::battle_martial::Binding { member, variant })?
                .clone();
            let mut action = self.prepare_authored_attack(resources, model, tp_cost, timeline)?;
            let ActionExecution::Attack(attack) = &mut action.execution else {
                unreachable!("martial preparation produces an attack")
            };
            let recovery_pose = attack
                .events
                .iter()
                .find_map(|(at, event)| match event {
                    AttackEvent::Pose(pose) if *at == attack.end_at => Some(*pose),
                    _ => None,
                })
                .context("missing Rising Falcon recovery pose")?;
            let dive_at = attack
                .events
                .iter()
                .find_map(|(at, event)| {
                    matches!(event, AttackEvent::PassThrough { enabled: true, .. }).then_some(*at)
                })
                .context("missing Rising Falcon dive")?;
            let contact_at = attack
                .events
                .iter()
                .filter_map(|(at, event)| {
                    matches!(event, AttackEvent::Contact { .. }).then_some(*at)
                })
                .max()
                .context("missing Rising Falcon dive contact")?;
            let landing_at = dive_at.max(contact_at) + 1;
            // Landing ends contact, while collision bypass keeps its authored lifetime.
            attack.events.retain(|(at, event)| {
                *at < landing_at || matches!(event, AttackEvent::PassThrough { enabled: false, .. })
            });
            for (at, event) in &mut attack.events {
                if matches!(event, AttackEvent::PassThrough { enabled: false, .. }) {
                    // The closing command's tick is the last bypassed update.
                    *at = at
                        .checked_add(1)
                        .context("Rising Falcon bypass exceeds timeline")?;
                }
            }
            attack.events.push((
                landing_at,
                AttackEvent::Land {
                    pose: Some(AttackPose {
                        motion: crate::battle::model::motion(model, 64)
                            .context("missing Rising Falcon landing pose")?,
                        start: 0.,
                        end: None,
                        rate: 0.5,
                        blend: 2,
                        repeat: false,
                    }),
                    effect: None,
                    settle: Some(30),
                    recovery_pose: Some(recovery_pose),
                },
            ));
            attack.events.sort_by_key(|(at, _)| *at);
            Ok(Arc::new(action))
        };
        let variants = [prepare(0)?, prepare(1)?];
        let mut entry = (*variants[0]).clone();
        let ActionExecution::Attack(attack) = &mut entry.execution else {
            unreachable!("martial preparation produces an attack")
        };
        attack.events = vec![(0, AttackEvent::ChooseAirborne(variants))];
        Ok(entry)
    }

    pub(super) fn prepare_falcons_crest<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        use resonance_content::battle_normal::Event;
        let mut timeline = resources
            .voices
            .party_table()
            .martial_attack(self.catalogue, self.character as u8)?
            .clone();
        let hits = std::mem::take(&mut timeline.callback_hits);
        let poses = timeline
            .pose_sequence
            .take()
            .context("missing Falcon's Crest poses")?;
        // This scene's elapsed schedule includes its cut-in hold. The stored pose
        // and hit data remain shared with ordinary attacks; no script clock runs here.
        for (at, event) in &mut timeline.events {
            *at = match event {
                Event::Contact(_) => 232,
                Event::AnimationRate(_) => match *at {
                    92 => 127,
                    155 => 210,
                    _ => bail!("unexpected Falcon's Crest rate change"),
                },
                _ => match *at {
                    0 => 15,
                    10 => 25,
                    75 => 90,
                    90 => 105,
                    98 => 116,
                    153 => 205,
                    159 => 211,
                    224 => 276,
                    _ => bail!("unexpected Falcon's Crest command time"),
                },
            };
        }
        for step in poses {
            let at = match step.pose.clip {
                80 => 0,
                69 => 105,
                70 => 123,
                28 => 277,
                16 => 310,
                0 => 331,
                _ => bail!("unexpected Falcon's Crest pose"),
            };
            timeline.events.push((at, Event::Pose(step.pose)));
        }
        timeline.duration = 365;
        timeline.chain_at = None;
        let mut model = model.context("missing Falcon's Crest model")?.clone();
        for weapon in resources
            .skill_weapons
            .get(&(self.character as u8, self.catalogue))
            .context("missing Falcon's Crest sword")?
        {
            model.weapons.retain(|old| old.slot != weapon.slot);
            model.weapons.push(Arc::clone(weapon));
        }
        let mut action =
            self.prepare_authored_attack(resources, Some(&model), tp_cost, timeline)?;
        let ActionExecution::Attack(attack) = &mut action.execution else {
            unreachable!("martial preparation produces an attack")
        };
        // The final idle pose has already begun before recovery.
        attack
            .events
            .retain(|(at, event)| *at != attack.end_at || !matches!(event, AttackEvent::Pose(_)));
        let scene: resonance_content::battle_scene::Scene = resources
            .files
            .json(&resonance_content::battle_scene::skill_path(self.catalogue))?;
        let bank = resources
            .skill_scenes
            .get(&self.catalogue)
            .context("missing Falcon's Crest artwork")?
            .clone();
        let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
        for (at, member, rule) in [(30, 1, 0), (105, 2, 1), (237, 3, 2), (284, 4, 3)] {
            let row = scene
                .projectiles
                .get(member)
                .context("missing Falcon's Crest projectile")?;
            let hit = hits
                .get(&rule)
                .context("missing Falcon's Crest projectile hit")?;
            let mut projectile = resources.prepared.projectile(
                resources.files,
                &row.projectile,
                crate::battle::hit::authored(hit, &recoil)?,
                &[
                    (0, &resources.common),
                    (1, &resources.techniques),
                    (2, &bank),
                ],
            )?;
            Arc::make_mut(&mut projectile)
                .contact
                .as_mut()
                .context("Falcon's Crest projectile has no contact")?
                .cooldown = hit.cooldown;
            attack
                .events
                .push((at, AttackEvent::Projectile(projectile)));
        }
        crate::battle::skill_scene::falcon(resources, attack, &model)?;
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(action)
    }
}

impl<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>
    crate::battle::encounter::resources::Resources<'_, S>
{
    pub(crate) fn lightning_blade_follow_up(
        &mut self,
        hit: &ActionHit,
        with_impact: bool,
    ) -> Result<(u16, AttackEvent)> {
        let recoil = crate::battle::recoil::Parameters::load(self.files)?;
        const LIGHTNING_EFFECT: u16 = 75;
        const IMPACT_EFFECT: u16 = 107;
        const RELEASE_AT: u16 = 34;
        self.effect(true, &[LIGHTNING_EFFECT])?;
        if with_impact {
            self.effect(true, &[IMPACT_EFFECT])?;
        }
        let projectile = Arc::new(ProjectileDefinition {
            lifetime: Some(20),
            velocity: [0.; 3],
            acceleration: [0.; 3],
            offset: [0.; 3],
            clamp_ground: true,
            active: None,
            birth: Some(EffectAppearance {
                resource: self.techniques.resource,
                member: LIGHTNING_EFFECT,
            }),
            motion: Default::default(),
            effects: Default::default(),
            contact: Some(ProjectileContact {
                hit: crate::battle::hit::authored(hit, &recoil)?,
                cooldown: hit.cooldown,
                repeat_limit: 0,
                radius: 40.,
                height: 400.,
                shape: HitShape::Cylinder,
                offset: [0.; 3],
                radius_growth: 0.,
                height_growth: 0.,
                survives_contact: true,
                clashes: false,
            }),
        });
        Ok((
            RELEASE_AT,
            AttackEvent::ContactBurst {
                definition: projectile,
                appearance: with_impact.then_some(EffectAppearance {
                    resource: self.techniques.resource,
                    member: IMPACT_EFFECT,
                }),
            },
        ))
    }
}

pub(super) fn projectile<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>(
    resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
    row: &resonance_content::battle_projectile::Projectile,
    hit: &ActionHit,
    feedback: Option<ProjectileFeedback>,
    recoil: &crate::battle::recoil::Parameters,
) -> Result<Arc<ProjectileDefinition>> {
    let mut rule = crate::battle::hit::authored(hit, recoil)?;
    if let Some(member) = hit.effect {
        resources.effect(true, &[member])?;
        rule.effect = Some(EffectAppearance {
            resource: resources.techniques.resource,
            member,
        });
    }
    let mut projectile = resources.prepared.projectile(
        resources.files,
        row,
        rule,
        &[(0, &resources.common), (1, &resources.techniques)],
    )?;
    let projectile_mut = Arc::make_mut(&mut projectile);
    projectile_mut
        .contact
        .as_mut()
        .context("martial projectile has no contact")?
        .cooldown = hit.cooldown;
    if let Some(feedback) = feedback {
        resources.effect(true, &[feedback.effect])?;
        projectile_mut.effects.contact = Some(ProjectileContactFeedback {
            appearance: EffectAppearance {
                resource: resources.techniques.resource,
                member: feedback.effect,
            },
            on_guard: feedback.on_guard,
            sound: if feedback.sound == 0 {
                None
            } else {
                (resources.sound)(Sound::Cue(feedback.sound))?
            },
        });
    }
    Ok(projectile)
}
