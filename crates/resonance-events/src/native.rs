//! Native service shims registered by typed call ID. The VM knows neither
//! actors nor assets; these handlers operate independently of scene/event IDs.
use crate::animation::slot;
use crate::world::{Fade, Overlay, OverlayKind, SpriteOverlay};
use crate::{Actor, Animation, CameraTrack, GameWorld, Particle, ResourceKind, ResourceLibrary};
use crate::{
    dialogue::{DIALOGUE_SLOTS, Dialogue, DialogueAnchor, Movie, flags},
    operation::Wait,
};
use symphonia_script::{NativeCall, Program};
use symphonia_script_vm::{Memory, NativeResult};
mod battle;
mod bindings;
mod camera_path;
mod field;
mod menu;
mod party;
mod skit;
mod wait;

pub(crate) struct EventCommand {
    /// One-based pool slot, matching the original reusable VM pointers.
    pub handle: i32,
    pub action: EventAction,
}
pub(crate) enum EventAction {
    Spawn(u32),
    Pause(bool),
    ControlGate(bool),
    Release,
}

pub(crate) type MotionResolver<'a> = dyn FnMut(crate::MotionUpdate, i32, &mut Actor, [f32; 3]) + 'a;

pub(crate) struct NativeHost<'a> {
    pub world: &'a mut GameWorld,
    pub resolve_motion: &'a mut MotionResolver<'a>,
    pub resources: &'a ResourceLibrary,
    pub program: &'a Program,
    pub event_actor: i16,
    pub registers: &'a mut [i32; 6],
    pub events: &'a mut Vec<EventCommand>,
    pub free_slots: u32,
    pub wait: &'a mut Option<Wait>,
    pub resource_waits: Option<&'a std::collections::VecDeque<crate::ResourceWaitObservation>>,
    pub resource_wait: &'a mut Option<crate::ResourceWaitObservation>,
}
fn require(ok: bool, what: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(what.into()) }
}
const MOVEMENT_SPEED: i32 = 5;
fn sprite_property(
    actor: &mut Actor,
    overlay: &mut Overlay,
    selector: i32,
    value: Option<i32>,
) -> Option<i32> {
    let OverlayKind::Sprite(sprite) = &mut overlay.kind else {
        return None;
    };
    let previous = match selector {
        8 => actor.properties.get(&8).copied().unwrap_or(0),
        15 => sprite.alpha_step as i32,
        30..=32 => (sprite.scale[(selector - 30) as usize] * 100.) as i32,
        4 | 37 => actor.heading as i32,
        42..=44 => i32::from(overlay.rgba[(selector - 42) as usize]),
        62 => i32::from(sprite.image),
        _ => return None,
    };
    if let Some(value) = value {
        match selector {
            4 => actor.target_heading = value as f32,
            8 => {
                overlay.rgba[3] = value as u8;
                actor.properties.insert(8, i32::from(value as u8));
            }
            15 => sprite.alpha_step = value as f32,
            30..=32 => sprite.scale[(selector - 30) as usize] = value as f32 / 100.,
            37 => {
                actor.heading = value as f32;
                actor.target_heading = actor.heading;
            }
            42..=44 => overlay.rgba[(selector - 42) as usize] = value as u8,
            62 => sprite.image = value as u8,
            _ => unreachable!(),
        }
    }
    Some(previous)
}
impl NativeHost<'_> {
    // fn_8004C628 invokes the actor immediately, including when clearing an
    // override. Resolve movement before the next script instruction observes it.
    fn update_bound_actor(&mut self, id: i32) {
        let actor = self.world.actors.get_mut(&id).unwrap();
        let previous = actor.position;
        actor.step_motion();
        (self.resolve_motion)(
            crate::MotionUpdate::AnimationBinding {
                event_paused: self.world.mapped_input_disabled,
                input_enabled: self.world.input_enabled,
            },
            id,
            actor,
            previous,
        );
        actor.step_heading(
            self.world.input_enabled && id == self.world.controlled_actor,
            actor.motion.is_some() || actor.position[..2] != previous[..2],
        );
    }
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
        if let Some((ResourceKind::UnboundGeometry, id)) =
            self.world.loaded_resources.get(&script_id)
        {
            return Err(format!(
                "geometry resource {id:#x} requires caller-supplied textures"
            ));
        }
        self.world
            .loaded_resources
            .get(&script_id)
            .filter(|(k, _)| *k == kind)
            .map(|(_, id)| *id)
            .map(Ok)
            .unwrap_or_else(|| self.resources.resolve(script_id, kind))
    }
    fn attachment(&self, id: i32, node: i32) -> Result<[i32; 3], String> {
        let Some(actor) = self.world.actors.get(&id) else {
            return Ok([0; 3]);
        };
        let node = usize::try_from(node).map_err(|_| "invalid bone index")?;
        self.resources
            .attachment_point(actor, node, self.world.tick)
            .map(|point| point.map(|v| v.trunc() as i32))
    }
    fn dispatch(
        &mut self,
        op: NativeCall,
        a: &[i32],
        memory: &mut Memory,
    ) -> Result<NativeResult, String> {
        let mut value = None;
        match op {
            NativeCall::Atan2Degrees => {
                let angle = if a[0] == 0 && a[1] == 0 {
                    0.
                } else {
                    (a[0] as f32).atan2(a[1] as f32).to_degrees()
                };
                value = Some(angle as i32);
            }
            NativeCall::ScaledSquareRoot => {
                const SCALE: f32 = 1000.;
                let sample = a[0] as f32 / SCALE;
                let root = if sample > 0. { sample.sqrt() } else { sample };
                value = Some((root * SCALE) as i32);
            }
            NativeCall::SinDegrees | NativeCall::CosDegrees => {
                let angle = (a[0] as f32).to_radians();
                value = Some(
                    (4096.
                        * if op == NativeCall::SinDegrees {
                            angle.sin()
                        } else {
                            angle.cos()
                        }) as i32,
                );
            }
            NativeCall::SetActorAnimationFlags => {
                let id = if a[0] == crate::CONTROLLED_ACTOR {
                    self.world.controlled_actor
                } else {
                    a[0]
                };
                if let Some(animation) = self
                    .world
                    .actors
                    .get_mut(&id)
                    .and_then(|actor| actor.animation.as_mut())
                {
                    let flags = a[1] as u16;
                    animation.script_pause(flags & 2 != 0, self.world.tick);
                    if flags != 0 && flags != 2 {
                        animation.repeat = flags & 8 == 0;
                    }
                }
            }
            NativeCall::IsDebugSession => value = Some(i32::from(self.world.debug_session)),
            NativeCall::ConfigureScreenCopy => {
                require((0..=5).contains(&a[0]), "unknown screen copy command")?;
                let index = (a[0] & 1) as usize;
                let previous = self.world.screen_copy_depth[index];
                if a[0] < 2 {
                    self.world.screen_copy_depth[index] = a[1] as f32 / 100.;
                } else if a[0] >= 4 {
                    self.world.screen_copy_depth[index] = [174., 190.][index];
                }
                value = Some((previous * 100.) as i32);
            }
            NativeCall::WaitActorAnimationFrame => {
                *self.wait = Some(Wait::Service {
                    condition: Box::new(Wait::ActorAnimationFrame(a[0], a[1])),
                    ready_at: None,
                });
                return Ok(NativeResult::Suspend);
            }
            NativeCall::IsActorAnimationFinished => {
                let id = if a[0] == crate::CONTROLLED_ACTOR {
                    self.world.controlled_actor
                } else {
                    a[0]
                };
                value = Some(i32::from(
                    self.world
                        .actors
                        .get(&id)
                        .and_then(|actor| actor.animation.as_ref())
                        .is_some_and(|animation| {
                            !animation.repeat
                                && animation.elapsed(self.world.tick, 0)
                                    >= animation.duration_ticks as f32
                        }),
                ));
            }
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
                        && self
                            .world
                            .actors
                            .get(&a[3])
                            .is_some_and(|actor| actor.heading != actor.target_heading))
                    .then_some(a[3]),
                    flags,
                    dimensions,
                    height_offset: if dimensions.is_none() { a[5] as i16 } else { 0 },
                };
                if let Some(old) = self.world.dialogue.insert(a[0] as u8, dialogue) {
                    old.operation.cancel();
                }
            }
            NativeCall::GetEventActor => value = Some(i32::from(self.event_actor)),
            NativeCall::SetActorPosition => {
                // Position updates for absent actors are ignored.
                if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                    actor.position = [a[1] as f32, a[2] as f32, a[3] as f32];
                }
            }
            NativeCall::GetActorProperty | NativeCall::SetActorProperty => {
                const CONDITIONS: i32 = 100;
                const SHADE_RED: i32 = 57;
                const SHADE_BLUE: i32 = 59;
                const DISABLE_SECONDARY_MOTION: i32 = 40;
                const TOON_LIGHTING: i32 = 38;
                // Ordinary property writes return the previous value.
                let id = if a[0] == crate::CONTROLLED_ACTOR {
                    self.world.controlled_actor
                } else {
                    a[0]
                };
                if a[1] == 49 {
                    // Both native property commands only read this detection bit.
                    let alert = self
                        .world
                        .actors
                        .get(&id)
                        .and_then(|actor| actor.enemy.as_ref())
                        .is_some_and(|enemy| enemy.alerted);
                    return Ok(NativeResult::Continue(Some(i32::from(alert))));
                }
                if (SHADE_RED..=SHADE_BLUE).contains(&a[1]) {
                    let color = self.world.actors.get(&id).map_or(0, |actor| {
                        let light = actor.light.clone().unwrap_or_default();
                        i32::from(light.shade[(a[1] - SHADE_RED) as usize])
                    });
                    return Ok(NativeResult::Continue(Some(color)));
                }
                if a[1] == crate::emitter::PHASE_PROPERTY || (113..=122).contains(&a[1]) {
                    let previous = self
                        .world
                        .actors
                        .get_mut(&id)
                        .and_then(|actor| actor.emitter.as_mut())
                        .map(|emitter| {
                            emitter
                                .property(a[1], (op == NativeCall::SetActorProperty).then(|| a[2]))
                        })
                        .transpose()?
                        .unwrap_or(0);
                    return Ok(NativeResult::Continue(Some(previous)));
                }
                if let (Some(actor), Some(overlay)) = (
                    self.world.actors.get_mut(&id),
                    self.world.overlays.get_mut(&id),
                ) && let Some(previous) = sprite_property(
                    actor,
                    overlay,
                    a[1],
                    (op == NativeCall::SetActorProperty).then(|| a[2]),
                ) {
                    return Ok(NativeResult::Continue(Some(previous)));
                }
                require(
                    matches!(a[1], 1..=4 | MOVEMENT_SPEED | 7..=23 | 26..=27 | 30..=32 | 34..=37 | TOON_LIGHTING | 39 | DISABLE_SECONDARY_MOTION | 41..=48 | 50..=51 | 53..=54 | 56 | 66 | CONDITIONS | 101 | 102 | 104 | 112)
                        && (a[1] != 112 || op == NativeCall::GetActorProperty),
                    "actor property shim is not implemented",
                )?;
                if a[1] == CONDITIONS {
                    let member = self.world.party.as_mut().and_then(|party| {
                        usize::try_from(id - 1)
                            .ok()
                            .and_then(|id| party.members.get_mut(id))
                    });
                    let previous = member.map_or(0, |member| {
                        let previous = member.conditions as i32;
                        if op == NativeCall::SetActorProperty {
                            member.conditions = a[2] as u32;
                        }
                        previous
                    });
                    return Ok(NativeResult::Continue(Some(previous)));
                }
                if matches!(a[1], 102 | 104) {
                    let member = self.world.party.as_mut().and_then(|party| {
                        usize::try_from(id - 1)
                            .ok()
                            .and_then(|id| party.members.get_mut(id))
                    });
                    let previous = member.map_or(0, |member| {
                        let stat = if a[1] == 102 {
                            &mut member.hp
                        } else {
                            &mut member.tp
                        };
                        let previous = i32::from(*stat as i16);
                        if op == NativeCall::SetActorProperty {
                            *stat = a[2] as u16;
                        }
                        previous
                    });
                    return Ok(NativeResult::Continue(Some(previous)));
                }
                if a[1] == 101 {
                    // Party level is queried independently of a rendered actor.
                    let level = self
                        .world
                        .party
                        .as_ref()
                        .and_then(|party| {
                            usize::try_from(id - 1)
                                .ok()
                                .and_then(|id| party.members.get(id))
                        })
                        .map_or(0, |member| i32::from(member.level));
                    return Ok(NativeResult::Continue(Some(
                        if op == NativeCall::GetActorProperty {
                            level
                        } else {
                            0
                        },
                    )));
                }
                if a[1] == 112 {
                    let luck = if (1..=9).contains(&id) {
                        let party = self
                            .world
                            .party
                            .as_ref()
                            .ok_or("party is not initialized")?;
                        let data = self
                            .resources
                            .menu_data
                            .as_ref()
                            .ok_or("menu data is not cooked")?;
                        i32::from(party.members[id as usize - 1].stats(data).luck)
                    } else {
                        0
                    };
                    return Ok(NativeResult::Continue(Some(luck)));
                }
                if a[1] == 66 {
                    let luck = if op == NativeCall::SetActorProperty
                        && (1..=9).contains(&id)
                        && self.world.actors.contains_key(&id)
                    {
                        let party = self
                            .world
                            .party
                            .as_mut()
                            .ok_or("party is not initialized")?;
                        let member = &mut party.members[id as usize - 1];
                        member.luck =
                            (crate::world::random(&mut self.world.random_state) % 100) as u8;
                        // This command rerolls luck; its value argument is unused.
                        i32::from(member.luck) * 10
                    } else {
                        0
                    };
                    return Ok(NativeResult::Continue(Some(luck)));
                }
                if a[1] == 53 {
                    let block = self.world.grabbed_block.filter(|id| {
                        self.world
                            .actors
                            .get(id)
                            .is_some_and(crate::Actor::pushable)
                    });
                    return Ok(NativeResult::Continue(Some(block.unwrap_or(0))));
                }
                let Some(actor) = self.world.actors.get_mut(&id) else {
                    return Ok(NativeResult::Continue(Some(0)));
                };
                let previous = match a[1] {
                    1..=3 => actor.position[(a[1] - 1) as usize] as i32,
                    4 => actor.heading as i32,
                    MOVEMENT_SPEED => actor.movement_speed() as i32,
                    7 => actor.properties.get(&7).copied().unwrap_or(0),
                    8 => actor.properties.get(&8).copied().unwrap_or(255),
                    9 => i32::from(!actor.collidable),
                    10 => i32::from(!actor.grounded),
                    11 => i32::from(!actor.casts_shadow),
                    12 => i32::from(actor.appearance.model_hidden),
                    13 | 14 => actor
                        .properties
                        .get(&a[1])
                        .copied()
                        .unwrap_or(i32::from(!actor.cull_outside_view)),
                    15 => {
                        actor
                            .autonomy
                            .as_ref()
                            .ok_or("actor wandering state is missing")?
                            .radius as i32
                    }
                    16 => i32::from(actor.appearance.expression),
                    17 => actor.interaction_label(),
                    18 => i32::from(actor.model_collision.is_some()),
                    19 => i32::from(actor.pushable()),
                    20 => i32::from(actor.contact_event),
                    21..=22 => actor.enemy.as_ref().map_or(0, |enemy| {
                        i32::from(enemy.event_parameters[(a[1] - 21) as usize] as u16)
                    }),
                    23 => actor
                        .enemy
                        .as_ref()
                        .map_or(0, |enemy| enemy.normal_speed as i32),
                    26 => actor
                        .enemy
                        .as_ref()
                        .map_or(0, |enemy| i32::from(enemy.behavior)),
                    27 => actor.enemy.as_ref().map_or(0, |enemy| {
                        actor
                            .properties
                            .get(&27)
                            .copied()
                            .unwrap_or(i32::from(enemy.random_turns))
                    }),
                    30..=32 => actor.properties.get(&a[1]).copied().unwrap_or(100),
                    34 => actor.autonomy.as_ref().map_or(0, |ai| ai.behavior as i32),
                    35 | 36 => actor.properties.get(&a[1]).copied().unwrap_or(0),
                    37 => actor.heading as i32,
                    TOON_LIGHTING => actor
                        .properties
                        .get(&TOON_LIGHTING)
                        .copied()
                        .unwrap_or_else(|| {
                            i32::from(
                                self.resources
                                    .model(actor.resource)
                                    .is_some_and(|model| model.toon_lighting),
                            )
                        }),
                    39 => actor.properties.get(&39).copied().unwrap_or(2),
                    DISABLE_SECONDARY_MOTION => {
                        i32::from(actor.appearance.secondary_motion_disabled)
                    }
                    47 => actor.radius as i32,
                    51 => i32::from(actor.shadow_alpha),
                    54 => actor
                        .enemy
                        .as_ref()
                        .map_or(0, |enemy| i32::from(enemy.contact_cooldown)),
                    56 => actor
                        .enemy
                        .as_ref()
                        .map_or(0, |enemy| i32::from(enemy.pause_effect_mode)),
                    41 | 48 | 50 => actor.properties.get(&a[1]).copied().unwrap_or(0),
                    42..=44 => actor
                        .properties
                        .get(&a[1])
                        .copied()
                        .unwrap_or(i32::from(crate::effect::NEUTRAL_TINT)),
                    46 => i32::from(!actor.depth_write),
                    45 => actor.blend.map_or(0, |blend| blend as i32),
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
                        MOVEMENT_SPEED => actor.set_movement_speed(a[2] as f32),
                        1..=3 => actor.position[(a[1] - 1) as usize] = a[2] as f32,
                        8 => {
                            actor.properties.insert(8, i32::from(a[2] as u8));
                            actor.visible = a[2] as u8 != 0;
                        }
                        9 => actor.collidable = a[2] & 1 == 0,
                        10 => actor.grounded = a[2] & 1 == 0,
                        11 => actor.casts_shadow = a[2] & 1 == 0,
                        12 => actor.appearance.model_hidden = a[2] & 1 != 0,
                        13 | 14 => {
                            let other = if a[1] == 13 { 14 } else { 13 };
                            actor.properties.insert(a[1], a[2] & 1);
                            actor.cull_outside_view = a[2] & 1 == 0
                                && actor.properties.get(&other).copied().unwrap_or(0) == 0;
                        }
                        15 => actor.autonomy.as_mut().unwrap().radius = a[2] as f32,
                        16 => actor.appearance.expression = a[2] as u8,
                        17 => {
                            actor.properties.insert(17, i32::from(a[2] as i16));
                        }
                        18 => {
                            actor.model_collision = if a[2] & 1 != 0 {
                                Some(
                                    self.resources
                                        .model(actor.resource)
                                        .ok_or("collision model is missing")?
                                        .collision
                                        .clone(),
                                )
                            } else {
                                None
                            };
                        }
                        19 => {
                            actor.properties.insert(19, a[2] & 1);
                            if actor.pushable() {
                                // fn_8001A6FC selects the block controller and
                                // its contact radius when property 19 is set.
                                actor.radius = 50.;
                            }
                        }
                        21..=22 => {
                            if let Some(enemy) = &mut actor.enemy {
                                enemy.event_parameters[(a[1] - 21) as usize] = a[2] as i16;
                            }
                        }
                        23 => {
                            if let Some(enemy) = &mut actor.enemy {
                                enemy.normal_speed = a[2] as f32;
                            }
                        }
                        26 => {
                            if let Some(enemy) = &mut actor.enemy {
                                enemy.behavior = a[2] as u8;
                                let behavior = crate::Behavior::enemy(enemy.behavior);
                                actor
                                    .autonomy
                                    .get_or_insert_with(|| {
                                        crate::Autonomy::new(
                                            behavior,
                                            enemy.normal_speed,
                                            actor.position,
                                        )
                                    })
                                    .set_behavior(behavior);
                            }
                        }
                        27 => {
                            if let Some(enemy) = &mut actor.enemy {
                                enemy.random_turns = a[2] as u8 != 0;
                                actor.properties.insert(27, i32::from(a[2] as u8));
                            }
                        }
                        20 => actor.contact_event = a[2] & 1 != 0,
                        34 => {
                            let behavior = crate::Behavior::try_from(i32::from(a[2] as u8))
                                .map_err(|e| e.to_string())?;
                            actor
                                .autonomy
                                .get_or_insert_with(|| {
                                    crate::Autonomy::new(behavior, 0., actor.position)
                                })
                                .set_behavior(behavior);
                        }
                        37 => actor.face(a[2] as f32),
                        TOON_LIGHTING => {
                            actor
                                .properties
                                .insert(TOON_LIGHTING, i32::from(a[2] as u8));
                        }
                        39 => {
                            actor.properties.insert(39, i32::from(a[2] as i8));
                        }
                        DISABLE_SECONDARY_MOTION => {
                            actor.appearance.secondary_motion_disabled = a[2] & 1 != 0
                        }
                        47 => actor.radius = a[2] as f32,
                        51 => actor.shadow_alpha = a[2] as u8,
                        54 => {
                            if let Some(enemy) = &mut actor.enemy {
                                enemy.contact_cooldown = a[2] as i16;
                            }
                        }
                        56 => {
                            if let Some(enemy) = &mut actor.enemy {
                                require(
                                    !matches!(a[2] as u8, 4 | 6),
                                    "enemy pause effect is not implemented",
                                )?;
                                enemy.pause_effect_mode = a[2] as u8;
                            }
                        }
                        46 => actor.depth_write = a[2] & 1 == 0,
                        45 => {
                            require(a[2] & 7 <= 2, "actor blend mode is not implemented")?;
                            actor.blend = Some((a[2] & 7).try_into()?);
                        }
                        30..=32 | 35 | 36 => {
                            actor.properties.insert(a[1], a[2]);
                        }
                        42..=44 => {
                            actor.properties.insert(a[1], i32::from(a[2] as u8));
                        }
                        41 | 48 | 50 => {
                            actor.properties.insert(a[1], a[2] & 1);
                        }
                        _ => unreachable!(),
                    }
                }
                value = Some(previous);
            }
            NativeCall::ReleaseResourceInstance => {
                require(self.events.len() < 32, "event command limit exceeded")?;
                self.events.push(EventCommand {
                    handle: a[0],
                    action: EventAction::Release,
                });
            }
            NativeCall::StopCameraTrack => self.world.camera = None,
            NativeCall::SpawnEvent => {
                require(self.events.len() < 32, "event command limit exceeded")?;
                let key = u32::try_from(a[0]).map_err(|_| "invalid event key")?;
                require(
                    self.program.event(2, key).is_some(),
                    "missing event resource",
                )?;
                let slot = self.free_slots.trailing_zeros();
                require(slot < 32, "event pool exhausted (32 instances)")?;
                self.free_slots &= !(1 << slot);
                let handle = slot as i32 + 1;
                value = Some(handle);
                self.events.push(EventCommand {
                    handle,
                    action: EventAction::Spawn(key),
                });
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
                    self.world.texture_animation_tick = 0;
                }
                _ => return Err("render configuration command is not implemented".into()),
            },
            NativeCall::CreateOverlay => {
                let resource = self.resolve(a[1], ResourceKind::Overlay)?;
                self.world.insert_actor(
                    a[0],
                    Actor {
                        heading: a[6] as f32,
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
                        rgba: [a[7] as u8, a[8] as u8, a[9] as u8, a[10] as u8],
                        duration: a[11].max(0) as u32,
                        kind: if a[0] == 999_989 {
                            OverlayKind::LocationCaption {
                                hold_ticks: a[12].max(0) as u32,
                            }
                        } else {
                            OverlayKind::Sprite(SpriteOverlay::new(a[12], a[10] as u8, a[11]))
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
                use crate::dialogue::{ChoiceConfirmation, choice_flags};
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
                require(a[4] & !choice_flags::ALL == 0, "unsupported choice flags")?;
                let initial = ((a[4] & choice_flags::INITIAL_LINE) - 1).clamp(first, last);
                let choice = crate::dialogue::Choice {
                    operation: self.world.operations.begin()?,
                    first_line: first as u8,
                    last_line: last as u8,
                    selected_line: initial as u8,
                    cancel_allowed: a[4] & choice_flags::DISABLE_CANCEL == 0,
                    confirmation: if a[4] & choice_flags::SHOULDER_CONFIRM != 0 {
                        ChoiceConfirmation::AcceptOrShoulder
                    } else {
                        ChoiceConfirmation::Accept
                    },
                    timeout_ticks: (a[3] > 0).then_some(a[3] as u16),
                };
                *self.wait = Some(Wait::Choice {
                    result: choice.operation.clone(),
                    window: Box::new(Wait::Service {
                        condition: Box::new(Wait::Complete(dialogue.operation.clone())),
                        ready_at: None,
                    }),
                });
                if let Some(old) = self.world.choices.insert(slot, choice) {
                    old.operation.cancel();
                }
                return Ok(NativeResult::Suspend);
            }
            NativeCall::SetTransitionMode => {
                require(
                    (0..=5).contains(&a[0]) && a[1] >= 0,
                    "transition mode is not implemented",
                )?;
                if a[0] == 5 {
                    // This is the next scene's clear color, not a duration.
                    require(
                        matches!(a[1] as u8, 0 | 255),
                        "unsupported transition clear shade",
                    )?;
                    self.world.next_transition_white = Some(a[1] as u8 == 255);
                    return Ok(NativeResult::Continue(None));
                }
                let from = self
                    .world
                    .fade
                    .as_ref()
                    .map_or(255., |f| f.before_update(self.world.tick));
                if a[0] == 4 {
                    // fn_8004E260 -> fn_80018928: keep a captured scene at 255,
                    // reducing its opacity by 256/duration after each draw.
                    let duration = if a[1] == 0 { 10 } else { a[1] as u32 };
                    self.world.scene_dissolve = Some(crate::world::SceneDissolve {
                        start_tick: self.world.tick,
                        duration,
                    });
                    // This command shares the ordinary fade's rate variable;
                    // an already-visible color overlay continues increasing.
                    if from >= 1. {
                        self.world.fade = Some(Fade::new(
                            self.world.tick,
                            duration,
                            from,
                            from + 256.,
                            self.world.fade.as_ref().is_some_and(|f| f.white),
                        ));
                    }
                    return Ok(NativeResult::Continue(None));
                }
                self.world.fade = Some(Fade::new(
                    self.world.tick,
                    a[1] as u32,
                    from,
                    if a[0] & 1 == 0 { 0. } else { 256. },
                    a[0] >= 2,
                ));
            }
            NativeCall::YieldCommand => return self.yield_command(a[0], a[1]),
            // This command only consumes its argument; the VM has already done that.
            NativeCall::DiscardValue => {}
            NativeCall::ConfigureActorAnimation => {
                use crate::animation::AnimationSource;
                // Native lookup precedes resource resolution; removed scenery is a no-op.
                if !self.world.actors.contains_key(&a[0]) {
                    return Ok(NativeResult::Continue(None));
                }
                if a[1] == 0 {
                    if let Some(actor) = self.world.actors.get_mut(&a[0]) {
                        actor.scripted_animation = false;
                        // Releasing the override does not erase the evaluated model pose.
                        // Attachment reads remain valid until locomotion selects its clip.
                    }
                    self.update_bound_actor(a[0]);
                    return Ok(NativeResult::Continue(None));
                }
                let resolved = if a[1] == -1 {
                    None
                } else {
                    Some(match self.world.loaded_resources.get(&a[1]) {
                        Some((ResourceKind::Animation, id)) => (*id, AnimationSource::Resource),
                        _ => (
                            self.resolve(a[1], ResourceKind::Model)?,
                            AnimationSource::Model,
                        ),
                    })
                };
                let actor = self
                    .world
                    .actors
                    .get_mut(&a[0])
                    .ok_or("animation actor missing")?;
                let (resource, source) =
                    resolved.unwrap_or((actor.resource, AnimationSource::Model));
                let clips = self
                    .resources
                    .clips(resource, source)
                    .ok_or("animation resource missing")?;
                let requested =
                    u16::try_from(a[2].max(12)).map_err(|_| "invalid animation slot")?;
                let slot = if clips.contains_key(&requested) {
                    requested
                } else {
                    12
                };
                require(clips.contains_key(&slot), "animation slot is not cooked")?;
                require(
                    a[4] & !11 == 0,
                    "animation playback flags are not implemented",
                )?;
                actor.animation = Some(Animation {
                    source,
                    blend_ticks: if a[3] < 0 { 1 } else { a[3] as u32 },
                    repeat: a[4] & 8 == 0,
                    rate: if a[4] & 2 != 0 { 0. } else { 1. },
                    paused_rate: (a[4] & 2 != 0).then_some(1.),
                    ..Animation::new(resource, slot, clips[&slot].duration_ticks, self.world.tick)
                });
                actor.scripted_animation = true;
                self.world.pending_animation_bindings.insert(a[0]);
                self.update_bound_actor(a[0]);
                let size = if a[0] == self.world.controlled_actor {
                    self.world.player_size.model_scale()
                } else {
                    1.
                };
                self.world
                    .actors
                    .get_mut(&a[0])
                    .unwrap()
                    .record_animation_binding(self.world.tick, size);
            }
            NativeCall::PlayCameraTrack => {
                require(a[1..] == [0, 0], "camera playback mode is not implemented")?;
                let resource = self.resolve(a[0], ResourceKind::Camera)?;
                self.world.camera = Some(CameraTrack {
                    resource,
                    start_tick: self.world.tick,
                });
            }
            NativeCall::CreateSceneActor | NativeCall::SpawnInteractionActor => {
                require(
                    a[6..] == [0, 0],
                    "scene actor movement mode is not implemented",
                )?;
                let locator = self.resources.locators.contains(&a[5]);
                let interaction = op == NativeCall::SpawnInteractionActor;
                let resource = if locator {
                    a[5] as u32
                } else {
                    self.resolve(a[5], ResourceKind::Model)?
                };
                require(self.world.actors.len() < 4096, "actor limit exceeded")?;
                // fn_80059838 allocates a fresh object even when its script key
                // is already in use. Setters keep addressing the first object.
                let key = self.world.scene_actor_key(a[0])?;
                if key != a[0] {
                    self.world.duplicate_actors.insert(key, a[0]);
                }
                // fn_80059838 initializes the model immediately. Following calls
                // can pause it before the first scheduler update (Thoda's rocks).
                let model = self.resources.model(resource);
                let animation = model
                    .and_then(|model| model.clips.get(&slot::IDLE))
                    .map(|clip| {
                        Animation::new(resource, slot::IDLE, clip.duration_ticks, self.world.tick)
                    });
                self.world.insert_actor(
                    key,
                    Actor {
                        heading: (a[4] as f32).rem_euclid(360.),
                        target_heading: (a[4] as f32).rem_euclid(360.),
                        cull_outside_view: false,
                        grounded: false,
                        collidable: false,
                        casts_shadow: false,
                        visible: !locator,
                        interaction_anchor: locator,
                        // fn_8001A6FC hides optional "kk" geometry for these
                        // constructors too, including Mana's remote Lloyd.
                        appearance: crate::Appearance {
                            hidden_nodes: model.map(|m| m.hidden_nodes.clone()).unwrap_or_default(),
                            ..Default::default()
                        },
                        role: if interaction {
                            crate::ActorRole::Interaction
                        } else {
                            crate::ActorRole::Ordinary
                        },
                        properties: if locator {
                            [(17, 0), (48, i32::from(!interaction))].into()
                        } else {
                            Default::default()
                        },
                        animation,
                        ..Actor::new(resource, [a[1] as f32, a[2] as f32, a[3] as f32])
                    },
                );
            }
            NativeCall::FaceActorAfterMovement => {
                let actor = if a[0] == crate::CONTROLLED_ACTOR {
                    self.world.controlled_actor
                } else {
                    a[0]
                };
                *self.wait = Some(Wait::FaceAfterMotion {
                    actor,
                    heading: (a[1] as f32).rem_euclid(360.),
                });
                return Ok(NativeResult::Suspend);
            }
            NativeCall::TransformActorNode => {
                // fn_8004568C's rotation branch is used by authored doorway hinges.
                if a[2] == -1 {
                    return Ok(NativeResult::Continue(None));
                }
                let id = if a[1] == crate::CONTROLLED_ACTOR {
                    self.world.controlled_actor
                } else {
                    a[1]
                };
                if let Some(actor) = self.world.actors.get_mut(&id) {
                    require(
                        matches!(a[0], 2 | 3),
                        "node transform operation is not implemented",
                    )?;
                    let model = self
                        .resources
                        .model(actor.resource)
                        .ok_or("node model missing")?;
                    let Some(name) = usize::try_from(a[2]).ok().and_then(|i| model.names.get(i))
                    else {
                        return Ok(NativeResult::Continue(None));
                    };
                    let slot =
                        u8::try_from(a[2]).map_err(|_| "node transform slot exceeds range")?;
                    let old = actor
                        .appearance
                        .bone_adjustments
                        .get(&slot)
                        .map_or([0.; 3], |v| v.sample(self.world.tick));
                    self.registers[..3].copy_from_slice(&old.map(|v| v as i32));
                    if a[0] == 3 {
                        actor.appearance.bone_adjustments.insert(
                            slot,
                            crate::BoneAdjustment {
                                bone: crate::BoneTarget::Name(name.clone()),
                                absolute_rotation: true,
                                angles: [a[3] as f32, a[4] as f32, a[5] as f32],
                                from: old,
                                duration_ticks: 1,
                                start_tick: self.world.tick,
                                translation: None,
                                scale: None,
                            },
                        );
                    }
                }
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
            NativeCall::ReadActorOffset => {
                const ACTOR_OFFSET: i32 = 2000;
                if a[0] == ACTOR_OFFSET {
                    let id = if a[1] == crate::CONTROLLED_ACTOR {
                        self.world.controlled_actor
                    } else {
                        a[1]
                    };
                    let actor = self
                        .world
                        .actors
                        .get(&id)
                        .ok_or("offset actor is missing")?;
                    let (sin, cos) = (actor.heading + a[2] as f32).to_radians().sin_cos();
                    self.registers[..3].copy_from_slice(&[
                        (actor.position[0] + sin * a[3] as f32) as i32,
                        (actor.position[1] - cos * a[3] as f32) as i32,
                        actor.position[2] as i32,
                    ]);
                }
            }
            NativeCall::ReadActorAttachment => {
                if a[1] != -1 {
                    let point = self.attachment(a[0], a[1])?;
                    self.registers[..3].copy_from_slice(&point);
                }
            }
            NativeCall::CreateParticle => {
                if [
                    crate::effect::STATION_GLOW_SPRITE,
                    crate::effect::CAMERA_DISC_SPRITE,
                    crate::effect::WORLD_GLOW_SPRITE,
                    crate::effect::ORB_SPRITE,
                    crate::effect::RING_SPRITE,
                    crate::effect::STAR_SPRITE,
                    crate::effect::SPINNING_STAR_SPRITE,
                    crate::effect::ELECTRIC_SPARK_SPRITE,
                ]
                .map(i32::from)
                .contains(&a[0])
                {
                    return self.field(op, a, memory);
                }
                let kind = self
                    .resources
                    .particles
                    .get(&a[0])
                    .ok_or("particle kind is not cooked")?;
                require(
                    (0..=0x7fff).contains(&a[1])
                        && (a[12] == 0 || matches!(kind, crate::ParticleKind::Flutter(_))),
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
                if let Some(effect) = self.world.refractions.get_mut(&a[0]) {
                    const SIZE_GROWTH: i32 = 135;
                    require(
                        a[1] == SIZE_GROWTH,
                        "refraction property is not implemented",
                    )?;
                    effect.growth = a[2] as f32 / 100.;
                    return Ok(NativeResult::Continue(Some(0)));
                }
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
                    | NativeCall::SetActorPathPoint
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
                    | NativeCall::TurnActorHead
                    | NativeCall::ConfigureActorHeadNeck
                    | NativeCall::ConfigureActorAttachment
                    | NativeCall::ConfigureActorBoneTranslation
                    | NativeCall::ConfigureActorBoneScale
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
