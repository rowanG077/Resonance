//! Field service bindings. Scene setup supplies all asset and event mappings.
use super::{NativeHost, NativeResult, require};
use crate::{
    Actor, Animation, ResourceKind,
    animation::slot,
    camera::CameraRig,
    world::{Attachment, AudioCommand, BoneAdjustment, Emote, EventRecord, Face, Trigger},
};
use symphonia_script::NativeCall;
use symphonia_script_vm::Memory;

const MAIN_SCENERY: i32 = 999_996;
const SECOND_SCENERY: i32 = 999_997;
const THIRD_SCENERY: i32 = 999_998;

#[derive(Clone, Copy)]
#[repr(i32)]
enum FieldSystemCommand {
    FieldLeader = 13,
    SetFieldLeader = 14,
    SuppressTransitionFade = 15,
    BattleCount = 17,
    SetDoorInteractionRadius = 19,
}

impl TryFrom<i32> for FieldSystemCommand {
    type Error = &'static str;

    fn try_from(id: i32) -> Result<Self, Self::Error> {
        match id {
            13 => Ok(Self::FieldLeader),
            14 => Ok(Self::SetFieldLeader),
            15 => Ok(Self::SuppressTransitionFade),
            17 => Ok(Self::BattleCount),
            19 => Ok(Self::SetDoorInteractionRadius),
            _ => Err("field system command is not implemented"),
        }
    }
}

impl NativeHost<'_> {
    pub(super) fn field(
        &mut self,
        op: NativeCall,
        a: &[i32],
        _memory: &mut Memory,
    ) -> Result<NativeResult, String> {
        let mut value = None;
        match op {
            NativeCall::PlayActorSound => {
                require(
                    self.world.audio_commands.len() < 512,
                    "audio command queue is not being consumed",
                )?;
                let actor = if a[0] == crate::CONTROLLED_ACTOR {
                    self.world.controlled_actor
                } else {
                    a[0]
                };
                self.world.actor_sound(
                    actor,
                    crate::AmbientSound {
                        id: a[1] as i16,
                        volume: if a[2] as u8 == 255 { 127 } else { a[2] as u8 },
                        radius: a[3] as f32,
                    },
                );
            }
            NativeCall::ReadMappedInput => {
                value = Some(
                    self.world
                        .input
                        .read(a[0], a[1], self.world.mapped_input_disabled),
                );
            }
            NativeCall::SetPlayerSize => {
                self.world.player_size = if a[0] as u8 == 0 {
                    crate::world::PlayerSize::Normal
                } else {
                    crate::world::PlayerSize::Small
                };
            }
            NativeCall::RumbleController => {
                self.world.rumble = Some(crate::rumble::Rumble::new(
                    a[0],
                    a[1],
                    a[2] != 0,
                    self.world.tick,
                )?);
            }
            NativeCall::GetCurrentField => {
                value = Some(self.world.current_field.ok_or("field owner is missing")? as i32);
            }
            NativeCall::SetActorPathPoint => {
                if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                    require(
                        (0..12).contains(&a[1]),
                        "actor path point exceeds native capacity",
                    )?;
                    let path = &mut actor.path;
                    path.points[a[1] as usize] = [a[2] as f32, a[3] as f32, a[4] as f32];
                    path.count = a[1] as u8 + 1;
                    if a[5] as i8 != -1 {
                        path.reverse_at_end = a[5] as i8 != 0;
                    }
                }
            }
            NativeCall::SetTreasureModel => {
                if (3..=4).contains(&a[0]) {
                    self.world.treasure_models[(a[0] - 3) as usize] =
                        Some(self.resolve(a[1], ResourceKind::Model)?);
                }
            }
            NativeCall::CreateTreasureChest => {
                if !(0..1024).contains(&a[0]) {
                    return Ok(NativeResult::Continue(None));
                }
                require(
                    self.world.treasures.len() < 1024,
                    "field treasure limit exceeded",
                )?;
                use crate::TreasureKind;
                let kind = TreasureKind::try_from(a[2]).unwrap_or(TreasureKind::UnknownId0);
                let resource = match kind {
                    TreasureKind::CustomModel0 | TreasureKind::CustomModel1 => {
                        let index = usize::from(kind as u8 - TreasureKind::CustomModel0 as u8);
                        self.world.treasure_models[index]
                            .unwrap_or(resonance_content::field::TREASURE_RESOURCE_BASE)
                    }
                    _ => resonance_content::field::TREASURE_RESOURCE_BASE + u32::from(kind as u8),
                };
                let model = self
                    .resources
                    .model(resource)
                    .ok_or("treasure model is not cooked")?;
                let clip = model
                    .clips
                    .get(&slot::IDLE)
                    .ok_or("treasure opening animation is not cooked")?;
                let id = i32::MIN + 1024 + self.world.treasures.len() as i32;
                let mut actor = Actor::new(resource, [a[3] as f32, a[4] as f32, a[5] as f32]);
                actor.radius = 50.;
                actor.face(a[6] as f32);
                actor.grounded = false;
                actor.casts_shadow = false;
                actor.scripted_animation = true;
                let mut animation =
                    Animation::new(resource, slot::IDLE, clip.duration_ticks, self.world.tick);
                animation.rate = 0.;
                animation.repeat = false;
                if self
                    .world
                    .party
                    .as_ref()
                    .is_some_and(|party| party.travel.opened_treasures.contains(&(a[0] as u16)))
                {
                    animation.start_frame = clip.duration_ticks as f32;
                }
                actor.animation = Some(animation);
                self.world.insert_actor(id, actor);
                self.world.treasures.push(crate::TreasureChest {
                    actor: id,
                    flag: a[0] as u16,
                    reward: crate::TreasureReward::from_source(a[1] as u16),
                    kind,
                });
            }
            NativeCall::PreloadVoiceBank => {
                let bank = (a[0] as u32 >> 16) as u16;
                if bank != 0 {
                    // Select one of two CRI voice archives. Field
                    // preparation has decoded all referenced lines before entry.
                    self.world.voice_banks[usize::from(a[1] != 0)] = Some(bank);
                }
            }
            NativeCall::SetSoundReverb => {
                self.world
                    .audio_commands
                    .push(AudioCommand::SoundReverb(match a[0] as u16 {
                        2 => 2,
                        3 => 3,
                        _ => 1,
                    }));
            }
            NativeCall::ConfigureSceneryAnimation => {
                let id = match a[0] {
                    2 | SECOND_SCENERY => SECOND_SCENERY,
                    3 | THIRD_SCENERY => THIRD_SCENERY,
                    4 | 0xF422C => 0xF422C,
                    _ => MAIN_SCENERY,
                };
                require((-1..4).contains(&a[1]), "invalid scenery motion channel")?;
                let (kind, resource) = self
                    .world
                    .loaded_resources
                    .get(&a[2])
                    .copied()
                    .or_else(|| self.resources.binding(a[2]))
                    .ok_or("scenery motion is not cooked")?;
                require(
                    kind == ResourceKind::Animation,
                    "scenery motion has the wrong resource type",
                )?;
                let clip = self
                    .resources
                    .animations
                    .get(&resource)
                    .and_then(|clips| clips.get(&slot::IDLE))
                    .ok_or("scenery motion clip is not cooked")?;
                require(a[5] & !11 == 0, "unknown scenery motion flags")?;
                let mut animation =
                    Animation::new(resource, slot::IDLE, clip.duration_ticks, self.world.tick);
                animation.source = crate::animation::AnimationSource::Resource;
                animation.start_frame = (a[3] as f32 * 2.).min(clip.duration_ticks as f32);
                animation.rate = if a[5] & 2 != 0 {
                    0.
                } else {
                    a[4] as f32 / 100.
                };
                animation.paused_rate = (a[5] & 2 != 0).then_some(a[4] as f32 / 100.);
                animation.repeat = a[5] & 8 == 0;
                let actor = self
                    .world
                    .actors
                    .get_mut(&id)
                    .ok_or("scenery layer is missing")?;
                if a[1] == -1 {
                    actor.animation = Some(animation);
                } else {
                    actor.scenery_animations.insert(a[1] as i8, animation);
                }
            }
            NativeCall::ClearSceneryAnimation => {
                let id = match a[0] {
                    2 => SECOND_SCENERY,
                    3 => THIRD_SCENERY,
                    _ => MAIN_SCENERY,
                };
                require((-1..4).contains(&a[1]), "invalid scenery motion channel")?;
                if let Some(actor) = self.world.actors.get_mut(&id) {
                    if a[1] == -1 {
                        actor.animation = None;
                    } else {
                        actor.scenery_animations.remove(&(a[1] as i8));
                    }
                }
            }
            NativeCall::SetActorAmbientSound => {
                if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                    actor.ambient_sound = (a[1] != -1).then_some(crate::AmbientSound {
                        id: a[1] as i16,
                        volume: if a[2] as u8 == 255 { 127 } else { a[2] as u8 },
                        radius: a[3] as f32,
                    });
                }
            }
            NativeCall::CreateRingStation => {
                let resource = self.resolve(a[5], ResourceKind::Model)?;
                require(self.world.actors.len() < 4096, "actor limit exceeded")?;
                let mut actor = Actor::new(resource, [a[1] as f32, a[2] as f32, a[3] as f32]);
                actor.ring_station = true;
                actor.radius = 65.;
                actor.grounded = false;
                actor.casts_shadow = false;
                actor.depth_write = false;
                actor.blend = Some(crate::effect::Blend::Additive);
                actor.opacity = 64;
                actor.interaction_label = 5;
                actor.draw_layer = -1;
                self.world.insert_actor(a[0], actor);
            }
            NativeCall::CreateSavePoint | NativeCall::CreateSealedSavePoint => {
                let unlock_flag = (op == NativeCall::CreateSealedSavePoint).then_some(a[3] as u16);
                let sealed =
                    unlock_flag.is_some_and(|flag| !self.world.event_flags.contains(&flag));
                let resource = resonance_content::field::SAVE_POINT_RESOURCE;
                let model = self
                    .resources
                    .model(resource)
                    .ok_or("save-point model is not cooked")?;
                let idle = model
                    .clips
                    .get(&slot::IDLE)
                    .ok_or("save-point animation is not cooked")?;
                require(
                    self.world.save_points.len() < 16,
                    "save-point limit exceeded",
                )?;
                // Field services own a reserved actor range, separate from script IDs.
                let id = i32::MIN + self.world.save_points.len() as i32;
                require(
                    !self.world.actors.contains_key(&id),
                    "save-point actor ID is occupied",
                )?;
                let position = [a[0] as f32, a[1] as f32, a[2] as f32];
                let mut actor = Actor::new(resource, [position[0], position[1], position[2] + 10.]);
                actor.contact = crate::ActorContact::None;
                actor.grounded = false;
                actor.collidable = false;
                actor.casts_shadow = false;
                actor.depth_write = false;
                actor.scripted_animation = true;
                actor.appearance.hidden_nodes = model
                    .names
                    .iter()
                    .enumerate()
                    .filter(|(_, name)| name.starts_with(if sealed { "LIVE_" } else { "HID_" }))
                    .map(|(i, _)| i as u16)
                    .collect();
                let mut animation =
                    Animation::new(resource, slot::IDLE, idle.duration_ticks, self.world.tick);
                animation.rate = if sealed { 0. } else { 0.1 }; // Two ticks per authored frame.
                actor.animation = Some(animation);
                self.world.insert_actor(id, actor);
                self.world.save_points.push(crate::SavePoint {
                    actor: id,
                    position,
                    resource,
                    born: self.world.tick,
                    active: false,
                    unlock_flag,
                    glow_scale: 0.08,
                });
            }
            NativeCall::Unknown92 => {
                match FieldSystemCommand::try_from(a[0])? {
                    FieldSystemCommand::FieldLeader => {
                        value = Some(i32::from(
                            self.world
                                .party
                                .as_ref()
                                .ok_or("party is not initialized")?
                                .field_leader,
                        ));
                    }
                    FieldSystemCommand::SetFieldLeader => {
                        require((1..=9).contains(&a[1]), "invalid field leader")?;
                        let party = self
                            .world
                            .party
                            .as_mut()
                            .ok_or("party is not initialized")?;
                        value = Some(i32::from(party.field_leader));
                        party.field_leader = a[1] as u8;
                    }
                    FieldSystemCommand::SuppressTransitionFade => {
                        // Suppress the implicit native
                        // transition fade. Our scene loader already leaves fades
                        // to the script and holds its rendered pose until ready.
                        value = Some(0);
                    }
                    FieldSystemCommand::SetDoorInteractionRadius => {
                        require(a[1] >= 0, "negative door interaction range")?;
                        value = Some(self.world.door_interaction_radius.unwrap_or(250.) as i32);
                        self.world.door_interaction_radius = Some(a[1] as f32);
                    }
                    FieldSystemCommand::BattleCount => {
                        let party = self
                            .world
                            .party
                            .as_ref()
                            .ok_or("party is not initialized")?;
                        value = Some(i32::from(party.battles.count(a[1])?));
                    }
                }
            }
            NativeCall::PreloadField => {
                self.world.preload_field = if a[0] == -1 {
                    None
                } else {
                    let map = u32::try_from(a[0]).map_err(|_| "invalid map")?;
                    // Advisory only: destination preparation owns missing-file
                    // errors, retaining the source scene so the player can retry.
                    Some(map)
                };
            }
            NativeCall::PlayWorldCinematic => {
                let location = u16::try_from(a[0]).map_err(|_| "invalid world cinematic")?;
                require((513..=526).contains(&location), "invalid world cinematic")?;
                let map = u32::try_from(a[1]).map_err(|_| "invalid cinematic destination")?;
                require(
                    self.resources.fields.contains(&3000),
                    "world is not available",
                )?;
                require(
                    self.resources.fields.contains(&map.min(3000)),
                    "cinematic destination is not available",
                )?;
                if map >= 3000 {
                    require(
                        a[2] == 0 || (1..=98).contains(&a[2]) || (257..=337).contains(&a[2]),
                        "invalid world landmark",
                    )?;
                    require(i16::try_from(a[5]).is_ok(), "invalid world exit direction")?;
                }
                let operation = self.world.request_world(
                    location,
                    0,
                    Some(crate::SceneDestination {
                        map,
                        position: [a[2] as f32, a[3] as f32, a[4] as f32],
                        heading: a[5] as f32,
                    }),
                )?;
                *self.wait = Some(crate::operation::Wait::Complete(operation));
                return Ok(NativeResult::Suspend);
            }
            NativeCall::ChangeField => {
                let map = u32::try_from(a[0]).map_err(|_| "invalid map")?;
                if map >= 3000 {
                    require(
                        self.resources.fields.contains(&3000),
                        "world is not available",
                    )?;
                    require(
                        self.world.world_transition.is_none()
                            && self.world.field_transition.is_none()
                            && self.world.field_exit.is_none(),
                        "scene transition is already pending",
                    )?;
                    let location = u16::try_from(a[1]).map_err(|_| "invalid world landmark")?;
                    require(
                        location == 0
                            || (1..=98).contains(&location)
                            || (257..=337).contains(&location),
                        "invalid world landmark",
                    )?;
                    let direction =
                        i16::try_from(a[4]).map_err(|_| "invalid world exit direction")?;
                    let operation = self.world.operations.begin()?;
                    *self.wait = Some(crate::operation::Wait::Complete(operation.clone()));
                    self.world.world_transition = Some(crate::WorldTransition {
                        location,
                        direction,
                        following: None,
                        operation,
                    });
                    self.world.input_enabled = false;
                    return Ok(NativeResult::Suspend);
                }
                // A partial cooked library must produce a recoverable loader
                // error, not terminate the running event VM.
                require(
                    self.world.field_transition.is_none()
                        && self.world.world_transition.is_none()
                        && self.world.field_exit.is_none(),
                    "field transition is already pending",
                )?;
                let operation = self.world.operations.begin()?;
                *self.wait = Some(crate::operation::Wait::Complete(operation.clone()));
                let request = crate::world::FieldTransition {
                    map,
                    position: [a[1] as f32, a[2] as f32, a[3] as f32],
                    heading: (a[4] as f32).rem_euclid(360.),
                    camera: self
                        .world
                        .field_camera
                        .as_ref()
                        .and_then(|rig| rig.entry.clone()),
                    operation,
                };
                self.world.begin_field_exit(request, self.resources)?;
                self.world.input_enabled = false;
                return Ok(NativeResult::Suspend);
            }
            NativeCall::ReturnFieldControl => {
                self.world.mapped_input_disabled = false;
                // Fade in the scene before returning field input.
                let from = self
                    .world
                    .fade
                    .as_ref()
                    .map_or(255., |f| f.before_update(self.world.tick));
                if from as i32 == 0 {
                    self.world.fade = Some(crate::Fade {
                        start_tick: self.world.tick,
                        duration: 0,
                        from: 0.,
                        to: 0.,
                        white: false,
                    });
                    self.world.input_enabled = true;
                    // An already clear scene releases input now; only the
                    // current script waits until the next dispatcher update.
                    *self.wait = Some(crate::operation::Wait::ControlReleased);
                    return Ok(NativeResult::Suspend);
                }
                let white = self.world.fade.as_ref().is_some_and(|f| f.white);
                self.world.fade = Some(crate::Fade::new(self.world.tick, 10, from, 0., white));
                self.world.input_enabled = false;
                *self.wait = Some(crate::operation::Wait::ControlHandoff(
                    self.world.tick.checked_add(10).ok_or("clock overflow")?,
                ));
                return Ok(NativeResult::Suspend);
            }
            NativeCall::ConfigureSound
            | NativeCall::SelectAudioBank
            | NativeCall::AudioCommand
            | NativeCall::SetAudioFade
            | NativeCall::PlaySoundSimple
            | NativeCall::PlaySound => {
                require(
                    self.world.audio_commands.len() < 512,
                    "audio command queue is not being consumed",
                )?;
                let command = match op {
                    NativeCall::ConfigureSound => {
                        let slot = a[0] as u16;
                        match a[1] {
                            100 => AudioCommand::SoundVolume {
                                slot,
                                volume: if a[2] == 255 { 127 } else { a[2] as u8 },
                            },
                            101 => AudioCommand::SoundPan {
                                slot,
                                pan: sound_pan(a[2]),
                            },
                            102 => AudioCommand::StopSound(slot),
                            _ => return Err("sound slot operation is not implemented".into()),
                        }
                    }
                    NativeCall::SelectAudioBank => AudioCommand::SelectBank((a[0] & 7) as u8),
                    NativeCall::AudioCommand => {
                        AudioCommand::Music(crate::MusicCommand::try_from(a[0] as i16)?)
                    }
                    NativeCall::SetAudioFade => AudioCommand::MusicVolume {
                        volume: a[1].clamp(0, 127) as u8,
                        duration_ticks: a[2].max(0) as u32,
                    },
                    NativeCall::PlaySound if a[0] == -1 && a[3] != 255 => AudioCommand::StopSound(
                        u16::try_from(a[3]).map_err(|_| "invalid sound slot")?,
                    ),
                    _ => AudioCommand::Sound {
                        id: a[0] as i16,
                        volume: if op == NativeCall::PlaySound && a[2] != 255 {
                            a[2].clamp(0, 127) as u8
                        } else {
                            127
                        },
                        slot: if op == NativeCall::PlaySound && a[3] != 255 {
                            Some(u8::try_from(a[3]).map_err(|_| "invalid sound slot")?)
                        } else {
                            None
                        },
                        pan: sound_pan(a[1]),
                    },
                };
                self.world.audio_commands.push(command);
            }
            NativeCall::IsMappedInputDisabled => {
                value = Some(i32::from(self.world.mapped_input_disabled))
            }
            NativeCall::MoveActorRelative => {
                if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                    if a[2] == 0 && a[3] == 0 {
                        actor.motion = None;
                    } else {
                        let angle = (a[1] as f32).to_radians();
                        let distance = a[2] as f32;
                        let target = [
                            actor.position[0] + angle.sin() * distance,
                            actor.position[1] - angle.cos() * distance,
                            actor.position[2],
                        ];
                        let speed = if a[3] < 0 {
                            distance / (a[3] as u32 & 0x7fff_ffff).max(1) as f32
                        } else {
                            a[3] as f32
                        };
                        require(
                            speed > 0. || distance.abs() < 1.,
                            "actor movement has zero speed",
                        )?;
                        actor.motion = Some(crate::world::ActorMotion { target, speed });
                        actor.set_movement_speed(speed);
                        if a[4] != 0 {
                            actor.appearance.fixed_heading.get_or_insert(actor.heading);
                        } else {
                            actor.appearance.fixed_heading = None;
                        }
                    }
                }
            }
            NativeCall::MoveActor => {
                if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                    let target = [a[1] as f32, a[2] as f32, a[3] as f32];
                    let distance = (0..3)
                        .map(|i| (target[i] - actor.position[i]).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    let speed = if a[4] < 0 {
                        distance / (a[4] as u32 & 0x7fff_ffff).max(1) as f32
                    } else {
                        a[4] as f32
                    };
                    require(speed > 0. || distance < 1., "actor movement has zero speed")?;
                    actor.motion = Some(crate::world::ActorMotion { target, speed });
                    actor.set_movement_speed(speed);
                }
            }
            NativeCall::SetActorOrientation => {
                require((0..=4).contains(&a[1]), "unknown heading operation")?;
                if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                    match a[1] {
                        0 => actor.target_heading = (a[2] as f32).rem_euclid(360.),
                        1 => {
                            actor.target_heading =
                                (actor.target_heading + a[2] as f32).rem_euclid(360.)
                        }
                        2 => actor.face(a[2] as f32),
                        3 => actor.face(actor.target_heading + a[2] as f32),
                        4 => actor.turn_speed = (a[2] as f32 / 100.).abs(),
                        _ => unreachable!(),
                    }
                }
            }
            NativeCall::RandomMod => {
                require(a[0] != 0, "random modulo zero")?;
                value = Some(self.world.random() as i32 % a[0]);
            }
            NativeCall::CreateModelParticle => {
                let resource = self.resolve(a[0], ResourceKind::Model)?;
                value = Some(self.world.emit_model_particle(
                    crate::model_particle::ModelParticle::from_native(resource, a),
                )?);
            }
            NativeCall::SetModelParticleProperty => {
                if let Some(particle) = self.world.model_particles.get_mut(&a[0]) {
                    particle.set_property(a[1], a[2])?;
                }
                value = Some(0);
            }
            NativeCall::CreateEffectEmitter => {
                let mut emitter = crate::emitter::Emitter::from_native(a)?;
                // The descending seal lights gather into the character beneath them.
                if a[5] == 31
                    && let Some(recipient) = self
                        .world
                        .actors
                        .values()
                        .filter(|actor| (1..=9).contains(&actor.resource))
                        .min_by(|left, right| {
                            let distance = |actor: &Actor| {
                                (actor.position[0] - a[1] as f32)
                                    .hypot(actor.position[1] - a[2] as f32)
                            };
                            distance(left).total_cmp(&distance(right))
                        })
                {
                    let mut target = recipient.position;
                    target[2] += crate::ACTOR_CONTACT_HEIGHT * 2. / 3.;
                    emitter.aim_at(target);
                }
                let resource = if a[4] == 0 || a[5] == 47 {
                    0
                } else {
                    self.resolve(a[4], ResourceKind::Model)?
                };
                let mut actor = Actor::new(resource, [a[1] as f32, a[2] as f32, a[3] as f32]);
                // Recipe 46 keeps its model solely for independent afterimages.
                actor.visible = resource != 0 && a[5] != 46;
                actor.contact = crate::ActorContact::None;
                actor.collidable = false;
                actor.grounded = false;
                actor.casts_shadow = false;
                actor.emitter = Some(emitter);
                actor.set_movement_speed(a[7] as f32);
                self.world.insert_actor(a[0], actor);
            }
            NativeCall::BindEffectTexture => {
                require(
                    (0..8).contains(&a[0]) && (0..256).contains(&a[2]),
                    "invalid effect texture binding",
                )?;
                let resource = self.resolve(a[1], ResourceKind::Overlay)?;
                self.world
                    .effect_textures
                    .insert(a[0] as u8, (resource, a[2] as u8));
            }
            NativeCall::CreateEffectObject | NativeCall::CreateParticle => {
                const GLOW: i32 = crate::effect::GLOW_SPRITE as i32;
                const SMOKE: i32 = resonance_content::effect::SMOKE_SPRITE as i32;
                const STREAK: i32 = resonance_content::effect::STREAK_SPRITE as i32;
                const ORB: i32 = crate::effect::ORB_SPRITE as i32;
                const STATION_GLOW: i32 = crate::effect::STATION_GLOW_SPRITE as i32;
                const CAMERA_DISC: i32 = crate::effect::CAMERA_DISC_SPRITE as i32;
                const CAMERA_RING: i32 = 40;
                const WORLD_GLOW: i32 = crate::effect::WORLD_GLOW_SPRITE as i32;
                const RING: i32 = crate::effect::RING_SPRITE as i32;
                const CAMERA_RIPPLE: i32 = 27;
                const WORLD_RIPPLE: i32 = 28;
                const EXPANDING_GLOW: i32 = 32;
                const SPINNING_STAR: i32 = crate::effect::SPINNING_STAR_SPRITE as i32;
                const STAR: i32 = crate::effect::STAR_SPRITE as i32;
                const SEAL_SPARK: i32 = 69;
                const FALLING_SPARK: i32 = 43;
                const DEBRIS_FIRST: i32 = 52;
                const DEBRIS_LAST: i32 = 54;
                const STAR_ROTATION: f32 = 45.;
                const ELECTRIC_SPARK: i32 = crate::effect::ELECTRIC_SPARK_SPRITE as i32;
                const ELECTRIC_ARC: i32 = crate::effect::ELECTRIC_ARC_SPRITE as i32;
                const STREAK_ASPECT: f32 = 6.;
                let directed = op == NativeCall::CreateEffectObject;
                let offset = usize::from(directed);
                let [size, alpha, fade, palette, parameter] = a[8 + offset..].try_into().unwrap();
                let lifetime = if a[1] as i16 == i16::MAX {
                    u32::MAX
                } else {
                    u32::from(a[1] as u16) + 1
                };
                let effect_fade = if fade == 0 {
                    crate::effect::Fade::tail(lifetime)
                } else {
                    crate::effect::Fade::Linear(fade as f32)
                };
                if matches!(a[0], CAMERA_RIPPLE | WORLD_RIPPLE) {
                    require(
                        (0..resonance_content::effect::FIELD_PALETTE_COLORS as i32)
                            .contains(&palette)
                            && parameter == 0,
                        "invalid refraction palette or parameter",
                    )?;
                    require(
                        a[5..8 + offset].iter().all(|v| *v == 0),
                        "moving refraction particles are not implemented",
                    )?;
                    let handle = self.world.emit_refraction(crate::effect::RefractionPulse {
                        operation: None,
                        owner: None,
                        image: crate::effect::RefractionImage::Ripple,
                        palette: palette as u8,
                        orientation: if a[0] == WORLD_RIPPLE {
                            crate::effect::SpriteOrientation::World
                        } else {
                            crate::effect::SpriteOrientation::Camera
                        },
                        rotation: [0.; 3],
                        position: [a[2] as f32, a[3] as f32, a[4] as f32],
                        born: self.world.tick,
                        lifetime,
                        size: size as f32,
                        growth: 0.,
                        alpha: alpha as u8 as f32,
                        fade: effect_fade,
                    })?;
                    return Ok(NativeResult::Continue(Some(handle)));
                }
                let supported = match a[0] {
                    STATION_GLOW => !directed,
                    GLOW | SMOKE | EXPANDING_GLOW | STREAK | SEAL_SPARK | FALLING_SPARK => directed,
                    CAMERA_DISC
                    | CAMERA_RING
                    | WORLD_GLOW
                    | ORB
                    | RING
                    | STAR
                    | SPINNING_STAR
                    | ELECTRIC_SPARK
                    | ELECTRIC_ARC
                    | DEBRIS_FIRST..=DEBRIS_LAST => true,
                    _ => false,
                };
                require(
                    supported
                        && (0..resonance_content::effect::FIELD_PALETTE_COLORS as i32)
                            .contains(&palette)
                        && (!directed
                            || matches!(a[0], WORLD_GLOW | SPINNING_STAR)
                            || parameter == 0),
                    "effect recipe is not implemented",
                )?;
                let spin = match a[0] {
                    SPINNING_STAR => parameter as f32,
                    GLOW | SMOKE | EXPANDING_GLOW | ELECTRIC_ARC => {
                        if self.world.effect_tick & 1 == 0 {
                            -3.
                        } else {
                            3.
                        }
                    }
                    _ => 0.,
                };
                let velocity = [a[5] as f32, a[6] as f32, a[7] as f32];
                let length = velocity.iter().map(|x| x * x).sum::<f32>().sqrt();
                let speed = a[8] as f32 / 100.;
                let flutter = if a[0] == FALLING_SPARK {
                    let Some(crate::ParticleKind::Flutter(recipe)) =
                        self.resources.particles.get(&25)
                    else {
                        return Err("falling spark motion recipe is not cooked".into());
                    };
                    let mut flutter = crate::effect::Flutter::new(recipe);
                    flutter.initialize(&mut || self.world.random());
                    Some(flutter)
                } else {
                    None
                };
                let handle = self.world.emit_billboard(crate::effect::BillboardEffect {
                    texture: u8::try_from(a[0] - CAMERA_RING)
                        .ok()
                        .and_then(|slot| self.world.effect_textures.get(&slot).copied()),
                    field_lighting: true,
                    orientation: if matches!(a[0], WORLD_GLOW | RING | FALLING_SPARK) {
                        crate::effect::SpriteOrientation::World
                    } else {
                        crate::effect::SpriteOrientation::Camera
                    },
                    palette: Some(palette as u16),
                    // Native recipes 5/6/40 share their atlas; only facing differs.
                    recipe: if a[0] == EXPANDING_GLOW {
                        crate::effect::ORB_SPRITE as i32
                    } else if matches!(a[0], CAMERA_DISC | CAMERA_RING) {
                        WORLD_GLOW
                    } else if a[0] == FALLING_SPARK {
                        68
                    } else {
                        a[0]
                    } as u16,
                    size_delta: if a[0] == EXPANDING_GLOW { 6. } else { 0. },
                    born: self.world.tick,
                    lifetime: lifetime.min(
                        if a[0] as u16 == resonance_content::effect::SMOKE_SPRITE {
                            resonance_content::effect::SMOKE_UPDATES
                        } else {
                            u32::MAX
                        },
                    ),
                    position: [a[2] as f32, a[3] as f32, a[4] as f32],
                    velocity: if directed {
                        velocity.map(|v| if length > 0. { v / length * speed } else { 0. })
                    } else {
                        velocity
                    },
                    rotation: flutter.as_ref().map_or(
                        [
                            0.,
                            0.,
                            if matches!(a[0], SMOKE | SPINNING_STAR) {
                                (self.world.effect_tick & 127) as f32
                            } else if a[0] == STAR {
                                STAR_ROTATION
                            } else {
                                0.
                            },
                        ],
                        |flutter| flutter.rotation,
                    ),
                    controller: flutter.map(crate::effect::BillboardController::Flutter),
                    angular_velocity: [
                        if a[0] == WORLD_GLOW {
                            parameter as f32
                        } else {
                            0.
                        },
                        0.,
                        spin,
                    ],
                    size: [
                        size as f32,
                        size as f32 / if a[0] == STREAK { STREAK_ASPECT } else { 1. },
                    ],
                    rgba: [64, 64, 64, alpha as u8],
                    fade: effect_fade,
                    ..Default::default()
                })?;
                value = Some(handle);
            }
            NativeCall::SetEffectProperty => {
                const PARTICLE_MODE: i32 = 146;
                const QUAD_LAYOUT: i32 = 147;
                const FIELD_FOG: i32 = 148;
                require(
                    matches!(a[1], 120..=128 | 132..=135 | 141..=145 | PARTICLE_MODE | QUAD_LAYOUT | FIELD_FOG),
                    "effect property is not implemented",
                )?;
                if let Some(effect) = self.world.billboards.get_mut(&a[0]) {
                    match a[1] {
                        120..=122 => effect.position[(a[1] - 120) as usize] = a[2] as f32,
                        123..=124 => effect.size[(a[1] - 123) as usize] = a[2] as f32,
                        125..=128 => effect.rgba[(a[1] - 125) as usize] = a[2] as u8,
                        132..=134 => {
                            effect.angular_velocity[(a[1] - 132) as usize] = a[2] as f32 / 100.
                        }
                        135 => effect.size_delta = a[2] as f32 / 100.,
                        141..=143 => effect.rotation[(a[1] - 141) as usize] = a[2] as f32 / 100.,
                        144 => {
                            effect.orientation = if a[2] & 1 == 0 {
                                crate::effect::SpriteOrientation::World
                            } else {
                                crate::effect::SpriteOrientation::Camera
                            }
                        }
                        145 => {
                            effect.blend = if a[2] & 3 == 3 {
                                None
                            } else {
                                Some(a[2].try_into()?)
                            }
                        }
                        PARTICLE_MODE => {
                            const PROPORTIONAL_FADE: i32 = 8;
                            require(a[2] == PROPORTIONAL_FADE, "unsupported particle mode")?;
                            effect.rgba[3] = effect.alpha(self.world.tick) as u8;
                            effect.fade = crate::effect::Fade::Proportional {
                                after: self.world.tick.saturating_sub(effect.born),
                                lifetime: effect.lifetime,
                            };
                        }
                        QUAD_LAYOUT => {
                            use resonance_content::effect::VerticalAnchor;
                            effect.anchor = match a[2] {
                                0 => VerticalAnchor::Center,
                                4 => VerticalAnchor::UpperHalf,
                                8 => VerticalAnchor::LowerHalf,
                                _ => return Err("unsupported particle quad layout".into()),
                            };
                        }
                        FIELD_FOG => effect.field_fog = a[2] & 1 != 0,
                        _ => unreachable!(),
                    }
                }
                value = Some(0);
            }
            NativeCall::SetActorAnimationProperty => {
                require((0..=6).contains(&a[1]), "unknown animation property")?;
                let mut previous = 0.;
                if let Some(animation) = self
                    .world
                    .actors
                    .get_mut(&a[0])
                    .and_then(|actor| actor.animation.as_mut())
                {
                    let duration = self
                        .resources
                        .animation(animation)
                        .ok_or("animation clip is not cooked")?
                        .duration_ticks as f32;
                    let position = animation.sample(self.world.tick, 0, duration);
                    match a[1] {
                        0 | 1 => {
                            // Cooked animation ticks are twice the script’s frame unit;
                            // a script rate of 100 advances one cooked tick per update.
                            previous = animation.script_rate() * 100.;
                            let rate = if a[1] == 0 {
                                a[2] as f32 / 100.
                            } else {
                                duration / 2. / if a[2] == 0 { 1. } else { a[2] as f32 }
                            };
                            animation.set_script_rate(rate, self.world.tick);
                        }
                        2 => {
                            previous = position / 2.;
                            let position = a[2] as f32 * 2.;
                            if (0. ..=duration).contains(&position) {
                                animation.seek(position, self.world.tick);
                            }
                        }
                        3 => previous = animation.script_rate() * 100.,
                        4 => previous = position / 2.,
                        5 => previous = duration / 2.,
                        6 => {
                            previous = animation.loop_start / 2.;
                            let position = a[2] as f32 * 2.;
                            if (0. ..=duration).contains(&position) {
                                animation.loop_start = position;
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                value = Some(previous as i32);
            }
            NativeCall::FindActorBodyPart => {
                // Map script bone selectors to model node names.
                const NAMES: [&str; 17] = [
                    "Bone_kubi",
                    "Bone_atama",
                    "Bone_sebone01",
                    "Bone_sebone02",
                    "Bone_sebone03",
                    "Bone_ude01_L",
                    "Bone_ude02_L",
                    "Bone_ude01_R",
                    "Bone_ude02_R",
                    "Bone_ashi01_L",
                    "Bone_ashi02_L",
                    "Bone_ashi03_L",
                    "Bone_ashi04_L",
                    "Bone_ashi01_R",
                    "Bone_ashi02_R",
                    "Bone_ashi03_R",
                    "Bone_ashi04_R",
                ];
                value = Some(
                    self.world
                        .actors
                        .get(&a[0])
                        .and_then(|actor| self.resources.model(actor.resource))
                        .and_then(|model| {
                            let name = NAMES.get(usize::try_from(a[1]).ok()?)?;
                            model.names.iter().position(|n| n == name)
                        })
                        .map_or(-1, |index| index as i32),
                );
            }
            NativeCall::ConfigureActorBoneRotation => {
                return self.field(
                    NativeCall::ConfigureActorAttachment,
                    &[a[0], a[1], a[2], a[3], a[4], a[5], 30],
                    _memory,
                );
            }
            NativeCall::ConfigureActorAttachment
            | NativeCall::ConfigureActorBoneTranslation
            | NativeCall::ConfigureActorBoneScale => {
                if a[2] != -1
                    && let Some(actor) = self.world.actors.get_mut(&a[0])
                {
                    require((0..8).contains(&a[1]), "invalid bone controller slot")?;
                    require(a[6] >= 0, "negative bone adjustment duration")?;
                    let bone = self
                        .resources
                        .model(actor.resource)
                        .and_then(|m| m.names.get(usize::try_from(a[2]).ok()?))
                        .ok_or("bone adjustment target is not cooked")?
                        .clone();
                    if op != NativeCall::ConfigureActorAttachment {
                        let adjustment = actor
                            .appearance
                            .bone_adjustments
                            .entry(a[1] as u8)
                            .or_insert_with(|| BoneAdjustment {
                                bone: crate::BoneTarget::Name(bone.clone()),
                                absolute_rotation: false,
                                angles: [0.; 3],
                                from: [0.; 3],
                                duration_ticks: 1,
                                start_tick: self.world.tick,
                                translation: None,
                                scale: None,
                            });
                        adjustment.bone = crate::BoneTarget::Name(bone);
                        let target = [a[3] as f32, a[4] as f32, a[5] as f32];
                        if op == NativeCall::ConfigureActorBoneScale {
                            const SCALE_PERCENT: f32 = 100.;
                            adjustment.scale = Some(crate::BoneScale::new(
                                adjustment.scale.as_ref(),
                                target.map(|v| v / SCALE_PERCENT),
                                a[6] as u32,
                                self.world.tick,
                            ));
                        } else {
                            adjustment.translation = Some(crate::world::BoneTranslation {
                                from: adjustment.translation(self.world.tick),
                                to: target,
                                duration_ticks: (a[6] as u32).max(1),
                                start_tick: self.world.tick,
                            });
                        }
                    } else {
                        adjust_bone(
                            actor,
                            a[1] as u8,
                            bone,
                            [a[3] as f32, a[5] as f32, a[4] as f32],
                            a[6] as u32,
                            self.world.tick,
                        );
                    }
                }
            }
            NativeCall::MotionCommand => self.camera_path(a)?,
            NativeCall::AttachActorToMember => {
                let resolve = |id| {
                    if id == crate::CONTROLLED_ACTOR {
                        self.world.controlled_actor
                    } else {
                        id
                    }
                };
                let child = resolve(a[0]);
                let parent = resolve(a[1]);
                if a[2] == -1 {
                    if let Some(actor) = self.world.actors.get_mut(&child) {
                        actor.attachment = None;
                    }
                } else if let Some(owner) = self.world.actors.get(&parent) {
                    let bone = self
                        .resources
                        .model(owner.resource)
                        .and_then(|model| model.names.get(usize::try_from(a[2]).ok()?))
                        .ok_or("attachment bone is not cooked")?
                        .clone();
                    let mut ancestor = Some(parent);
                    for _ in 0..=self.world.actors.len() {
                        let Some(id) = ancestor else {
                            break;
                        };
                        require(id != child, "actor attachment cycle")?;
                        ancestor = self
                            .world
                            .actors
                            .get(&id)
                            .and_then(|actor| actor.attachment.as_ref())
                            .map(|a| a.actor);
                    }
                    if let Some(actor) = self.world.actors.get_mut(&child) {
                        actor.attachment = Some(Attachment {
                            actor: parent,
                            bone,
                        });
                    }
                }
            }
            NativeCall::MeasureActorGeometry => {
                // Arguments are operation, first actor, second actor.
                require((0..=3).contains(&a[0]), "unknown actor geometry query")?;
                let actor = |id| {
                    self.world.actors.get(&match id {
                        crate::CONTROLLED_ACTOR => self.world.controlled_actor,
                        crate::ring::SCRIPT_ACTOR => self.world.ring.bomb()?,
                        _ => id,
                    })
                };
                value = Some(match (actor(a[1]), actor(a[2])) {
                    (Some(first), Some(second)) => {
                        let d: [f32; 3] =
                            std::array::from_fn(|i| second.position[i] - first.position[i]);
                        match a[0] {
                            0 => d[0].atan2(-d[1]).to_degrees().rem_euclid(360.) as i32,
                            1 => d[0].hypot(d[1]) as i32,
                            2 => d[2] as i32,
                            _ => d[0].hypot(d[1]).hypot(d[2]) as i32,
                        }
                    }
                    _ => 0,
                });
            }
            NativeCall::SetActorFace | NativeCall::SetActorMouth => {
                if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                    let face = match a[1] {
                        0 => Face::Disabled,
                        1 => Face::Blink,
                        2..=17 if op == NativeCall::SetActorFace || a[1] <= 9 => {
                            Face::Frame((a[1] - 2) as u8)
                        }
                        _ => return Ok(NativeResult::Continue(None)),
                    };
                    if op == NativeCall::SetActorFace {
                        actor.appearance.face = face;
                        actor.appearance.eyes = None;
                    } else {
                        actor.appearance.mouth = Some(face);
                    }
                }
            }
            NativeCall::ConfigureActorHeadNeck | NativeCall::TurnActorHead => {
                const HEAD_TURN_TICKS: i32 = 30;
                let duration = if op == NativeCall::TurnActorHead {
                    HEAD_TURN_TICKS
                } else {
                    a[6]
                };
                // Drive neck and head with two independent additive bone controllers.
                if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                    let names = &self
                        .resources
                        .model(actor.resource)
                        .ok_or("actor model missing")?
                        .names;
                    if ["Bone_kubi", "Bone_atama"]
                        .iter()
                        .all(|name| names.iter().any(|n| n == name))
                    {
                        for (slot, bone) in [(a[1], "Bone_kubi"), (a[2], "Bone_atama")] {
                            require((0..8).contains(&slot), "invalid bone controller slot")?;
                            require(duration >= 0, "negative bone adjustment duration")?;
                            adjust_bone(
                                actor,
                                slot as u8,
                                bone.into(),
                                [(a[3] / 2) as f32, (a[5] / 2) as f32, (a[4] / 2) as f32],
                                duration as u32,
                                self.world.tick,
                            );
                        }
                    }
                }
            }
            NativeCall::SetActorAnimation => {
                if a[1] != -1
                    && let Some(actor) = self.world.actors.get_mut(&a[0])
                {
                    let index = u16::try_from(a[1]).map_err(|_| "invalid node index")?;
                    require(
                        self.resources
                            .model(actor.resource)
                            .is_some_and(|m| (index as usize) < m.names.len()),
                        "node is not cooked",
                    )?;
                    if a[2] == 0 {
                        actor.appearance.hidden_nodes.insert(index);
                    } else {
                        actor.appearance.hidden_nodes.remove(&index);
                    }
                }
            }
            NativeCall::SpawnEnemyActor => {
                if a[0] == crate::CONTROLLED_ACTOR {
                    return Ok(NativeResult::Continue(None));
                }
                let actor = self.enemy_actor(a)?;
                self.world.insert_actor(a[0], actor);
            }
            NativeCall::CreateEnemySource => {
                if a[1] == crate::CONTROLLED_ACTOR {
                    return Ok(NativeResult::Continue(None));
                }
                let enemy = self.enemy_actor(&a[1..17])?;
                let mut source = Actor::new(enemy.resource, enemy.position);
                source.contact = crate::ActorContact::None;
                source.face(enemy.heading);
                source.animation = enemy.animation.clone();
                source.grounded = false;
                source.collidable = false;
                source.casts_shadow = false;
                source.visible = false;
                source.interaction_label = 0;
                source.enemy_source = Some(crate::enemy_source::EnemySource::new(
                    a[1], enemy, a[17], a[18],
                )?);
                self.world.insert_actor(a[0], source);
            }
            NativeCall::SpawnActor
            | NativeCall::SpawnCollisionActor
            | NativeCall::SpawnSceneryActor => {
                if a[0] < 0 {
                    let target = if a[5] == crate::CONTROLLED_ACTOR {
                        self.world.controlled_actor
                    } else {
                        a[5]
                    };
                    if (-299..=-100).contains(&a[0]) && self.world.actors.contains_key(&target) {
                        require((0..=19).contains(&a[4]), "unknown emote recipe")?;
                        require(self.world.emotes.len() < 200, "emote limit exceeded")?;
                        self.world.emotes.insert(
                            a[0],
                            Emote {
                                actor: target,
                                kind: a[4] as u16,
                                offset: [a[1] as f32, a[2] as f32, a[3] as f32],
                                start_tick: self.world.tick,
                                duration: (a[7] != -1).then_some(a[7].max(0) as u32),
                            },
                        );
                    }
                    return Ok(NativeResult::Continue(None));
                }
                if a[0] == 0xF423F {
                    return Ok(NativeResult::Continue(None));
                }
                require(self.world.actors.len() < 4096, "actor limit exceeded")?;
                let locator = self.resources.locators.contains(&a[5]);
                let resource = if locator {
                    a[5] as u32
                } else {
                    self.resolve(if a[5] == 0 { 1 } else { a[5] }, ResourceKind::Model)?
                };
                let mut actor = Actor::new(resource, [a[1] as f32, a[2] as f32, a[3] as f32]);
                if self.world.field_camera.is_some() {
                    let behavior = crate::Behavior::try_from(a[6]).map_err(|e| e.to_string())?;
                    require(a[7] >= 0, "negative ambient movement speed")?;
                    require(
                        locator
                            || matches!(
                                behavior,
                                crate::Behavior::Stationary
                                    | crate::Behavior::WatchPlayer
                                    | crate::Behavior::Player
                            )
                            || self.resources.model(resource).is_some_and(|m| {
                                m.clips.contains_key(&crate::animation::slot::WALK)
                            }),
                        "ambient actor walk animation is not cooked",
                    )?;
                    actor.autonomy =
                        Some(crate::Autonomy::new(behavior, a[7] as f32, actor.position));
                }
                if let Some(model) = self.resources.model(resource) {
                    actor
                        .appearance
                        .hidden_nodes
                        .clone_from(&model.hidden_nodes);
                }
                actor.face(a[4] as f32);
                actor.visible = !locator;
                actor.interaction_anchor = locator;
                actor.interaction_label = if locator { 0 } else { 2 };
                if locator && op == NativeCall::SpawnActor {
                    actor.ring_contact_disabled = true;
                }
                if op != NativeCall::SpawnActor {
                    // Scenery carries its own collision
                    // mesh and bypasses character grounding, shadows and culling.
                    actor.grounded = false;
                    actor.collidable = false;
                    actor.casts_shadow = false;
                    actor.cull_outside_view = false;
                    actor.culling_flags = [None, Some(true)];
                    actor.appearance.model_hidden = op == NativeCall::SpawnCollisionActor;
                    if op == NativeCall::SpawnSceneryActor {
                        const BLOCK_RADIUS: f32 = 50.;
                        const GRAB_HINT: i32 = 20;
                        actor.pushable = true;
                        actor.radius = BLOCK_RADIUS;
                        actor.interaction_label = GRAB_HINT;
                    }
                    actor.model_collision = Some(
                        self.resources
                            .model(resource)
                            .ok_or("collision model is missing")?
                            .collision
                            .clone(),
                    );
                }
                if let Some(clip) = self
                    .resources
                    .model(resource)
                    .and_then(|m| m.clips.get(&crate::animation::slot::IDLE))
                {
                    actor.animation = Some(Animation {
                        ..Animation::new(
                            resource,
                            crate::animation::slot::IDLE,
                            clip.duration_ticks,
                            self.world.tick,
                        )
                    });
                }
                // Authored wing slots select the profile once, at the script boundary.
                match a[0] {
                    crate::wings::AUTOMATIC_WINGS | 90024 => {
                        actor.set_wings(crate::WingStyle::Layered)
                    }
                    90026 => actor.set_wings(crate::WingStyle::Echo),
                    _ => {}
                }
                let id = if a[0] == 0 {
                    self.world.unaddressable_actor_key()?
                } else {
                    a[0]
                };
                self.world.insert_actor(id, actor);
            }
            NativeCall::DespawnActor => {
                self.world.despawn_scene_actors(a[0]);
            }
            NativeCall::SetActorHeading => {
                if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                    // Change the target heading; the actor update turns toward it.
                    actor.target_heading = (a[1] as f32).rem_euclid(360.);
                }
            }
            NativeCall::SelectPartyMember => {
                value = Some(self.world.controlled_actor);
                if a[0] != -1 {
                    // Restore the selected party leader as the controlled actor.
                    let id = if a[0] == crate::CONTROLLED_ACTOR {
                        self.world
                            .party
                            .as_ref()
                            .map(|p| p.field_leader)
                            .map_or(1, i32::from)
                    } else if a[0] > 9 {
                        1
                    } else {
                        a[0]
                    };
                    if a[0] == crate::CONTROLLED_ACTOR {
                        self.world.select_party_member(self.resources, id)
                    } else {
                        // An explicit member reloads the controlled actor, even
                        // when that member is already selected.
                        self.world.replace_party_member(self.resources, id)
                    }
                    .map_err(|e| e.to_string())?;
                }
            }
            NativeCall::TriggerExists => {
                value = Some(i32::from(self.world.triggers.iter().any(|trigger| {
                    trigger.kind() as i16 == a[0] as i16 && trigger.key == a[1] as u32
                })));
            }
            NativeCall::RemoveAutomaticEventTriggers
            | NativeCall::RemoveTouchTriggers
            | NativeCall::RemoveConfirmedTriggers => {
                self.world.triggers.retain(|trigger| {
                    trigger.key != a[0] as u32
                        || match op {
                            NativeCall::RemoveAutomaticEventTriggers => !trigger.automatic_event,
                            NativeCall::RemoveConfirmedTriggers => trigger.transition.is_none(),
                            _ => trigger.transition.is_some() || trigger.automatic_event,
                        }
                });
            }
            NativeCall::CreateScriptRecord
            | NativeCall::CreateAutomaticEventTrigger
            | NativeCall::CreateCircleTrigger
            | NativeCall::CreateAutomaticCircleTrigger
            | NativeCall::CreateConfirmedCircleTrigger
            | NativeCall::CreateTriangleTrigger
            | NativeCall::CreateScriptRecordVariant
            | NativeCall::CreateAreaTrigger
            | NativeCall::CreateConfirmedTriangleTrigger
            | NativeCall::CreateConfirmedAreaTrigger => {
                require(
                    self.world.triggers.len() < 200,
                    "field trigger limit exceeded",
                )?;
                let confirmed = matches!(
                    op,
                    NativeCall::CreateScriptRecordVariant
                        | NativeCall::CreateConfirmedCircleTrigger
                        | NativeCall::CreateConfirmedTriangleTrigger
                        | NativeCall::CreateConfirmedAreaTrigger
                );
                let offset = if confirmed { 4 } else { 1 };
                let point = |i| std::array::from_fn(|axis| a[offset + i * 3 + axis] as i16 as f32);
                let shape = match op {
                    NativeCall::CreateCircleTrigger
                    | NativeCall::CreateAutomaticCircleTrigger
                    | NativeCall::CreateConfirmedCircleTrigger => crate::TriggerShape::Circle {
                        center: point(0),
                        radius: a[offset + 3] as i16 as f32,
                    },
                    NativeCall::CreateAreaTrigger | NativeCall::CreateConfirmedAreaTrigger => {
                        crate::TriggerShape::Quad(std::array::from_fn(point))
                    }
                    NativeCall::CreateConfirmedTriangleTrigger
                    | NativeCall::CreateTriangleTrigger => {
                        crate::TriggerShape::Triangle(std::array::from_fn(point))
                    }
                    _ => crate::TriggerShape::Line(std::array::from_fn(point)),
                };
                const RING_BARRIER_KEYS: std::ops::Range<i32> = 0..100;
                self.world.triggers.push(Trigger {
                    activations: 0,
                    ring_barrier: !confirmed
                        && RING_BARRIER_KEYS.contains(&a[0])
                        && !matches!(
                            op,
                            NativeCall::CreateAutomaticEventTrigger
                                | NativeCall::CreateAutomaticCircleTrigger
                        ),
                    key: a[0] as u32,
                    automatic_event: matches!(
                        op,
                        NativeCall::CreateAutomaticEventTrigger
                            | NativeCall::CreateAutomaticCircleTrigger
                    ),
                    shape,
                    height: a[a.len() - 1] as i16 as f32,
                    transition: confirmed
                        .then(|| [a[1] as u16 as u32, a[2] as u16 as u32, a[3] as u32]),
                    touch_metadata: [0; 3],
                });
            }
            NativeCall::SetTouchTriggerMetadata => {
                if let Some(trigger) = self
                    .world
                    .triggers
                    .iter_mut()
                    .find(|trigger| trigger.key == a[0] as u32 && trigger.transition.is_none())
                {
                    trigger.touch_metadata = [a[1] as u16 as u32, a[2] as u16 as u32, a[3] as u32];
                }
            }
            NativeCall::SetTriggerMetadata => {
                // Only confirmed-interaction records carry this metadata.
                if let Some(values) = self
                    .world
                    .triggers
                    .iter_mut()
                    .filter(|trigger| trigger.key == a[0] as u32)
                    .find_map(|trigger| trigger.transition.as_mut())
                {
                    *values = [u32::from(a[1] as u16), u32::from(a[2] as u16), a[3] as u32];
                }
            }
            NativeCall::DisableMappedInput => {
                self.world.mapped_input_disabled = true;
                self.world.input_enabled = false;
                if let Some(actor) = self.world.actors.get_mut(&self.world.controlled_actor) {
                    actor.motion = None;
                }
            }
            NativeCall::EnableMappedInput => {
                self.world.mapped_input_disabled = false;
                self.world.input_enabled = true;
            }
            NativeCall::SetEventBit | NativeCall::ClearEventBit | NativeCall::TestEventBit => {
                require(
                    (0..2048).contains(&a[0]),
                    "event flag is outside the session",
                )?;
                let flag = a[0] as u16;
                match op {
                    NativeCall::SetEventBit => {
                        self.world.event_flags.insert(flag);
                    }
                    NativeCall::ClearEventBit => {
                        self.world.event_flags.remove(&flag);
                    }
                    _ => value = Some(i32::from(self.world.event_flags.contains(&flag))),
                }
            }
            NativeCall::SetScenarioTimer => {
                require(
                    (0..=200).contains(&a[0]),
                    "event record is outside the session",
                )?;
                self.world.event_records.insert(
                    a[0] as u8,
                    EventRecord {
                        value: a[1] as u8,
                        extra: a[2] as u8,
                        tick: self.world.tick,
                        level: self
                            .world
                            .party
                            .as_ref()
                            .map(|party| party.members[0].level),
                        recorded_at: self.world.calendar_time.or_else(|| {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .ok()
                                .and_then(|time| i64::try_from(time.as_secs()).ok())
                        }),
                    },
                );
            }
            NativeCall::GetScenarioTimerValue => {
                require(
                    (0..=200).contains(&a[0]),
                    "event record is outside the session",
                )?;
                value = Some(
                    self.world
                        .event_records
                        .get(&(a[0] as u8))
                        .map_or(0, |e| i32::from(e.value)),
                );
            }
            NativeCall::ResolveScriptResource => {
                let binding = self
                    .resources
                    .binding(a[0])
                    .ok_or("requested resource is not cooked")?;
                let handle = (0..128)
                    .map(|i| (0xffff0000u32 | i) as i32)
                    .find(|h| !self.world.loaded_resources.contains_key(h))
                    .ok_or("resource handle pool exhausted")?;
                self.world.loaded_resources.insert(handle, binding);
                value = Some(handle);
            }
            NativeCall::ReleaseScriptResource => {
                self.world.loaded_resources.remove(&a[0]);
                value = Some(0);
            }
            NativeCall::ShakeCamera => {
                self.world
                    .field_camera
                    .get_or_insert_default()
                    .shake
                    .configure(a[0] as f32, a[1] as u16, a[2]);
            }
            NativeCall::SelectCamera => {
                require(
                    (-2..4).contains(&a[0]),
                    "camera selector is not implemented",
                )?;
                let rig = self.world.field_camera.get_or_insert_default();
                value = Some(rig.selected as i32);
                if a[0] == -2 {
                    rig.start_path();
                } else if a[0] == -1 {
                    rig.entry = Some(crate::camera::EntryCamera::following(
                        self.world.controlled_actor,
                    ));
                } else {
                    rig.selected = a[0] as usize;
                    rig.motion = None;
                    rig.position_settled = false;
                    rig.target_settled = false;
                }
            }
            NativeCall::GetCameraProperty => {
                let rig = self.world.field_camera.get_or_insert_default();
                let camera = rig
                    .entry
                    .as_ref()
                    .map_or(rig.current(), |entry| &entry.camera);
                value = Some(match a[0] {
                    0 => i32::from(camera.follow),
                    1 => i32::from(camera.anchor_to_actor),
                    2 => (camera.fov_degrees * 100.) as i32,
                    3 => rig.position_rate as i32,
                    4 => rig.target_rate as i32,
                    5..=7 => i32::from(camera.axes[(a[0] - 5) as usize]),
                    20..=22 => rig.angles[(a[0] - 20) as usize] as i32,
                    23 => camera.distance as i32,
                    _ => return Err("camera property query is not implemented".into()),
                });
            }
            NativeCall::SetCameraProperty => {
                let rig = self.world.field_camera.get_or_insert_default();
                value = Some(camera_property(rig, a[0], a[1])?);
            }
            NativeCall::SetCameraTransitionValues => {
                let rig = self.world.field_camera.get_or_insert_default();
                rig.position_settled = false;
                if !rig.command_camera().follow {
                    rig.target_settled = false;
                }
                let camera = rig.command_camera();
                camera.angles = [a[0] as f32, a[1] as f32, a[2] as f32];
                camera.distance = a[3] as f32;
                camera.follow = true;
            }
            NativeCall::SetCameraEye => {
                self.world.field_camera.get_or_insert_default().set_eye(
                    a[0],
                    [a[1] as f32, a[2] as f32, a[3] as f32],
                    &self.world.actors,
                );
            }
            NativeCall::SelectActor => {
                if self.world.actors.contains_key(&a[0]) {
                    let rig = self.world.field_camera.get_or_insert_default();
                    if rig.command_camera().actor != a[0] || !rig.command_camera().follow {
                        rig.position_settled = false;
                        rig.target_settled = false;
                    }
                    let camera = rig.command_camera();
                    camera.actor = a[0];
                    camera.follow = true;
                }
            }
            NativeCall::SetCameraPosition => {
                let rig = self.world.field_camera.get_or_insert_default();
                let offset = [a[0] as f32, a[1] as f32, a[2] as f32];
                if rig.command_camera().offset != offset {
                    rig.target_settled = false;
                    rig.command_camera().offset = offset;
                }
            }
            NativeCall::ResetCameraBounds => {
                let rig = self.world.field_camera.get_or_insert_default();
                let camera = rig.command_camera();
                camera.position_bounds = [[-100000., 100000.]; 3];
                camera.target_bounds = camera.position_bounds;
                rig.position_settled = false;
                rig.target_settled = false;
            }
            NativeCall::ConfigureCameraParameters => {
                let rig = self.world.field_camera.get_or_insert_default();
                // Pin each nonzero coordinate by setting both
                // bounds. Zero leaves that axis or interpolation rate alone.
                for (index, value) in a[..6].iter().copied().enumerate() {
                    if value != 0 {
                        let camera = rig.command_camera();
                        let bounds = if index < 3 {
                            &mut camera.position_bounds
                        } else {
                            &mut camera.target_bounds
                        };
                        bounds[index % 3] = [value as f32; 2];
                    }
                }
                if a[6] != 0 {
                    rig.position_rate = a[6] as f32;
                }
                if a[7] != 0 {
                    rig.target_rate = a[7] as f32;
                }
                rig.position_settled = false;
                rig.target_settled = false;
            }
            NativeCall::ConfigureCameraAuxiliary => {
                // Configure GX fog: mode 1 selects exponential-squared
                // perspective fog (GX type 5), all other modes disable it.
                let rig = self.world.field_camera.get_or_insert_default();
                rig.command_camera().fog = (a[0] == 1).then_some(crate::camera::Fog {
                    start: a[1] as f32,
                    end: a[2] as f32,
                    color: [a[3] as u8, a[4] as u8, a[5] as u8],
                });
            }
            _ => return Err(format!("unimplemented native {op:?}")),
        }
        Ok(NativeResult::Continue(value))
    }
}

fn sound_pan(value: i32) -> u8 {
    let pan = value / 2 + 64;
    if (0..128).contains(&pan) {
        pan as u8
    } else {
        64
    }
}

fn adjust_bone(
    actor: &mut Actor,
    slot: u8,
    bone: String,
    angles: [f32; 3],
    duration: u32,
    tick: u32,
) {
    let adjustments = &mut actor.appearance.bone_adjustments;
    let from = adjustments.get(&slot).map_or([0.; 3], |a| a.sample(tick));
    let translation = adjustments.get(&slot).and_then(|a| a.translation.clone());
    let scale = adjustments.get(&slot).and_then(|a| a.scale.clone());
    adjustments.insert(
        slot,
        BoneAdjustment {
            from,
            bone: crate::BoneTarget::Name(bone),
            absolute_rotation: false,
            angles,
            duration_ticks: duration.max(1),
            start_tick: tick,
            translation,
            scale,
        },
    );
}

fn camera_property(rig: &mut CameraRig, prop: i32, v: i32) -> Result<i32, String> {
    match prop {
        0 | 1 | 15..=19 => rig.target_settled = false,
        5..=14 => rig.position_settled = false,
        _ => {}
    }
    Ok(match prop {
        3 | 4 => {
            // Interpolation rates are global even while editing an entry camera;
            // the native return value still comes from the selected template.
            let old = rig.entry.as_ref().map(|entry| {
                if prop == 3 {
                    entry.position_rate
                } else {
                    entry.target_rate
                }
            });
            let rate = if prop == 3 {
                &mut rig.position_rate
            } else {
                &mut rig.target_rate
            };
            let previous = std::mem::replace(rate, (v as f32).max(1.));
            old.unwrap_or(previous) as i32
        }
        prop => {
            let camera = rig.command_camera();
            match prop {
                0 | 1 | 5..=7 => {
                    let flag = match prop {
                        0 => &mut camera.follow,
                        1 => &mut camera.anchor_to_actor,
                        _ => &mut camera.axes[(prop - 5) as usize],
                    };
                    i32::from(std::mem::replace(flag, v & 1 != 0))
                }
                2 => {
                    let old = camera.fov_degrees as i32 * 100;
                    require((100..17900).contains(&v), "invalid camera field of view")?;
                    camera.fov_degrees = v as f32 / 100.;
                    old
                }
                8..=19 => {
                    let (bounds, index) = if prop < 14 {
                        (&mut camera.position_bounds, (prop - 8) as usize)
                    } else {
                        (&mut camera.target_bounds, (prop - 14) as usize)
                    };
                    let old = bounds[index / 2][index % 2];
                    bounds[index / 2][index % 2] = v as f32;
                    old as i32
                }
                _ => return Err("camera property is not implemented".into()),
            }
        }
    })
}

impl NativeHost<'_> {
    fn enemy_actor(&self, a: &[i32]) -> Result<Actor, String> {
        require(self.world.actors.len() < 4096, "actor limit exceeded")?;
        let locator = self.resources.locators.contains(&a[10]);
        let resource = if locator {
            a[10] as u32
        } else {
            self.resolve(a[10], ResourceKind::Model)?
        };
        let mut actor = Actor::new(resource, [a[3] as f32, a[4] as f32, a[5] as f32]);
        actor.face(a[6] as f32);
        actor.visible = !locator;
        actor.radius = 32.;
        actor.turn_speed = 10.;
        // Native B6=6 selects enemy contact; B4 (property 17) is the action
        // label and remains zero. Enemies are not ordinary action targets.
        let behavior = crate::Behavior::enemy(a[11] as u8);
        actor.autonomy = Some(crate::Autonomy::new(
            behavior,
            a[7].max(0) as f32,
            actor.position,
        ));
        actor.autonomy.as_mut().unwrap().radius = a[14] as f32;
        actor.enemy = Some(crate::world::Enemy {
            event: a[9] as u16,
            behavior: a[11] as u8,
            normal_speed: a[7].max(0) as f32,
            alert_speed: a[8].max(0) as f32,
            random_turns: a[12] as u8,
            chase_on_sight: a[13] as u8 != 0,
            sight_angle: 90.,
            sight_distance: 600.,
            alerted: false,
            event_parameters: [a[1] as i16, a[2] as i16],
            pause_ticks: 0,
            reaction: crate::effect::StunEffect::None,
        });
        if let Some(model) = self.resources.model(resource) {
            actor
                .appearance
                .hidden_nodes
                .clone_from(&model.hidden_nodes);
            if let Some(clip) = model.clips.get(&slot::IDLE) {
                actor.animation = Some(Animation::new(
                    resource,
                    slot::IDLE,
                    clip.duration_ticks,
                    self.world.tick,
                ));
            }
        }
        Ok(actor)
    }
}
