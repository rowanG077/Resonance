//! Native service shims registered by typed call ID. The VM knows neither
//! actors nor assets; these handlers operate independently of scene/event IDs.
use crate::world::{Fade, Overlay};
use crate::{Actor, Animation, CameraTrack, GameWorld, Particle, ResourceKind, ResourceLibrary};
use crate::{
    dialogue::{DIALOGUE_SLOTS, Dialogue, DialogueAnchor, Movie, flags},
    operation::Wait,
};
use symphonia_script::{NativeCall, Program};
use symphonia_script_vm::{Memory, NativeResult};
mod bindings;
mod camera_path;
mod field;
mod party;
mod skit;
mod wait;

pub(crate) struct EventCommand {
    pub handle: i32,
    pub action: EventAction,
}
pub(crate) enum EventAction {
    Spawn(u32),
    Pause(bool),
    ControlGate(bool),
}

pub(crate) struct NativeHost<'a> {
    pub world: &'a mut GameWorld,
    pub resources: &'a ResourceLibrary,
    pub program: &'a Program,
    pub registers: &'a mut [i32; 6],
    pub events: &'a mut Vec<EventCommand>,
    pub next_handle: &'a mut i32,
    pub wait: &'a mut Option<Wait>,
}
fn require(ok: bool, what: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(what.into()) }
}
impl NativeHost<'_> {
    fn yield_update(&mut self) -> Result<NativeResult, String> {
        *self.wait = Some(Wait::Tick(
            self.world
                .tick
                .checked_add(1)
                .ok_or("wait clock overflow")?,
        ));
        Ok(NativeResult::Suspend)
    }
    fn resolve(&self, script_id: i32, kind: ResourceKind) -> Result<u32, String> {
        self.world
            .loaded_resources
            .get(&script_id)
            .filter(|(k, _)| *k == kind)
            .map(|(_, id)| *id)
            .map(Ok)
            .unwrap_or_else(|| self.resources.resolve(script_id, kind))
    }
    fn attachment(&self, id: i32, node: i32) -> Result<[i32; 3], String> {
        let actor = self
            .world
            .actors
            .get(&id)
            .ok_or("attachment actor is missing")?;
        let animation = actor
            .animation
            .as_ref()
            .ok_or("attachment actor has no animation")?;
        let model = self
            .resources
            .model(actor.resource)
            .ok_or("actor model is missing")?;
        let name = model
            .names
            .get(usize::try_from(node).map_err(|_| "invalid bone index")?)
            .ok_or("bone index is missing")?;
        let clip = self
            .resources
            .model(animation.resource)
            .and_then(|m| m.clips.get(&animation.slot))
            .ok_or("animation is missing")?;
        let sample = animation.sample(self.world.tick, 0, clip.duration_ticks as f32) as u32;
        let track = clip
            .attachments
            .get(name)
            .ok_or("attachment is not cooked for this animation")?;
        let point = track
            .sample(sample)
            .ok_or("attachment query outside cooked range")?;
        Ok(std::array::from_fn(|i| {
            (point[i] + actor.position[i]).trunc() as i32
        }))
    }
    fn dispatch(
        &mut self,
        op: NativeCall,
        a: &[i32],
        memory: &mut Memory,
    ) -> Result<NativeResult, String> {
        let mut value = None;
        match op {
            NativeCall::ActorExists => {
                value = Some(i32::from(self.world.skit.as_ref().map_or_else(
                    || self.world.actors.contains_key(&a[0]),
                    |s| s.portraits.values().any(|p| p.id == a[0]),
                )))
            }
            NativeCall::SetDialogueSlotFlag => {
                let mask = match a[1] {
                    0 => flags::PERSISTENT,
                    1 => flags::AUTO_PAGES,
                    _ => return Err("unsupported dialogue flag selector".into()),
                };
                if (0..i32::from(DIALOGUE_SLOTS)).contains(&a[0])
                    && let Some(dialogue) = self.world.dialogue.get_mut(&(a[0] as u8))
                {
                    dialogue.flags = if a[2] != 0 {
                        dialogue.flags | mask
                    } else {
                        dialogue.flags & !mask
                    };
                }
            }
            NativeCall::CloseDialogue => {
                if let Some(choice) = self.world.choices.remove(&(a[0] as u8)) {
                    choice.operation.cancel();
                }
                if let Some(dialogue) = self.world.dialogue.remove(&(a[0] as u8))
                    && dialogue.operation.is_pending()
                {
                    dialogue.operation.complete(None)?;
                }
            }
            NativeCall::ConfigureDialogue => {
                require(
                    (0..i32::from(DIALOGUE_SLOTS)).contains(&a[0]),
                    "invalid dialogue slot",
                )?;
                if let Some(choice) = self.world.choices.remove(&(a[0] as u8)) {
                    choice.operation.cancel();
                }
                let message = |index| {
                    self.resources
                        .messages
                        .get(usize::try_from(index).map_err(|_| "negative message index")?)
                        .cloned()
                        .ok_or("missing cooked message")
                };
                let names = self.resources.names(self.world.party.as_ref());
                let speaker = crate::dialogue::resolve(
                    &message(a[6])?,
                    memory,
                    &names,
                    &self.resources.text,
                    self.world.controlled_actor,
                )?;
                let body = crate::dialogue::resolve(
                    &message(a[7])?,
                    memory,
                    &names,
                    &self.resources.text,
                    self.world.controlled_actor,
                )?;
                let anchor = match a[2] {
                    -1 => DialogueAnchor::Actor(a[3]),
                    -2 => DialogueAnchor::ScreenGrid(
                        u8::try_from(a[3])
                            .ok()
                            .filter(|v| *v < 9)
                            .ok_or("invalid dialogue anchor")?,
                    ),
                    -18..=-10 => DialogueAnchor::ScreenGrid((-a[2] - 10) as u8),
                    x => DialogueAnchor::Screen([x as f32, a[3] as f32]),
                };
                let mut flags = a[1] as u16;
                if flags & flags::PLACEMENT == 0 {
                    flags |= flags::AUTO_SIDE;
                }
                if flags & flags::AUTO_SIDE != 0 {
                    flags |= flags::ABOVE;
                }
                let dimensions = if a[4] == 0 {
                    None
                } else {
                    require(
                        (1..=558).contains(&a[4]) && (1..=398).contains(&a[5]),
                        "invalid dialogue dimensions",
                    )?;
                    Some([a[4] as u16, a[5] as u16])
                };
                let dialogue = Dialogue {
                    operation: self.world.operations.begin()?,
                    speaker,
                    body,
                    anchor,
                    speaker_actor: matches!(a[2], -1 | -18..=-10).then_some(
                        if a[3] == crate::CONTROLLED_ACTOR {
                            self.world.controlled_actor
                        } else {
                            a[3]
                        },
                    ),
                    opening_actor: (a[2] == -1
                        && a[3] != crate::CONTROLLED_ACTOR
                        && self.world.actors.contains_key(&a[3]))
                    .then_some(a[3]),
                    flags,
                    dimensions,
                    height_offset: if dimensions.is_none() { a[5] as i16 } else { 0 },
                };
                if let Some(old) = self.world.dialogue.insert(a[0] as u8, dialogue) {
                    old.operation.cancel();
                }
            }
            NativeCall::SetActorPosition => {
                // Position updates for absent actors are ignored.
                if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                    actor.position = [a[1] as f32, a[2] as f32, a[3] as f32];
                }
            }
            NativeCall::GetActorProperty | NativeCall::SetActorProperty => {
                // Property writes return the previous value.
                require(
                    matches!(a[1], 1..=4 | 7..=13 | 16 | 17 | 46),
                    "actor property shim is not implemented",
                )?;
                let id = if a[0] == crate::CONTROLLED_ACTOR {
                    self.world.controlled_actor
                } else {
                    a[0]
                };
                let Some(actor) = self.world.actors.get_mut(&id) else {
                    return Ok(NativeResult::Continue(Some(0)));
                };
                let previous = match a[1] {
                    1..=3 => actor.position[(a[1] - 1) as usize] as i32,
                    4 => actor.heading as i32,
                    7 => actor.properties.get(&7).copied().unwrap_or(0),
                    8 => actor.properties.get(&8).copied().unwrap_or(255),
                    9 => i32::from(!actor.collidable),
                    10 => i32::from(!actor.grounded),
                    11 => i32::from(!actor.casts_shadow),
                    12 => i32::from(actor.appearance.model_hidden),
                    13 => i32::from(!actor.cull_outside_view),
                    16 => i32::from(actor.appearance.expression),
                    17 => actor.properties.get(&17).copied().unwrap_or(2),
                    46 => i32::from(!actor.depth_write),
                    _ => unreachable!(),
                };
                if op == NativeCall::SetActorProperty {
                    match a[1] {
                        7 => {
                            let flags = a[2] & 15;
                            actor.properties.insert(7, flags);
                            if flags == 0 {
                                actor.appearance.fixed_heading = None;
                            } else if previous == 0 {
                                actor.appearance.fixed_heading = Some(actor.heading);
                            }
                        }
                        4 => actor.target_heading = (a[2] as f32).rem_euclid(360.),
                        1..=3 => actor.position[(a[1] - 1) as usize] = a[2] as f32,
                        8 => {
                            actor.properties.insert(8, i32::from(a[2] as u8));
                            actor.visible = a[2] as u8 != 0;
                        }
                        9 => actor.collidable = a[2] & 1 == 0,
                        10 => actor.grounded = a[2] & 1 == 0,
                        11 => actor.casts_shadow = a[2] & 1 == 0,
                        12 => actor.appearance.model_hidden = a[2] & 1 != 0,
                        13 => actor.cull_outside_view = a[2] & 1 == 0,
                        16 => actor.appearance.expression = a[2] as u8,
                        17 => {
                            actor.properties.insert(17, i32::from(a[2] as i16));
                        }
                        46 => actor.depth_write = a[2] & 1 == 0,
                        _ => unreachable!(),
                    }
                }
                value = Some(previous);
            }
            NativeCall::SpawnEvent => {
                require(self.events.len() < 32, "event command limit exceeded")?;
                let key = u32::try_from(a[0]).map_err(|_| "invalid event key")?;
                require(
                    self.program.event(2, key).is_some(),
                    "missing event resource",
                )?;
                value = Some(*self.next_handle);
                self.events.push(EventCommand {
                    handle: *self.next_handle,
                    action: EventAction::Spawn(key),
                });
                *self.next_handle = self
                    .next_handle
                    .checked_add(1)
                    .ok_or("event handle overflow")?;
            }
            NativeCall::ControlEvent => {
                require(self.events.len() < 32, "event command limit exceeded")?;
                self.events.push(EventCommand {
                    handle: a[0],
                    action: match a[1] as u8 {
                        0 | 1 => EventAction::Pause(a[1] as u8 == 1),
                        50 | 51 => EventAction::ControlGate(a[1] as u8 == 51),
                        _ => return Err("unknown event control command".into()),
                    },
                });
            }
            NativeCall::ConfigureRendering => match a[0] {
                0..=7 => {
                    self.world.render_settings.insert(a[0], a[1]);
                }
                128 => {
                    self.world.render_settings.insert(128, i32::from(a[1] != 0));
                }
                129 => {
                    self.world.render_settings.insert(129, 0);
                }
                _ => return Err("render configuration command is not implemented".into()),
            },
            NativeCall::CreateOverlay => {
                let resource = self.resources.resolve(a[1], ResourceKind::Overlay)?;
                self.world.insert_actor(
                    a[0],
                    Actor {
                        cull_outside_view: false,
                        grounded: false,
                        collidable: false,
                        casts_shadow: false,
                        ..Actor::new(resource, [a[2] as f32, a[3] as f32, 0.])
                    },
                );
                self.world.overlays.insert(
                    a[0],
                    Overlay {
                        born: self.world.tick,
                        size: [a[4], a[5]],
                        angle: a[6],
                        rgba: [a[7] as u8, a[8] as u8, a[9] as u8, a[10] as u8],
                        duration: a[11].max(0) as u32,
                        kind: if a[0] == 999_989 {
                            crate::world::OverlayKind::LocationCaption {
                                hold_ticks: a[12].max(0) as u32,
                            }
                        } else {
                            crate::world::OverlayKind::Sprite { depth: a[12] }
                        },
                    },
                );
            }
            NativeCall::SetEffectSetting => {
                require(
                    (-2..32).contains(&a[0]),
                    "character light selector is out of range",
                )?;
                let selector = if a[0] == -1 { 0 } else { a[0] };
                self.world
                    .character_lights
                    .entry(selector)
                    .or_default()
                    .set(a[1], [a[2], a[3], a[4]])?;
                self.world
                    .effect_settings
                    .insert((a[0], a[1]), [a[2], a[3], a[4]]);
            }
            NativeCall::PlayMovieBlocking | NativeCall::PlayMovie if a[0] != -1 => {
                let resource = u32::try_from(a[0]).map_err(|_| "invalid movie ID")?;
                require(
                    self.resources.movies.contains(&resource),
                    "movie is not cooked",
                )?;
                let movie = Movie {
                    resource,
                    blocking: op == NativeCall::PlayMovieBlocking,
                    operation: self.world.operations.begin()?,
                };
                self.world.voice = None;
                *self.wait = Some(if movie.blocking {
                    Wait::Complete(movie.operation.clone())
                } else {
                    Wait::Ready(movie.operation.clone())
                });
                if let Some(old) = self.world.movie.replace(movie) {
                    old.operation.cancel();
                }
                return Ok(NativeResult::Suspend);
            }
            NativeCall::PlayMovie => {
                require(a[0] == -1, "unsupported movie control")?;
                if let Some(movie) = &self.world.movie
                    && movie.operation.is_pending()
                {
                    movie.operation.complete(None)?;
                }
            }
            NativeCall::ShowChoice => {
                require((0..3).contains(&a[0]), "invalid choice slot")?;
                let slot = a[0] as u8;
                let dialogue = self
                    .world
                    .dialogue
                    .get(&slot)
                    .ok_or("choice dialogue is missing")?;
                require(dialogue.operation.is_pending(), "choice dialogue is closed")?;
                require(
                    !dialogue.persistent(),
                    "persistent dialogue cannot own a choice",
                )?;
                // Arguments specify first/last lines, timeout, and flags; the low
                // flag byte encodes the initial one-based line.
                require(
                    (0..=128).contains(&a[1]) && (1..=128).contains(&a[2]),
                    "invalid choice lines",
                )?;
                let first = (a[1] - 1).max(0);
                let last = a[2] - 1;
                require(
                    (0..128).contains(&last) && first <= last && last - first < 16,
                    "invalid choice line range",
                )?;
                require((0..=32767).contains(&a[3]), "invalid choice timeout")?;
                require((0..=0x1ff).contains(&a[4]), "unsupported choice flags")?;
                let initial = ((a[4] & 255) - 1).clamp(first, last);
                let choice = crate::dialogue::Choice {
                    operation: self.world.operations.begin()?,
                    first_line: first as u8,
                    last_line: last as u8,
                    selected_line: initial as u8,
                    cancel_allowed: a[4] & 0x100 == 0,
                    timeout_ticks: (a[3] > 0).then_some(a[3] as u16),
                };
                *self.wait = Some(Wait::Choice(choice.operation.clone()));
                if let Some(old) = self.world.choices.insert(slot, choice) {
                    old.operation.cancel();
                }
                return Ok(NativeResult::Suspend);
            }
            NativeCall::SetTransitionMode => {
                require(
                    (0..=3).contains(&a[0]) && a[1] >= 0,
                    "transition mode is not implemented",
                )?;
                let from = self
                    .world
                    .fade
                    .as_ref()
                    .map_or(255., |f| f.alpha(self.world.tick));
                self.world.fade = Some(Fade {
                    start_tick: self.world.tick,
                    duration: (a[1] as u32).max(1),
                    from,
                    to: if a[0] & 1 == 0 { 0. } else { 255. },
                    white: a[0] >= 2,
                });
            }
            NativeCall::YieldCommand => return self.yield_command(a[0], a[1]),
            // This command only consumes its argument; the VM has already done that.
            NativeCall::DiscardValue => {}
            NativeCall::ConfigureActorAnimation => {
                if a[1] == 0 {
                    if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                        actor.scripted_animation = false;
                        actor.animation = None;
                    }
                    return Ok(NativeResult::Continue(None));
                }
                let resolved = if a[1] == -1 {
                    None
                } else {
                    Some(self.resolve(a[1], ResourceKind::Model)?)
                };
                let actor = self
                    .world
                    .actors
                    .get_mut(&a[0])
                    .ok_or("animation actor missing")?;
                let resource = resolved.unwrap_or(actor.resource);
                let model = self
                    .resources
                    .model(resource)
                    .ok_or("animation resource missing")?;
                let requested =
                    u16::try_from(a[2].max(12)).map_err(|_| "invalid animation slot")?;
                let slot = if model.clips.contains_key(&requested) {
                    requested
                } else {
                    12
                };
                require(
                    model.clips.contains_key(&slot),
                    "animation slot is not cooked",
                )?;
                require(
                    matches!(a[4], 1 | 8),
                    "animation playback flags are not implemented",
                )?;
                actor.animation = Some(Animation {
                    blend_ticks: if a[3] < 0 { 1 } else { a[3] as u32 },
                    repeat: a[4] == 1,
                    ..Animation::new(
                        resource,
                        slot,
                        model.clips[&slot].duration_ticks,
                        self.world.tick,
                    )
                });
                actor.scripted_animation = true;
                self.world.pending_animation_bindings.insert(a[0]);
            }
            NativeCall::PlayCameraTrack => {
                require(a[1..] == [0, 0], "camera playback mode is not implemented")?;
                let resource = self.resources.resolve(a[0], ResourceKind::Camera)?;
                self.world.camera = Some(CameraTrack {
                    resource,
                    start_tick: self.world.tick,
                });
            }
            NativeCall::CreateSceneActor => {
                require(
                    a[4] == 0 && a[6..] == [0, 0],
                    "actor rotation/mode shim is not implemented",
                )?;
                let resource = self.resources.resolve(a[5], ResourceKind::Model)?;
                require(!self.world.actors.contains_key(&a[0]), "duplicate actor ID")?;
                require(self.world.actors.len() < 4096, "actor limit exceeded")?;
                self.world.insert_actor(
                    a[0],
                    Actor {
                        cull_outside_view: false,
                        grounded: false,
                        collidable: false,
                        casts_shadow: false,
                        ..Actor::new(resource, [a[1] as f32, a[2] as f32, a[3] as f32])
                    },
                );
            }
            NativeCall::FindActorNode => {
                let name = self
                    .program
                    .string(u16::try_from(a[1]).map_err(|_| "invalid name index")?)
                    .ok_or("name is missing")?;
                let result = self
                    .world
                    .actors
                    .get(&a[0])
                    .and_then(|a| self.resources.model(a.resource))
                    .and_then(|m| m.names.iter().position(|n| n.as_bytes() == name));
                value = Some(result.map_or(-1, |i| i as i32));
            }
            NativeCall::ReadCoordinateRegister => {
                value = Some(
                    *self
                        .registers
                        .get(usize::try_from(a[0]).map_err(|_| "invalid coordinate register")?)
                        .ok_or("invalid coordinate register")?,
                )
            }
            NativeCall::ReadActorAttachment => {
                let point = self.attachment(a[0], a[1])?;
                self.registers[..3].copy_from_slice(&point);
            }
            NativeCall::CreateParticle => {
                let kind = self
                    .resources
                    .particles
                    .get(&a[0])
                    .ok_or("particle kind is not cooked")?;
                require(
                    (0..=0x7fff).contains(&a[1]) && a[12] == 0,
                    "particle lifetime/color mode is not implemented",
                )?;
                let (rgba, flutter) = match kind {
                    crate::ParticleKind::Glow => {
                        require(a[11] == 0, "particle color is not cooked")?;
                        ([255., 255., 255., a[9] as f32], None)
                    }
                    crate::ParticleKind::Flutter(recipe) => {
                        let mut rgba = recipe
                            .palette
                            .get(a[11] as usize)
                            .ok_or("particle color is not cooked")?
                            .map(f32::from);
                        rgba[3] = f32::from(a[9] as u8);
                        (rgba, Some(crate::effect::Flutter::new(recipe)))
                    }
                };
                let handle = self.world.emit_particle(Particle {
                    kind: a[0],
                    handle: 0,
                    born: self.world.tick,
                    lifetime: a[1] as u32,
                    position: [a[2] as f32, a[3] as f32, a[4] as f32],
                    velocity: [a[5] as f32, a[6] as f32, a[7] as f32],
                    size: a[8] as f32,
                    size_delta: 0.,
                    rgba,
                    alpha_delta: a[10] as f32,
                    flutter,
                })?;
                value = Some(handle);
            }
            NativeCall::SetEffectProperty => {
                if self.world.billboards.contains_key(&a[0]) {
                    return self.field(op, a, memory);
                }
                let p = self
                    .world
                    .particles
                    .iter_mut()
                    .find(|p| p.handle == a[0])
                    .ok_or("effect handle missing")?;
                match a[1] {
                    125..=128 => p.rgba[(a[1] - 125) as usize] = (a[2] as u8) as f32,
                    135 => p.size_delta = a[2] as f32 / 100.,
                    _ => return Err("unsupported effect property".into()),
                }
                value = Some(0);
            }
            _ => return Err(format!("unimplemented native {op:?}")),
        }
        Ok(NativeResult::Continue(value))
    }
}
impl NativeHost<'_> {
    fn invoke(
        &mut self,
        call: NativeCall,
        arguments: &[i32],
        memory: &mut Memory,
        handler: fn(&mut Self, NativeCall, &[i32], &mut Memory) -> Result<NativeResult, String>,
    ) -> Result<NativeResult, String> {
        if self.world.skit.is_some()
            && matches!(
                call,
                NativeCall::DespawnActor
                    | NativeCall::PlayMovie
                    | NativeCall::YieldCommand
                    | NativeCall::SetActorProperty
                    | NativeCall::GetActorProperty
            )
        {
            return self
                .skit(call, arguments, memory)
                .map_err(|e| format!("{call:?} ({:#04x}) {arguments:?}: {e}", call as u8));
        }
        // Resolve the controlled-actor alias before adapting actor-service arguments.
        let mut adapted;
        let values = if arguments.first() == Some(&crate::CONTROLLED_ACTOR)
            && matches!(
                call,
                NativeCall::SetActorHeading
                    | NativeCall::ActorExists
                    | NativeCall::SelectActor
                    | NativeCall::MoveActor
                    | NativeCall::SetActorPosition
                    | NativeCall::SetActorOrientation
                    | NativeCall::ConfigureActorAnimation
                    | NativeCall::SetActorAnimationProperty
                    | NativeCall::FindActorNode
                    | NativeCall::SetActorAnimation
                    | NativeCall::SetActorFace
                    | NativeCall::SetActorMouth
                    | NativeCall::ReadActorAttachment
            ) {
            adapted = arguments.to_vec();
            adapted[0] = self.world.controlled_actor;
            adapted.as_slice()
        } else {
            arguments
        };
        handler(self, call, values, memory)
            .map_err(|e| format!("{call:?} ({:#04x}) {arguments:?}: {e}", call as u8))
    }
}
