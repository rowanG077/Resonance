//! Field service bindings. Scene setup supplies all asset and event mappings.
use super::{NativeHost, NativeResult, require};
use crate::{
    Actor, Animation, ResourceKind,
    camera::CameraRig,
    world::{Attachment, AudioCommand, BoneAdjustment, Emote, EventRecord, Face, Trigger},
};
use symphonia_script::NativeCall;
use symphonia_script_vm::Memory;

impl NativeHost<'_> {
    pub(super) fn field(
        &mut self,
        op: NativeCall,
        a: &[i32],
        _memory: &mut Memory,
    ) -> Result<NativeResult, String> {
        let mut value = None;
        match op {
            NativeCall::CreateSavePoint => {
                let resource = resonance_content::field::SAVE_POINT_RESOURCE;
                require(
                    self.resources.model(resource).is_some_and(|model| {
                        model.clips.contains_key(&crate::animation::slot::IDLE)
                    }),
                    "save-point model or animation is not cooked",
                )?;
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
                let model = self.resources.model(resource).unwrap();
                let mut actor = Actor::new(resource, [position[0], position[1], position[2] + 10.]);
                actor.grounded = false;
                actor.collidable = false;
                actor.casts_shadow = false;
                actor.depth_write = false;
                actor.scripted_animation = true;
                actor.appearance.hidden_nodes = model
                    .names
                    .iter()
                    .enumerate()
                    .filter(|(_, name)| name.starts_with("HID_"))
                    .map(|(i, _)| i as u16)
                    .collect();
                let mut animation = Animation::new(
                    resource,
                    crate::animation::slot::IDLE,
                    model.clips[&crate::animation::slot::IDLE].duration_ticks,
                    self.world.tick,
                );
                animation.rate = 0.1; // Cooked poses use two ticks per authored model frame.
                actor.animation = Some(animation);
                self.world.insert_actor(id, actor);
                self.world.save_points.push(crate::SavePoint {
                    actor: id,
                    position,
                    resource,
                    born: self.world.tick,
                    active: false,
                    glow_scale: 0.08,
                });
            }
            NativeCall::Unknown92 => {
                require(a[0] == 19, "system command is not implemented")?;
                require(a[1] >= 0, "negative door interaction range")?;
                value = Some(self.world.door_interaction_radius.unwrap_or(250.) as i32);
                self.world.door_interaction_radius = Some(a[1] as f32);
            }
            NativeCall::PreloadField => {
                self.world.preload_field = if a[0] == -1 {
                    None
                } else {
                    let map = u32::try_from(a[0]).map_err(|_| "invalid map")?;
                    require(
                        self.resources.fields.contains(&map),
                        "field is not available",
                    )?;
                    Some(map)
                };
            }
            NativeCall::ChangeField => {
                let map = u32::try_from(a[0]).map_err(|_| "invalid map")?;
                require(
                    self.resources.fields.contains(&map),
                    "field is not available",
                )?;
                require(
                    self.world.field_transition.is_none() && self.world.field_exit.is_none(),
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
                // Fade in the scene before returning field input.
                let from = self
                    .world
                    .fade
                    .as_ref()
                    .map_or(255., |f| f.alpha(self.world.tick));
                let white = self.world.fade.as_ref().is_some_and(|f| f.white) && from as i32 != 0;
                self.world.fade = Some(crate::Fade {
                    start_tick: self.world.tick,
                    duration: 10,
                    from,
                    to: 0.,
                    white,
                });
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
                    NativeCall::AudioCommand => AudioCommand::Music(a[0] as i16),
                    NativeCall::SetAudioFade => AudioCommand::MusicVolume {
                        volume: a[1].clamp(0, 127) as u8,
                        duration_ticks: a[2].max(0) as u32,
                    },
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
            NativeCall::CreateEffectObject => {
                // Recipe zero is a rotating atlas billboard.
                require(
                    a[0] == 0 && a[12..] == [0, 0],
                    "effect recipe is not implemented",
                )?;
                let direction = if self.world.tick & 1 == 0 { -3. } else { 3. };
                let velocity = [a[5] as f32, a[6] as f32, a[7] as f32];
                let length = velocity.iter().map(|x| x * x).sum::<f32>().sqrt();
                let speed = (a[8] / 100) as f32;
                let handle = self.world.emit_billboard(crate::effect::BillboardEffect {
                    recipe: 0,
                    born: self.world.tick,
                    lifetime: u32::from(a[1] as u16),
                    position: [a[2] as f32, a[3] as f32, a[4] as f32],
                    velocity: velocity.map(|v| if length > 0. { v / length * speed } else { 0. }),
                    rotation: [0.; 3],
                    angular_velocity: [0., 0., direction],
                    size: [a[9] as f32; 2],
                    size_delta: 0.,
                    rgba: [64, 64, 64, a[10] as u8],
                    alpha_delta: a[11] as f32,
                })?;
                value = Some(handle);
            }
            NativeCall::SetEffectProperty => {
                require(
                    matches!(a[1], 134 | 135),
                    "effect property is not implemented",
                )?;
                if let Some(effect) = self.world.billboards.get_mut(&a[0]) {
                    match a[1] {
                        134 => effect.angular_velocity[2] = a[2] as f32 / 100.,
                        135 => effect.size_delta = a[2] as f32 / 100.,
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
                        .model(animation.resource)
                        .and_then(|m| m.clips.get(&animation.slot))
                        .ok_or("animation clip is not cooked")?
                        .duration_ticks as f32;
                    let position = animation.sample(self.world.tick, 0, duration);
                    match a[1] {
                        0 | 1 => {
                            // Cooked animation ticks are twice the script’s frame unit;
                            // a script rate of 100 advances one cooked tick per update.
                            previous = animation.rate * 100.;
                            animation.seek(position, self.world.tick);
                            animation.rate = if a[1] == 0 {
                                a[2] as f32 / 100.
                            } else {
                                duration / 2. / if a[2] == 0 { 1. } else { a[2] as f32 }
                            };
                        }
                        2 => {
                            previous = position / 2.;
                            let position = a[2] as f32 * 2.;
                            if (0. ..=duration).contains(&position) {
                                animation.seek(position, self.world.tick);
                            }
                        }
                        3 => previous = animation.rate * 100.,
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
            NativeCall::ConfigureActorAttachment => {
                // Add a timed rotation to one bone controller.
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
            NativeCall::MotionCommand => self.camera_path(a)?,
            NativeCall::AttachActorToMember => {
                if a[2] == -1 {
                    if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                        actor.attachment = None;
                    }
                } else if let Some(owner) = self.world.actors.get(&a[1]) {
                    let bone = self
                        .resources
                        .model(owner.resource)
                        .and_then(|model| model.names.get(usize::try_from(a[2]).ok()?))
                        .ok_or("attachment bone is not cooked")?
                        .clone();
                    let mut ancestor = Some(a[1]);
                    for _ in 0..=self.world.actors.len() {
                        let Some(id) = ancestor else {
                            break;
                        };
                        require(id != a[0], "actor attachment cycle")?;
                        ancestor = self
                            .world
                            .actors
                            .get(&id)
                            .and_then(|actor| actor.attachment.as_ref())
                            .map(|a| a.actor);
                    }
                    if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                        actor.attachment = Some(Attachment { actor: a[1], bone });
                    }
                }
            }
            NativeCall::MeasureActorGeometry => {
                // Arguments are operation, first actor, second actor.
                require((0..=3).contains(&a[0]), "unknown actor geometry query")?;
                value = Some(
                    match (self.world.actors.get(&a[1]), self.world.actors.get(&a[2])) {
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
                    },
                );
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
            NativeCall::ConfigureActorHeadNeck => {
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
                            require(a[6] >= 0, "negative bone adjustment duration")?;
                            adjust_bone(
                                actor,
                                slot as u8,
                                bone.into(),
                                [(a[3] / 2) as f32, (a[5] / 2) as f32, (a[4] / 2) as f32],
                                a[6] as u32,
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
            NativeCall::SpawnActor => {
                if a[0] < 0 {
                    if (-299..=-100).contains(&a[0]) && self.world.actors.contains_key(&a[5]) {
                        require((0..=19).contains(&a[4]), "unknown emote recipe")?;
                        require(self.world.emotes.len() < 200, "emote limit exceeded")?;
                        self.world.emotes.insert(
                            a[0],
                            Emote {
                                actor: a[5],
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
                    self.resolve(a[5], ResourceKind::Model)?
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
                actor.properties.insert(17, if locator { 0 } else { 2 });
                if let Some(clip) = self
                    .resources
                    .model(resource)
                    .and_then(|m| m.clips.get(&crate::animation::slot::IDLE))
                {
                    actor.animation = Some(Animation::new(
                        resource,
                        crate::animation::slot::IDLE,
                        clip.duration_ticks,
                        self.world.tick,
                    ));
                }
                self.world.insert_actor(a[0], actor);
            }
            NativeCall::DespawnActor => {
                self.world.actors.remove(&a[0]);
                self.world.overlays.remove(&a[0]);
                self.world.emotes.remove(&a[0]);
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
                    self.world
                        .select_party_member(self.resources, id)
                        .map_err(|e| e.to_string())?;
                }
            }
            NativeCall::CreateScriptRecord
            | NativeCall::CreateScriptRecordVariant
            | NativeCall::CreateAreaTrigger => {
                require(
                    self.world.triggers.len() < 200,
                    "field trigger limit exceeded",
                )?;
                let offset = if op == NativeCall::CreateScriptRecordVariant {
                    4
                } else {
                    1
                };
                let point = |i| std::array::from_fn(|axis| a[offset + i * 3 + axis] as i16 as f32);
                let shape = if op == NativeCall::CreateAreaTrigger {
                    crate::TriggerShape::Quad(std::array::from_fn(point))
                } else {
                    crate::TriggerShape::Line(std::array::from_fn(point))
                };
                self.world.triggers.push(Trigger {
                    key: a[0] as u32,
                    shape,
                    height: a[a.len() - 1] as i16 as f32,
                    transition: (op == NativeCall::CreateScriptRecordVariant)
                        .then(|| [a[1] as u32, a[2] as u32, a[3] as u32]),
                });
            }
            NativeCall::SetTriggerMetadata => {
                // Type 1 touch records do not carry interaction metadata.
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
                self.world.input_enabled = false;
                if let Some(actor) = self.world.actors.get_mut(&self.world.controlled_actor) {
                    actor.motion = None;
                }
            }
            NativeCall::EnableMappedInput => self.world.input_enabled = true,
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
                let binding = *self
                    .resources
                    .bindings
                    .get(&a[0])
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
    adjustments.insert(
        slot,
        BoneAdjustment {
            from,
            bone,
            angles,
            duration_ticks: duration.max(1),
            start_tick: tick,
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
