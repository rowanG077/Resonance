//! Contact artwork/audio, camera impacts and controller pulses owned by the scene.
#[cfg(test)]
use resonance_battle::ResourceId;
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
    remaining: u32,
    strength: f32,
    offset: f32,
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
        for cue in &frame.cues {
            match *cue {
                Cue::Backstepped { actor } | Cue::Taunted { actor } => {
                    let Some(control) = self.resources.actors.get(actor.index()) else {
                        continue;
                    };
                    let position = frame.actors[actor.index()].audio_position();
                    if matches!(cue, Cue::Backstepped { .. }) {
                        cues.extend(control.backstep.map(|sound| Cue::Sound {
                            actor,
                            sound,
                            priority: 0,
                            position,
                        }));
                    } else {
                        cues.extend(control.taunt.map(|sound| Cue::Voice {
                            actor,
                            sound,
                            priority: VoicePriority::Action,
                            position,
                            centered: false,
                        }));
                    }
                }
                Cue::ResultAdvancement {
                    ref actors,
                    level_up,
                } => {
                    if let Some(effect) = self.resources.result_advancement {
                        cues.extend(actors.iter().map(|&actor| {
                            let owner = &frame.actors[actor.index()];
                            Cue::Effect(EffectRequest {
                                origin: std::array::from_fn(|axis| {
                                    owner.position[axis] + owner.body.center_offset[axis] * 1.75
                                }),
                                scale: 0.8,
                                ..actor_effect(actor, owner, effect)
                            })
                        }));
                    }
                    for &actor in actors {
                        let mut position = frame.actors[actor.index()].condition_label_origin();
                        position[1] -= 8.;
                        cues.push(Cue::CustomLabel {
                            actor,
                            position,
                            left: if level_up { "LEVEL" } else { "NEW" }.into(),
                            right: if level_up { "UP" } else { "EX SKILL" }.into(),
                            duration: if level_up { 60 } else { 90 },
                        });
                    }
                    if let Some((&actor, sound)) =
                        actors.last().zip(self.resources.result_advancement_sound)
                    {
                        cues.push(Cue::Sound {
                            actor,
                            sound,
                            priority: 1,
                            position: frame.actors[actor.index()].audio_position(),
                        });
                    }
                }
                Cue::TechniqueAcquired { actor, spell, .. } => {
                    let owner = &frame.actors[actor.index()];
                    if let Some(appearance) = self.resources.acquisition {
                        cues.push(Cue::EffectDuringPause(actor_effect(
                            actor, owner, appearance,
                        )));
                    }
                    if !owner.airborne()
                        && let Some(motion) = self
                            .resources
                            .actors
                            .get(actor.index())
                            .and_then(|control| control.acquisition_motion)
                    {
                        frame
                            .model_requests
                            .push(resonance_battle::ModelRequest::Play {
                                actor,
                                motion,
                                pose: resonance_battle::Pose {
                                    blend: 8,
                                    ..Default::default()
                                },
                            });
                    }
                    cues.push(Cue::EffectNotice {
                        actor,
                        text: if spell {
                            "-Acquired Spell-"
                        } else {
                            "-Acquired Skill-"
                        }
                        .into(),
                        duration: 90,
                    });
                    cues.extend(self.resources.acquisition_sound.map(|sound| Cue::Sound {
                        actor,
                        sound,
                        priority: 1,
                        position: owner.audio_position(),
                    }));
                }
                Cue::TargetChanged { .. } => {
                    cues.extend(
                        self.resources
                            .target_changed
                            .map(|sound| Cue::GlobalSound { sound, priority: 1 }),
                    );
                }
                Cue::Casting {
                    actor,
                    action,
                    phase,
                } => {
                    use resonance_battle::CastPhase;
                    let summon_release = phase == CastPhase::Released
                        && frame
                            .summon
                            .as_ref()
                            .is_some_and(|summon| summon.owner == Some(actor));
                    let Some(cast) = self.resources.casting.get(&action) else {
                        continue;
                    };
                    let owner = &frame.actors[actor.index()];
                    if matches!(
                        phase,
                        CastPhase::Started | CastPhase::Chanting | CastPhase::Incantation
                    ) && !matches!(owner.activity, resonance_battle::Activity::Casting { .. })
                    {
                        continue;
                    }
                    let (appearance, sound, voice) = match phase {
                        CastPhase::Started => (cast.chant_effect, cast.start_sound, None),
                        CastPhase::Incantation => (cast.chant_effect, None, cast.chant_voice),
                        CastPhase::Chanting => (cast.chant_effect, None, None),
                        CastPhase::Charged => (cast.charged_effect, None, None),
                        CastPhase::Stored => (cast.stored_effect, None, None),
                        CastPhase::SceneOpening => {
                            let Some(opening) = cast.opening_effect else {
                                continue;
                            };
                            (opening, cast.release_sound, cast.release_voice)
                        }
                        CastPhase::Released if cast.opening_effect.is_some() => {
                            (cast.release_effect, None, None)
                        }
                        CastPhase::Released => {
                            (cast.release_effect, cast.release_sound, cast.release_voice)
                        }
                    };
                    let chanting = phase == CastPhase::Chanting;
                    if !summon_release {
                        if !matches!(phase, CastPhase::Started | CastPhase::Incantation) {
                            let request = EffectRequest {
                                origin: if chanting {
                                    owner.position
                                } else {
                                    owner.effect_origin()
                                },
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
                                ..actor_effect(actor, owner, appearance)
                            };
                            cues.push(if phase == CastPhase::SceneOpening {
                                Cue::InitializedEffect(request)
                            } else {
                                Cue::Effect(request)
                            });
                        }
                        cues.extend(sound.map(|sound| Cue::Sound {
                            actor,
                            sound,
                            position: owner.audio_position(),
                            priority: 1,
                        }));
                    }
                    cues.extend(voice.map(|sound| Cue::Voice {
                        actor,
                        sound,
                        position: owner.audio_position(),
                        priority: VoicePriority::Action,
                        centered: false,
                    }));
                    if phase == CastPhase::SceneOpening
                        || (phase == CastPhase::Released && cast.opening_effect.is_none())
                    {
                        cues.push(Cue::Notice {
                            actor,
                            action,
                            duration: cast.notice_duration(),
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
                        cues.push(Cue::ActorPulse { actor, color });
                    }
                }
                Cue::Recovered {
                    actor,
                    nominal: 1..,
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
                        if owner.side == Side::Party {
                            let feedback = &items.actors[user.index()];
                            let mut sparkle = attached_effect(user, owner, items.use_effect);
                            sparkle.origin = owner.local_point(std::array::from_fn(|axis| {
                                owner.body.center_offset[axis] + feedback.use_offset[axis]
                            }));
                            sparkle.follow = None;
                            cues.push(Cue::Effect(sparkle));
                            if let Some(sound) = items.sound {
                                cues.push(Cue::Sound {
                                    actor: user,
                                    sound,
                                    position: owner.audio_position(),
                                    priority: 1,
                                });
                            }
                            if let Some(sound) =
                                feedback.discovery.filter(|_| discovered).or(feedback.used)
                            {
                                cues.push(Cue::Voice {
                                    actor: user,
                                    sound,
                                    priority: VoicePriority::Action,
                                    position: owner.audio_position(),
                                    centered: false,
                                });
                            }
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
                            let recipient = &frame.actors[target.index()];
                            let mut request =
                                attached_effect(target, recipient, items.revival_effect);
                            cues.push(if owner.side == Side::Enemy {
                                request.origin = recipient.position;
                                request.follow =
                                    Some(resonance_battle::EffectFollow::Actor(target));
                                Cue::InitializedEffect(request)
                            } else {
                                Cue::Effect(request)
                            });
                        }
                    }
                }
                Cue::Jumped { actor, position } | Cue::Landed { actor, position } => {
                    let appearance = if matches!(cue, Cue::Jumped { .. }) {
                        self.resources.takeoff
                    } else {
                        self.resources.landing
                    };
                    if let Some(appearance) = appearance {
                        cues.push(Cue::Effect(EffectRequest {
                            origin: position,
                            ..actor_effect(actor, &frame.actors[actor.index()], appearance)
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
                    let position = owner.condition_label_origin();
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
                            cues.push(Cue::Effect(attached_effect(actor, owner, effect)));
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
                Cue::Paralyzed { actor } => cues.push(Cue::ActorPulse {
                    actor,
                    color: [192, 192, 160],
                }),
                Cue::Breakfall { actor } => {
                    cues.push(Cue::ActorPulse {
                        actor,
                        color: [192; 3],
                    });
                    if let Some(Some(feedback)) = self.resources.breakfalls.get(actor.index()) {
                        let owner = &frame.actors[actor.index()];
                        cues.push(Cue::Effect(attached_effect(actor, owner, feedback.effect)));
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
                        cues.push(Cue::EffectDuringPause(attached_effect(
                            actor,
                            target,
                            rescue.appearance,
                        )));
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
                            // Confirmed defeats are contact feedback. Their first
                            // particles appear after the current particle visit.
                            cues.push(Cue::DeferredEffect(attached_effect(
                                actor,
                                target,
                                death.appearance,
                            )));
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
                _ => {}
            }
        }
        frame.cues.extend(cues);
        self.contacts(frame);
        // Rescue recovery is the final appearance change of its lethal contact.
        if let Some(color) = self.resources.recovery_tint {
            for index in 0..frame.cues.len() {
                if let Cue::Rescued { actor, .. } = frame.cues[index] {
                    frame.cues.push(Cue::ActorFlash { actor, color });
                }
            }
        }
    }

    /// Sample activation attachments only after the current actor pose has been published.
    pub fn resolve_model_effects(&self, frame: &mut BattleFrame) -> anyhow::Result<()> {
        use anyhow::Context;
        use resonance_content::animation::transform_point;
        if frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::CombatRetired))
        {
            return Ok(());
        }
        let mut effects = Vec::new();
        for cue in &frame.cues {
            let Cue::OverLimitEntered { actor, .. } = *cue else {
                continue;
            };
            let Some(effect) = self
                .resources
                .actors
                .get(actor.index())
                .and_then(|feedback| feedback.overlimit.as_ref())
            else {
                continue;
            };
            let model = frame
                .models
                .iter()
                .find(|model| model.actor == actor)
                .context("Over Limit actor has no prepared model pose")?;
            let bone = model
                .bones
                .get(usize::from(effect.bone))
                .context("Over Limit attachment bone is unavailable")?;
            effects.push(Cue::EffectDuringPause(EffectRequest {
                origin: transform_point(model.world, transform_point(*bone, [0.; 3])),
                scale: effect.scale,
                ..actor_effect(actor, &frame.actors[actor.index()], effect.appearance)
            }));
        }
        frame.cues.extend(effects);
        Ok(())
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
                contact_sound,
                suppress_guard_sound,
                was_casting,
                overlimit,
                stunned,
                repeated,
                result,
                ..
            } = *cue
            else {
                continue;
            };
            let target = &frame.actors[actor.index()];
            if let Some(art) = &self.resources.contact_art {
                let tint = art.elements[element.map_or(0, |element| element as usize + 1)];
                let mut emit = |effect_owner, appearance| {
                    cues.push(Cue::DeferredEffect(EffectRequest {
                        owner: effect_owner,
                        target: actor,
                        appearance,
                        origin: position,
                        heading: 0.,
                        follow: None,
                        scale: 1.,
                        tint: Default::default(),
                    }));
                };
                if overlimit {
                    emit(actor, art.overlimit);
                } else {
                    match result.guard {
                        GuardResult::Broken => {
                            emit(
                                actor,
                                if repeated {
                                    art.repeated_guard
                                } else {
                                    art.guard
                                },
                            );
                            let mut effect = attached_effect(actor, &target.state, art.guard_break);
                            effect.heading = 0.;
                            cues.push(Cue::DeferredEffect(effect));
                        }
                        GuardResult::Blocked { .. } => emit(
                            actor,
                            if repeated {
                                art.repeated_guard
                            } else {
                                art.guard
                            },
                        ),
                        GuardResult::None => {
                            if element.is_some()
                                && let Some(appearance) = tint.effect
                            {
                                emit(actor, appearance);
                            }
                            if element.is_some() || repeated {
                                emit(actor, art.compact_impact);
                            } else {
                                emit(actor, art.ordinary);
                            }
                            if result.critical {
                                emit(owner, art.critical);
                            }
                        }
                    }
                }
                if result.is_unblocked_damage()
                    && !overlimit
                    && !frame.cues.iter().any(
                        |cue| matches!(cue, Cue::Rescued { actor: rescued, .. } if *rescued == actor),
                    )
                {
                    cues.push(Cue::ActorPulse {
                        actor,
                        color: tint.color,
                    });
                }
            }
            let Some(audio) = &self.resources.contact_audio else {
                continue;
            };
            if !overlimit
                && matches!(result.guard, GuardResult::Blocked { .. })
                && let Some(sound) = contact_sound
            {
                cues.push(Cue::Sound {
                    actor,
                    sound,
                    position,
                    priority: 1,
                });
            }
            let sound = if overlimit {
                audio.overlimit
            } else {
                match result.guard {
                    GuardResult::Broken => audio.guard_break,
                    GuardResult::Blocked { .. } => audio.guard.filter(|_| !suppress_guard_sound),
                    GuardResult::None => contact_sound.or_else(|| {
                        element.map_or(audio.actors[owner.index()].neutral, |element| {
                            audio.elements[element as usize]
                        })
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
    pub fn advance(&mut self, frame: &mut BattleFrame, paused: bool, actor_ticks: &[u16]) {
        self.appearance.advance(frame, paused, actor_ticks);
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
                shake.offset = if (shake.remaining / 2).is_multiple_of(2) {
                    0.
                } else {
                    0.5 * shake.strength
                };
                shake.remaining = shake.remaining.saturating_sub(1);
                if shake.remaining == 0 {
                    self.shake = None;
                }
            }
        }
        for cue in &frame.cues {
            match *cue {
                Cue::Shake {
                    duration,
                    amplitude,
                } => {
                    // Requests replace the timer after this visit's displacement is published.
                    let shake = self.shake.get_or_insert(Shake {
                        remaining: 0,
                        strength: 0.,
                        offset: 0.,
                    });
                    shake.remaining = duration;
                    shake.strength = amplitude as f32;
                    if duration > 0 && amplitude > 0 {
                        let motor_duration = duration.min(30) as u8;
                        for actor in &frame.actors {
                            self.pulse(actor, (motor_duration / 2).max(1));
                        }
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
        camera.eye[1] += shake.offset;
        camera.focus[1] += shake.offset;
    }

    fn pulse(&mut self, actor: &Actor, duration: u8) {
        if actor.side == Side::Party && matches!(actor.control, Control::Manual | Control::SemiAuto)
        {
            let motor = &mut self.motors[actor.control_slot.index()];
            *motor = (*motor).max(duration);
        }
    }

    pub fn motors(&self, paused: bool) -> [bool; 4] {
        self.motors.map(|remaining| !paused && remaining > 0)
    }
}

fn actor_effect(
    actor: resonance_battle::ActorId,
    target: &Actor,
    appearance: resonance_battle::EffectAppearance,
) -> EffectRequest {
    EffectRequest {
        owner: actor,
        target: actor,
        appearance,
        origin: target.effect_origin(),
        heading: target.heading,
        follow: None,
        scale: target.effect_scale,
        tint: Default::default(),
    }
}

fn attached_effect(
    actor: resonance_battle::ActorId,
    target: &Actor,
    appearance: resonance_battle::EffectAppearance,
) -> EffectRequest {
    EffectRequest {
        follow: Some(resonance_battle::EffectFollow::Center(actor)),
        ..actor_effect(actor, target, appearance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{BattleResult, CameraDefinition, PreparedBattle};

    #[test]
    fn resolved_battle_voices_reach_the_title_counter_without_audio_playback() -> anyhow::Result<()>
    {
        use resonance_battle::{ActionKey, CastPhase, EffectAppearance, Sound};
        use resonance_game::battle::feedback::{ActorFeedback, CastingFeedback};
        let mut battle = PreparedBattle::new(
            [Side::Party, Side::Enemy]
                .map(|side| {
                    (
                        crate::test_support::actor(side, 100, 20),
                        Default::default(),
                    )
                })
                .into(),
            Default::default(),
            1,
        )?
        .finish()?;
        let actor = battle.actor_ids().next().unwrap();
        let effect = EffectAppearance {
            resource: ResourceId(1),
            member: 1,
        };
        let mut feedback = Feedback::new(
            Resources {
                actors: vec![ActorFeedback {
                    taunt: Some(Sound::Stream(1)),
                    technique_command: Some(Sound::Stream(2)),
                    overlimit_voice: Some(Sound::Stream(3)),
                    ..Default::default()
                }],
                casting: [(
                    ActionKey(0),
                    CastingFeedback {
                        catalogue: resonance_battle::TechniqueId(75),
                        chant_voice: Some(Sound::Stream(4)),
                        release_voice: Some(Sound::Stream(5)),
                        start_sound: None,
                        release_sound: None,
                        chant_effect: effect,
                        charged_effect: effect,
                        stored_effect: effect,
                        release_effect: effect,
                        opening_effect: None,
                        tint: Default::default(),
                    },
                )]
                .into(),
                ..Default::default()
            },
            1,
        );
        for _ in 0..10 {
            let mut frame = battle.publish(vec![
                Cue::Taunted { actor },
                Cue::TechniqueQueued { actor },
                Cue::OverLimitEntered {
                    actor,
                    position: [0.; 3],
                },
                Cue::Casting {
                    actor,
                    action: ActionKey(0),
                    phase: CastPhase::Incantation,
                },
                Cue::Casting {
                    actor,
                    action: ActionKey(0),
                    phase: CastPhase::Released,
                },
            ]);
            frame.actors[actor.index()].activity =
                resonance_battle::Activity::Casting { held: false };
            let core_cues = frame.cues.len();
            feedback.resolve(&mut frame);
            battle.record_voice_requests(&frame.cues[core_cues..]);
        }
        assert_eq!(battle.ledger().achievements[0].voices, 50);
        assert_eq!(battle.ledger().achievements[1].voices, 0);
        Ok(())
    }

    #[test]
    fn result_advancement_uses_one_fanfare_for_all_awarded_members() -> anyhow::Result<()> {
        let battle = PreparedBattle::new(
            [Side::Party, Side::Party, Side::Enemy]
                .into_iter()
                .map(|side| {
                    (
                        crate::test_support::actor(side, 100, 20),
                        Default::default(),
                    )
                })
                .collect(),
            Default::default(),
            1,
        )?
        .finish()?;
        let effect = resonance_battle::EffectAppearance {
            resource: ResourceId(1),
            member: 20,
        };
        let sound = resonance_battle::Sound::Cue(resonance_game::battle::audio::RESULT_ADVANCEMENT);
        let mut feedback = Feedback::new(
            Resources {
                result_advancement: Some(effect),
                result_advancement_sound: Some(sound),
                ..Default::default()
            },
            1,
        );
        let mut frame = battle.snapshot();
        frame.cues = vec![Cue::ResultAdvancement {
            level_up: true,
            actors: vec![
                resonance_battle::ActorId::from_index(0)?,
                resonance_battle::ActorId::from_index(1)?,
            ],
        }];
        feedback.resolve(&mut frame);
        assert_eq!(
            frame
                .cues
                .iter()
                .filter(|cue| matches!(cue,
            Cue::Effect(request) if request.appearance == effect))
                .count(),
            2
        );
        let sounds: Vec<_> = frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Sound { .. }))
            .collect();
        assert_eq!(sounds.len(), 1);
        assert!(
            matches!(sounds[0], Cue::Sound { actor, sound: emitted, priority: 1, .. }
            if *actor == resonance_battle::ActorId::from_index(1)? && *emitted == sound)
        );
        Ok(())
    }

    #[test]
    fn confirmed_enemy_defeat_defers_its_attached_burst_to_contact_phase() -> anyhow::Result<()> {
        let battle = PreparedBattle::new(
            [Side::Party, Side::Enemy]
                .into_iter()
                .map(|side| {
                    (
                        crate::test_support::actor(side, 100, 20),
                        Default::default(),
                    )
                })
                .collect(),
            Default::default(),
            1,
        )?
        .finish()?;
        let actor = resonance_battle::ActorId::from_index(1)?;
        let appearance = resonance_battle::EffectAppearance {
            resource: ResourceId(1),
            member: 14,
        };
        let mut feedback = Feedback::new(
            Resources {
                death: Some(resonance_game::battle::death::Feedback {
                    appearance,
                    sound: None,
                    allies: Vec::new(),
                }),
                ..Default::default()
            },
            1,
        );
        let mut frame = battle.snapshot();
        let actors = frame.actors.clone();
        frame.cues.push(Cue::Defeated { actor });
        feedback.resolve(&mut frame);
        let effects: Vec<_> = frame
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Effect(_) | Cue::DeferredEffect(_)))
            .collect();
        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0], Cue::DeferredEffect(request)
            if request.owner == actor && request.appearance == appearance
            && request.follow == Some(resonance_battle::EffectFollow::Center(actor))));
        assert_eq!(frame.actors, actors);
        Ok(())
    }

    #[test]
    fn item_feedback_uses_committed_events_and_one_user_voice() -> anyhow::Result<()> {
        use resonance_battle::{EffectAppearance, Sound, item::Effect};
        use resonance_game::battle::items::{
            ActorFeedback as ItemActorFeedback, Feedback as ItemFeedback,
        };
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
            resource: ResourceId(1),
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
                    actors: vec![
                        ItemActorFeedback {
                            use_offset: [0.; 3],
                            used: Some(Sound::Stream(1)),
                            discovery: Some(Sound::Stream(2)),
                        },
                        ItemActorFeedback::default(),
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
                            catalogue: resonance_battle::TechniqueId(1),
                            player_range: [0., 120.],
                            ai_range: [0., 120.],
                            capabilities: Default::default(),
                            element: None,
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
                    events: vec![(
                        0,
                        resonance_battle::AttackEvent::Projectile(std::sync::Arc::new(
                            resonance_battle::ProjectileDefinition {
                                lifetime: Some(30),
                                ..Default::default()
                            },
                        )),
                    )],
                    ..PreparedAttack::new(10)
                }),
            }])
            .into(),
            1,
        )?
        .finish()?;
        let ids: Vec<_> = battle.actor_ids().collect();
        let admitted = battle.step(BattleInput::from(ActionRequest {
            actor: ids[0],
            target: ids[1],
            action: resonance_battle::ActionKey(0),
        }))?;
        let source = ContactSource::Melee {
            actor: ids[0],
            action: admitted.actions[0].0,
        };
        let base = battle.snapshot();
        let gameplay_random = battle.random_state();
        let effect = |member| EffectAppearance {
            resource: ResourceId(1),
            member,
        };
        let art = ContactArt {
            ordinary: effect(1),
            compact_impact: effect(6),
            guard: effect(2),
            repeated_guard: effect(8),
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
        feedback.resources.target_changed = Some(Sound::Cue(108));
        let mut entry = base.clone();
        feedback.resolve(&mut entry);
        assert!(entry.cues.iter().any(|cue| matches!(cue,
            Cue::Voice { actor, sound: Sound::Stream(90), centered: true, .. } if *actor == ids[0])));
        let mut next = base.clone();
        next.cues.push(Cue::TargetChanged {
            actor: ids[0],
            target: ids[1],
        });
        feedback.resolve(&mut next);
        assert!(!next.cues.iter().any(|cue| matches!(cue, Cue::Voice { .. })));
        assert!(next.cues.iter().any(|cue| matches!(
            cue,
            Cue::GlobalSound {
                sound: Sound::Cue(108),
                priority: 1
            }
        )));
        for (guard, was_casting, hp, critical, impacts, sound, voice) in [
            (
                GuardResult::None,
                true,
                100,
                false,
                &[7, 6][..],
                11,
                Some(4),
            ),
            (
                GuardResult::Blocked {
                    first: true,
                    special: false,
                },
                false,
                100,
                false,
                &[2][..],
                12,
                Some(3),
            ),
            (
                GuardResult::Broken,
                false,
                100,
                false,
                &[2, 4][..],
                13,
                Some(6),
            ),
            (GuardResult::None, false, 0, false, &[7, 6][..], 11, None),
            (GuardResult::None, false, 100, false, &[7, 6][..], 11, None),
            (
                GuardResult::None,
                false,
                100,
                true,
                &[7, 6, 3][..],
                11,
                Some(6),
            ),
        ] {
            let mut frame = base.clone();
            frame.actors[1].state.hp = hp;
            frame.cues = vec![Cue::Hit {
                source,
                owner: ids[0],
                actor: ids[1],
                position: [1., 2., 3.],
                element: Some(Element::Fire),
                contact_sound: None,
                suppress_guard_sound: false,
                was_casting,
                overlimit: false,
                stunned: false,
                repeated: false,
                result: HitResult {
                    amount: 10,
                    hp_change: -10,
                    critical,
                    boosted: false,
                    reaction_suppressed: false,
                    reaction_forced: false,
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
                    Cue::DeferredEffect(request) => Some(request),
                    _ => None,
                })
                .collect();
            assert_eq!(
                effects
                    .iter()
                    .map(|request| request.appearance)
                    .collect::<Vec<_>>(),
                impacts
                    .iter()
                    .map(|&member| effect(member))
                    .collect::<Vec<_>>()
            );
            for request in effects {
                if request.appearance == effect(4) {
                    assert_eq!(request.origin, frame.actors[1].effect_origin());
                    assert_eq!(
                        request.follow,
                        Some(resonance_battle::EffectFollow::Center(ids[1]))
                    );
                    assert_eq!(request.scale, frame.actors[1].effect_scale);
                } else {
                    assert_eq!(request.origin, [1., 2., 3.]);
                }
                assert_eq!(request.target, ids[1]);
                assert_eq!(
                    request.owner,
                    if request.appearance == effect(3) {
                        ids[0]
                    } else {
                        ids[1]
                    }
                );
            }
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
            if matches!(guard, GuardResult::Blocked { .. }) {
                for (authored, suppressed, expected) in [
                    (None, false, vec![Sound::Cue(12)]),
                    (None, true, vec![]),
                    (
                        Some(Sound::Cue(107)),
                        false,
                        vec![Sound::Cue(107), Sound::Cue(12)],
                    ),
                    (Some(Sound::Cue(107)), true, vec![Sound::Cue(107)]),
                ] {
                    let mut guarded = frame.clone();
                    guarded.cues.retain(|cue| matches!(cue, Cue::Hit { .. }));
                    let Cue::Hit {
                        contact_sound,
                        suppress_guard_sound,
                        ..
                    } = &mut guarded.cues[0]
                    else {
                        unreachable!()
                    };
                    *contact_sound = authored;
                    *suppress_guard_sound = suppressed;
                    feedback.contacts(&mut guarded);
                    let sounds: Vec<_> = guarded
                        .cues
                        .iter()
                        .filter_map(|cue| match cue {
                            Cue::Sound { sound, .. } => Some(*sound),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(sounds, expected);
                    assert_eq!(guarded.actors, actors);
                }
            }
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
        for kind in [
            resonance_battle::RecoveryKind::Hp,
            resonance_battle::RecoveryKind::Tp,
        ] {
            for nominal in [0, 10] {
                let mut capped = base.clone();
                capped.cues = vec![Cue::Recovered {
                    floating: true,
                    actor: ids[0],
                    kind,
                    nominal,
                    applied: 0,
                }];
                feedback.resolve(&mut capped);
                assert_eq!(
                    capped.cues.contains(&Cue::ActorFlash {
                        actor: ids[0],
                        color: [40, 112, 40],
                    }),
                    nominal > 0
                );
            }
        }
        feedback.resources.self_cure_notice = Some("Self Cure".into());
        feedback.resources.rescue_names[0] = "Angel's Tear".into();
        feedback
            .resources
            .rescues
            .push(resonance_game::battle::feedback::RescueFeedback {
                motion: Some(resonance_battle::MotionBinding {
                    model: ResourceId(1),
                    clip: 3,
                }),
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
                floating: true,
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
        assert!(frame.cues.contains(&Cue::ActorPulse {
            actor: ids[0],
            color: [10, 20, 30]
        }));
        assert!(frame.cues.contains(&Cue::ActorFlash {
            actor: ids[0],
            color: [40, 112, 40]
        }));
        assert!(frame.cues.iter().any(
            |cue| matches!(cue, Cue::EffectDuringPause(request) if request.appearance == effect(19))
        ));
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
                overlimit: Some(resonance_game::battle::feedback::OverLimitFeedback {
                    appearance: effect(45),
                    bone: 1,
                    scale: 1.25,
                }),
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
                catalogue: resonance_battle::TechniqueId(75),
                chant_voice: Some(Sound::Stream(26)),
                release_voice: Some(Sound::Stream(27)),
                start_sound: Some(Sound::Cue(109)),
                release_sound: Some(Sound::Cue(123)),
                chant_effect: effect(3),
                charged_effect: effect(6),
                stored_effect: effect(50),
                release_effect: effect(7),
                opening_effect: None,
                tint: Default::default(),
            },
        );
        for phase in [
            resonance_battle::CastPhase::Started,
            resonance_battle::CastPhase::Chanting,
            resonance_battle::CastPhase::Incantation,
        ] {
            let mut frame = base.clone();
            frame.actors[ids[0].index()].activity =
                resonance_battle::Activity::Casting { held: false };
            frame.cues = vec![Cue::Casting {
                actor: ids[0],
                action: resonance_battle::ActionKey(0),
                phase,
            }];
            feedback.resolve(&mut frame);
            assert_eq!(
                frame.cues.iter().any(|cue| matches!(
                    cue,
                    Cue::Voice {
                        sound: Sound::Stream(26),
                        ..
                    }
                )),
                phase == resonance_battle::CastPhase::Incantation
            );
            assert_eq!(
                frame.cues.iter().any(|cue| matches!(
                    cue,
                    Cue::Sound {
                        sound: Sound::Cue(109),
                        ..
                    }
                )),
                phase == resonance_battle::CastPhase::Started
            );
            assert_eq!(
                frame.cues.iter().any(|cue| matches!(cue, Cue::Effect(_))),
                phase == resonance_battle::CastPhase::Chanting
            );
        }
        let mut frame = base.clone();
        let hammer = frame.projectiles[0].id;
        frame.projectiles[0].position = [5., 80., 0.];
        frame.cues.extend([
            Cue::Taunted { actor: ids[0] },
            Cue::Backstepped { actor: ids[0] },
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
        assert!(frame.models.is_empty());
        assert!(
            !frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::EffectDuringPause(request)
            if request.appearance == effect(45)))
        );
        let identity = bevy::math::Mat4::IDENTITY.to_cols_array_2d();
        frame.models.push(resonance_battle::ModelFrame {
            actor: ids[0],
            visible: true,
            depth_write: true,
            tint: [128; 4],
            material: resonance_battle::ModelMaterial::Normal,
            texture_layers: [0; 4],
            texture_tick: 0,
            light: None,
            shadow: None,
            resource: ResourceId(1),
            clip: 0,
            frame: 0.,
            blend_weight: 0.,
            root_translation: [0.; 3],
            world: bevy::math::Mat4::from_translation(bevy::math::Vec3::new(10., 20., 30.))
                .to_cols_array_2d(),
            particle_world: bevy::math::Mat4::from_translation(bevy::math::Vec3::new(
                10., 20., 30.,
            ))
            .to_cols_array_2d(),
            bones: std::sync::Arc::new(vec![
                identity,
                bevy::math::Mat4::from_translation(bevy::math::Vec3::new(3., 4., 5.))
                    .to_cols_array_2d(),
            ]),
        });
        feedback.resolve_model_effects(&mut frame)?;
        assert!(
            frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::EffectDuringPause(request)
            if request.appearance == effect(45) && request.origin == [13., 24., 35.]
                && request.scale == 1.25 && request.follow.is_none()))
        );
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
        feedback
            .resources
            .casting
            .get_mut(&resonance_battle::ActionKey(0))
            .unwrap()
            .opening_effect = Some(effect(37));
        for phase in [
            resonance_battle::CastPhase::SceneOpening,
            resonance_battle::CastPhase::Released,
        ] {
            let mut frame = base.clone();
            frame.cues = vec![Cue::Casting {
                actor: ids[0],
                action: resonance_battle::ActionKey(0),
                phase,
            }];
            feedback.resolve(&mut frame);
            let opening = phase == resonance_battle::CastPhase::SceneOpening;
            assert_eq!(
                frame.cues.iter().any(|cue| matches!(cue,
                Cue::InitializedEffect(request) if request.appearance == effect(37))),
                opening
            );
            assert_eq!(
                frame.cues.iter().any(|cue| matches!(cue,
                Cue::Effect(request) if request.appearance == effect(7))),
                !opening
            );
            for count in [
                frame
                    .cues
                    .iter()
                    .filter(|cue| matches!(cue, Cue::Voice { .. }))
                    .count(),
                frame
                    .cues
                    .iter()
                    .filter(|cue| matches!(cue, Cue::Sound { .. }))
                    .count(),
                frame
                    .cues
                    .iter()
                    .filter(|cue| matches!(cue, Cue::Notice { .. }))
                    .count(),
            ] {
                assert_eq!(count, usize::from(opening));
            }
        }
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
            actor.control_slot = resonance_battle::ControllerPort::from_index(slot).unwrap();
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
                auto_zoom: false,
                yaw_limits: None,
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
        feedback.advance(&mut frame, false, &[]);
        assert_eq!(frame.camera, base.camera);
        for expected in [0., 4.] {
            frame = base.clone();
            feedback.advance(&mut frame, false, &[]);
            assert_eq!(
                frame.camera.unwrap().eye[1] - base.camera.unwrap().eye[1],
                expected
            );
        }
        assert_eq!(feedback.motors(false), [false, true, false, true]);
        assert_eq!(frame.actors, base.actors);
        assert_eq!(battle.snapshot(), base);
        let mut held = base.clone();
        feedback.advance(&mut held, true, &[]);
        assert_eq!(held.camera, frame.camera);
        assert_eq!(feedback.motors(true), [false; 4]);
        assert_eq!(feedback.motors(false), [false, true, false, true]);
        frame = base.clone();
        frame.cues.push(Cue::Shake {
            duration: 0,
            amplitude: 0,
        });
        feedback.advance(&mut frame, false, &[]);
        assert_eq!(
            frame.camera.unwrap().eye[1] - base.camera.unwrap().eye[1],
            4.
        );
        held = base.clone();
        feedback.advance(&mut held, true, &[]);
        assert_eq!(held.camera, frame.camera);
        for _ in 0..40 {
            frame = base.clone();
            feedback.advance(&mut frame, false, &[]);
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
        feedback.advance(&mut frame, false, &[]);
        assert_eq!(feedback.motors(false), [false, false, false, true]);
        assert_eq!(frame.camera, base.camera);
        frame = base.clone();
        frame.cues.push(Cue::Shake {
            duration: 120,
            amplitude: 100,
        });
        feedback.advance(&mut frame, false, &[]);
        assert_eq!(frame.camera, base.camera);
        for expected in [0., 50., 50., 0.].into_iter().cycle().take(120) {
            frame = base.clone();
            feedback.advance(&mut frame, false, &[]);
            assert_eq!(
                frame.camera.unwrap().eye[1] - base.camera.unwrap().eye[1],
                expected
            );
        }
        assert_eq!(frame, base);
        assert_eq!(feedback.motors(false), [false; 4]);

        frame.cues.push(Cue::Shake {
            duration: 12,
            amplitude: 8,
        });
        feedback.advance(&mut frame, false, &[]);
        frame = base.clone();
        frame.recognized_result = Some(BattleResult::Victory);
        feedback.advance(&mut frame, true, &[]);
        assert_eq!(frame.camera, base.camera);
        assert_eq!(feedback.motors(false), [false; 4]);
    }
}
