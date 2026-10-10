//! Contact artwork/audio, camera impacts and controller pulses owned by the scene.
use resonance_battle::{
    Actor, BattleFrame, Control, Cue, EffectRequest, GuardResult, Random, Side, VoicePriority,
};
use resonance_game::battle::feedback::Feedback as Resources;

#[derive(Default)]
pub(super) struct Feedback {
    shake: Option<Shake>,
    motors: [u8; 4],
    appearance: super::appearance::Appearance,
    pub(super) resources: Resources,
    random: Random,
}

struct Shake {
    age: u8,
    duration: u8,
    strength: f32,
}

impl Feedback {
    pub fn new(resources: Resources, seed: u64) -> Self {
        Self {
            resources,
            random: Random::new(seed),
            ..Default::default()
        }
    }

    /// Resolve semantic events before models, effects and audio consume the frame.
    pub fn resolve(&mut self, frame: &mut BattleFrame) {
        if frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::CombatRetired))
        {
            return;
        }
        let mut cues = Vec::new();
        if let Some((actor, sound)) = self.resources.entry_voice.take() {
            cues.push(Cue::Voice {
                actor,
                sound,
                priority: VoicePriority::Reaction,
                position: frame.actors[actor.index()].audio_position(),
                centered: true,
            });
        }
        for request in &frame.model_requests {
            let resonance_battle::ModelRequest::Common { actor, pose } = *request else {
                continue;
            };
            let Some(control) = self.resources.actors.get(actor.index()) else {
                continue;
            };
            let owner = &frame.actors[actor.index()];
            match pose {
                resonance_battle::CommonPose::Taunt => {
                    cues.extend(control.taunt.map(|sound| Cue::Voice {
                        actor,
                        sound,
                        priority: VoicePriority::Action,
                        position: owner.audio_position(),
                        centered: false,
                    }))
                }
                resonance_battle::CommonPose::Backstep => {
                    cues.extend(control.backstep.map(|sound| Cue::Sound {
                        actor,
                        sound,
                        priority: 0,
                        position: owner.audio_position(),
                    }))
                }
                _ => {}
            }
        }
        for cue in &frame.cues {
            match *cue {
                Cue::Casting {
                    actor,
                    action,
                    phase,
                } => {
                    use resonance_battle::CastPhase;
                    let Some(cast) = self.resources.casting.get(&action) else {
                        continue;
                    };
                    let owner = &frame.actors[actor.index()];
                    if phase == CastPhase::Chanting
                        && !matches!(owner.activity, resonance_battle::Activity::Casting { .. })
                    {
                        continue;
                    }
                    let (appearance, sound, voice) = match phase {
                        CastPhase::Chanting => {
                            (cast.chant_effect, cast.start_sound, cast.chant_voice)
                        }
                        CastPhase::Charged => (cast.charged_effect, None, None),
                        CastPhase::Stored => (cast.stored_effect, None, None),
                        CastPhase::Released => {
                            (cast.release_effect, cast.release_sound, cast.release_voice)
                        }
                    };
                    let chanting = phase == CastPhase::Chanting;
                    cues.push(Cue::Effect(EffectRequest {
                        owner: actor,
                        target: actor,
                        appearance,
                        origin: if chanting {
                            owner.position
                        } else {
                            owner.effect_origin()
                        },
                        heading: owner.heading,
                        scale: owner.effect_scale,
                        follow: Some(if chanting {
                            resonance_battle::EffectFollow::Actor(actor)
                        } else {
                            resonance_battle::EffectFollow::Center(actor)
                        }),
                        tint: if phase == CastPhase::Stored {
                            Default::default()
                        } else {
                            cast.tint
                        },
                    }));
                    cues.extend(sound.map(|sound| Cue::Sound {
                        actor,
                        sound,
                        position: owner.audio_position(),
                        priority: 1,
                    }));
                    cues.extend(voice.map(|sound| Cue::Voice {
                        actor,
                        sound,
                        position: owner.audio_position(),
                        priority: VoicePriority::Action,
                        centered: false,
                    }));
                    if phase == CastPhase::Released {
                        cues.push(Cue::Notice {
                            actor,
                            action,
                            duration: 90,
                        });
                    }
                }
                Cue::OverLimitEntered { actor, .. } => {
                    let position = frame.actors[actor.index()].audio_position();
                    cues.extend(self.resources.overlimit_sound.map(|sound| Cue::Sound {
                        actor,
                        sound,
                        position,
                        priority: 1,
                    }));
                    cues.extend(
                        self.resources
                            .actors
                            .get(actor.index())
                            .and_then(|feedback| feedback.overlimit_voice)
                            .map(|sound| Cue::Voice {
                                actor,
                                sound,
                                position,
                                priority: VoicePriority::Announcement,
                                centered: false,
                            }),
                    );
                }
                Cue::HammerRevenge { projectile } => {
                    if let Some(appearance) = self.resources.hammer
                        && let Some(projectile) =
                            frame.projectiles.iter().find(|p| p.id == projectile)
                    {
                        cues.push(Cue::Effect(EffectRequest {
                            owner: projectile.owner,
                            target: projectile.target,
                            appearance,
                            origin: projectile.position,
                            heading: projectile.heading,
                            scale: 1.,
                            follow: Some(resonance_battle::EffectFollow::Projectile(projectile.id)),
                            tint: Default::default(),
                        }));
                    }
                }
                Cue::TechniqueQueued { actor } => {
                    cues.extend(
                        self.resources
                            .actors
                            .get(actor.index())
                            .and_then(|control| control.technique_command)
                            .map(|sound| Cue::Voice {
                                actor,
                                sound,
                                priority: VoicePriority::Announcement,
                                position: frame.actors[actor.index()].audio_position(),
                                centered: false,
                            }),
                    );
                }
                Cue::Started {
                    actor,
                    definition: Some(action),
                    ..
                } => {
                    if let Some(&color) = self.resources.admission_flashes.get(&action) {
                        cues.push(Cue::ActorFlash { actor, color });
                    }
                }
                Cue::Recovered {
                    actor,
                    applied: 1..,
                    ..
                }
                | Cue::SelfCured { actor } => {
                    if let Some(color) = self.resources.recovery_tint {
                        cues.push(Cue::ActorFlash { actor, color });
                    }
                }
                Cue::ItemReleased {
                    user,
                    target,
                    effect,
                    discovered,
                } => {
                    if let Some(items) = &self.resources.items {
                        let owner = &frame.actors[user.index()];
                        cues.push(attached_effect(user, owner, items.use_effect));
                        if let Some(sound) = items.sound {
                            cues.push(Cue::Sound {
                                actor: user,
                                sound,
                                position: owner.audio_position(),
                                priority: 1,
                            });
                        }
                        let voice = &items.voices[user.index()];
                        if let Some(sound) = voice.discovery.filter(|_| discovered).or(voice.used) {
                            cues.push(Cue::Voice {
                                actor: user,
                                sound,
                                priority: VoicePriority::Action,
                                position: owner.audio_position(),
                                centered: false,
                            });
                        }
                        use resonance_battle::item::Effect;
                        let color = match effect {
                            Effect::Cure(_) => self.resources.recovery_tint,
                            Effect::Buff(_) => items.buff_tint,
                            Effect::Scan => items.scan_tint,
                            _ => None,
                        };
                        if let Some(color) = color {
                            cues.push(Cue::ActorFlash {
                                actor: target,
                                color,
                            });
                        }
                        if matches!(effect, Effect::Revive) {
                            cues.push(attached_effect(
                                target,
                                &frame.actors[target.index()],
                                items.revival_effect,
                            ));
                        }
                    }
                }
                Cue::Jumped { actor, position } => {
                    if let Some(appearance) = self.resources.takeoff {
                        cues.push(Cue::Effect(EffectRequest {
                            owner: actor,
                            target: actor,
                            appearance,
                            origin: position,
                            heading: frame.actors[actor.index()].heading,
                            scale: frame.actors[actor.index()].effect_scale,
                            follow: None,
                            tint: Default::default(),
                        }));
                    }
                }
                Cue::UnisonReady => cues.extend(
                    self.resources
                        .unison_ready
                        .map(|sound| Cue::GlobalSound { sound, priority: 1 }),
                ),
                Cue::KnockdownImpact { actor } => {
                    cues.extend(
                        self.resources
                            .actors
                            .get(actor.index())
                            .and_then(|control| control.knockdown)
                            .map(|sound| Cue::Sound {
                                actor,
                                sound,
                                position: frame.actors[actor.index()].audio_position(),
                                priority: 1,
                            }),
                    );
                }
                Cue::GuardReady { actor }
                | Cue::Countered { actor }
                | Cue::Charged { actor, .. } => {
                    let owner = &frame.actors[actor.index()];
                    let position = std::array::from_fn(|axis| {
                        owner.position[axis] + owner.body.center_offset[axis] * 2.
                    });
                    let failed = matches!(
                        cue,
                        Cue::Charged {
                            level: resonance_battle::ChargeLevel::None,
                            ..
                        }
                    );
                    if failed {
                        cues.push(Cue::CustomLabel {
                            actor,
                            position,
                            left: "ABILITY".into(),
                            right: "FAILED".into(),
                            duration: 45,
                        });
                    } else {
                        cues.push(Cue::ExSkillLabel { actor, position });
                        let effect = if matches!(cue, Cue::Countered { .. }) {
                            self.resources.counter
                        } else {
                            self.resources.skill_ready
                        };
                        if let Some(effect) = effect {
                            cues.push(attached_effect(actor, owner, effect));
                        }
                    }
                    if let Cue::Charged { .. } = cue {
                        cues.extend(
                            self.resources
                                .actors
                                .get(actor.index())
                                .and_then(|control| {
                                    if failed {
                                        control.charge_failed
                                    } else {
                                        control.charge
                                    }
                                })
                                .map(|sound| Cue::Voice {
                                    actor,
                                    sound,
                                    priority: VoicePriority::Action,
                                    position: owner.audio_position(),
                                    centered: false,
                                }),
                        );
                    }
                }
                Cue::Paralyzed { actor } => cues.push(Cue::ActorFlash {
                    actor,
                    color: [192, 192, 160],
                }),
                Cue::Breakfall { actor } => {
                    cues.push(Cue::ActorFlash {
                        actor,
                        color: [192; 3],
                    });
                    if let Some(Some(feedback)) = self.resources.breakfalls.get(actor.index()) {
                        let owner = &frame.actors[actor.index()];
                        cues.push(attached_effect(actor, owner, feedback.effect));
                        if let Some(sound) = feedback.sound {
                            cues.push(Cue::Sound {
                                actor,
                                sound,
                                position: owner.audio_position(),
                                priority: 0,
                            });
                        }
                        if let Some(sound) = feedback.voice {
                            cues.push(Cue::Voice {
                                actor,
                                sound,
                                priority: VoicePriority::Reaction,
                                position: owner.audio_position(),
                                centered: false,
                            });
                        }
                    }
                }
                Cue::Rescued { actor, .. } => {
                    if let Some(rescue) = self.resources.rescues.get(actor.index()) {
                        let target = &frame.actors[actor.index()];
                        cues.push(attached_effect(actor, target, rescue.appearance));
                        if let Some(sound) = rescue.sound {
                            cues.push(Cue::Sound {
                                actor,
                                sound,
                                position: target.audio_position(),
                                priority: 1,
                            });
                        }
                        if let Some(layers) = rescue.expression {
                            frame
                                .model_requests
                                .push(resonance_battle::ModelRequest::Expression { actor, layers });
                        }
                        if let Some(motion) = rescue.motion {
                            frame
                                .model_requests
                                .push(resonance_battle::ModelRequest::Play {
                                    actor,
                                    motion,
                                    pose: resonance_battle::Pose {
                                        restart: false,
                                        ..Default::default()
                                    },
                                });
                        }
                    }
                }
                Cue::Defeated { actor } => {
                    let target = &frame.actors[actor.index()];
                    if let Some(sound) = self
                        .resources
                        .contact_audio
                        .as_ref()
                        .and_then(|audio| audio.actors.get(actor.index()))
                        .and_then(|audio| audio.voices.defeat)
                    {
                        cues.push(Cue::Voice {
                            actor,
                            sound,
                            priority: VoicePriority::Announcement,
                            position: target.audio_position(),
                            centered: false,
                        });
                    }
                    if let Some(death) = &self.resources.death {
                        if target.side == Side::Enemy {
                            cues.push(attached_effect(actor, target, death.appearance));
                            if let Some(sound) = death.sound {
                                cues.push(Cue::Sound {
                                    actor,
                                    sound,
                                    position: target.audio_position(),
                                    priority: 1,
                                });
                            }
                        } else {
                            let variant = usize::from(self.random.next_u16() & 1);
                            for reaction in death
                                .allies
                                .iter()
                                .filter(|reaction| reaction.victim == actor)
                            {
                                let recipient = &frame.actors[reaction.recipient.index()];
                                if let Some(sound) =
                                    reaction.voices[variant].filter(|_| recipient.available())
                                {
                                    cues.push(Cue::Voice {
                                        actor: reaction.recipient,
                                        sound,
                                        priority: VoicePriority::Announcement,
                                        position: recipient.audio_position(),
                                        centered: false,
                                    });
                                }
                            }
                        }
                    }
                }
                Cue::Landed { actor, position } => {
                    if let Some(appearance) = self.resources.landing {
                        let target = &frame.actors[actor.index()];
                        cues.push(Cue::Effect(EffectRequest {
                            owner: actor,
                            target: actor,
                            appearance,
                            origin: position,
                            heading: target.heading,
                            follow: None,
                            scale: target.effect_scale,
                            tint: Default::default(),
                        }));
                    }
                }
                _ => {}
            }
        }
        frame.cues.extend(cues);
        self.contacts(frame);
    }

    /// Resolve contact presentation once, before effects and audio consume this frame.
    fn contacts(&mut self, frame: &mut BattleFrame) {
        if frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::CombatRetired))
        {
            return;
        }
        let mut cues = Vec::new();
        for cue in &frame.cues {
            let Cue::Hit {
                owner,
                actor,
                position,
                element,
                was_casting,
                overlimit,
                stunned,
                result,
                ..
            } = *cue
            else {
                continue;
            };
            let target = &frame.actors[actor.index()];
            if let Some(art) = &self.resources.contact_art {
                let tint = art.elements[element.map_or(0, |element| element as usize + 1)];
                let appearance = if overlimit {
                    art.overlimit
                } else {
                    match result.guard {
                        GuardResult::Broken => art.guard_break,
                        GuardResult::Blocked { .. } => art.guard,
                        GuardResult::None if result.critical => art.critical,
                        GuardResult::None => tint.effect.unwrap_or(art.ordinary),
                    }
                };
                cues.push(Cue::Effect(EffectRequest {
                    owner: actor,
                    target: actor,
                    appearance,
                    origin: position,
                    heading: 0.,
                    follow: None,
                    scale: 1.,
                    tint: Default::default(),
                }));
                if result.is_unblocked_damage() && !overlimit {
                    cues.push(Cue::ActorFlash {
                        actor,
                        color: tint.color,
                    });
                }
            }
            let Some(audio) = &self.resources.contact_audio else {
                continue;
            };
            let sound = if overlimit {
                audio.overlimit
            } else {
                match result.guard {
                    GuardResult::Broken => audio.guard_break,
                    GuardResult::Blocked { .. } => audio.guard,
                    GuardResult::None => element
                        .map_or(audio.actors[owner.index()].neutral, |element| {
                            audio.elements[element as usize]
                        }),
                }
            };
            if let Some(sound) = sound {
                cues.push(Cue::Sound {
                    actor,
                    sound,
                    position,
                    priority: 1,
                });
            }
            let voices = &audio.actors[actor.index()].voices;
            let (voice, priority) = if result.guard == GuardResult::Broken {
                (voices.critical, VoicePriority::Action)
            } else if let GuardResult::Blocked { first, .. } = result.guard {
                (voices.guard.filter(|_| first), VoicePriority::Reaction)
            } else if result.suppresses_reaction() {
                (None, VoicePriority::Reaction)
            } else {
                let reaction = if stunned {
                    voices.stunned
                } else if was_casting {
                    voices.interrupted_cast
                } else if result.critical {
                    voices.critical
                } else {
                    None
                };
                if reaction.is_some() {
                    (reaction, VoicePriority::Action)
                } else {
                    let hurt = voices.hurt[usize::from(self.random.next_u16() & 1)]
                        .or(voices.hurt[0])
                        .or(voices.hurt[1]);
                    (hurt, VoicePriority::Reaction)
                }
            };
            if let Some(sound) = voice {
                cues.push(Cue::Voice {
                    actor,
                    sound,
                    priority,
                    position: target.audio_position(),
                    centered: false,
                });
            }
        }
        frame.cues.extend(cues);
    }

    /// Called once with the completed simulation snapshot, never while drawing.
    pub fn advance(&mut self, frame: &mut BattleFrame, paused: bool) {
        self.appearance.advance(frame, paused);
        if frame.recognized_result.is_some() || frame.outcome.is_some() {
            self.shake = None;
            self.motors.fill(0);
            return;
        }
        if !paused {
            for motor in &mut self.motors {
                *motor = motor.saturating_sub(1);
            }
            if let Some(shake) = &mut self.shake {
                shake.age += 1;
                if shake.age >= shake.duration {
                    self.shake = None;
                }
            }
        }
        for cue in &frame.cues {
            match *cue {
                Cue::Shake {
                    duration,
                    amplitude,
                } if duration > 0 && amplitude > 0 => {
                    // At 60 updates/second: at most half a second and 12 world units.
                    let duration = duration.min(30) as u8;
                    self.shake = Some(Shake {
                        age: 0,
                        duration,
                        strength: amplitude.min(24) as f32 * 0.5,
                    });
                    for actor in &frame.actors {
                        self.pulse(actor, (duration / 2).max(1));
                    }
                }
                Cue::IncidentalDamage { actor, amount } if amount > 0 => {
                    if let Some(actor) = frame.actors.get(actor.index()) {
                        self.pulse(actor, 6);
                    }
                }
                _ => {}
            }
        }
        let (Some(shake), Some(camera)) = (&self.shake, &mut frame.camera) else {
            return;
        };
        // Oscillate around the unmodified camera and settle smoothly to rest.
        let fade = 1. - f32::from(shake.age) / f32::from(shake.duration);
        let offset = (f32::from(shake.age) * std::f32::consts::TAU / 6.).cos()
            * shake.strength
            * fade
            * fade;
        camera.eye[1] += offset;
        camera.focus[1] += offset;
    }

    fn pulse(&mut self, actor: &Actor, duration: u8) {
        if actor.side == Side::Party
            && matches!(actor.control, Control::Manual | Control::SemiAuto)
            && let Some(motor) = self.motors.get_mut(usize::from(actor.control_slot))
        {
            *motor = (*motor).max(duration);
        }
    }

    pub fn motors(&self, paused: bool) -> [bool; 4] {
        self.motors.map(|remaining| !paused && remaining > 0)
    }
}

fn attached_effect(
    actor: resonance_battle::ActorId,
    target: &Actor,
    appearance: resonance_battle::EffectAppearance,
) -> Cue {
    Cue::Effect(EffectRequest {
        owner: actor,
        target: actor,
        appearance,
        origin: target.effect_origin(),
        heading: target.heading,
        follow: Some(resonance_battle::EffectFollow::Center(actor)),
        scale: target.effect_scale,
        tint: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{BattleResult, CameraDefinition, PreparedBattle};

    #[test]
    fn item_feedback_uses_committed_events_and_one_user_voice() -> anyhow::Result<()> {
        use resonance_battle::{EffectAppearance, Sound, item::Effect};
        use resonance_game::battle::items::{Feedback as ItemFeedback, ItemVoice};
        let battle = PreparedBattle::new(
            vec![
                (
                    crate::test_support::actor(Side::Party, 100, 20),
                    Default::default(),
                ),
                (
                    crate::test_support::actor(Side::Enemy, 100, 20),
                    Default::default(),
                ),
            ],
            Default::default(),
            1,
        )?
        .finish()?;
        let ids: Vec<_> = battle.actor_ids().collect();
        let event = Cue::ItemReleased {
            user: ids[0],
            target: ids[1],
            effect: Effect::Scan,
            discovered: true,
        };
        let appearance = EffectAppearance {
            resource: 1,
            member: 9,
        };
        let mut feedback = Feedback::new(
            Resources {
                items: Some(ItemFeedback {
                    use_effect: appearance,
                    revival_effect: appearance,
                    sound: None,
                    buff_tint: None,
                    scan_tint: Some([20, 40, 60]),
                    voices: vec![
                        ItemVoice {
                            used: Some(Sound::Stream(1)),
                            discovery: Some(Sound::Stream(2)),
                        },
                        ItemVoice::default(),
                    ],
                }),
                ..Default::default()
            },
            1,
        );
        let mut frame = battle.snapshot();
        frame.cues = vec![event.clone()];
        feedback.resolve(&mut frame);
        let voices: Vec<_> = frame
            .cues
            .iter()
            .filter_map(|cue| match cue {
                Cue::Voice { actor, sound, .. } => Some((*actor, *sound)),
                _ => None,
            })
            .collect();
        assert_eq!(voices, [(ids[0], Sound::Stream(2))]);
        assert!(frame.cues.contains(&Cue::ActorFlash {
            actor: ids[1],
            color: [20, 40, 60]
        }));
        assert_eq!(frame.actors[1].hp, 100);
        frame.cues = vec![event.clone()];
        Feedback::default().resolve(&mut frame);
        assert_eq!(frame.cues, [event]);
        Ok(())
    }

    #[test]
    fn contact_feedback_uses_resolved_hits_and_keeps_gameplay_unchanged() -> anyhow::Result<()> {
        use resonance_battle::{
            ActionDefinition, ActionExecution, ActionRequest, Affinity, BattleInput, ContactSource,
            EffectAppearance, Element, HitProtection, HitResult, PreparedAttack, Sound,
        };
        use resonance_game::battle::{
            contact_audio::{ContactActorAudio, ContactAudio, ContactVoices},
            contact_feedback::{ContactArt, ContactElementFeedback},
        };
        let mut battle = PreparedBattle::new(
            vec![
                (
                    crate::test_support::actor(Side::Party, 100, 20),
                    resonance_battle::ActorSetup {
                        techniques: vec![resonance_battle::PreparedTechnique {
                            action: resonance_battle::ActionKey(0),
                            catalogue: 1,
                            player_range: [0., 120.],
                            ai_range: [0., 120.],
                            capabilities: Default::default(),
                            element: 0,
                        }],
                        ..Default::default()
                    },
                ),
                (
                    crate::test_support::actor(Side::Enemy, 100, 20),
                    Default::default(),
                ),
            ],
            (vec![ActionDefinition {
                normal: None,
                tp_cost: 0,
                execution: ActionExecution::Attack(PreparedAttack {
                    chain_at: None,
                    end_at: 10,
                    opening: None,
                    events: vec![(
                        0,
                        resonance_battle::AttackEvent::Projectile(std::sync::Arc::new(
                            resonance_battle::ProjectileDefinition {
                                lifetime: Some(30),
                                velocity: [0.; 3],
                                acceleration: [0.; 3],
                                offset: [0.; 3],
                                clamp_ground: false,
                                active: None,
                                birth: None,
                                motion: Default::default(),
                                effects: Default::default(),
                                contact: None,
                            },
                        )),
                    )],
                    recovery: 0,
                }),
            }])
            .into(),
            1,
        )?
        .finish()?;
        let ids: Vec<_> = battle.actor_ids().collect();
        let admitted = battle.step(BattleInput {
            actions: vec![ActionRequest {
                actor: ids[0],
                target: ids[1],
                action: resonance_battle::ActionKey(0),
            }],
            ..Default::default()
        })?;
        let source = ContactSource::Melee {
            actor: ids[0],
            action: admitted.actions[0].0,
        };
        let base = battle.snapshot();
        let gameplay_random = battle.random_state();
        let effect = |member| EffectAppearance {
            resource: 1,
            member,
        };
        let art = ContactArt {
            ordinary: effect(1),
            guard: effect(2),
            critical: effect(3),
            guard_break: effect(4),
            overlimit: effect(5),
            elements: [ContactElementFeedback {
                effect: Some(effect(7)),
                color: [64, 96, 128],
            }; 9],
        };
        let audio = ContactAudio {
            actors: vec![
                ContactActorAudio {
                    neutral: Some(Sound::Cue(10)),
                    voices: ContactVoices {
                        hurt: [Some(Sound::Stream(1)), Some(Sound::Stream(2))],
                        guard: Some(Sound::Stream(3)),
                        interrupted_cast: Some(Sound::Stream(4)),
                        defeat: Some(Sound::Stream(5)),
                        critical: Some(Sound::Stream(6)),
                        ..Default::default()
                    },
                };
                2
            ],
            elements: [Some(Sound::Cue(11)); 8],
            guard: Some(Sound::Cue(12)),
            guard_break: Some(Sound::Cue(13)),
            overlimit: Some(Sound::Cue(14)),
        };
        let mut feedback = Feedback::new(
            Resources {
                contact_art: Some(art),
                contact_audio: Some(audio),
                ..Default::default()
            },
            99,
        );
        feedback.resources.entry_voice = Some((ids[0], Sound::Stream(90)));
        let mut entry = base.clone();
        feedback.resolve(&mut entry);
        assert!(entry.cues.iter().any(|cue| matches!(cue,
            Cue::Voice { actor, sound: Sound::Stream(90), centered: true, .. } if *actor == ids[0])));
        let mut next = base.clone();
        feedback.resolve(&mut next);
        assert!(!next.cues.iter().any(|cue| matches!(cue, Cue::Voice { .. })));
        for (guard, was_casting, hp, impact, sound, voice) in [
            (GuardResult::None, true, 100, 7, 11, Some(4)),
            (
                GuardResult::Blocked {
                    first: true,
                    special: false,
                },
                false,
                100,
                2,
                12,
                Some(3),
            ),
            (GuardResult::Broken, false, 100, 4, 13, Some(6)),
            (GuardResult::None, false, 0, 7, 11, None),
            (GuardResult::None, false, 100, 7, 11, None),
        ] {
            let mut frame = base.clone();
            frame.actors[1].state.hp = hp;
            frame.cues = vec![Cue::Hit {
                source,
                owner: ids[0],
                actor: ids[1],
                position: [1., 2., 3.],
                element: Some(Element::Fire),
                was_casting,
                overlimit: false,
                stunned: false,
                result: HitResult {
                    amount: 10,
                    hp_change: -10,
                    critical: false,
                    boosted: false,
                    affinity: Affinity::Normal,
                    guard,
                    protection: HitProtection::None,
                },
            }];
            let actors = frame.actors.clone();
            feedback.contacts(&mut frame);
            let effects: Vec<_> = frame
                .cues
                .iter()
                .filter_map(|cue| match cue {
                    Cue::Effect(request) => Some(request),
                    _ => None,
                })
                .collect();
            assert_eq!(effects.len(), 1);
            assert_eq!(effects[0].appearance, effect(impact));
            assert_eq!(effects[0].origin, [1., 2., 3.]);
            assert!(frame.cues.iter().any(|cue| matches!(cue, Cue::Sound { sound: emitted, .. } if *emitted == Sound::Cue(sound))));
            let voices: Vec<_> = frame
                .cues
                .iter()
                .filter_map(|cue| match cue {
                    Cue::Voice { sound, .. } => Some(*sound),
                    _ => None,
                })
                .collect();
            assert_eq!(voices.len(), 1);
            if let Some(voice) = voice {
                assert_eq!(voices[0], Sound::Stream(voice));
            } else {
                assert!([Sound::Stream(1), Sound::Stream(2)].contains(&voices[0]));
            }
            assert_eq!(frame.actors, actors);
            if hp == 0 {
                frame.cues.retain(|cue| matches!(cue, Cue::Hit { .. }));
                frame.cues.push(frame.cues[0].clone());
                frame.cues.push(Cue::Defeated { actor: ids[1] });
                feedback.resolve(&mut frame);
                assert_eq!(
                    frame
                        .cues
                        .iter()
                        .filter(|cue| matches!(
                            cue,
                            Cue::Voice {
                                sound: Sound::Stream(5),
                                ..
                            }
                        ))
                        .count(),
                    1
                );
            }
        }
        feedback
            .resources
            .admission_flashes
            .insert(resonance_battle::ActionKey(0), [10, 20, 30]);
        feedback.resources.recovery_tint = Some([40, 112, 40]);
        feedback.resources.self_cure_notice = Some("Self Cure".into());
        feedback.resources.rescue_names[0] = "Angel's Tear".into();
        feedback
            .resources
            .rescues
            .push(resonance_game::battle::feedback::RescueFeedback {
                motion: Some(resonance_battle::MotionBinding { model: 1, clip: 3 }),
                expression: None,
                appearance: effect(19),
                sound: None,
            });
        feedback.resources.landing = Some(effect(17));
        feedback.resources.breakfalls.push(Some(
            resonance_game::battle::feedback::BreakfallFeedback {
                effect: effect(15),
                voice: Some(Sound::Stream(19)),
                sound: Some(Sound::Cue(15)),
            },
        ));
        let mut frame = admitted;
        frame.cues.extend([
            Cue::Recovered {
                actor: ids[0],
                kind: resonance_battle::RecoveryKind::Hp,
                nominal: 10,
                applied: 10,
            },
            Cue::SelfCured { actor: ids[0] },
            Cue::Breakfall { actor: ids[0] },
            Cue::Rescued {
                actor: ids[0],
                kind: resonance_battle::RescueKind::AngelTear,
            },
            Cue::Landed {
                actor: ids[0],
                position: [5., 0., 0.],
            },
        ]);
        let notices: Vec<_> = frame
            .cues
            .iter()
            .filter_map(|cue| feedback.resources.notice(cue))
            .collect();
        assert_eq!(notices, [(ids[0], "Self Cure"), (ids[0], "Angel's Tear")]);
        feedback.resolve(&mut frame);
        assert!(frame.cues.contains(&Cue::ActorFlash {
            actor: ids[0],
            color: [10, 20, 30]
        }));
        assert!(frame.cues.contains(&Cue::ActorFlash {
            actor: ids[0],
            color: [40, 112, 40]
        }));
        assert!(
            frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Effect(request) if request.appearance == effect(19)))
        );
        assert!(
            frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Effect(request)
            if request.appearance == effect(17) && request.origin == [5., 0., 0.]))
        );
        assert!(frame.cues.iter().any(|cue| matches!(cue,
            Cue::Effect(request) if request.appearance == effect(15))));
        assert!(frame.cues.iter().any(|cue| matches!(cue,
            Cue::Voice { actor, sound: Sound::Stream(19), .. } if *actor == ids[0])));
        assert!(frame.model_requests.iter().any(|request| matches!(request,
            resonance_battle::ModelRequest::Play { actor, motion, .. } if *actor == ids[0] && motion.clip == 3)));
        feedback
            .resources
            .actors
            .push(resonance_game::battle::feedback::ActorFeedback {
                overlimit_voice: Some(Sound::Stream(25)),
                technique_command: Some(Sound::Stream(24)),
                taunt: Some(Sound::Stream(21)),
                backstep: Some(Sound::Cue(22)),
                charge_failed: Some(Sound::Stream(23)),
                ..Default::default()
            });
        feedback.resources.takeoff = Some(effect(17));
        feedback.resources.skill_ready = Some(effect(50));
        feedback.resources.unison_ready = Some(Sound::Cue(77));
        feedback.resources.overlimit_sound = Some(Sound::Cue(77));
        feedback.resources.hammer = Some(effect(14));
        feedback.resources.casting.insert(
            resonance_battle::ActionKey(0),
            resonance_game::battle::feedback::CastingFeedback {
                chant_voice: Some(Sound::Stream(26)),
                release_voice: Some(Sound::Stream(27)),
                start_sound: Some(Sound::Cue(109)),
                release_sound: Some(Sound::Cue(123)),
                chant_effect: effect(3),
                charged_effect: effect(6),
                stored_effect: effect(50),
                release_effect: effect(7),
                tint: Default::default(),
            },
        );
        let mut frame = base.clone();
        let hammer = frame.projectiles[0].id;
        frame.projectiles[0].position = [5., 80., 0.];
        frame.model_requests.extend([
            resonance_battle::ModelRequest::Common {
                actor: ids[0],
                pose: resonance_battle::CommonPose::Taunt,
            },
            resonance_battle::ModelRequest::Common {
                actor: ids[0],
                pose: resonance_battle::CommonPose::Backstep,
            },
        ]);
        frame.cues.extend([
            Cue::OverLimitEntered {
                actor: ids[0],
                position: [0.; 3],
            },
            Cue::HammerRevenge { projectile: hammer },
            Cue::Casting {
                actor: ids[0],
                action: resonance_battle::ActionKey(0),
                phase: resonance_battle::CastPhase::Released,
            },
            Cue::TechniqueQueued { actor: ids[0] },
            Cue::Jumped {
                actor: ids[0],
                position: [2., 0., 3.],
            },
            Cue::Charged {
                actor: ids[0],
                level: resonance_battle::ChargeLevel::None,
            },
            Cue::GuardReady { actor: ids[0] },
            Cue::UnisonReady,
        ]);
        feedback.resolve(&mut frame);
        assert!(frame.cues.iter().any(|cue| matches!(cue, Cue::Effect(request) if request.origin == [2., 0., 3.] && request.follow.is_none())));
        assert!(
            frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::CustomLabel { right, .. } if right == "FAILED"))
        );
        assert!(
            frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::ExSkillLabel { .. }))
        );
        assert!(frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::GlobalSound {
                sound: Sound::Cue(77),
                ..
            }
        )));
        assert!(frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Sound {
                sound: Sound::Cue(22),
                ..
            }
        )));
        assert!(
            frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Effect(request)
            if request.appearance == effect(14)
                && request.origin == [5., 80., 0.]
                && request.follow == Some(resonance_battle::EffectFollow::Projectile(hammer))))
        );
        assert!(frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Sound {
                sound: Sound::Cue(77),
                ..
            }
        )));
        for expected in [
            Sound::Stream(21),
            Sound::Stream(23),
            Sound::Stream(24),
            Sound::Stream(25),
            Sound::Stream(27),
        ] {
            assert!(
                frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Voice { sound, .. } if *sound == expected))
            );
        }
        assert!(frame.cues.iter().any(|cue| matches!(cue,
            Cue::Effect(request) if request.appearance == effect(7))));
        assert!(
            frame.cues.iter().any(|cue| matches!(cue,
            Cue::Notice { actor, action: resonance_battle::ActionKey(0), .. } if *actor == ids[0]))
        );
        assert_eq!(battle.snapshot(), base);
        assert_eq!(battle.random_state(), gameplay_random);
        Ok(())
    }

    #[test]
    fn impacts_route_hold_expire_and_end_without_changing_gameplay() {
        let actors: Vec<_> = [
            (Side::Party, Control::Manual, 3),
            (Side::Party, Control::SemiAuto, 1),
            (Side::Party, Control::Auto, 2),
            (Side::Enemy, Control::Enemy, 0),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (side, control, slot))| {
            let mut actor = crate::test_support::actor(side, 100, 20);
            actor.control = control;
            actor.control_slot = slot;
            actor.position[0] = index as f32 * 100.;
            actor
        })
        .collect();
        let prepared = PreparedBattle::new(
            (actors)
                .into_iter()
                .map(|actor| (actor, Default::default()))
                .collect(),
            Default::default(),
            1,
        )
        .unwrap();
        let leader = prepared.actor_ids().next().unwrap();
        let battle = prepared
            .with_camera(CameraDefinition {
                leader,
                stage_pitch: 0.,
                adaptive: false,
            })
            .unwrap()
            .finish()
            .unwrap();
        let base = battle.snapshot();
        let mut frame = base.clone();
        frame.cues.push(Cue::Shake {
            duration: 12,
            amplitude: 8,
        });
        let mut feedback = Feedback::default();
        feedback.advance(&mut frame, false);
        assert_ne!(frame.camera, base.camera);
        assert_eq!(feedback.motors(false), [false, true, false, true]);
        assert_eq!(frame.actors, base.actors);
        assert_eq!(battle.snapshot(), base);
        let mut held = base.clone();
        feedback.advance(&mut held, true);
        assert_eq!(held.camera, frame.camera);
        assert_eq!(feedback.motors(true), [false; 4]);
        assert_eq!(feedback.motors(false), [false, true, false, true]);
        for _ in 0..40 {
            frame = base.clone();
            feedback.advance(&mut frame, false);
        }
        assert_eq!(frame, base);
        assert_eq!(feedback.motors(false), [false; 4]);

        frame.cues = vec![
            Cue::IncidentalDamage {
                actor: leader,
                amount: 2,
            },
            Cue::IncidentalDamage {
                actor: battle.actor_ids().nth(2).unwrap(),
                amount: 2,
            },
        ];
        feedback.advance(&mut frame, false);
        assert_eq!(feedback.motors(false), [false, false, false, true]);
        assert_eq!(frame.camera, base.camera);
        frame = base.clone();
        frame.cues.push(Cue::Shake {
            duration: 120,
            amplitude: 100,
        });
        feedback.advance(&mut frame, false);
        assert!((frame.camera.unwrap().eye[1] - base.camera.unwrap().eye[1]).abs() <= 12.);
        for _ in 0..30 {
            frame = base.clone();
            feedback.advance(&mut frame, false);
        }
        assert_eq!(frame, base);
        assert_eq!(feedback.motors(false), [false; 4]);

        frame.cues.push(Cue::Shake {
            duration: 12,
            amplitude: 8,
        });
        feedback.advance(&mut frame, false);
        frame = base.clone();
        frame.recognized_result = Some(BattleResult::Victory);
        feedback.advance(&mut frame, true);
        assert_eq!(frame.camera, base.camera);
        assert_eq!(feedback.motors(false), [false; 4]);
    }
}
