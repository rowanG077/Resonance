//! Scene setup supplies the original script with cooked resource bindings.
use anyhow::{Result, ensure};
use resonance_content::field::FieldAssets;
use resonance_content::field::SCENERY_RESOURCE_BASE;
use resonance_events::{
    Actor, AnimationClip, EventRuntime, ModelResource, ResourceKind, ResourceLibrary,
};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::Program;
pub mod navigation;
pub mod replay;

/// Typed field-entry data; a transition carries state, never old scene handles.
#[derive(Default)]
pub struct FieldEntry {
    pub persistent: resonance_events::PersistentState,
    pub data: Option<Arc<resonance_content::session::SessionData>>,
    pub available_fields: std::collections::BTreeSet<u32>,
    pub position: [f32; 3],
    pub heading: f32,
    pub idle_animation: Option<u16>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FieldInput {
    /// Camera-relative stick input: right and forward, in [-1, 1].
    pub direction: [f32; 2],
    pub run: bool,
    pub interact: bool,
    pub cancel: bool,
}

/// Owns one field's gameplay and event lifetime. Presentation consumes the
/// resulting actors and operations without needing to understand bytecode.
pub struct FieldSession {
    pub events: EventRuntime,
    pub dialogue: BTreeMap<u8, crate::dialogue::DialoguePlayer>,
    choices: crate::choice::ChoicePlayer,
    walkmesh: navigation::WalkMesh,
    light_regions: Option<navigation::WalkMesh>,
    lighting: BTreeMap<i32, resonance_events::effect::CharacterLight>,
    conversation_facing: Option<(i32, f32, f32)>,
    active_triggers: std::collections::BTreeSet<u32>,
    pub voice_durations: Arc<BTreeMap<u32, u32>>,
    pub voice_feedback: Option<Arc<dyn crate::dialogue::VoiceFeedback>>,
    /// Actor -> first update of its current continuous dialogue mouth cycle.
    pub talking: BTreeMap<i32, u32>,
}
impl FieldSession {
    pub fn story_progress(&self) -> Result<i32> {
        Ok(self
            .events
            .memory()
            .read(0x40, symphonia_script::Width::S32)?)
    }
    pub fn new(
        script: &[u8],
        messages: Vec<symphonia_script::message::Message>,
        assets: &FieldAssets,
    ) -> Result<Self> {
        Self::enter(script, messages, assets, FieldEntry::default())
    }
    pub fn enter(
        script: &[u8],
        messages: Vec<symphonia_script::message::Message>,
        assets: &FieldAssets,
        entry: FieldEntry,
    ) -> Result<Self> {
        use sha2::{Digest, Sha256};
        ensure!(
            format!("{:x}", Sha256::digest(script)) == assets.script.sha256,
            "field script digest mismatch"
        );
        Ok(Self {
            events: start_with_entry(script, messages, assets, entry)?,
            dialogue: BTreeMap::new(),
            choices: Default::default(),
            lighting: BTreeMap::new(),
            conversation_facing: None,
            active_triggers: Default::default(),
            voice_durations: Default::default(),
            voice_feedback: None,
            talking: Default::default(),
            walkmesh: navigation::WalkMesh::new(&assets.ground)?,
            light_regions: (!assets.regions.is_empty())
                .then(|| navigation::WalkMesh::new(&assets.regions))
                .transpose()?,
        })
    }
    pub fn step(&mut self, input: FieldInput) -> Result<()> {
        let talking = self.step_dialogue(input)?;
        let can_trigger = self.events.world.input_enabled && !talking;
        if self.events.world.input_enabled && !talking {
            let id = self.events.world.controlled_actor;
            if let Some(actor) = self.events.world.actors.get(&id) {
                let start = actor.position;
                let mut stick = input
                    .direction
                    .map(|v| if v.is_finite() { v.clamp(-1., 1.) } else { 0. });
                let length = stick[0].hypot(stick[1]);
                if length > 1. {
                    stick = stick.map(|v| v / length);
                }
                let forward = self
                    .events
                    .world
                    .field_camera
                    .as_ref()
                    .map_or([0., 1.], |c| {
                        [c.target[0] - c.position[0], c.target[1] - c.position[1]]
                    });
                let norm = forward[0].hypot(forward[1]).max(0.001);
                let forward = forward.map(|v| v / norm);
                let speed = if input.run { 8. } else { 4. };
                let delta = [
                    (forward[1] * stick[0] + forward[0] * stick[1]) * speed,
                    (-forward[0] * stick[0] + forward[1] * stick[1]) * speed,
                ];
                let target = self.walkmesh.move_by(start, delta, 15., |p| {
                    self.events.world.actors.iter().any(|(other, a)| {
                        *other != id
                            && a.visible
                            && a.collidable
                            && a.resource < SCENERY_RESOURCE_BASE
                            && a.resource != 24
                            && (p[2] - a.position[2]).abs() < 60.
                            && (p[0] - a.position[0]).hypot(p[1] - a.position[1]) < 35.
                    })
                });
                let actor = self.events.world.actors.get_mut(&id).unwrap();
                actor.motion = if (target[0] - start[0]).hypot(target[1] - start[1]) >= 0.01 {
                    Some(resonance_events::ActorMotion { target, speed })
                } else {
                    None
                };
                if input.interact
                    && let Some(target) = self.interaction_target()
                    && self.events.interact(target)?
                {
                    // Ordinary field conversations turn the selected person
                    // toward Lloyd while leaving the player's facing alone.
                    // Independent NPC 304/305 oracle checkpoints verify this.
                    let other = self.events.world.actors.get_mut(&target).unwrap();
                    let previous_heading = other.target_heading;
                    other.target_heading = (start[0] - other.position[0])
                        .atan2(other.position[1] - start[1])
                        .to_degrees()
                        .rem_euclid(360.)
                        .trunc();
                    self.conversation_facing =
                        Some((target, previous_heading, other.target_heading));
                }
            }
        }
        let mut resolved = BTreeMap::new();
        self.events.step_with_motion(|id, actor, previous| {
            if actor.grounded && actor.resource < SCENERY_RESOURCE_BASE && actor.resource != 24 {
                actor.position = self
                    .walkmesh
                    .resolve_motion(previous, actor.position)
                    .unwrap_or(previous);
            }
            resolved.insert(id, actor.position);
        })?;
        if can_trigger && self.events.world.input_enabled {
            self.step_triggers(input.interact)?;
        }
        if self.events.world.input_enabled
            && let Some((id, heading, automatic_heading)) = self.conversation_facing.take()
            && let Some(actor) = self.events.world.actors.get_mut(&id)
            && actor.target_heading == automatic_heading
        {
            // A script may select its own return heading (Colette uses 0x14).
            // Restore the previous target only if the script left ours alone.
            actor.target_heading = heading;
        }
        for (id, actor) in &mut self.events.world.actors {
            if actor.grounded
                && actor.resource < SCENERY_RESOURCE_BASE
                && actor.resource != 24
                // Newly spawned/repositioned actors still need their initial
                // floor height. Preserve positions already resolved this tick.
                && resolved.get(id) != Some(&actor.position)
                && let Some(z) = self.walkmesh.height(actor.position, 32.)
            {
                actor.position[2] = z;
            }
        }
        let targets: Vec<_> = self
            .events
            .world
            .actors
            .iter()
            .map(|(&id, actor)| (id, self.target_light(actor.position)))
            .collect();
        self.lighting
            .retain(|id, _| self.events.world.actors.contains_key(id));
        for (id, target) in targets {
            self.lighting
                .entry(id)
                .and_modify(|light| light.approach(&target))
                .or_insert(target);
        }
        Ok(())
    }
    fn step_triggers(&mut self, confirm: bool) -> Result<()> {
        let world = &self.events.world;
        let Some(actor) = world.actors.get(&world.controlled_actor) else {
            return Ok(());
        };
        // Trigger contact uses the actor’s radius and the line’s vertical span.
        let touching: Vec<_> = world
            .triggers
            .iter()
            .filter(|trigger| navigation::touches_trigger(trigger, actor.position, 42.))
            .cloned()
            .collect();
        self.active_triggers
            .retain(|key| touching.iter().any(|t| t.key == *key));
        for trigger in touching {
            let confirmed = trigger.transition.is_some();
            if (confirmed && !confirm) || self.active_triggers.contains(&trigger.key) {
                continue;
            }
            if self.events.trigger(trigger.key, confirmed)? {
                self.active_triggers.insert(trigger.key);
                break;
            }
        }
        Ok(())
    }
    fn step_dialogue(&mut self, input: FieldInput) -> Result<bool> {
        self.dialogue.retain(|slot, player| {
            self.events
                .world
                .dialogue
                .get(slot)
                .is_some_and(|d| d.operation.id() == player.operation.id())
        });
        for (&slot, request) in &self.events.world.dialogue {
            if request.opening_actor.is_none()
                && request.operation.is_pending()
                && !self.dialogue.contains_key(&slot)
            {
                self.dialogue.insert(
                    slot,
                    crate::dialogue::DialoguePlayer::new(request, 3)?
                        .with_voice_durations(self.voice_durations.clone())
                        .with_voice_feedback(self.voice_feedback.clone()),
                );
            }
        }
        let choice_slot = self
            .events
            .world
            .choices
            .iter()
            .find(|(_, choice)| choice.operation.is_pending())
            .map(|(&slot, _)| slot);
        let focus = choice_slot.or_else(|| {
            self.dialogue
                .iter()
                .find(|(_, d)| !d.closed && !d.persistent && d.operation.is_pending())
                .map(|(slot, _)| *slot)
        });
        for (slot, player) in &mut self.dialogue {
            for voice in
                player.step(input.interact && choice_slot.is_none() && focus == Some(*slot))?
            {
                self.events.world.audio_commands.push(match voice {
                    crate::dialogue::VoiceAction::Play(id) => {
                        self.events.world.voice = Some(resonance_events::VoicePlayback {
                            resource: id,
                            end_tick: self.events.world.tick.saturating_add(
                                self.voice_durations.get(&id).copied().unwrap_or(0),
                            ),
                        });
                        resonance_events::AudioCommand::Voice(id)
                    }
                    crate::dialogue::VoiceAction::Stop => {
                        self.events.world.voice = None;
                        resonance_events::AudioCommand::StopVoice
                    }
                });
            }
        }
        let speakers: std::collections::BTreeSet<_> = self
            .dialogue
            .iter()
            .filter(|(_, p)| p.is_talking())
            .filter_map(|(slot, p)| {
                self.events
                    .world
                    .dialogue
                    .get(slot)
                    .filter(|d| d.operation.id() == p.operation.id())
                    .and_then(|d| d.speaker_actor)
            })
            .collect();
        self.talking.retain(|actor, _| speakers.contains(actor));
        for actor in speakers {
            self.talking.entry(actor).or_insert(self.events.tick());
        }
        if let Some(slot) = choice_slot {
            let player = self
                .dialogue
                .get(&slot)
                .ok_or_else(|| anyhow::anyhow!("choice dialogue player is missing"))?;
            let choice = self.events.world.choices.get_mut(&slot).unwrap();
            let lines = 1 + player
                .current()
                .glyphs
                .iter()
                .filter(|g| g.character == '\n')
                .count();
            ensure!(
                usize::from(choice.last_line) < lines,
                "choice extends beyond dialogue lines"
            );
            let (reason, moved) = self.choices.step(
                choice,
                crate::choice::ChoiceInput {
                    direction: if input.direction[1] > 0.5 {
                        -1
                    } else if input.direction[1] < -0.5 {
                        1
                    } else {
                        0
                    },
                    confirm: input.interact,
                    cancel: input.cancel,
                },
                player.accepts_input() && player.fully_revealed(),
            );
            if moved {
                self.events
                    .world
                    .audio_commands
                    .push(resonance_events::AudioCommand::Sound {
                        id: 1,
                        pan: 64,
                        volume: 127,
                        slot: None,
                    });
            }
            if let Some(reason) = reason {
                choice.finish(reason).map_err(anyhow::Error::msg)?;
                player
                    .operation
                    .complete(None)
                    .map_err(anyhow::Error::msg)?;
                use resonance_events::dialogue::ChoiceExit;
                if reason != ChoiceExit::Timeout {
                    self.events
                        .world
                        .audio_commands
                        .push(resonance_events::AudioCommand::Sound {
                            id: if reason == ChoiceExit::Confirm { 2 } else { 3 },
                            pan: 64,
                            volume: 127,
                            slot: None,
                        });
                }
            }
        }
        Ok(focus.is_some())
    }
    pub fn interaction_target(&self) -> Option<i32> {
        let id = self.events.world.controlled_actor;
        let player = self.events.world.actors.get(&id)?;
        let angle = player.heading.to_radians();
        self.events
            .world
            .actors
            .iter()
            .filter_map(|(id, actor)| {
                if !actor.visible || !self.events.has_interaction(*id) {
                    return None;
                }
                let dx = actor.position[0] - player.position[0];
                let dy = actor.position[1] - player.position[1];
                let distance = dx.hypot(dy);
                let facing = dx * angle.sin() - dy * angle.cos();
                (distance > 0.
                    && distance <= 130.
                    && facing >= distance * 0.25
                    && (actor.position[2] - player.position[2]).abs() < 80.)
                    .then_some((*id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
    pub fn ground_surface(&self, point: [f32; 3]) -> Option<navigation::GroundSurface> {
        self.walkmesh.surface(point, 32.)
    }
    pub fn character_light(&self, id: i32) -> resonance_events::effect::CharacterLight {
        self.lighting.get(&id).cloned().unwrap_or_else(|| {
            self.events
                .world
                .actors
                .get(&id)
                .map_or_else(Default::default, |actor| self.target_light(actor.position))
        })
    }
    fn target_light(&self, point: [f32; 3]) -> resonance_events::effect::CharacterLight {
        // The ground attribute mesh selects the light using its low five bits.
        let selector = self
            .light_regions
            .as_ref()
            .and_then(|mesh| mesh.surface(point, f32::INFINITY))
            .map_or(0, |surface| (surface.attributes & 31) as i32);
        self.events
            .world
            .character_lights
            .get(&selector)
            .cloned()
            .unwrap_or_default()
    }
}

pub fn start(
    script: &[u8],
    messages: Vec<symphonia_script::message::Message>,
    assets: &FieldAssets,
) -> Result<EventRuntime> {
    start_with_entry(script, messages, assets, FieldEntry::default())
}

fn start_with_entry(
    script: &[u8],
    messages: Vec<symphonia_script::message::Message>,
    assets: &FieldAssets,
    entry: FieldEntry,
) -> Result<EventRuntime> {
    assets.validate()?;
    ensure!(
        matches!(assets.map_id, 5 | 340),
        "field entry setup has not been defined for this map"
    );
    let mut resources = ResourceLibrary {
        messages,
        session_data: entry.data,
        fields: entry.available_fields,
        actor_names: [(1, "Lloyd"), (2, "Colette"), (3, "Genis"), (4, "Raine")]
            .into_iter()
            .map(|(id, name)| (id, name.into()))
            .collect(),
        ..Default::default()
    };
    resources.movies.insert(1);
    resources.locators.insert(24);
    for character in &assets.actors {
        let model = character
            .parts
            .first()
            .ok_or_else(|| anyhow::anyhow!("character has no model"))?;
        let resource = character.resource;
        resources
            .bindings
            .insert(resource as i32, (ResourceKind::Model, resource));
        resources.models.insert(
            resource,
            ModelResource {
                names: model.bone_names.clone(),
                hidden_nodes: character.hidden_nodes.iter().copied().collect(),
                clips: BTreeMap::new(),
            },
        );
        for clip in &model.clips {
            let animation = clip.animation_resource.unwrap_or(resource);
            resources
                .bindings
                .insert(animation as i32, (ResourceKind::Model, animation));
            resources
                .models
                .entry(animation)
                .or_default()
                .clips
                .insert(clip.resource_slot, AnimationClip::from(clip));
        }
    }
    let (mut world, memory) = entry.persistent.into_world();
    world.controlled_actor = 1;
    // Both the camera orbit anchor and look target follow the player; updating
    // only the target leaves the eye orbiting the wrong point.
    let mut camera = resonance_events::camera::CameraRig::default();
    camera.current_mut().follow = true;
    camera.current_mut().anchor_to_actor = true;
    world.field_camera = Some(camera);
    ensure!(resources.models.contains_key(&1), "Lloyd is not cooked");
    let mut lloyd = Actor::new(1, entry.position);
    lloyd.face(entry.heading);
    if let Some(slot) = entry.idle_animation {
        ensure!(
            resources.models[&1].clips.contains_key(&slot),
            "field entry idle animation is missing"
        );
        lloyd.idle_animation = slot;
    }
    lloyd
        .appearance
        .hidden_nodes
        .clone_from(&resources.models[&1].hidden_nodes);
    world.actors.insert(1, lloyd);
    for (part, actor) in assets.parts.iter().zip([0xF423C, 0xF423D]) {
        let resource = SCENERY_RESOURCE_BASE + u32::from(part.resource);
        resources.models.insert(
            resource,
            ModelResource {
                names: part.bone_names.clone(),
                clips: part
                    .clips
                    .iter()
                    .map(|clip| (clip.resource_slot, AnimationClip::from(clip)))
                    .collect(),
                ..Default::default()
            },
        );
        let mut instance = Actor::new(resource, [0.; 3]);
        instance.grounded = false;
        instance.collidable = false;
        instance.casts_shadow = false;
        if part.autoplay {
            let clip = part
                .clips
                .first()
                .ok_or_else(|| anyhow::anyhow!("field autoplay clip is missing"))?;
            instance.scripted_animation = true;
            instance.animation = Some(resonance_events::Animation::new(
                resource,
                clip.resource_slot,
                clip.duration_ticks(),
                world.tick,
            ));
        }
        world.actors.insert(actor, instance);
    }
    EventRuntime::with_state(
        Arc::new(Program::decode(script)?),
        Arc::new(resources),
        world,
        memory,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia_script::{
        Width,
        message::{Message, Token},
    };

    fn choice_session() -> FieldSession {
        fn native(words: &mut Vec<u16>, op: u8, arguments: &[i32]) {
            for &value in arguments {
                words.extend([
                    0x0200,
                    value as u16,
                    (value as u32 >> 16) as u16,
                    0x3000,
                    0x4000,
                ]);
            }
            words.push(0x2000 | u16::from(op));
        }
        let mut code = vec![4, 0, 0, 0];
        native(&mut code, 0x0c, &[0, 0x20, -2, 1, 0, 0, 0, 1]);
        native(&mut code, 0x64, &[3, 0]);
        native(&mut code, 0x0c, &[1, 0, -2, 7, 0, 0, 0, 2]);
        native(&mut code, 0x64, &[3, 1]);
        native(&mut code, 0x66, &[1, 1, 2, 0, 0x100]);
        code.extend([0x3000, 0x1200, 0x100, 0x1200, 0x20, 0x3010, 0x3000]);
        native(&mut code, 0x0a, &[0]);
        native(&mut code, 0x0a, &[1]);
        native(&mut code, 0x61, &[]);
        code.push(0x20ff);
        let resources = ResourceLibrary {
            messages: ["", "Question?", "Yes\nNo"]
                .into_iter()
                .map(|text| Message {
                    tokens: vec![Token::Text { text: text.into() }],
                })
                .collect(),
            ..Default::default()
        };
        FieldSession {
            conversation_facing: None,
            active_triggers: Default::default(),
            voice_durations: Default::default(),
            voice_feedback: None,
            talking: Default::default(),
            events: EventRuntime::new(
                Arc::new(
                    Program::decode(
                        &code
                            .into_iter()
                            .flat_map(u16::to_be_bytes)
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
                ),
                Arc::new(resources),
            )
            .unwrap(),
            dialogue: BTreeMap::new(),
            choices: Default::default(),
            walkmesh: navigation::WalkMesh::new(&[resonance_content::field::CollisionGroup {
                surface: 0,
                vertices: vec![[0., 0., 0.], [10., 0., 0.], [0., 10., 0.]],
                triangles: vec![[0, 1, 2]],
            }])
            .unwrap(),
            light_regions: None,
            lighting: BTreeMap::new(),
        }
    }

    fn reveal_choices(session: &mut FieldSession) {
        for _ in 0..120 {
            if session.events.world.choices.contains_key(&1)
                && session
                    .dialogue
                    .get(&1)
                    .is_some_and(|page| page.accepts_input())
            {
                return;
            }
            session.step(FieldInput::default()).unwrap();
        }
        panic!("script never finished revealing the question and choices");
    }

    #[test]
    fn fast_confirm_does_not_close_a_page_before_its_choice_activates() {
        let mut session = choice_session();
        for _ in 0..200 {
            session
                .step(FieldInput {
                    interact: true,
                    ..Default::default()
                })
                .unwrap();
            if session.events.world.input_enabled {
                break;
            }
        }
        assert_eq!(session.events.memory().read(0x100, Width::S32).unwrap(), 1);
        assert!(session.events.world.input_enabled);
        assert!(session.events.world.dialogue.is_empty());
        assert_eq!(session.events.active_instances(), 0);
    }

    #[test]
    fn persistent_question_does_not_steal_choice_input_or_release_movement() {
        let mut session = choice_session();
        // An early confirm cannot dismiss the persistent question.
        session
            .step(FieldInput {
                interact: true,
                ..Default::default()
            })
            .unwrap();
        reveal_choices(&mut session);
        assert!(!session.dialogue[&0].closed);
        assert_eq!(session.events.world.choices[&1].selected_line, 0);
        assert!(!session.events.world.input_enabled);
        session
            .step(FieldInput {
                cancel: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            session.events.world.choices[&1]
                .operation
                .progress()
                .outcome,
            None
        );
        session
            .step(FieldInput {
                direction: [0., -1.],
                interact: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(session.events.memory().read(0x100, Width::S32).unwrap(), 2);
        assert_eq!(session.events.memory().read(0x24, Width::S32).unwrap(), 0);
        assert!(session.events.world.dialogue.is_empty());
        assert!(session.events.world.input_enabled);
        assert_eq!(session.events.active_instances(), 0);
    }

    #[test]
    fn choice_wrap_repeat_and_timeout_wait_for_revealed_text() {
        use crate::choice::{ChoiceInput, ChoicePlayer};
        use resonance_events::dialogue::ChoiceExit;
        let mut session = choice_session();
        reveal_choices(&mut session);
        let mut choice = session.events.world.choices[&1].clone();
        choice.timeout_ticks = Some(30);
        let mut player = ChoicePlayer::default();
        let up = ChoiceInput {
            direction: -1,
            ..Default::default()
        };
        for _ in 0..100 {
            assert_eq!(player.step(&mut choice, up, false), (None, false));
        }
        assert_eq!(player.step(&mut choice, up, true), (None, true));
        assert_eq!(choice.selected_line, 1); // Wrap from first to last.
        for _ in 0..19 {
            assert_eq!(player.step(&mut choice, up, true), (None, false));
        }
        assert_eq!(player.step(&mut choice, up, true), (None, true));
        assert_eq!(choice.selected_line, 0);
        for _ in 0..8 {
            assert_eq!(
                player.step(&mut choice, ChoiceInput::default(), true),
                (None, false)
            );
        }
        assert_eq!(
            player.step(&mut choice, ChoiceInput::default(), true),
            (Some(ChoiceExit::Timeout), false)
        );
        // A new operation resets countdown and repeat state.
        let mut fresh = choice_session();
        reveal_choices(&mut fresh);
        let choice = fresh.events.world.choices.get_mut(&1).unwrap();
        assert_eq!(player.step(choice, up, true), (None, true));
        assert_eq!(choice.selected_line, 1);
    }
}
