//! Native service shims registered by typed call ID. The VM knows neither
//! actors nor assets; these handlers operate independently of scene/event IDs.
use crate::animation::slot;
use crate::world::{Fade, Overlay, OverlayKind, SpriteOverlay};
use crate::{Actor, Animation, CameraTrack, GameWorld, ResourceKind, ResourceLibrary};
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
    pub handle: i32,
    pub action: EventAction,
}
pub(crate) enum EventAction {
    Spawn(u32),
    Pause(bool),
    ControlGate(bool),
    Release,
}

pub(crate) struct NativeHost<'a> {
    pub world: &'a mut GameWorld,
    pub resources: &'a ResourceLibrary,
    pub program: &'a Program,
    pub event_actor: i16,
    pub registers: &'a mut [i32; 6],
    pub events: &'a mut Vec<EventCommand>,
    pub free_slots: u32,
    pub wait: &'a mut Option<Wait>,
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
        8 => i32::from(actor.opacity),
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
                actor.opacity = value as u8;
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
    fn wait_for_choice(
        &mut self,
        slot: u8,
        choice: crate::dialogue::Choice,
    ) -> Result<NativeResult, String> {
        *self.wait = Some(Wait::Choice {
            result: choice.operation.clone(),
            window: Box::new(Wait::Service {
                condition: Box::new(Wait::Complete(self.world.dialogue[&slot].operation.clone())),
                ready_at: None,
            }),
        });
        if let Some(old) = self.world.choices.insert(slot, choice) {
            old.operation.cancel();
        }
        Ok(NativeResult::Suspend)
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
            NativeCall::GetActorHeading | NativeCall::IsActorMoving => {
                value = Some(self.world.actors.get(&a[0]).map_or(0, |actor| {
                    if op == NativeCall::GetActorHeading {
                        actor.heading as i16 as i32
                    } else {
                        i32::from(actor.motion.is_some())
                    }
                }));
            }
            NativeCall::DespawnActorAfterMovement => {
                let actor = if a[0] == crate::CONTROLLED_ACTOR {
                    self.world.controlled_actor
                } else {
                    a[0]
                };
                let mut wait = Wait::Service {
                    condition: Box::new(Wait::DespawnAfterMotion(actor)),
                    ready_at: None,
                };
                wait.poll(self.world)?;
                *self.wait = Some(wait);
                return Ok(NativeResult::Suspend);
            }
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
                    // Scalar property writes carry unsigned 16-bit values;
                    // the emitter constructor accepts signed coordinates.
                    let value = (op == NativeCall::SetActorProperty).then(|| {
                        if (117..=122).contains(&a[1]) {
                            i32::from(a[2] as u16)
                        } else {
                            a[2]
                        }
                    });
                    let previous = self
                        .world
                        .actors
                        .get_mut(&id)
                        .and_then(|actor| actor.emitter.as_mut())
                        .map(|emitter| emitter.property(a[1], value))
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
                            .is_some_and(|actor| actor.pushable)
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
                    7 => i32::from(actor.heading_lock),
                    8 => i32::from(actor.opacity),
                    9 => i32::from(!actor.collidable),
                    10 => i32::from(!actor.grounded),
                    11 => i32::from(!actor.casts_shadow),
                    12 => i32::from(actor.appearance.model_hidden),
                    13 | 14 => i32::from(
                        actor.culling_flags[(a[1] - 13) as usize]
                            .unwrap_or(!actor.cull_outside_view),
                    ),
                    15 => {
                        actor
                            .autonomy
                            .as_ref()
                            .ok_or("actor wandering state is missing")?
                            .radius as i32
                    }
                    16 => i32::from(actor.appearance.expression),
                    17 => actor.interaction_label,
                    18 => i32::from(actor.model_collision.is_some()),
                    19 => i32::from(actor.pushable),
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
                    27 => actor
                        .enemy
                        .as_ref()
                        .map_or(0, |enemy| i32::from(enemy.random_turns)),
                    30..=32 => actor.scale_percent[(a[1] - 30) as usize],
                    34 => actor.autonomy.as_ref().map_or(0, |ai| ai.behavior as i32),
                    35 | 36 => actor.tilt[(a[1] - 35) as usize],
                    37 => actor.heading as i32,
                    TOON_LIGHTING => actor.toon_lighting.map(i32::from).unwrap_or_else(|| {
                        i32::from(
                            self.resources
                                .model(actor.model_resource())
                                .is_some_and(|model| model.toon_lighting),
                        )
                    }),
                    39 => i32::from(actor.draw_layer),
                    DISABLE_SECONDARY_MOTION => {
                        i32::from(actor.appearance.secondary_motion_disabled)
                    }
                    47 => actor.radius as i32,
                    51 => i32::from(actor.shadow_alpha),
                    54 => actor
                        .enemy
                        .as_ref()
                        .map_or(0, |enemy| i32::from(enemy.pause_ticks)),
                    56 => actor
                        .enemy
                        .as_ref()
                        .map_or(0, |enemy| reaction_code(enemy.reaction)),
                    41 => i32::from(actor.unlit),
                    48 => i32::from(actor.ring_contact_disabled),
                    50 => i32::from(actor.interaction_disabled),
                    42..=44 => i32::from(actor.tint[(a[1] - 42) as usize]),
                    46 => i32::from(!actor.depth_write),
                    45 => actor.blend.map_or(0, |blend| blend as i32),
                    _ => unreachable!(),
                };
                if op == NativeCall::SetActorProperty {
                    match a[1] {
                        7 => {
                            let flags = a[2] & 15;
                            actor.heading_lock = flags as u8;
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
                            actor.opacity = a[2] as u8;
                            actor.visible = a[2] as u8 != 0;
                        }
                        9 => actor.collidable = a[2] & 1 == 0,
                        10 => actor.grounded = a[2] & 1 == 0,
                        11 => actor.casts_shadow = a[2] & 1 == 0,
                        12 => actor.appearance.model_hidden = a[2] & 1 != 0,
                        13 | 14 => {
                            actor.culling_flags[(a[1] - 13) as usize] = Some(a[2] & 1 != 0);
                            actor.cull_outside_view = !actor
                                .culling_flags
                                .into_iter()
                                .flatten()
                                .any(|disabled| disabled);
                        }
                        15 => actor.autonomy.as_mut().unwrap().radius = a[2] as f32,
                        16 => actor.appearance.expression = a[2] as u8,
                        17 => {
                            actor.interaction_label = i32::from(a[2] as i16);
                        }
                        18 => {
                            actor.model_collision = if a[2] & 1 != 0 {
                                Some(
                                    self.resources
                                        .model(actor.model_resource())
                                        .ok_or("collision model is missing")?
                                        .collision
                                        .clone(),
                                )
                            } else {
                                None
                            };
                        }
                        19 => {
                            actor.pushable = a[2] & 1 != 0;
                            if actor.pushable {
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
                                enemy.random_turns = a[2] as u8;
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
                            actor.toon_lighting = Some(a[2] as u8);
                        }
                        39 => {
                            actor.draw_layer = a[2] as i8;
                        }
                        DISABLE_SECONDARY_MOTION => {
                            actor.appearance.secondary_motion_disabled = a[2] & 1 != 0
                        }
                        47 => actor.radius = a[2] as f32,
                        51 => actor.shadow_alpha = a[2] as u8,
                        54 => {
                            if let Some(enemy) = &mut actor.enemy {
                                enemy.pause_ticks = a[2] as i16;
                            }
                        }
                        56 => {
                            if let Some(enemy) = &mut actor.enemy {
                                require(
                                    !matches!(a[2] as u8, 4 | 6),
                                    "enemy pause effect is not implemented",
                                )?;
                                use crate::effect::StunEffect::*;
                                enemy.reaction = match a[2] as u8 {
                                    5 => Electric,
                                    11 => TetheallaElectric,
                                    13 => Lightning,
                                    14 => Ice,
                                    16 => Darkness,
                                    _ => None,
                                };
                            }
                        }
                        46 => actor.depth_write = a[2] & 1 == 0,
                        45 => {
                            require(a[2] & 7 <= 2, "actor blend mode is not implemented")?;
                            actor.blend = Some((a[2] & 7).try_into()?);
                        }
                        30..=32 => {
                            if self.world.tick >= actor.visible_from {
                                actor.rendered_scale.get_or_insert(actor.scale_percent);
                            }
                            actor.scale_percent[(a[1] - 30) as usize] = a[2];
                        }
                        35 | 36 => actor.tilt[(a[1] - 35) as usize] = a[2],
                        42..=44 => {
                            actor.tint[(a[1] - 42) as usize] = a[2] as u8;
                        }
                        41 => actor.unlit = a[2] & 1 != 0,
                        48 => actor.ring_contact_disabled = a[2] & 1 != 0,
                        50 => actor.interaction_disabled = a[2] & 1 != 0,
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
                    self.world.texture_bindings.insert(a[0], a[1]);
                }
                128 => {
                    self.world.texture_animation_enabled = a[1] != 0;
                }
                129 => {
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
                        opacity: 0,
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
            NativeCall::PlayMovieBlocking => {
                let resource = u32::try_from(a[0]).map_err(|_| "invalid movie ID")?;
                require(
                    self.resources.movies.contains(&resource),
                    "movie is not cooked",
                )?;
                let movie = Movie {
                    resource,
                    operation: self.world.operations.begin()?,
                };
                self.world.voice = None;
                *self.wait = Some(Wait::Complete(movie.operation.clone()));
                if let Some(old) = self.world.movie.replace(movie) {
                    old.operation.cancel();
                }
                return Ok(NativeResult::Suspend);
            }
            NativeCall::PlayVoice => {
                // Voice IDs pair a bank in the high half with a line in the low half.
                // A bare line number does not select a stream.
                if a[0] == -1 {
                    self.world.voice = None;
                    self.world
                        .audio_commands
                        .push(crate::AudioCommand::StopVoice);
                } else if a[0] as u32 >> 16 != 0 {
                    let resource = a[0] as u32;
                    let duration = self
                        .world
                        .voice_durations
                        .get(&resource)
                        .ok_or("voice is not cooked")?;
                    self.world.voice = Some(crate::VoicePlayback {
                        resource,
                        end_tick: self.world.tick.saturating_add(*duration),
                    });
                    self.world
                        .audio_commands
                        .push(crate::AudioCommand::Voice(resource));
                    return self.yield_update();
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
                // Script lines span the whole message; the cursor belongs to its final page.
                let mut line = 0;
                let mut page_start = 0;
                for token in &dialogue.body.tokens {
                    if let crate::dialogue::TextToken::Text { text } = token {
                        for character in text.chars() {
                            if matches!(character, '\n' | '\u{c}') {
                                line += 1;
                            }
                            if character == '\u{c}' {
                                page_start = line;
                            }
                        }
                    }
                }
                require(
                    first >= page_start && last <= line,
                    "choice is outside the final message page",
                )?;
                let choice = crate::dialogue::Choice {
                    operation: self.world.operations.begin()?,
                    selection: crate::dialogue::Selection::Lines(crate::dialogue::LineSelection {
                        first_line: (first - page_start) as u8,
                        last_line: (last - page_start) as u8,
                        selected_line: (initial - page_start) as u8,
                    }),
                    cancel_allowed: a[4] & choice_flags::DISABLE_CANCEL == 0,
                    confirmation: if a[4] & choice_flags::SHOULDER_CONFIRM != 0 {
                        ChoiceConfirmation::AcceptOrShoulder
                    } else {
                        ChoiceConfirmation::Accept
                    },
                    timeout_ticks: (a[3] > 0).then_some(a[3] as u16),
                };
                return self.wait_for_choice(slot, choice);
            }
            NativeCall::ShowNumberInput => {
                use crate::dialogue::{
                    Choice, ChoiceConfirmation, NumberSelection, Selection, TextToken,
                };
                require(
                    (0..i32::from(DIALOGUE_SLOTS)).contains(&a[0]),
                    "invalid number input slot",
                )?;
                let slot = a[0] as u8;
                let dialogue = self
                    .world
                    .dialogue
                    .get(&slot)
                    .ok_or("number input dialogue is missing")?;
                require(
                    dialogue.operation.is_pending() && !dialogue.persistent(),
                    "number input needs an open dialogue",
                )?;
                let digits = dialogue
                    .body
                    .tokens
                    .iter()
                    .rev()
                    .find_map(|token| match token {
                        TextToken::Control { opcode: 8, value } => Some(*value),
                        _ => None,
                    })
                    .ok_or("number input dialogue has no digit field")?;
                require((1..=10).contains(&digits), "invalid number input width")?;
                require(
                    a[2] >= 0 && a[2] <= a[3] && i64::from(a[3]) < 10_i64.pow(digits as u32),
                    "invalid number input bounds",
                )?;
                let choice = Choice {
                    operation: self.world.operations.begin()?,
                    selection: Selection::Number(NumberSelection {
                        value: a[1].clamp(a[2], a[3]),
                        minimum: a[2],
                        maximum: a[3],
                        digits: digits as u8,
                        place: 0,
                        wrap_digits: a[4] != 0,
                    }),
                    cancel_allowed: true,
                    confirmation: ChoiceConfirmation::Accept,
                    timeout_ticks: None,
                };
                return self.wait_for_choice(slot, choice);
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
                    }
                    let actor = self.world.actors.get_mut(&a[0]).unwrap();
                    if let Some(model) = self.resources.model(actor.resource) {
                        let dialogue = self.world.dialogue.values().any(|d| d.operation.is_pending()
                            && matches!(d.anchor, crate::dialogue::DialogueAnchor::Actor(speaker) if speaker == a[0]));
                        actor.select_automatic_animation(
                            model,
                            self.world.tick,
                            crate::animation::Locomotion {
                                movement_speed: actor.motion.as_ref().map(|m| m.speed),
                                walking: false,
                                turn: actor.turn_direction(),
                                dialogue,
                                event_controlled: !self.world.input_enabled
                                    && a[0] == self.world.controlled_actor,
                                player_controlled: self.world.input_enabled
                                    && a[0] == self.world.controlled_actor,
                                release: Some(a[3]),
                            },
                        );
                    }
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
            }
            NativeCall::PlayCameraTrack => {
                let resource = self.resolve(a[0], ResourceKind::Camera)?;
                let mut playback = CameraTrack::new(resource, self.world.tick);
                playback.playing = a[1] == 0;
                playback.repeat = a[2] == 1;
                self.world.camera = Some(playback);
            }
            NativeCall::ConfigureCameraTrack | NativeCall::MapCameraTrackPosition => {
                let playback = self
                    .world
                    .camera
                    .as_mut()
                    .ok_or("camera track is not active")?;
                let duration = self
                    .resources
                    .camera_tracks
                    .get(&playback.resource)
                    .and_then(|keys| keys.last())
                    .ok_or("camera track is not cooked")?
                    .time;
                if op == NativeCall::MapCameraTrackPosition {
                    require(a[2] != a[3], "camera mapping range is empty")?;
                    let fraction =
                        ((a[1] as f64 - a[2] as f64) / (a[3] as f64 - a[2] as f64)).clamp(0., 1.);
                    playback.retime(self.world.tick, duration);
                    playback.seek(duration * fraction as f32, self.world.tick);
                } else {
                    let argument = if a[0] == 11 && a[1] == crate::CONTROLLED_ACTOR {
                        self.world.controlled_actor
                    } else {
                        a[1]
                    };
                    value = Some(playback.configure(a[0], argument, self.world.tick, duration)?);
                }
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
                let key = self.world.scene_actor_key(a[0])?;
                if key != a[0] {
                    self.world.duplicate_actors.insert(key, a[0]);
                }
                let costume = self
                    .world
                    .party
                    .as_ref()
                    .and_then(|party| party.members.get(resource.wrapping_sub(1) as usize))
                    .map_or(0, |member| member.costume);
                let model = self
                    .resources
                    .model(resonance_content::appearance::costume_resource(
                        resource, costume,
                    ));
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
                        appearance: crate::Appearance {
                            costume,
                            hidden_nodes: model.map(|m| m.hidden_nodes.clone()).unwrap_or_default(),
                            ..Default::default()
                        },
                        role: if interaction {
                            crate::ActorRole::Interaction
                        } else {
                            crate::ActorRole::Ordinary
                        },
                        interaction_label: if locator { 0 } else { 2 },
                        ring_contact_disabled: locator && !interaction,
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
                        .model(actor.model_resource())
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
            NativeCall::ReadActorLocalOffset => {
                require(
                    a[0] == 3000 && a[6..].iter().all(|v| *v == 0),
                    "unsupported local coordinate space",
                )?;
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
                    (actor.position[0] + cos * a[3] as f32 - sin * a[4] as f32) as i32,
                    (actor.position[1] + sin * a[3] as f32 + cos * a[4] as f32) as i32,
                    (actor.position[2] + a[5] as f32) as i32,
                ]);
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
                let rising = a[0] == 26;
                let recipe_id = if rising { 25 } else { a[0] };
                let Some(kind) = self.resources.particles.get(&recipe_id) else {
                    return self.field(op, a, memory);
                };
                require(
                    (0..=0x7fff).contains(&a[1])
                        && (a[12] == 0 || matches!(kind, crate::ParticleKind::Flutter(_))),
                    "particle lifetime/color mode is not implemented",
                )?;
                if let crate::ParticleKind::Flutter(recipe) = kind {
                    use crate::effect::{
                        BillboardController, BillboardEffect, Fade, Flutter, SpriteOrientation,
                    };
                    let mut rgba = *recipe
                        .palette
                        .get(a[11] as usize)
                        .ok_or("particle color is not cooked")?;
                    rgba[3] = a[9] as u8;
                    let flutter = Flutter::pending(recipe, rising);
                    let lifetime = a[1] as u32 + 1;
                    return Ok(NativeResult::Continue(Some(self.world.emit_billboard(
                        BillboardEffect {
                            recipe: recipe_id.try_into().map_err(|_| "invalid leaf recipe")?,
                            blend: rising.then_some(crate::effect::Blend::Additive),
                            born: self.world.tick + 1,
                            lifetime,
                            position: [a[2] as f32, a[3] as f32, a[4] as f32],
                            orientation: SpriteOrientation::World,
                            size: [a[8] as f32, a[8] as f32 / recipe.aspect_ratio],
                            rgba,
                            fade: if a[10] == 0 {
                                Fade::tail(lifetime)
                            } else {
                                Fade::Linear(a[10] as f32)
                            },
                            controller: Some(BillboardController::Flutter(flutter)),
                            ..Default::default()
                        },
                    )?)));
                }
                require(a[11] == 0, "particle color is not cooked")?;
                let handle = self.world.emit_billboard(crate::effect::BillboardEffect {
                    recipe: a[0] as u16,
                    born: self.world.tick,
                    lifetime: a[1] as u32 + 1,
                    position: [a[2] as f32, a[3] as f32, a[4] as f32],
                    velocity: [a[5] as f32, a[6] as f32, a[7] as f32],
                    size: [a[8] as f32; 2],
                    rgba: [255, 255, 255, a[9] as u8],
                    fade: crate::effect::Fade::Linear(a[10] as f32),
                    ..Default::default()
                })?;
                value = Some(handle);
            }
            NativeCall::SetEffectProperty => return self.field(op, a, memory),
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
                    | NativeCall::PlayVoice
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
                    | NativeCall::GetActorHeading
                    | NativeCall::IsActorMoving
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

fn reaction_code(effect: crate::effect::StunEffect) -> i32 {
    use crate::effect::StunEffect::*;
    match effect {
        None => 0,
        Electric => 5,
        TetheallaElectric => 11,
        Lightning => 13,
        Ice => 14,
        Darkness => 16,
    }
}
