//! Optional enemy opening shots and final-enemy voice holds share the battle clock.
use crate::{
    Actor, ActorId, Battle, BattleClock, CameraPose, Cue, ResourceId, Side, UnisonFeedback,
};
use anyhow::{Result, ensure};
use resonance_content::battle_enemy::{
    Entrance, EntranceCue, Opening as OpeningDefinition, Presentation,
};

// Scene time is measured in fixed battle updates, independently of audio playback.
const OPENING_TAIL: u16 = 30;
const DEFEAT_DELAY: u8 = 10;
const DEFEAT_FEEDBACK_TAIL: u16 = 60;
const DISSOLVE_STEP: u8 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyPresentationFrame {
    pub hide_party: bool,
    pub hide_hud: bool,
    pub dissolve: u8,
    pub flash: Option<crate::TransitionFrame>,
    pub feedback: Option<UnisonFeedback>,
    pub letterbox: u16,
}

/// Prepared resources for an enemy's optional entrance and defeat presentation.
#[derive(Debug, Clone)]
pub struct EnemyPresentation {
    pub actor: ActorId,
    pub definition: Presentation,
    pub model: ResourceId,
    pub effects: ResourceId,
    pub speaker: Option<ActorId>,
    pub speaker_model: Option<ResourceId>,
}

#[derive(Clone)]
struct Opening {
    actor: ActorId,
    definition: OpeningDefinition,
    model: ResourceId,
    effects: ResourceId,
    anchor: [f32; 3],
    speaker: Option<ActorId>,
    speaker_model: Option<ResourceId>,
    party_anchor: [f32; 3],
    age: u16,
}

impl Opening {
    fn flash(&self) -> Option<crate::TransitionFrame> {
        let OpeningDefinition::Entrance(entrance) = &self.definition else {
            return None;
        };
        let flash = entrance.flash?;
        let age = self.age.checked_sub(flash.tick)?;
        let rise = 255_u16.div_ceil(u16::from(flash.step));
        // Completing the rise takes one update before the authored hold begins.
        let plateau = rise + 1 + u16::from(flash.hold);
        let alpha = if age <= rise {
            (age * u16::from(flash.step)).min(255) as u8
        } else if age <= plateau {
            255
        } else {
            let fall = age - plateau;
            if fall > rise {
                return None;
            }
            255_u16.saturating_sub(fall * u16::from(flash.step)) as u8
        };
        Some(crate::TransitionFrame {
            color: flash.color,
            alpha,
        })
    }
}

#[derive(Clone)]
struct Enemy {
    actor: ActorId,
    voice_ticks: u16,
    disabled_ticks: u8,
    finished: bool,
}

#[derive(Clone)]
enum Hold {
    Voice(u16),
    Release,
}

#[derive(Default, Clone)]
pub(crate) struct State {
    enemies: Vec<Enemy>,
    opening: Option<Opening>,
    hold: Option<Hold>,
    dissolve: u8,
    feedback: Option<UnisonFeedback>,
    generation: u64,
}

impl State {
    pub(crate) fn new(
        actors: &mut [Actor],
        definitions: &[EnemyPresentation],
        entry: bool,
    ) -> Self {
        let first_enemy = actors.iter().position(|actor| actor.side == Side::Enemy);
        let opening = definitions.iter().find_map(|prepared| {
            let definition = (entry && Some(prepared.actor.index()) == first_enemy)
                .then_some(prepared.definition.opening.as_ref())
                .flatten()?;
            let mut anchor = actors[prepared.actor.index()].effect_origin();
            if let OpeningDefinition::Entrance(entrance) = definition
                && let Some(staging) = &entrance.staging
            {
                let speaker = prepared.speaker.expect("prepared entrance speaker");
                actors[speaker.index()].position = staging.party_positions[0];
                let mut positions = staging.party_positions.iter().skip(1);
                for (index, actor) in actors.iter_mut().enumerate() {
                    if actor.side == Side::Party && index != speaker.index() {
                        actor.position = *positions.next().expect("prepared entrance positions");
                    }
                }
                actors[prepared.actor.index()].position = staging.enemy_position;
                anchor = staging.enemy_position;
            }
            Some(Opening {
                actor: prepared.actor,
                definition: definition.clone(),
                model: prepared.model,
                effects: prepared.effects,
                anchor,
                speaker: prepared.speaker,
                speaker_model: prepared.speaker_model,
                party_anchor: prepared
                    .speaker
                    .map_or([0.; 3], |id| actors[id.index()].position),
                age: 0,
            })
        });
        Self {
            opening,
            enemies: definitions
                .iter()
                .map(|prepared| Enemy {
                    actor: prepared.actor,
                    voice_ticks: prepared.definition.defeat.ticks,
                    disabled_ticks: 0,
                    finished: false,
                })
                .collect(),
            ..Default::default()
        }
    }

    pub(crate) fn opening(&self) -> bool {
        self.opening.is_some()
    }

    pub(crate) fn holding(&self) -> bool {
        self.opening() || self.hold.is_some()
    }

    pub(crate) fn delays_result(&self, actors: &[Actor]) -> bool {
        self.holding()
            || (!actors
                .iter()
                .any(|actor| actor.side == Side::Enemy && actor.available())
                && self.enemies.iter().any(|enemy| !enemy.finished))
    }

    pub(crate) fn camera(&self) -> Option<CameraPose> {
        self.opening.as_ref().map(|opening| {
            let mut focus = opening.anchor;
            let (eye, yaw, horizontal) = match &opening.definition {
                OpeningDefinition::Voice(_) => {
                    focus[1] += -20. + 0.1 * f32::from(opening.age);
                    (
                        [focus[0] - 1050., 1500. * 5_f32.to_radians().sin(), focus[2]],
                        180.,
                        1050.,
                    )
                }
                OpeningDefinition::Entrance(entrance) => {
                    let shot = entrance.staging.as_ref().and_then(|stage| {
                        stage.shots.iter().rev().find(|(at, _)| *at <= opening.age)
                    });
                    let (camera, age, lateral, vertical) = if let Some((at, shot)) = shot {
                        if shot.party {
                            focus = opening.party_anchor;
                        }
                        let age = f32::from(opening.age.saturating_sub(*at));
                        (
                            shot.camera,
                            age,
                            shot.lateral + shot.lateral_step * age,
                            shot.vertical + shot.vertical_step * age,
                        )
                    } else {
                        (
                            entrance.camera,
                            f32::from(opening.age.saturating_sub(1)),
                            0.,
                            0.,
                        )
                    };
                    focus[2] += lateral;
                    let yaw = (camera.yaw + camera.yaw_step * age)
                        .min(camera.maximum_yaw.unwrap_or(f32::INFINITY));
                    let elevation = (camera.elevation + camera.elevation_step * age)
                        .max(camera.minimum_elevation);
                    focus[1] = camera.focus_height + camera.focus_height_step * age + vertical;
                    (
                        [
                            focus[0] + camera.radius * yaw.to_radians().cos(),
                            camera.height_radius * elevation.to_radians().sin() + vertical,
                            focus[2] + camera.radius * yaw.to_radians().sin(),
                        ],
                        yaw,
                        camera.radius,
                    )
                }
            };
            let vertical = eye[1] - focus[1];
            CameraPose {
                eye,
                focus,
                pitch: vertical.atan2(horizontal).to_degrees(),
                yaw,
                radius: horizontal.hypot(vertical),
            }
        })
    }

    pub(crate) fn frame(&self) -> Option<EnemyPresentationFrame> {
        let (hide_party, hide_hud, letterbox) =
            self.opening
                .as_ref()
                .map_or((false, false, 0), |opening| match &opening.definition {
                    OpeningDefinition::Entrance(entrance) => (
                        entrance
                            .staging
                            .as_ref()
                            .map_or(!entrance.show_party, |staging| {
                                staging.hide_party_at.is_some_and(|at| opening.age >= at)
                            }),
                        !entrance.show_hud,
                        entrance.letterbox.min(opening.age),
                    ),
                    OpeningDefinition::Voice(_) => (true, true, 0),
                });
        (!self.enemies.is_empty()).then_some(EnemyPresentationFrame {
            hide_party,
            hide_hud,
            dissolve: self.dissolve,
            flash: self.opening.as_ref().and_then(Opening::flash),
            feedback: self.feedback,
            letterbox,
        })
    }

    fn feedback(&mut self, remaining: u16, limit: u8, alpha: u8) {
        self.generation += 1;
        self.feedback = Some(UnisonFeedback {
            generation: self.generation,
            expansion: self.feedback.map_or(0, |feedback| feedback.expansion),
            alpha,
            limit,
            remaining,
        });
    }

    pub(crate) fn victory(&mut self) {
        if !self.enemies.is_empty() {
            self.feedback(45, 8, 128);
        }
    }

    fn tick_feedback(&mut self) {
        self.dissolve = self.dissolve.saturating_sub(DISSOLVE_STEP);
        if let Some(feedback) = &mut self.feedback {
            if feedback.remaining != 0 {
                feedback.remaining -= 1;
                if feedback.expansion < feedback.limit {
                    feedback.expansion += 1;
                }
            } else {
                feedback.expansion = feedback.expansion.saturating_sub(1);
                if feedback.expansion == 0 {
                    self.feedback = None;
                }
            }
        }
    }
}

impl crate::PreparedBattle {
    pub fn with_enemy_presentations(mut self, definitions: Vec<EnemyPresentation>) -> Result<Self> {
        let mut actors = std::collections::BTreeSet::new();
        for prepared in &definitions {
            let actor = prepared.actor;
            let definition = &prepared.definition;
            ensure!(
                self.actors
                    .get(actor.index())
                    .is_some_and(|actor| actor.side == Side::Enemy)
                    && actors.insert(actor),
                "enemy presentation requires distinct enemy actors"
            );
            ensure!(
                definition.defeat.ticks <= u16::MAX - DEFEAT_FEEDBACK_TAIL
                    && definition
                        .opening
                        .as_ref()
                        .is_none_or(|opening| match opening {
                            OpeningDefinition::Voice(voice) =>
                                voice.ticks < u16::MAX - OPENING_TAIL,
                            OpeningDefinition::Entrance(entrance) =>
                                entrance.ticks > 0
                                    && entrance.ticks < u16::MAX
                                    && entrance.gravity.is_finite()
                                    && entrance.flash.is_none_or(|flash| flash.step != 0),
                        }),
                "invalid enemy presentation timing or motion"
            );
            if let Some(OpeningDefinition::Entrance(entrance)) = &definition.opening
                && let Some(staging) = &entrance.staging
            {
                ensure!(
                    prepared.speaker.is_some_and(|speaker| self
                        .actors
                        .get(speaker.index())
                        .is_some_and(|actor| actor.side == Side::Party)),
                    "entrance conversation requires its party speaker"
                );
                ensure!(
                    staging.party_positions.len()
                        >= self
                            .actors
                            .iter()
                            .filter(|actor| actor.side == Side::Party)
                            .count()
                        && staging
                            .party_positions
                            .iter()
                            .flatten()
                            .chain(staging.enemy_position.iter())
                            .all(|coordinate| coordinate.is_finite())
                        && staging.shots.first().is_some_and(|(at, _)| *at == 0)
                        && staging
                            .shots
                            .windows(2)
                            .all(|shots| shots[0].0 < shots[1].0)
                        && staging.shots.iter().all(|(at, _)| *at < entrance.ticks),
                    "invalid entrance conversation staging"
                );
            }
        }
        self.resources.enemy_presentations = definitions;
        Ok(self)
    }
}

impl Battle {
    fn restore_combat_camera(&mut self) -> Result<()> {
        if let Some(camera) = &mut self.camera {
            *camera = crate::camera::Camera::new(
                camera.definition.clone(),
                &self.actors,
                self.runtime[camera.definition.leader.index()].target,
            )?;
        }
        Ok(())
    }

    fn advance_entrance(&mut self, opening: &Opening, entrance: &Entrance, cues: &mut Vec<Cue>) {
        let id = opening.actor;
        if opening.age == 0 && entrance.staging.is_some() {
            let speaker = opening.speaker.expect("prepared entrance speaker");
            self.model_requests.push(crate::ModelRequest::Common {
                actor: speaker,
                pose: crate::CommonPose::Idle,
            });
        }
        let actor = &mut self.actors[id.index()];
        if opening.age == 0 {
            actor.movement.gravity = entrance.gravity;
        }
        for &(age, vertical) in &entrance.launches {
            if age == opening.age {
                actor.movement.vertical = vertical;
            }
        }
        actor.movement.integrate(&mut actor.position);
        crate::movement::floor(actor);
        for &(_, event) in entrance
            .cues
            .iter()
            .filter(|(tick, _)| *tick == opening.age + 1)
        {
            match event {
                EntranceCue::Height(height) => actor.position[1] = height,
                EntranceCue::Vertical(vertical) => actor.movement.vertical = vertical,
                EntranceCue::Pose {
                    clip,
                    rate,
                    repeat,
                    blend,
                } => self.model_requests.push(crate::ModelRequest::Play {
                    actor: id,
                    motion: crate::MotionBinding {
                        model: opening.model,
                        clip,
                    },
                    pose: crate::Pose {
                        rate,
                        repeat,
                        blend,
                        ..Default::default()
                    },
                }),
                EntranceCue::StageTint {
                    color,
                    duration,
                    step,
                } => {
                    cues.push(Cue::InitializedStageTint {
                        tint: crate::StageTint { color, duration },
                        step,
                    });
                }
                EntranceCue::Feedback {
                    duration,
                    limit,
                    alpha,
                } => {
                    self.enemy_presentation
                        .feedback(duration.saturating_sub(1), limit, alpha);
                    if let Some(feedback) = &mut self.enemy_presentation.feedback {
                        feedback.expansion = 1.min(limit);
                    }
                }
                EntranceCue::Expression(expression) => {
                    self.model_requests.push(crate::ModelRequest::Expression {
                        actor: id,
                        layers: [expression, 0, 0, 0],
                    })
                }
                EntranceCue::ActiveExpression => self
                    .model_requests
                    .push(crate::ModelRequest::ActiveExpression { actor: id }),
                EntranceCue::Effect(member) => {
                    cues.push(Cue::InitializedEffect(crate::EffectRequest {
                        owner: id,
                        target: id,
                        appearance: crate::EffectAppearance {
                            resource: opening.effects,
                            member,
                        },
                        origin: actor.position,
                        heading: actor.heading,
                        follow: Some(crate::EffectFollow::Actor(id)),
                        scale: actor.effect_scale,
                        tint: Default::default(),
                    }))
                }
                EntranceCue::Voice(sound) => cues.push(Cue::Voice {
                    actor: id,
                    sound,
                    priority: crate::VoicePriority::Announcement,
                    position: actor.audio_position(),
                    centered: true,
                }),
                EntranceCue::PartyVoice(sound) => {
                    let speaker = opening.speaker.expect("prepared entrance speaker");
                    cues.push(Cue::Voice {
                        actor: speaker,
                        sound,
                        priority: crate::VoicePriority::Announcement,
                        position: opening.party_anchor,
                        centered: true,
                    });
                }
                EntranceCue::PartyActiveExpression => {
                    self.model_requests
                        .push(crate::ModelRequest::ActiveExpression {
                            actor: opening.speaker.expect("prepared entrance speaker"),
                        })
                }
                EntranceCue::PartyPose {
                    clip,
                    rate,
                    repeat,
                    blend,
                } => {
                    self.model_requests.push(crate::ModelRequest::Play {
                        actor: opening.speaker.expect("prepared entrance speaker"),
                        motion: crate::MotionBinding {
                            model: opening
                                .speaker_model
                                .expect("prepared entrance speaker model"),
                            clip,
                        },
                        pose: crate::Pose {
                            rate,
                            repeat,
                            blend,
                            ..Default::default()
                        },
                    });
                }
                EntranceCue::Dissolve => self.enemy_presentation.dissolve = u8::MAX,
                EntranceCue::Sound(sound) => cues.push(Cue::Sound {
                    actor: id,
                    sound,
                    position: actor.position,
                    priority: 2,
                }),
                EntranceCue::Shake {
                    duration,
                    amplitude,
                } => cues.push(Cue::Shake {
                    duration,
                    amplitude,
                }),
            }
        }
        self.publish_landing(id.index(), cues);
    }

    /// Returns true only when this update belongs to the opening or defeat hold.
    pub(crate) fn advance_enemy_presentation(&mut self, cues: &mut Vec<Cue>) -> Result<bool> {
        self.enemy_presentation.tick_feedback();
        if self
            .enemy_presentation
            .opening
            .as_ref()
            .is_some_and(|opening| matches!(&opening.definition, OpeningDefinition::Entrance(entrance) if opening.age < entrance.ticks))
        {
            for index in 0..self.actors.len() {
                self.advance_actor_common(index, cues)?;
            }
        }
        if let Some(mut opening) = self.enemy_presentation.opening.take() {
            self.clock = BattleClock::Running;
            let end = match &opening.definition {
                OpeningDefinition::Voice(voice) => {
                    if opening.age == voice.ticks + OPENING_TAIL {
                        self.restore_combat_camera()?;
                        return Ok(false);
                    }
                    if opening.age == 0
                        && let Some(sound) = voice.sound
                    {
                        cues.push(Cue::Voice {
                            actor: opening.actor,
                            sound,
                            priority: crate::VoicePriority::Announcement,
                            position: self.actors[opening.actor.index()].audio_position(),
                            centered: true,
                        });
                    }
                    voice.ticks + OPENING_TAIL
                }
                OpeningDefinition::Entrance(entrance) => {
                    if opening.age == entrance.ticks {
                        if let Some(speaker) = opening.speaker {
                            self.model_requests.push(crate::ModelRequest::Common {
                                actor: speaker,
                                pose: crate::CommonPose::Idle,
                            });
                        }
                        self.restore_combat_camera()?;
                        return Ok(false);
                    }
                    self.advance_entrance(&opening, entrance, cues);
                    entrance.ticks
                }
            };
            opening.age += 1;
            if opening.age == end && matches!(opening.definition, OpeningDefinition::Voice(_)) {
                self.enemy_presentation.dissolve = u8::MAX;
            }
            if opening.age <= end {
                self.enemy_presentation.opening = Some(opening);
            } else {
                self.restore_combat_camera()?;
            }
            return Ok(true);
        }
        if let Some(hold) = self.enemy_presentation.hold.take() {
            self.clock = BattleClock::Held;
            self.enemy_presentation.hold = match hold {
                Hold::Voice(0) => Some(Hold::Release),
                Hold::Voice(ticks) => Some(Hold::Voice(ticks - 1)),
                Hold::Release => {
                    self.clock = BattleClock::Running;
                    None
                }
            };
            return Ok(true);
        }
        let active = self
            .actors
            .iter()
            .any(|actor| actor.side == Side::Enemy && actor.available());
        let mut hold_ticks: Option<u16> = None;
        for enemy in &mut self.enemy_presentation.enemies {
            if enemy.finished || self.actors[enemy.actor.index()].available() {
                continue;
            }
            if active {
                enemy.finished = true;
            } else if enemy.disabled_ticks < DEFEAT_DELAY {
                enemy.disabled_ticks += 1;
            } else {
                enemy.finished = true;
                hold_ticks = Some(hold_ticks.unwrap_or(0).max(enemy.voice_ticks));
            }
        }
        if let Some(ticks) = hold_ticks {
            self.enemy_presentation.hold = Some(Hold::Voice(ticks));
            self.enemy_presentation
                .feedback(ticks + DEFEAT_FEEDBACK_TAIL, 2, 176);
            self.clock = BattleClock::Held;
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActorAvailability, ActorSetup, BattleInput, BattlePhase, BattleResult, PreparedBattle,
        Sound,
    };
    use resonance_content::battle_voice::TimedVoice;

    #[test]
    fn staged_opening_plays_both_speakers_before_starting_combat() -> Result<()> {
        use resonance_content::battle_enemy::{EntranceCamera, EntranceShot, EntranceStaging};

        let camera = EntranceCamera {
            radius: 1150.,
            height_radius: 1500.,
            focus_height: 75.,
            focus_height_step: 0.,
            yaw: -100.,
            yaw_step: 0.,
            maximum_yaw: None,
            elevation: 5.,
            elevation_step: 0.,
            minimum_elevation: 5.,
        };
        let entrance = Entrance {
            ticks: 715,
            gravity: 0.,
            letterbox: 0,
            show_party: false,
            show_hud: false,
            flash: None,
            launches: vec![],
            cues: vec![
                (25, EntranceCue::Voice(Sound::Stream(2047))),
                (300, EntranceCue::Dissolve),
                (320, EntranceCue::PartyVoice(Sound::Stream(2048))),
                (320, EntranceCue::PartyActiveExpression),
                (530, EntranceCue::Voice(Sound::Stream(2049))),
                (530, EntranceCue::ActiveExpression),
                (550, EntranceCue::Dissolve),
            ],
            camera,
            staging: Some(EntranceStaging {
                character: 1,
                party_positions: vec![[-300., 0., 0.], [-450., 0., 200.]],
                enemy_position: [300., 0., 0.],
                hide_party_at: Some(550),
                fallback: None,
                shots: vec![
                    (
                        0,
                        EntranceShot {
                            party: true,
                            camera,
                            lateral: 20.,
                            lateral_step: -0.25,
                            vertical: 0.,
                            vertical_step: 0.,
                        },
                    ),
                    (
                        301,
                        EntranceShot {
                            party: false,
                            camera,
                            lateral: 20.,
                            lateral_step: -0.25,
                            vertical: 0.,
                            vertical_step: 0.,
                        },
                    ),
                    (
                        551,
                        EntranceShot {
                            party: false,
                            camera,
                            lateral: 0.,
                            lateral_step: 0.,
                            vertical: 0.,
                            vertical_step: 0.,
                        },
                    ),
                ],
            }),
        };
        // The speaker is not necessarily the controlled or first party member.
        let mut battle = PreparedBattle::new(
            vec![
                (crate::tests::actor(Side::Party), ActorSetup::default()),
                (crate::tests::actor(Side::Party), ActorSetup::default()),
                (crate::tests::actor(Side::Enemy), ActorSetup::default()),
            ],
            Default::default(),
            1,
        )?
        .with_enemy_presentations(vec![EnemyPresentation {
            actor: ActorId(2),
            definition: Presentation {
                opening: Some(OpeningDefinition::Entrance(entrance)),
                defeat: TimedVoice {
                    sound: None,
                    ticks: 0,
                },
            },
            model: ResourceId(0),
            effects: ResourceId(0),
            speaker: Some(ActorId(1)),
            speaker_model: None,
        }])?
        .with_entry()
        .finish()?;
        let definitions = [[7, 9, 0, 0], [4, 6, 0, 0], [8, 5, 0, 0]]
            .map(|expression| {
                let mut model = crate::tests::model_definition(0, [(0, 60.)]);
                model.active_expression = expression;
                Some(std::sync::Arc::new(model))
            })
            .into();
        battle.set_models(crate::Models::new(
            battle.actors(),
            definitions,
            Default::default(),
            Default::default(),
        )?);
        let initial = battle.snapshot();
        assert_eq!(initial.actors[0].position, [-450., 0., 200.]);
        assert_eq!(initial.actors[1].position, [-300., 0., 0.]);
        assert_eq!(initial.actors[2].position, [300., 0., 0.]);
        assert_eq!(initial.camera.unwrap().focus, [-300., 75., 20.]);
        let mut voices = Vec::new();
        for update in 1..=716 {
            let mut frame = battle.step(BattleInput::default())?;
            battle.advance_models(&mut frame, crate::BattleClock::Running, true)?;
            if update == 320 || update == 530 {
                assert_eq!(frame.models[0].texture_layers, [0; 4]);
                assert_eq!(frame.models[1].texture_layers, [4, 6, 0, 0]);
                assert_eq!(
                    frame.models[2].texture_layers,
                    if update == 530 { [8, 5, 0, 0] } else { [0; 4] }
                );
            }
            for cue in &frame.cues {
                if let Cue::Voice {
                    actor,
                    sound,
                    centered: true,
                    ..
                } = cue
                {
                    voices.push((update, *actor, *sound));
                }
            }
            let presentation = frame.enemy_presentation.unwrap();
            assert_eq!(frame.combat_active, update == 716);
            assert_eq!(battle.command_admission_allowed(), update == 716);
            assert_eq!(presentation.hide_hud, update <= 715);
            assert_eq!(presentation.hide_party, (550..=715).contains(&update));
            if update == 300 || update == 550 {
                assert_eq!(presentation.dissolve, 255);
            }
            if update == 301 {
                assert_eq!(frame.camera.unwrap().focus, [300., 75., 20.]);
            }
            if update == 551 {
                assert_eq!(frame.camera.unwrap().focus, [300., 75., 0.]);
            }
            if update == 320 {
                let paused = battle.step(BattleInput {
                    paused: true,
                    ..Default::default()
                })?;
                assert_eq!(paused.update, frame.update);
                assert!(paused.cues.is_empty());
            }
        }
        assert_eq!(
            voices,
            vec![
                (25, ActorId(2), Sound::Stream(2047)),
                (320, ActorId(1), Sound::Stream(2048)),
                (530, ActorId(2), Sound::Stream(2049)),
            ]
        );
        Ok(())
    }

    #[test]
    fn enemy_opening_and_final_voice_hold_pause_and_finish_once() -> Result<()> {
        let definition = Presentation {
            opening: Some(OpeningDefinition::Voice(TimedVoice {
                sound: Some(Sound::Stream(7)),
                ticks: 2,
            })),
            defeat: TimedVoice {
                sound: Some(Sound::Stream(8)),
                ticks: 3,
            },
        };
        let prepared = |companions| -> Result<PreparedBattle> {
            let mut actors = vec![
                (crate::tests::actor(Side::Party), ActorSetup::default()),
                (crate::tests::actor(Side::Enemy), ActorSetup::default()),
            ];
            if companions {
                actors.push((crate::tests::actor(Side::Enemy), ActorSetup::default()));
            }
            PreparedBattle::new(actors, Default::default(), 1)?.with_enemy_presentations(vec![
                EnemyPresentation {
                    actor: ActorId(1),
                    definition: definition.clone(),
                    model: ResourceId(0),
                    effects: ResourceId(0),
                    speaker: None,
                    speaker_model: None,
                },
            ])
        };
        let mut battle = prepared(false)?.with_entry().finish()?;
        assert_eq!(battle.phase(), BattlePhase::Entry);
        assert!(battle.snapshot().enemy_presentation.unwrap().hide_party);
        let paused = battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(paused.update, 0);
        assert!(paused.cues.is_empty());
        let first = battle.step(BattleInput::default())?;
        assert_eq!(
            first
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Voice { centered: true, .. }))
                .count(),
            1
        );
        for update in 2..=2 + OPENING_TAIL + 1 {
            let frame = battle.step(BattleInput::default())?;
            assert_eq!(frame.combat_active, update == 2 + OPENING_TAIL + 1);
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Voice { .. }))
            );
        }
        assert_eq!(battle.phase(), BattlePhase::Combat);
        assert!(!battle.snapshot().enemy_presentation.unwrap().hide_party);
        battle.actors[1].availability = ActorAvailability::Dead;
        battle.actors[1].hp = 0;
        for _ in 0..=DEFEAT_DELAY {
            assert_eq!(battle.recognize_result(), None);
            battle.step(BattleInput::default())?;
        }
        assert!(!battle.command_admission_allowed());
        assert!(battle.is_paused());
        let before = battle.snapshot().enemy_presentation;
        battle.step(BattleInput {
            paused: true,
            ..Default::default()
        })?;
        assert_eq!(battle.snapshot().enemy_presentation, before);
        for _ in 0..definition.defeat.ticks + 2 {
            assert_eq!(battle.recognize_result(), None);
            let frame = battle.step(BattleInput::default())?;
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Voice { .. }))
            );
        }
        assert_eq!(battle.recognize_result(), Some(BattleResult::Victory));

        // A defeated boss with a surviving companion does not delay a later victory.
        let mut battle = prepared(true)?.finish()?;
        battle.actors[1].availability = ActorAvailability::Dead;
        battle.actors[1].hp = 0;
        battle.step(BattleInput::default())?;
        battle.actors[2].availability = ActorAvailability::Dead;
        battle.actors[2].hp = 0;
        assert_eq!(battle.recognize_result(), Some(BattleResult::Victory));

        // Launches precede movement; landing presentation can start a later bounce.
        let entrance = Entrance {
            ticks: 300,
            gravity: crate::movement::GRAVITY,
            letterbox: 0,
            show_party: true,
            show_hud: true,
            flash: None,
            launches: vec![(0, 18.), (15, 18.)],
            cues: vec![
                (1, EntranceCue::Height(1000.)),
                (86, EntranceCue::Vertical(14.)),
            ],
            camera: resonance_content::battle_enemy::EntranceCamera {
                radius: 1750.,
                height_radius: 1200.,
                focus_height: 120.,
                focus_height_step: 0.,
                yaw: 180.,
                yaw_step: 0.01,
                maximum_yaw: None,
                elevation: 20.,
                elevation_step: -0.01,
                minimum_elevation: 16.5,
            },
            staging: None,
        };
        let mut falling = prepared(false)?;
        falling.resources.enemy_presentations[0].definition.opening =
            Some(OpeningDefinition::Entrance(entrance));
        let mut battle = falling.with_entry().finish()?;
        for update in 1..=301 {
            let frame = battle.step(BattleInput::default())?;
            let presentation = frame.enemy_presentation.as_ref().unwrap();
            assert!(!presentation.hide_party && !presentation.hide_hud);
            let expected = match update {
                1 => Some(1000.),
                41 => Some(1290.),
                42 => Some(1282.),
                85 | 86 | 115 | 241 => Some(0.),
                87 => Some(14.),
                100 => Some(105.),
                _ => None,
            };
            if let Some(height) = expected {
                assert_eq!(
                    frame.actors[1].position[1], height,
                    "entrance update {update}"
                );
            }
            assert_eq!(
                battle.phase(),
                if update <= 300 {
                    BattlePhase::Entry
                } else {
                    BattlePhase::Combat
                }
            );
            assert_eq!(frame.combat_active, update > 300);
            if update == 42 {
                let paused = battle.step(BattleInput {
                    paused: true,
                    ..Default::default()
                })?;
                assert_eq!(paused.update, frame.update);
                assert_eq!(paused.actors[1].position, frame.actors[1].position);
            }
        }
        Ok(())
    }
}
