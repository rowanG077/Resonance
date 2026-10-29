//! Martial projectiles retain the motion and impact feedback of each authored hit.
use super::{PreparedAttack, Sound, Technique};
use crate::battle::encounter::resources::Resources;
use anyhow::{Context, Result, bail};
use resonance_battle::{
    AttackEvent, AttackPose, EffectAppearance, HitShape, ModelDefinition,
    ProjectileContactFeedback, ProjectileDefinition,
};
use resonance_content::{battle_action::ActionHit, battle_martial::ProjectileFeedback};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_lightning_blade(
        self,
        resources: &mut Resources<'_>,
        model: Option<&ModelDefinition>,
    ) -> Result<PreparedAttack> {
        let timeline = self.authored_timeline(resources.voices.party_table())?;
        let hit = timeline
            .callback_hits
            .get(&1)
            .context("missing Lightning Blade follow-up hit")?;
        let follow_up = resources.lightning_blade_follow_up(hit, self.catalogue == 151)?;
        let mut action = self.prepare_authored_attack(resources, model, timeline)?;
        action.events.push(follow_up);
        action.events.sort_by_key(|(at, _)| *at);
        Ok(action)
    }

    pub(super) fn prepare_rising_falcon(
        self,
        resources: &mut Resources<'_>,
        model: Option<&ModelDefinition>,
        tp_cost: u16,
    ) -> Result<PreparedAttack> {
        let elemental = resonance_content::arte::elemental::find(self.catalogue);
        let base = elemental.map_or(self.catalogue, |variant| variant.base);
        let member = resources
            .voices
            .party_table()
            .martial_attacks
            .bindings
            .get(&base)
            .and_then(|characters| characters.get(&(self.character as u8)))
            .context("missing Rising Falcon binding")?
            .member;
        let mut prepare = |variant| -> Result<PreparedAttack> {
            let mut timeline = resources
                .voices
                .party_table()
                .martial_attacks
                .variant(resonance_content::battle_martial::Binding { member, variant })?
                .clone();
            if let Some(elemental) = elemental {
                for (_, event) in &mut timeline.events {
                    if let resonance_content::battle_normal::Event::Contact(contact) = event {
                        contact.hit.element = resonance_content::battle_action::HitElement::Element(
                            elemental.element,
                        );
                    }
                }
            }
            let mut action = self.prepare_authored_attack(resources, model, timeline)?;
            let recovery_pose =
                super::recovery_pose(&action).context("missing Rising Falcon recovery pose")?;
            let dive_at = action
                .events
                .iter()
                .find_map(|(at, event)| {
                    matches!(event, AttackEvent::PassThrough { enabled: true, .. }).then_some(*at)
                })
                .context("missing Rising Falcon dive")?;
            let contact_at = action
                .events
                .iter()
                .filter_map(|(at, event)| {
                    matches!(event, AttackEvent::Contact { .. }).then_some(*at)
                })
                .max()
                .context("missing Rising Falcon dive contact")?;
            let landing_at = dive_at.max(contact_at) + 1;
            // Landing changes the pose; the strike and collision bypass keep their own lifetimes.
            action.events.retain(|(at, event)| {
                *at < landing_at || matches!(event, AttackEvent::PassThrough { enabled: false, .. })
            });
            for (at, event) in &mut action.events {
                if matches!(event, AttackEvent::PassThrough { enabled: false, .. }) {
                    // The closing command's tick is the last bypassed update.
                    *at = at
                        .checked_add(1)
                        .context("Rising Falcon bypass exceeds timeline")?;
                }
            }
            action.events.push((
                landing_at,
                AttackEvent::Land {
                    pose: None,
                    effect: None,
                    settle: None,
                    recovery_pose: None,
                },
            ));
            action.events.push((
                landing_at,
                AttackEvent::Sound((resources.sound)(Sound::Cue(138))?),
            ));
            action.events.push((
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
            action.events.sort_by_key(|(at, _)| *at);
            Ok(action)
        };
        let variants = if elemental.is_some() {
            [prepare(2)?, prepare(3)?]
        } else {
            [prepare(0)?, prepare(1)?]
        };
        let mut entry = variants[0].clone();
        entry.events = vec![(
            0,
            AttackEvent::ChooseAirborne(
                variants.map(|attack| Arc::new(super::attack_definition(attack, tp_cost))),
            ),
        )];
        Ok(entry)
    }

    pub(super) fn prepare_falcons_crest(
        self,
        resources: &mut Resources<'_>,
        model: Option<&ModelDefinition>,
    ) -> Result<PreparedAttack> {
        use resonance_content::battle_normal::Event;
        let mut timeline = self.authored_timeline(resources.voices.party_table())?;
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
            .get(&(self.character, self.catalogue))
            .context("missing Falcon's Crest sword")?
        {
            model.weapons.retain(|old| old.slot != weapon.slot);
            model.weapons.push(Arc::clone(weapon));
        }
        let mut action = self.prepare_authored_attack(resources, Some(&model), timeline)?;
        // The final idle pose has already begun before recovery.
        action
            .events
            .retain(|(at, event)| *at != action.end_at || !matches!(event, AttackEvent::Pose(_)));
        let scene = resonance_content::battle_scene::document::load(
            resources.files,
            resonance_content::battle_scene::document::Kind::Skill,
            self.catalogue,
        )?;
        let bank = resources
            .skill_scenes
            .get(&self.catalogue)
            .context("missing Falcon's Crest artwork")?
            .clone();
        let recoil = resources.recoil;
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
                crate::battle::hit::authored(hit, recoil)?,
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
            action
                .events
                .push((at, AttackEvent::Projectile(projectile)));
        }
        crate::battle::skill_scene::falcon(resources, &mut action, &model)?;
        action.events.sort_by_key(|(at, _)| *at);
        Ok(action)
    }
}

impl Resources<'_> {
    pub(crate) fn lightning_blade_follow_up(
        &mut self,
        hit: &ActionHit,
        with_impact: bool,
    ) -> Result<(u16, AttackEvent)> {
        let recoil = self.recoil;
        const LIGHTNING_EFFECT: u16 = 75;
        const IMPACT_EFFECT: u16 = 107;
        const RELEASE_AT: u16 = 34;
        self.effect(true, &[LIGHTNING_EFFECT])?;
        if with_impact {
            self.effect(true, &[IMPACT_EFFECT])?;
        }
        let projectile = Arc::new(ProjectileDefinition {
            clamp_ground: true,
            birth: Some(EffectAppearance {
                resource: self.techniques.resource,
                member: LIGHTNING_EFFECT,
            }),
            ..super::stationary_contact(hit, recoil, 20, HitShape::Cylinder, [40., 400.])?
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

pub(super) fn projectile(
    resources: &mut Resources<'_>,
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
