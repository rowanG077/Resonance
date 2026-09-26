//! Deterministic keyboard replay from an ordinary field save, with file-only audio.
mod scene;
use super::*;
use crate::Clock;
use bevy::{app::PluginsState, time::TimeUpdateStrategy};
pub(crate) fn recording_scene(world: &World) -> Result<serde_json::Value> {
    let result = scene::diagnostic(world);
    match world.get_resource::<crate::diagnostics::Diagnostics>() {
        Some(diagnostics) => Ok(diagnostics
            .0
            .attempt("recording scene", result)?
            .unwrap_or_else(|| serde_json::json!({"phase":"unavailable"}))),
        None => result,
    }
}
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    thread,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointReplay {
    pub version: u32,
    pub updates: u32,
    /// Absolute source presentation counter before the replay's warm-up
    /// updates. This preserves UI animation phase while gameplay starts from a
    /// semantic checkpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_origin: Option<u32>,
    /// Running-session play time observed in the paired savestate. Ordinary
    /// quickloads reset this clock; source savestates retain it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_origin: Option<u64>,
    /// Observed entry seeds in field-request order. Each request keeps its seed
    /// across preparation retries; the same encounter can occur more than once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    battle_seeds: Vec<BattleSeed>,
    /// Controlled fixtures change live progress at free field control or Main.
    /// Initialization runs at the saved story, matching a live source-state edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    story_origin: Option<StoryOrigin>,
    pub inputs: Vec<KeyboardInput>,
    /// Update zero is the initialized field, before the first input/update.
    pub captures: BTreeMap<u32, String>,
    /// Select actual combat clocks without changing absolute inputs or time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    battle_captures: Vec<BattleCapture>,
    /// Updates during which the source produced no presentation (loading).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presentation_pauses: Vec<PresentationStall>,
    /// Source effect-counter stalls during loading. Gameplay still updates;
    /// only the oracle's effect phase omits these observed increments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_pauses: Vec<PresentationStall>,
    /// Extra UI ticks for source VIs omitted from the native gameplay timeline.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub presentation_advances: BTreeMap<u32, u32>,
    /// Asynchronous source loading still draws the menu and advances its effects.
    /// These omitted VIs advance both clocks without simulating a disc wait.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub effect_advances: BTreeMap<u32, u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    expected: BTreeMap<u32, ExpectedField>,
    /// One-time oracle registration of restarted scenery and field-service loops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ambient_origin: Option<AmbientOrigin>,
    /// Register each newly loaded catalogue animation once. Source disc waits
    /// change its start time; native asset preparation does not emulate those waits.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    preview_origins: BTreeMap<u32, PreviewOrigin>,
    /// Source-observed resource waits. Ticks are relative replay updates;
    /// installation converts them to this field runtime's clock once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    resource_waits: Vec<resonance_events::ResourceWaitObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BattleCapture {
    /// Zero-based active battle request ordinal within this replay.
    index: u32,
    encounter: u16,
    combat_tick: u32,
    name: String,
}

#[derive(Default)]
struct BattleCaptures {
    requests: BTreeMap<u64, u32>,
    captured: std::collections::BTreeSet<usize>,
}

impl BattleCaptures {
    fn select(&mut self, entries: &[BattleCapture], moment: Option<(u64, u16, u32)>) -> Vec<usize> {
        let Some((request, encounter, combat_tick)) = moment else {
            return Vec::new();
        };
        let next = self.requests.len() as u32;
        let index = *self.requests.entry(request).or_insert(next);
        entries
            .iter()
            .enumerate()
            .filter_map(|(slot, entry)| {
                (entry.index == index
                    && entry.encounter == encounter
                    && entry.combat_tick == combat_tick
                    && self.captured.insert(slot))
                .then_some(slot)
            })
            .collect()
    }

    fn finish(&self, entries: &[BattleCapture]) -> Result<()> {
        let missing: Vec<_> = entries
            .iter()
            .enumerate()
            .filter(|(slot, _)| !self.captured.contains(slot))
            .map(|(_, entry)| entry.name.as_str())
            .collect();
        ensure!(
            missing.is_empty(),
            "battle capture ticks were never reached: {}",
            missing.join(", ")
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BattleSeed {
    encounter: u16,
    /// Source entry narrows the elapsed-millisecond seed to sixteen bits.
    seed: u16,
}

/// Exists only while an explicit checkpoint replay seed list is installed.
#[derive(Resource)]
pub(crate) struct BattleSeeds {
    entries: Vec<BattleSeed>,
    assigned: BTreeMap<u64, BattleSeed>,
}

impl BattleSeeds {
    pub(crate) fn for_request(&mut self, generation: u64, encounter: u16) -> Result<u32> {
        if let Some(entry) = self.assigned.get(&generation) {
            ensure!(
                entry.encounter == encounter,
                "battle request changed its encounter"
            );
            return Ok(u32::from(entry.seed));
        }
        let entry = *self
            .entries
            .get(self.assigned.len())
            .context("replay has no seed for the next battle request")?;
        ensure!(
            entry.encounter == encounter,
            "replay expected encounter {}, received {encounter}",
            entry.encounter
        );
        self.assigned.insert(generation, entry);
        Ok(u32::from(entry.seed))
    }

    fn finish(&self) -> Result<()> {
        ensure!(
            self.assigned.len() == self.entries.len(),
            "replay did not reach every battle seed observation"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum PreviewOrigin {
    Monster { monster: u8, tick: u32 },
    Figurine { figurine: u16, tick: u32 },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoryOrigin {
    update: u32,
    from: i32,
    to: i32,
}
impl PreviewOrigin {
    fn selection(&self) -> (resonance_game::menu::preview::PreviewId, u32) {
        use resonance_game::menu::preview::PreviewId;
        match *self {
            Self::Monster { monster, tick } => (PreviewId::Monster(monster), tick),
            Self::Figurine { figurine, tick } => (PreviewId::Figurine(figurine), tick),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationStall {
    pub start: u32,
    pub end: u32,
}
impl PresentationStall {
    fn contains(&self, update: u32) -> bool {
        (self.start..=self.end).contains(&update)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedField {
    map_id: u32,
    story: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    free_control: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    saved_slot: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AmbientOrigin {
    update: u32,
    samples: BTreeMap<i32, f32>,
    /// A settled source view may retain fractional orbit angles that saves omit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    camera: Option<CameraOrigin>,
    /// Running effect phase, independent of paused field animation and UI time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effect_tick: Option<u32>,
    /// Retained post-draw hint state omitted by ordinary saves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    action_prompt: Option<ActionPromptOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    random_state: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gameplay_random: Option<GameplayRandomOrigin>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    eyes: BTreeMap<i32, resonance_events::EyeBlink>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    actors: BTreeMap<i32, resonance_events::ActorOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    flutters: Option<Vec<resonance_events::effect::FlutterOrigin>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    background_waits: Vec<resonance_events::BackgroundWaitOrigin>,
    /// Observed random births reconstructed through the ordinary effect recipe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    save_sparks: Option<Vec<SparkOrigin>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    poison_puffs: Option<Vec<PoisonOrigin>>,
    /// Source-observed transient notification phase; never part of a save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    skit: Option<SkitOrigin>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionPromptOrigin {
    id: u8,
    opacity: u8,
    remaining: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraOrigin {
    settings: resonance_events::camera::CameraSettings,
    angles: [f32; 3],
    distance: f32,
    position: [f32; 3],
    target: [f32; 3],
}
impl CameraOrigin {
    fn apply(
        &self,
        rig: &mut resonance_events::camera::CameraRig,
        actors: &BTreeMap<i32, resonance_events::Actor>,
        player: i32,
    ) -> Result<()> {
        ensure!(
            rig.settings(player).map_err(anyhow::Error::msg)? == self.settings,
            "camera origin changes the saved follow settings"
        );
        ensure!(
            self.angles.iter().all(|v| (0. ..360.).contains(v))
                && (1. ..=100_000.).contains(&self.distance)
                && self
                    .position
                    .iter()
                    .chain(&self.target)
                    .all(|v| v.is_finite() && v.abs() <= 100_000.),
            "invalid observed camera pose"
        );
        rig.snap_follow_view(actors);
        rig.angles = self.angles;
        rig.distance = self.distance;
        rig.position = self.position;
        rig.target = self.target;
        Ok(())
    }
}

#[cfg(test)]
mod camera_origin_tests {
    use super::*;

    #[test]
    fn observed_fractional_orbit_does_not_change_saved_settings() {
        use resonance_events::camera::CameraRig;
        let actors = [(1, resonance_events::Actor::new(0, [-2855., 1513., 66.]))].into();
        let mut rig = CameraRig::default();
        let camera = rig.current_mut();
        camera.actor = 1;
        camera.follow = true;
        camera.anchor_to_actor = true;
        camera.angles = [330., 0., 38.];
        camera.distance = 1669.;
        let settings = rig.settings(1).unwrap();
        let mut origin = CameraOrigin {
            settings: settings.clone(),
            angles: [330.875, 0., 38.875],
            distance: 1669.0005,
            position: [-1940., 378., 965.],
            target: [-2855., 1513., 153.],
        };
        origin.apply(&mut rig, &actors, 1).unwrap();
        assert_eq!((rig.angles, rig.distance), (origin.angles, origin.distance));
        assert_eq!((rig.position, rig.target), (origin.position, origin.target));
        assert_eq!(rig.settings(1).unwrap(), settings);
        origin.settings.distance += 1.;
        assert!(origin.apply(&mut rig, &actors, 1).is_err());
        origin.settings = settings;
        origin.position[0] = f32::NAN;
        assert!(origin.apply(&mut rig, &actors, 1).is_err());
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GameplayRandomOrigin {
    Uninitialized,
    State(resonance_events::GameplayRandom),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoisonOrigin {
    age: u32,
    position: [f32; 3],
    size: u8,
    speed_sixteenths: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkitOrigin {
    id: u16,
    control_ticks: u32,
    remaining: u16,
    opacity: u8,
    text_opacity: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SparkOrigin {
    save_point: usize,
    age: u32,
    offset: [i32; 2],
    size: u8,
    speed_eighths: u8,
}
impl SparkOrigin {
    fn effect(
        &self,
        world: &resonance_events::GameWorld,
        effect_tick: u32,
    ) -> Result<resonance_events::effect::BillboardEffect> {
        let point = world
            .save_points
            .get(self.save_point)
            .context("missing save point")?;
        ensure!(
            point.active
                && self.age <= 60
                && self.age <= world.tick
                && self.offset.iter().all(|v| (-31..=32).contains(v))
                && (32..=47).contains(&self.size)
                && (16..=47).contains(&self.speed_eighths),
            "spark origin is outside the active emitter's recipe"
        );
        let mut position = point.position;
        for (p, offset) in position.iter_mut().zip(self.offset) {
            *p += offset as f32;
        }
        let mut effect = resonance_events::effect::BillboardEffect::rising_spark(
            position,
            f32::from(self.size),
            f32::from(self.speed_eighths) / 8.,
            world.tick - self.age,
            effect_tick.wrapping_sub(self.age),
        );
        for _ in 0..self.age {
            effect.step();
        }
        Ok(effect)
    }
}
impl AmbientOrigin {
    fn apply(&self, field: &mut resonance_game::field::FieldSession) -> Result<()> {
        ensure!(
            field.player_has_control(),
            "ambient origin requires free player control"
        );
        if let Some(prompt) = &self.action_prompt {
            field.apply_action_prompt_origin(prompt.id, prompt.opacity, prompt.remaining)?;
        }
        let effect_tick = self.effect_tick.unwrap_or(field.effect_clock.tick());
        let world = &mut field.events.world;
        let sparks = self
            .save_sparks
            .as_ref()
            .map(|sparks| {
                sparks
                    .iter()
                    .map(|spark| spark.effect(world, effect_tick))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        for (&id, &sample) in &self.samples {
            let actor = world.actors.get(&id).context("missing ambient actor")?;
            let animation = actor.animation.as_ref().context("missing ambient loop")?;
            ensure!(
                ((resonance_content::field::SCENERY_RESOURCE_BASE
                    ..resonance_content::field::SAVE_POINT_RESOURCE)
                    .contains(&actor.resource)
                    || world.save_points.iter().any(|p| p.actor == id))
                    && animation.repeat
                    && animation.blend_ticks == 0
                    && (0. ..=animation.duration_ticks as f32).contains(&sample),
                "origin must select an existing scenery/service loop; actor {id}"
            );
        }
        for (&id, &sample) in &self.samples {
            let animation = world
                .actors
                .get_mut(&id)
                .unwrap()
                .animation
                .as_mut()
                .unwrap();
            animation.start_frame = sample;
            animation.phase_tick = world.tick;
            animation.binding_updates = 0;
        }
        if let Some(sparks) = sparks {
            world.billboards.retain(|_, effect| effect.recipe != 8);
            for spark in sparks {
                world.emit_billboard(spark).map_err(anyhow::Error::msg)?;
            }
        }
        if let Some(puffs) = &self.poison_puffs {
            world.billboards.retain(|_, effect| effect.recipe != 10);
            for puff in puffs {
                ensure!(
                    (1..=20).contains(&puff.age)
                        && (8..=31).contains(&puff.size)
                        && (32..=63).contains(&puff.speed_sixteenths)
                        && puff.position.iter().all(|v| v.is_finite()),
                    "invalid poison origin"
                );
                let born = world
                    .tick
                    .checked_sub(puff.age - 1)
                    .context("poison origin predates field initialization")?;
                // The source observation is after drawing and advancing the puff.
                let speed = f32::from(puff.speed_sixteenths) / 16.;
                let mut position = puff.position;
                position[2] -= speed;
                let effect = resonance_events::effect::BillboardEffect::poison(
                    position,
                    f32::from(puff.size),
                    speed,
                    born,
                );
                world.emit_billboard(effect).map_err(anyhow::Error::msg)?;
            }
        }
        field.effect_clock = resonance_game::clock::PresentationClock::new(effect_tick);
        if let Some(seed) = self.random_state {
            world.random_state = seed;
        }
        if let Some(random) = &self.gameplay_random {
            world.gameplay_random = match random {
                GameplayRandomOrigin::Uninitialized => Default::default(),
                GameplayRandomOrigin::State(state) => state.clone(),
            };
        }
        for (&actor, &eyes) in &self.eyes {
            field.events.apply_eye_origin(actor, eyes)?;
        }
        for (&actor, origin) in &self.actors {
            field.events.apply_actor_origin(actor, origin)?;
        }
        if let Some(camera) = &self.camera {
            let world = &mut field.events.world;
            camera.apply(
                world
                    .field_camera
                    .as_mut()
                    .context("missing field camera")?,
                &world.actors,
                world.controlled_actor,
            )?;
        }
        if let Some(flutters) = &self.flutters {
            field.events.apply_flutter_origin(flutters)?;
        }
        for wait in &self.background_waits {
            field.events.apply_background_wait_origin(wait)?;
        }
        if let Some(skit) = &self.skit {
            field.apply_skit_origin(
                skit.id,
                skit.control_ticks,
                skit.remaining,
                skit.opacity,
                skit.text_opacity,
            )?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyboardInput {
    pub update: u32,
    pub keys: Vec<Key>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Run,
    Interact,
    Skit,
    Cancel,
    Menu,
    Alternate,
    PreviousPage,
    NextPage,
    PageUp,
    PageDown,
    RotateLeft,
    RotateRight,
    Start,
    Quicksave,
    Quickload,
}
impl Key {
    fn event(self, state: bevy::input::ButtonState) -> bevy::input::keyboard::KeyboardInput {
        use bevy::input::keyboard::Key as Logical;
        let (key_code, logical_key) = match self {
            Self::Left => (KeyCode::ArrowLeft, Logical::ArrowLeft),
            Self::Right => (KeyCode::ArrowRight, Logical::ArrowRight),
            Self::Up => (KeyCode::ArrowUp, Logical::ArrowUp),
            Self::Down => (KeyCode::ArrowDown, Logical::ArrowDown),
            Self::Run => (KeyCode::ShiftLeft, Logical::Shift),
            Self::Interact => (KeyCode::Enter, Logical::Enter),
            Self::Skit => (KeyCode::KeyZ, Logical::Character("z".into())),
            Self::Cancel => (KeyCode::Escape, Logical::Escape),
            Self::Menu => (KeyCode::Tab, Logical::Tab),
            Self::Alternate => (KeyCode::KeyX, Logical::Character("x".into())),
            Self::PreviousPage => (KeyCode::KeyQ, Logical::Character("q".into())),
            Self::NextPage => (KeyCode::KeyE, Logical::Character("e".into())),
            Self::PageUp => (KeyCode::PageUp, Logical::PageUp),
            Self::PageDown => (KeyCode::PageDown, Logical::PageDown),
            Self::RotateLeft => (KeyCode::BracketLeft, Logical::Character("[".into())),
            Self::RotateRight => (KeyCode::BracketRight, Logical::Character("]".into())),
            Self::Start => (KeyCode::Home, Logical::Home),
            Self::Quicksave => (KeyCode::F5, Logical::F5),
            Self::Quickload => (KeyCode::F9, Logical::F9),
        };
        bevy::input::keyboard::KeyboardInput {
            key_code,
            logical_key,
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }
}
impl CheckpointReplay {
    fn register_effect_clock(
        &self,
        update: u32,
        clock: &mut resonance_game::clock::PresentationClock,
    ) {
        let extra = self.effect_advances.get(&update).copied().unwrap_or(0);
        let paused = self.effect_pauses.iter().any(|p| p.contains(update));
        // FieldSession::step still advances once. Cancel only that effect tick.
        *clock = resonance_game::clock::PresentationClock::new(
            clock
                .tick()
                .wrapping_add(extra)
                .wrapping_sub(u32::from(paused)),
        );
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.story_origin
                .as_ref()
                .is_none_or(|s| s.from >= 0 && s.to >= 0 && (1..=self.updates).contains(&s.update)),
            "invalid story origin"
        );
        ensure!(
            self.version == 1 && (1..=36_000).contains(&self.updates),
            "invalid checkpoint replay length/version"
        );
        ensure!(
            self.battle_seeds.len() <= self.updates as usize,
            "battle seed observations exceed replay duration"
        );
        ensure!(
            self.inputs.len() <= self.updates as usize
                && self.inputs.windows(2).all(|w| w[0].update < w[1].update)
                && self
                    .inputs
                    .iter()
                    .all(|i| (1..=self.updates).contains(&i.update) && i.keys.len() <= 10),
            "invalid checkpoint replay inputs"
        );
        let capture_count = self.captures.len() + self.battle_captures.len();
        let valid_name = |name: &str| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        };
        ensure!(
            (1..=256).contains(&capture_count)
                && self
                    .captures
                    .iter()
                    .all(|(&tick, name)| tick <= self.updates && valid_name(name))
                && self
                    .battle_captures
                    .iter()
                    .all(|capture| capture.index < self.updates
                        && (1..=self.updates).contains(&capture.combat_tick)
                        && valid_name(&capture.name)),
            "invalid checkpoint replay captures"
        );
        let names: std::collections::BTreeSet<_> = self
            .captures
            .values()
            .chain(self.battle_captures.iter().map(|capture| &capture.name))
            .collect();
        ensure!(
            names.len() == capture_count,
            "duplicate checkpoint capture name"
        );
        let moments: std::collections::BTreeSet<_> = self
            .battle_captures
            .iter()
            .map(|capture| (capture.index, capture.encounter, capture.combat_tick))
            .collect();
        ensure!(
            moments.len() == self.battle_captures.len(),
            "duplicate battle capture tick"
        );
        ensure!(
            self.expected
                .keys()
                .all(|update| self.captures.contains_key(update)),
            "expected field state requires a named capture"
        );
        ensure!(
            self.preview_origins
                .keys()
                .all(|update| *update <= self.updates),
            "preview origin exceeds replay duration"
        );
        ensure!(
            self.resource_waits.iter().all(|o| {
                o.request_tick > 0
                    && o.request_tick < o.resume_tick
                    && o.resume_tick <= self.updates
            }) && self
                .resource_waits
                .windows(2)
                .all(|w| w[0].request_tick <= w[1].request_tick),
            "invalid replay resource wait schedule"
        );
        for pauses in [&self.presentation_pauses, &self.effect_pauses] {
            ensure!(
                pauses
                    .iter()
                    .all(|p| p.start > 0 && p.start <= p.end && p.end <= self.updates)
                    && pauses.windows(2).all(|w| w[0].end < w[1].start),
                "invalid replay clock pause ranges"
            );
        }
        ensure!(
            self.presentation_advances
                .iter()
                .all(|(&update, &ticks)| update > 0
                    && update <= self.updates
                    && (1..=3600).contains(&ticks)),
            "invalid presentation clock advance"
        );
        ensure!(
            self.effect_advances.iter().all(|(update, ticks)| *ticks > 0
                && self
                    .presentation_advances
                    .get(update)
                    .is_some_and(|ui| ticks <= ui)),
            "effect clock advances require matching presentation advances"
        );
        for expected in self.expected.values() {
            if let Some(slot) = &expected.saved_slot {
                SlotId::new(slot)?;
            }
        }
        if let Some(origin) = &self.ambient_origin {
            ensure!(
                self.captures
                    .first_key_value()
                    .is_some_and(|(&tick, _)| origin.update == tick)
                    && origin.samples.len() <= 32
                    && origin.eyes.len() <= 32
                    && origin.actors.len() <= 32
                    && origin.background_waits.len() <= 32
                    && origin
                        .flutters
                        .as_ref()
                        .is_none_or(|leaves| leaves.len() <= 32)
                    && (origin.random_state.is_none() || origin.save_sparks.is_some())
                    && (!origin.samples.is_empty()
                        || origin.camera.is_some()
                        || origin.action_prompt.is_some()
                        || origin.gameplay_random.is_some()
                        || origin.poison_puffs.is_some()
                        || origin.skit.is_some()
                        || !origin.eyes.is_empty()
                        || !origin.actors.is_empty()
                        || origin.flutters.is_some()
                        || !origin.background_waits.is_empty()
                        || origin.save_sparks.is_some())
                    && origin
                        .poison_puffs
                        .as_ref()
                        .is_none_or(|puffs| puffs.len() <= 6)
                    && origin
                        .save_sparks
                        .as_ref()
                        .is_none_or(|sparks| sparks.len() <= 16)
                    && origin.samples.values().all(|v| v.is_finite()),
                "ambient origin must register the first capture with bounded finite samples"
            );
            ensure!(
                origin
                    .action_prompt
                    .as_ref()
                    .is_none_or(|p| p.opacity > 0 && (1..30).contains(&p.remaining)),
                "action hint origin requires visible retained state"
            );
        }
        Ok(())
    }
}

pub fn record_checkpoint(
    root: &Path,
    save: &Path,
    output: &Path,
    spec: &CheckpointReplay,
) -> Result<()> {
    record_checkpoint_with_display(root, save, output, spec, crate::Resolution::default())
}

pub fn record_checkpoint_with_display(
    root: &Path,
    save: &Path,
    output: &Path,
    spec: &CheckpointReplay,
    resolution: crate::Resolution,
) -> Result<()> {
    record_checkpoint_with_save_directory(root, save, output, spec, resolution, None)
}

/// An optional private store permits ordinary menu loads during a recording.
/// The output directory must still be fresh, independently of the save store.
pub fn record_checkpoint_with_save_directory(
    root: &Path,
    save: &Path,
    output: &Path,
    spec: &CheckpointReplay,
    resolution: crate::Resolution,
    save_directory: Option<&Path>,
) -> Result<()> {
    record_checkpoint_with_options(
        root,
        save,
        output,
        spec,
        CheckpointRecordingOptions {
            resolution,
            save_directory,
            paranoid: false,
        },
    )
}

/// Recording policy is shared with the live application's diagnostic session.
#[derive(Default)]
pub struct CheckpointRecordingOptions<'a> {
    pub resolution: crate::Resolution,
    pub save_directory: Option<&'a Path>,
    pub paranoid: bool,
}

pub fn record_checkpoint_with_options(
    root: &Path,
    save: &Path,
    output: &Path,
    spec: &CheckpointReplay,
    options: CheckpointRecordingOptions<'_>,
) -> Result<()> {
    spec.validate()?;
    let mut app = probe::app_with_saves_mode(
        root,
        output,
        SaveOptions {
            directory: Some(
                options
                    .save_directory
                    .map_or_else(|| output.join("slots"), Path::to_path_buf),
            ),
            quick_slot: Some("probe".into()),
            load: Some(save.into()),
        },
        options.resolution,
        options.paranoid,
    )?;
    let began = Instant::now();
    while app.plugins_state() == PluginsState::Adding {
        ensure!(
            began.elapsed().as_secs() < 60,
            "checkpoint renderer setup timed out"
        );
        bevy::tasks::tick_global_task_pools_on_main_thread();
        thread::sleep(std::time::Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    let (mixer, mut audio) = resonance_playback::Offline::new();
    record_live(&mut app, output, spec, &mixer, &mut audio)
}

/// Continue an already initialized game with its existing mixer and scene.
pub(crate) fn record_live(
    app: &mut App,
    output: &Path,
    spec: &CheckpointReplay,
    mixer: &resonance_playback::Control,
    audio: &mut impl Iterator<Item = f32>,
) -> Result<()> {
    spec.validate()?;
    let diagnostics = app
        .world()
        .resource::<crate::diagnostics::Diagnostics>()
        .0
        .clone();
    ensure!(
        !output.join("replay.json").exists(),
        "replay output already exists"
    );
    fs::create_dir_all(output)?;
    fs::write(output.join("replay.json"), serde_json::to_vec_pretty(spec)?)?;
    crate::audio::validate_startup(app, true, true)?;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    app.init_resource::<crate::field_audio::Trace>();
    if !spec.battle_seeds.is_empty() {
        ensure!(
            !app.world().contains_resource::<BattleSeeds>(),
            "battle replay seeds are already installed"
        );
        app.insert_resource(BattleSeeds {
            entries: spec.battle_seeds.clone(),
            assigned: BTreeMap::new(),
        });
    }
    let mut preparation_timed_out = false;
    wait_ready(app, true, &mut preparation_timed_out)?;
    let identity = app
        .world()
        .get_resource::<new_game::Session>()
        .map(|s| s.identity.clone());
    if let Some(tick) = spec.presentation_origin {
        *app.world_mut().resource_mut::<Clock>() =
            Clock(resonance_game::clock::PresentationClock::new(tick));
    }
    if let Some(session_ticks) = spec.session_origin {
        diagnostics.attempt(
            "checkpoint session origin",
            (|| {
                let mut session = app
                    .world_mut()
                    .get_resource_mut::<new_game::Session>()
                    .context("session origin requires a live field session")?;
                let field = &mut session.field;
                field.play_time = resonance_game::clock::PlayTime::with_session(
                    field.play_time.total(),
                    session_ticks,
                )
                .context("observed session time exceeds total play time")?;
                Ok(())
            })(),
        )?;
    }
    let mut initial = diagnostics
        .attempt("checkpoint initial state", checkpoint(app.world_mut()))?
        .map(serde_json::to_value)
        .transpose()?
        .unwrap_or_else(|| serde_json::json!({}));
    initial["presentation_counter"] =
        serde_json::json!(app.world().resource::<crate::Clock>().0.tick());
    let resource_wait_origin_tick = app
        .world()
        .get_resource::<new_game::Session>()
        .map_or(0, |s| s.field.events.tick());
    if !spec.resource_waits.is_empty() {
        let observations = spec
            .resource_waits
            .iter()
            .map(|o| {
                Ok(resonance_events::ResourceWaitObservation {
                    request_tick: resource_wait_origin_tick
                        .checked_add(o.request_tick)
                        .context("resource request clock overflow")?,
                    resume_tick: resource_wait_origin_tick
                        .checked_add(o.resume_tick)
                        .context("resource resume clock overflow")?,
                    ..*o
                })
            })
            .collect::<Result<Vec<_>>>()?;
        diagnostics.attempt(
            "checkpoint resource waits",
            (|| {
                app.world_mut()
                    .get_resource_mut::<new_game::Session>()
                    .context("resource waits require a live field session")?
                    .field
                    .events
                    .register_resource_wait_observations(observations)
            })(),
        )?;
    }
    diagnostics.attempt("checkpoint audio attachment", attach(app, mixer))?;
    let mut wave = hound::WavWriter::create(
        output.join("audio.partial.wav"),
        hound::WavSpec {
            channels: 2,
            sample_rate: 32028,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    let failure = std::sync::Arc::new(AtomicBool::new(false));
    let written = std::sync::Arc::new(AtomicU32::new(0));
    let mut captures = Vec::new();
    let mut battle_captures = BattleCaptures::default();
    let mut frames = 0;
    let mut held = Vec::<Key>::new();
    let mut inputs = spec.inputs.iter().peekable();
    let mut preview_instance = None;
    let mut preview_registered = false;
    let mut resource_waits_finished = spec.resource_waits.is_empty();
    let began = Instant::now();
    let mut completed_updates = 0;
    for update in 0..=spec.updates {
        if began.elapsed().as_secs() >= 600 {
            diagnostics.report(
                "checkpoint replay",
                anyhow::anyhow!("checkpoint replay timed out at update {update}"),
            )?;
            break;
        }
        if update > 0 {
            if let Some(&ticks) = spec.presentation_advances.get(&update) {
                let mut clock = app.world_mut().resource_mut::<Clock>();
                clock.0 = resonance_game::clock::PresentationClock::new(
                    clock.0.tick().wrapping_add(ticks),
                );
            }
            if let Some(mut session) = app.world_mut().get_resource_mut::<new_game::Session>() {
                // These registrations describe field visits. A retained field
                // consumes no effect tick while its battle caller is suspended.
                if !session.field.events.battle_pending() {
                    spec.register_effect_clock(update, &mut session.field.effect_clock);
                }
                if !resource_waits_finished {
                    resource_waits_finished = session
                        .field
                        .events
                        .finish_resource_wait_observations()
                        .is_ok();
                }
            }
            app.world_mut().resource_mut::<crate::PresentationPause>().0 = spec
                .presentation_pauses
                .iter()
                .any(|pause| pause.contains(update));
            if let Some(input) = inputs.next_if(|input| input.update == update) {
                // Feed the input plugin so one-shot keys survive its per-frame
                // edge reset, just as they do when received from a window.
                use bevy::input::ButtonState;
                for key in held.iter().filter(|key| !input.keys.contains(key)) {
                    app.world_mut()
                        .write_message(key.event(ButtonState::Released));
                }
                for key in input.keys.iter().filter(|key| !held.contains(key)) {
                    app.world_mut()
                        .write_message(key.event(ButtonState::Pressed));
                }
                held.clone_from(&input.keys);
            }
            app.insert_resource(TimeUpdateStrategy::ManualDuration(
                resonance_game::clock::UPDATE_STEP,
            ));
            app.update();
            crate::playthrough::check_exit(app)?;
            wait_ready(app, false, &mut preparation_timed_out)?;
            diagnostics.attempt("checkpoint audio attachment", attach(app, mixer))?;
            let end = u64::from(update) * 32028 * resonance_game::clock::UPDATE_RATE_DENOMINATOR
                / resonance_game::clock::UPDATE_RATE_NUMERATOR;
            for _ in frames..end {
                for _ in 0..2 {
                    let sample = diagnostics
                        .attempt(
                            "checkpoint audio",
                            (|| {
                                let sample = audio.next().context("checkpoint audio stopped")?;
                                ensure!(sample.is_finite(), "nonfinite checkpoint audio");
                                Ok(sample)
                            })(),
                        )?
                        .unwrap_or(0.);
                    wave.write_sample((sample * 32768.).round().clamp(-32768., 32767.) as i16)?;
                }
            }
            frames = end;
            if let Some(control) = app.world().get_resource::<crate::field_audio::Control>() {
                diagnostics.attempt("checkpoint audio", control.check())?;
            }
        }
        if let Some(origin) = &spec.story_origin
            && origin.update == update
        {
            diagnostics.attempt(
                "checkpoint story origin",
                (|| {
                    let mut session = app
                        .world_mut()
                        .get_resource_mut::<new_game::Session>()
                        .context("story origin requires a live field session")?;
                    let field = &mut session.field;
                    ensure!(
                        field.story_progress()? == origin.from,
                        "story origin differs from the live field"
                    );
                    if let Some(menu) = &mut field.menu {
                        ensure!(
                            menu.page == resonance_game::menu::Page::Main,
                            "story fixture must start at the main menu"
                        );
                        let progress = &mut menu
                            .checkpoint
                            .as_mut()
                            .context("missing menu checkpoint")?
                            .progress;
                        ensure!(
                            progress.script_globals[16] == origin.from,
                            "story origin differs from the saved field"
                        );
                        progress.script_globals[16] = origin.to;
                    } else {
                        ensure!(
                            field.player_has_control(),
                            "story fixture requires free field control"
                        );
                    }
                    field.events.set_global(16, origin.to)?;
                    Ok(())
                })(),
            )?;
        }
        if let Some(origin) = &spec.ambient_origin
            && origin.update == update
        {
            diagnostics.attempt(
                "checkpoint ambient origin",
                (|| {
                    let mut session = app
                        .world_mut()
                        .get_resource_mut::<new_game::Session>()
                        .context("ambient origin requires a live field session")?;
                    origin.apply(&mut session.field)?;
                    Ok(())
                })(),
            )?;
        }
        let preview = app
            .world()
            .get_resource::<new_game::Session>()
            .and_then(|session| session.field.menu.as_ref())
            .and_then(|menu| menu.preview().map(|preview| preview.id));
        if preview != preview_instance {
            preview_instance = preview;
            preview_registered = false;
        }
        if let Some(origin) = spec.preview_origins.get(&update) {
            diagnostics.attempt(
                "checkpoint preview origin",
                (|| {
                    ensure!(
                        !preview_registered,
                        "preview animation can only be registered once per selection"
                    );
                    let mut session = app
                        .world_mut()
                        .get_resource_mut::<new_game::Session>()
                        .context("preview origin requires a live field session")?;
                    let menu = session
                        .field
                        .menu
                        .as_mut()
                        .context("preview origin requires a catalogue menu")?;
                    let (id, tick) = origin.selection();
                    ensure!(
                        menu.register_preview(id, tick),
                        "preview origin must select a sample in the displayed model's animation"
                    );
                    preview_registered = true;
                    Ok(())
                })(),
            )?;
        }
        let moment = app
            .world()
            .get_resource::<crate::battle::Owner>()
            .and_then(|battle| battle.capture_combat());
        let selected = battle_captures.select(&spec.battle_captures, moment);
        let names = spec
            .captures
            .get(&update)
            .map(|name| (name, None))
            .into_iter()
            .chain(selected.iter().map(|&slot| {
                let capture = &spec.battle_captures[slot];
                (&capture.name, Some(capture))
            }));
        for (name, battle_capture) in names {
            if let Err(error) = crate::new_game_capture::screenshot_held(
                app,
                output.join(format!("{name}.png")),
                failure.clone(),
                written.clone(),
            ) {
                if failure.load(Ordering::Acquire) {
                    return Err(error);
                }
                diagnostics.report(&format!("checkpoint capture {name}"), error)?;
                continue;
            }
            let scene = diagnostics.attempt("checkpoint scene", recording_scene(app.world()))?;
            let Some(session) = app.world().get_resource::<new_game::Session>() else {
                if spec.expected.contains_key(&update) {
                    diagnostics.report(
                        "checkpoint expected field",
                        anyhow::anyhow!(
                            "capture {name} expects a field after its session was retired"
                        ),
                    )?;
                }
                captures.push(serde_json::json!({
                    "name":name, "update":update, "audio_frame":frames,
                    "presentation_counter":app.world().resource::<crate::Clock>().0.tick(),
                    "scene":scene, "battle_capture":battle_capture,
                    "audio_settings":app.world().get_resource::<crate::field_audio::Control>().map(|c|c.settings()),
                }));
                continue;
            };
            let field = &session.field;
            if let Some(expected) = spec.expected.get(&update) {
                diagnostics.attempt("checkpoint expected field", (|| {
                ensure!(
                    field.map_id == expected.map_id
                        && field.story_progress()? == expected.story
                        && expected
                            .free_control
                            .is_none_or(|free| free == field.player_has_control()),
                    "capture {name} missed its expected field/story/control state: map={}, story={}, checkpoint={:?}",
                    field.map_id,
                    field.story_progress()?,
                    field.checkpoint().map(|_| ())
                );
                if let Some(slot) = &expected.saved_slot {
                    let bytes = app
                        .world()
                        .resource::<Persistence>()
                        .store
                        .read(Kind::Save, &SlotId::new(slot)?)?;
                    let (_, saved): (_, FieldCheckpoint) =
                        resonance_persistence::decode(&bytes, &session.identity)?;
                    let current = field
                        .menu
                        .as_ref()
                        .and_then(|m| m.checkpoint.clone())
                        .map_or_else(|| field.checkpoint(), Ok)?;
                    ensure!(
                        saved.map_id == current.map_id
                            && saved.position == current.position
                            && saved.heading == current.heading
                            && saved.progress.script_globals == current.progress.script_globals
                            && saved.progress.event_flags == current.progress.event_flags
                            && serde_json::to_value(&saved.progress.party)?
                                == serde_json::to_value(&current.progress.party)?,
                        "capture {name} has no matching durable menu save"
                    );
                }
                    Ok(())
                })())?;
            }
            let actor = diagnostics.attempt(
                "checkpoint controlled actor",
                field
                    .events
                    .world
                    .actors
                    .get(&field.events.world.controlled_actor)
                    .context("missing controlled actor"),
            )?;
            let menu = field.menu.as_ref().map(|m| {
                let mut state = serde_json::json!({
                "page":format!("{:?}",m.page),"focus":format!("{:?}",m.focus),
                "selected":m.selected,"character":m.character,
                "first_character":m.first_character,"swap_character":m.swap_character,
                "statistics":{"party":m.party_statistics,"status":m.status.details},
                "status": &m.status,
                "ex_stats":(m.page==resonance_game::menu::Page::ExSkills).then(||m.member().stats(&m.resources.as_ref().unwrap().data)),
                "ex_compounds":(m.page==resonance_game::menu::Page::ExSkills).then(||m.ex_compounds()),
                "unison_choices":(m.page==resonance_game::menu::Page::Unison).then(|| {
                    let allowed = &m.resources.as_ref().unwrap().session.characters[m.unison_member_index()].allowed_techniques;
                    m.unison_techniques().iter().map(|id| allowed.iter().position(|a| a==id).unwrap()).collect::<Vec<_>>()
                }),
                "unison_selection":(m.page==resonance_game::menu::Page::Unison).then(||m.unison_selection()).flatten(),
                "tech_unison_available":(m.page==resonance_game::menu::Page::Tech).then(||m.tech_unison_available()),
                "tech_choices":(m.page==resonance_game::menu::Page::Tech).then(|| {
                    let allowed = &m.resources.as_ref().unwrap().session.characters[m.tech_member_index()].allowed_techniques;
                    m.technique_list().iter().map(|id| allowed.iter().position(|a| a==id).unwrap()).collect::<Vec<_>>()
                }),
                "tech_flags":(m.page==resonance_game::menu::Page::Tech).then(|| {
                    m.checkpoint.as_ref().unwrap().progress.party.members.iter()
                        .zip(&m.resources.as_ref().unwrap().session.characters)
                        .map(|(member,definition)| definition.allowed_techniques.iter().enumerate()
                            .fold([0u64; 2], |mut flags,(i,id)| {
                                if member.techniques.contains(id) {
                                    flags[0] |= 1 << i;
                                    if !member.disabled_techniques.contains(id) { flags[1] |= 1 << i; }
                                }
                                flags
                            })).collect::<Vec<_>>()
                }),
                "party":m.checkpoint.as_ref().map(|c|&c.progress.party),
                "inventory":{"category":m.inventory.category,"row":m.inventory.row,"first":m.inventory.first,"focus":format!("{:?}",m.inventory.focus),"notice":m.inventory.notice,
                    "target":m.inventory.target,"target_all":m.inventory.target_all,"target_equipment":m.inventory.target_equipment,"target_preview":m.inventory.target_preview,"target_ticks":m.inventory.target_ticks,
                    "target_opacity":m.inventory.target_opacity,"target_closing":m.inventory.target_closing,
                    "page_fade":m.inventory.page_fade,"page_closing":m.inventory.page_closing,
                    "scroll":m.inventory.scroll,
                    "transform":m.inventory.transform,
                    "description_previous":m.inventory.description_previous,"description_fade":m.inventory.description_fade,"description_opacity":m.inventory.description_opacity},
                "collection":m.collection,
                "world_map":m.world_map,
                "monsters":m.monsters,
                "manual":m.manual,
                "figurines":m.figurines,
                "figurine_selected":(m.page==resonance_game::menu::Page::Figurines).then(||m.figurine().map(|r|r.id)).flatten(),
                "figurine_animation":(m.page==resonance_game::menu::Page::Figurines).then(||m.preview()).flatten().and_then(|p| {
                    let resonance_game::menu::preview::PreviewId::Figurine(id) = p.id else {return None};
                    let duration = p.model.parts[0].scene.clips.first()?.duration_ticks();
                    Some(serde_json::json!({"figurine":id,"duration":duration,
                        "sample":p.sample(duration),"yaw":p.yaw,"distance":p.distance}))
                }),
                "monster_animation": (m.page == resonance_game::menu::Page::Monsters)
                    .then(|| m.displayed_monster()).flatten().and_then(|(r, _)| {
                        let duration = r.preview.parts[0].scene.clips.first()?.duration_ticks();
                        Some(serde_json::json!({"monster":r.id,"duration":duration,
                            "sample":m.monsters.sample(duration)}))
                    }),
                "at_save_point":m.at_save_point,"equipment":m.equipment,"tech":m.tech,"strategy":m.strategy,"unison":m.unison,
                "synopsis":m.synopsis,"cooking":m.cooking,"customize":m.customize,"ex_skills":m.ex_skills,
                "tick":m.tick,"bank":m.bank,"slot":m.slot,"confirmation":m.confirmation,
                "notice":m.notice,"popup":m.popup,"busy":m.busy});
                state["rename"] = serde_json::json!(&m.rename);
                state["system_opacity"] = serde_json::json!(m.system_opacity);
                state["system_closing"] = serde_json::json!(m.system_closing);
                if m.page == resonance_game::menu::Page::Equip {
                    state["equipment_count"] = serde_json::json!(m.equipment_items().len());
                }
                if m.page == resonance_game::menu::Page::Strategy {
                    state["strategy_presets"] = serde_json::json!(m.strategy_presets());
                }
                if m.page == resonance_game::menu::Page::Synopsis {
                    state["synopsis_records"] = serde_json::json!(&m.checkpoint.as_ref().unwrap().progress.event_records);
                    state["synopsis_ids"] = serde_json::json!(m.synopsis_records());
                }
                state["names"] = serde_json::json!(m.checkpoint.as_ref().map(|c| (0..c.progress.party.members.len()).map(|i| m.character_name(i)).collect::<Vec<_>>()));
                state["rename_gems"] = serde_json::json!(m.checkpoint.as_ref().map(|c| c.progress.party.items.get(&resonance_content::menu_data::RENAME_GEM).copied().unwrap_or(0)));
                state
            });
            let flutters: Vec<_> = field
                .events
                .world
                .particles
                .iter()
                .filter_map(|p| {
                    p.flutter.as_ref().map(|motion| {
                        serde_json::json!({
                            "kind":p.kind,"position":p.position,"motion":motion,
                            "age":field.events.world.tick - p.born,"lifetime":p.lifetime,
                            "alpha":p.alpha(field.events.world.tick),
                        })
                    })
                })
                .collect();
            let shop = field.shop.as_ref().map(|s| {
                let party = field.events.world.party.as_ref().expect("shop party");
                serde_json::json!({
                    "id":s.id,"choice":s.choice,"focus":s.focus,"row":s.row,"first":s.first,
                    "category":s.category,"character":s.character,"rows":s.rows,
                    "prices":s.rows.iter().map(|r|s.unit_price(r.id,party)).collect::<Vec<_>>(),
                    "total":s.total(party),"fade":s.fade,"scroll":s.scroll,
                    "description_previous":s.description_previous,"description_opacity":s.description_opacity,
                    "statistics":s.statistics,"items":party.items,"gald":party.gald,
                    "spent_gald":party.spent_gald,"visited":party.travel.visited_shops
                })
            });
            captures.push(
                serde_json::json!({"name":name, "update":update, "audio_frame":frames,
                "scene":scene,
                "battle_capture":battle_capture,
                "audio_settings":app.world().get_resource::<crate::field_audio::Control>().map(|c|c.settings()),
                "presentation_counter":app.world().resource::<crate::Clock>().0.tick(),
                "battle":app.world().get_resource::<crate::battle::Owner>().and_then(|battle|battle.diagnostic()),
                "effect_counter":field.effect_clock.tick(),
                "flutters":flutters,
                "background_waits":field.events.background_waits(),
                "random_state":field.events.world.random_state,
                "gameplay_random_index":field.events.world.gameplay_random.index(),
                "paralysis":field.events.world.paralysis,
                "main_menu_fade":field.menu.as_ref().map(|m|m.main_fade),
                // Normalize drawn poses to the source's post-draw memory observation.
                // The final visible pose expires during that source update.
                "poison_puffs":field.events.world.billboards.values()
                    .filter(|p|p.recipe == 10 && field.events.world.tick - p.born < 20)
                    .map(|p|PoisonOrigin {age:field.events.world.tick - p.born + 1,
                        position:std::array::from_fn(|i|p.position[i] + p.velocity[i]),
                        size:p.size[0] as u8,speed_sixteenths:(p.velocity[2] * 16.) as u8}).collect::<Vec<_>>(),
                "eyes":field.events.world.actors.iter().filter_map(|(id,actor)|
                    actor.appearance.eyes.map(|eyes|(id.to_string(),eyes))).collect::<BTreeMap<_,_>>(),
                "actors":field.events.world.actors.iter().filter_map(|(id,actor)| {
                    actor.autonomy.map(|autonomy| (id.to_string(), serde_json::json!({
                        "autonomy":autonomy,"position":actor.position,"heading":actor.heading,
                        "target_heading":actor.target_heading,
                        "animation":actor.animation.as_ref().map(|a|serde_json::json!({
                            "slot":a.slot,"sample":a.sample(field.events.tick(),0,a.duration_ticks as f32),
                            "rate":a.rate,"blend":a.blend_weight(field.events.tick())}))
                    })))
                }).collect::<BTreeMap<_,_>>(),
                "map_id":field.map_id, "story":diagnostics.attempt("checkpoint story", field.story_progress())?, "tick":field.events.tick(),
                "position":actor.map(|actor| actor.position), "heading":actor.map(|actor| actor.heading),
                "action_prompt":field.action_prompt().map(|p| serde_json::json!({
                    "id":p.action as u8,"opacity":p.opacity,"text_opacity":p.text_opacity})),
                "save_prompt":field.action_prompt().filter(|p|p.action == resonance_game::field::FieldAction::Save).map(|p| serde_json::json!({
                    "opacity":p.opacity, "text_opacity":p.text_opacity})),
                "skit_prompt":field.skit_prompt(),
                "skit":field.active_skit.as_ref().map(|p| serde_json::json!({
                    "id":p.id,"tick":p.events.tick(),"title":p.title,
                    "subtitle":p.events.world.skit.as_ref().map(|s|&s.subtitle),
                    "portraits":p.events.world.skit.as_ref().map(|s|s.portraits.values().map(|p|p.id).collect::<Vec<_>>())
                })),
                "ambient_animations":spec.ambient_origin.iter().flat_map(|o|o.samples.keys())
                    .filter_map(|id|field.events.world.actors.get(id)?.animation.as_ref().map(|a|
                        (id.to_string(),serde_json::json!({"sample":a.sample(field.events.tick(),0,a.duration_ticks as f32),"rate":a.rate}))))
                    .collect::<BTreeMap<_,_>>(),
                "save_points":field.events.world.save_points.iter().map(|p|serde_json::json!({"active":p.active,"glow_scale":p.glow_scale})).collect::<Vec<_>>(),
                "played_ticks":field.play_time.total(), "session_ticks":field.play_time.session(),
                "controlled_actor":field.events.world.controlled_actor,
                "party":field.events.world.party.as_ref().map(|p| serde_json::json!({
                    "formation":p.formation,"field_leader":p.field_leader,"leader_locked":p.leader_locked})),
                "persistent_party":field.events.world.party,
                "menu":menu,
                "shop":shop,
                "checkpoint":field.checkpoint().ok()}),
            );
        }
        completed_updates = update;
    }
    diagnostics.attempt(
        "checkpoint battle captures",
        battle_captures.finish(&spec.battle_captures),
    )?;
    // An observer reporting a write failure must still fail the command. A
    // missing requested moment is a diagnostic, not an output I/O failure.
    ensure!(
        !failure.load(Ordering::Acquire),
        "checkpoint capture write failed"
    );
    diagnostics.attempt(
        "checkpoint captures",
        (|| {
            ensure!(
                written.load(Ordering::Acquire) as usize
                    == spec.captures.len() + spec.battle_captures.len(),
                "checkpoint replay did not capture every requested frame"
            );
            Ok(())
        })(),
    )?;
    if !resource_waits_finished {
        diagnostics.attempt(
            "checkpoint resource waits",
            (|| {
                app.world()
                    .get_resource::<new_game::Session>()
                    .context("field retired before its resource wait observations were completed")?
                    .field
                    .events
                    .finish_resource_wait_observations()
            })(),
        )?;
    }
    if let Some(seeds) = app.world().get_resource::<BattleSeeds>() {
        diagnostics.attempt("checkpoint battle seeds", seeds.finish())?;
    }
    let late = app
        .world()
        .resource::<loading::Resident>()
        .late_reads
        .load(Ordering::Relaxed);
    let resolution = app.world().resource::<crate::display::Display>().0;
    let final_scene =
        diagnostics.attempt("checkpoint final scene", recording_scene(app.world()))?;
    finish_recording(
        output,
        &diagnostics,
        wave,
        late,
        serde_json::json!({
            "complete":completed_updates == spec.updates,"audio_device":false,"keyboard_input":true,"width":resolution.width,"height":resolution.height,
            "completed_updates":completed_updates,
            "output_stage":app.world().resource::<crate::display::OutputStage>(),
            "resource_waits":spec.resource_waits,"resource_wait_origin_tick":resource_wait_origin_tick,
            "battle_seeds":spec.battle_seeds,
            "updates":spec.updates,"audio_frames":frames,"late_reads":late,"initial":initial,"captures":captures,
            "identity":identity,
            "final_scene":final_scene,
            "current_identity":app.world().get_resource::<new_game::Session>().map(|session|&session.identity),
            "audio_commands":app.world().resource::<crate::field_audio::Trace>().0,
        }),
    )?;
    app.world_mut().remove_resource::<BattleSeeds>();
    Ok(())
}
fn finish_recording<W: std::io::Write + std::io::Seek>(
    output: &Path,
    diagnostics: &resonance_content::diagnostics::Diagnostics,
    wave: hound::WavWriter<W>,
    late_reads: u64,
    mut metadata: serde_json::Value,
) -> Result<()> {
    wave.finalize()?;
    fs::rename(output.join("audio.partial.wav"), output.join("audio.wav"))?;
    if late_reads != 0 {
        diagnostics.report(
            "checkpoint late reads",
            anyhow::anyhow!("checkpoint replay read an unprepared asset"),
        )?;
    }
    metadata["mode"] = serde_json::json!(if diagnostics.paranoid() {
        "paranoid"
    } else {
        "tolerant"
    });
    metadata["paranoid"] = serde_json::json!(diagnostics.paranoid());
    metadata["valid"] = serde_json::json!(!diagnostics.has_errors());
    metadata["diagnostics"] = serde_json::to_value(diagnostics.entries())?;
    fs::write(
        output.join("recording.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    Ok(())
}

fn attach(app: &mut App, mixer: &resonance_playback::Control) -> Result<()> {
    crate::playthrough::attach::<crate::field_audio::FieldSource>(app.world_mut(), mixer)?;
    crate::playthrough::attach::<crate::GameAudio>(app.world_mut(), mixer)
}
fn wait_ready(app: &mut App, initial: bool, timed_out: &mut bool) -> Result<()> {
    let diagnostics = app
        .world()
        .resource::<crate::diagnostics::Diagnostics>()
        .0
        .clone();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    let began = Instant::now();
    let mut settled = 0;
    while settled < 2 {
        let battle_clock = app
            .world()
            .get_resource::<crate::battle::Owner>()
            .and_then(|battle| battle.capture_clock());
        let entry_clock = app
            .world()
            .get_resource::<crate::battle::Owner>()
            .and_then(|battle| battle.entry_clock());
        let presentation_tick = app.world().resource::<Clock>().0.tick();
        let timeout = if app.world().contains_resource::<crate::battle::Owner>() {
            120
        } else {
            60
        };
        if began.elapsed().as_secs() >= timeout {
            diagnostics.report(
                "checkpoint preparation",
                anyhow::anyhow!("checkpoint scene preparation timed out"),
            )?;
            *timed_out = true;
            return Ok(());
        }
        app.update();
        crate::playthrough::check_exit(app)?;
        let world = app.world_mut();
        if world.resource::<Clock>().0.tick() != presentation_tick {
            diagnostics.report(
                "checkpoint preparation",
                anyhow::anyhow!("presentation advanced during checkpoint preparation"),
            )?;
        }
        if let Some(clock) = battle_clock
            && world
                .get_resource::<crate::battle::Owner>()
                .and_then(|battle| battle.capture_clock())
                != Some(clock)
        {
            diagnostics.report(
                "checkpoint preparation",
                anyhow::anyhow!("battle advanced during checkpoint preparation"),
            )?;
        }
        if let Some(clock) = entry_clock
            && world
                .get_resource::<crate::battle::Owner>()
                .and_then(|battle| battle.entry_clock())
                != Some(clock)
        {
            diagnostics.report(
                "checkpoint preparation",
                anyhow::anyhow!("entry transition advanced during checkpoint preparation"),
            )?;
        }
        if world
            .get_resource::<crate::battle::Owner>()
            .is_some_and(|battle| battle.failed())
        {
            diagnostics.report(
                "checkpoint preparation",
                anyhow::anyhow!("battle preparation failed during checkpoint replay"),
            )?;
            return Ok(());
        }
        let Some(phase) = diagnostics.attempt("checkpoint scene owner", scene::phase(world))?
        else {
            return Ok(());
        };
        let ready = scene::ready(world, phase)
            && !world.resource::<Persistence>().is_writing()
            && (!initial || checkpoint(world).is_ok());
        // After one bounded timeout, keep submitting updates and collecting
        // later diagnostics instead of repeating that wait at every tick.
        if !ready && *timed_out {
            return Ok(());
        }
        if ready {
            *timed_out = false;
        }
        settled = if ready { settled + 1 } else { 0 };
        if !ready {
            thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_game::clock::{PlayTime, PresentationClock};

    struct Output(PathBuf);
    impl Output {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "resonance-diagnostic-recording-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn wave(&self) -> hound::WavWriter<std::io::BufWriter<fs::File>> {
            let mut wave = hound::WavWriter::create(
                self.0.join("audio.partial.wav"),
                hound::WavSpec {
                    channels: 2,
                    sample_rate: 32028,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            wave.write_sample(123i16).unwrap();
            wave.write_sample(-123i16).unwrap();
            wave
        }
    }
    impl Drop for Output {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn tolerant_recording_finalizes_audio_and_reports_all_errors_as_invalid() {
        let output = Output::new();
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(false);
        let captures = BattleCaptures::default();
        let requested = [BattleCapture {
            index: 0,
            encounter: 1,
            combat_tick: 5,
            name: "missing".into(),
        }];
        diagnostics
            .attempt("checkpoint battle captures", captures.finish(&requested))
            .unwrap();
        diagnostics
            .report("battle effect", anyhow::anyhow!("missing texture"))
            .unwrap();
        finish_recording(
            &output.0,
            &diagnostics,
            output.wave(),
            2,
            serde_json::json!({"complete":true}),
        )
        .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(output.0.join("recording.json")).unwrap()).unwrap();
        assert_eq!(metadata["complete"], true);
        assert_eq!(metadata["mode"], "tolerant");
        assert_eq!(metadata["valid"], false);
        assert_eq!(metadata["diagnostics"].as_array().unwrap().len(), 3);
        assert!(!output.0.join("audio.partial.wav").exists());
        let samples = hound::WavReader::open(output.0.join("audio.wav"))
            .unwrap()
            .into_samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples, [123, -123]);
    }

    #[test]
    fn paranoid_recording_preserves_late_read_failure_and_no_success_manifest() {
        let output = Output::new();
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        let error = finish_recording(
            &output.0,
            &diagnostics,
            output.wave(),
            1,
            serde_json::json!({"complete":true}),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "checkpoint replay read an unprepared asset"
        );
        assert!(!output.0.join("recording.json").exists());
        assert!(output.0.join("audio.wav").is_file());
    }

    #[test]
    fn tolerant_recording_keeps_output_write_failures_actionable() {
        let output = Output::new();
        fs::create_dir(output.0.join("recording.json")).unwrap();
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(false);
        assert!(
            finish_recording(
                &output.0,
                &diagnostics,
                output.wave(),
                0,
                serde_json::json!({"complete":true})
            )
            .is_err()
        );
        assert!(
            !diagnostics.has_errors(),
            "output errors must not be converted to recoverable diagnostics"
        );
    }

    #[test]
    fn battle_capture_uses_actual_clock_once_and_distinguishes_repeated_encounters() {
        let entry = |index, tick, name: &str| BattleCapture {
            index,
            encounter: 1,
            combat_tick: tick,
            name: name.into(),
        };
        let entries = [
            entry(0, 5, "first-5"),
            entry(0, 10, "first-10"),
            entry(1, 5, "second-5"),
        ];
        let mut selector = BattleCaptures::default();
        assert!(selector.select(&entries, None).is_empty());
        assert!(selector.select(&entries, Some((31, 1, 0))).is_empty());
        assert_eq!(selector.select(&entries, Some((31, 1, 5))), vec![0]);
        // Selector/menu holds and extra zero-duration renders cannot recapture.
        assert!(selector.select(&entries, Some((31, 1, 5))).is_empty());
        assert!(selector.select(&entries, None).is_empty());
        assert_eq!(selector.select(&entries, Some((31, 1, 10))), vec![1]);
        assert!(selector.finish(&entries).is_err());
        assert_eq!(selector.select(&entries, Some((45, 1, 5))), vec![2]);
        selector.finish(&entries).unwrap();

        let mut missed = BattleCaptures::default();
        assert!(missed.select(&entries, Some((80, 2, 5))).is_empty());
        // A later matching encounter cannot replace a missed first request.
        assert_eq!(missed.select(&entries, Some((81, 1, 5))), vec![2]);
        assert!(
            missed
                .finish(&entries)
                .unwrap_err()
                .to_string()
                .contains("first-5")
        );
        let mut skipped = BattleCaptures::default();
        skipped.select(&entries, Some((90, 1, 4)));
        assert!(skipped.select(&entries, Some((90, 1, 6))).is_empty());
        assert!(skipped.finish(&entries).is_err());
    }

    #[test]
    fn battle_capture_validation_shares_names_and_limits_with_absolute_captures() {
        let mut replay: CheckpointReplay = serde_json::from_value(serde_json::json!({
            "version":1,"updates":300,"inputs":[],"captures":{"0":"initial"},
            "battle_captures":[{"index":0,"encounter":1,"combat_tick":5,"name":"combat-005"}]
        }))
        .unwrap();
        replay.validate().unwrap();
        replay.battle_captures[0].name = "initial".into();
        assert!(replay.validate().is_err());
        replay.battle_captures[0].name = "combat-005".into();
        for tick in [0, 301] {
            replay.battle_captures[0].combat_tick = tick;
            assert!(replay.validate().is_err());
        }
        replay.battle_captures[0].combat_tick = 5;
        replay.battle_captures.push(BattleCapture {
            name: "duplicate-tick".into(),
            ..replay.battle_captures[0].clone()
        });
        assert!(replay.validate().is_err());
        replay.battle_captures.pop();
        replay.captures = (0..256)
            .map(|tick| (tick, format!("absolute-{tick}")))
            .collect();
        assert!(replay.validate().is_err());
        replay.captures.clear();
        replay.validate().unwrap(); // A battle-only recording is valid.
        replay.battle_captures.clear();
        assert!(replay.validate().is_err());
    }

    #[test]
    fn observed_battle_seed_is_bound_once_to_its_request_generation() {
        let mut seeds = BattleSeeds {
            entries: vec![
                BattleSeed {
                    encounter: 10,
                    seed: 123,
                },
                BattleSeed {
                    encounter: 10,
                    seed: 456,
                },
                BattleSeed {
                    encounter: 11,
                    seed: 789,
                },
            ],
            assigned: BTreeMap::new(),
        };
        assert_eq!(seeds.for_request(17, 10).unwrap(), 123);
        // A retried preparation keeps the seed; a later identical encounter
        // gets the next observation because its requesting operation is new.
        assert_eq!(seeds.for_request(17, 10).unwrap(), 123);
        assert_eq!(seeds.for_request(24, 10).unwrap(), 456);
        assert!(seeds.for_request(17, 11).is_err());
        assert!(seeds.for_request(25, 12).is_err());
        assert!(seeds.finish().is_err());
        assert_eq!(seeds.for_request(25, 11).unwrap(), 789);
        seeds.finish().unwrap();
        assert!(seeds.for_request(30, 11).is_err());
    }

    #[test]
    fn replay_battle_seeds_are_explicit_source_width_values() {
        let mut json: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tools/oracle/cases/classroom-return-keyboard.json"
        ))
        .unwrap();
        let ordinary: CheckpointReplay = serde_json::from_value(json.clone()).unwrap();
        assert!(ordinary.battle_seeds.is_empty());
        json["battle_seeds"] = serde_json::json!([
            {"encounter": 10, "seed": 65535},
            {"encounter": 11, "seed": 0}
        ]);
        let replay: CheckpointReplay = serde_json::from_value(json.clone()).unwrap();
        replay.validate().unwrap();
        assert_eq!(replay.battle_seeds[0].seed, 65535);
        json["battle_seeds"][0]["seed"] = serde_json::json!(65536);
        assert!(serde_json::from_value::<CheckpointReplay>(json).is_err());
    }

    #[test]
    fn classroom_loading_registers_source_effect_ticks_without_skipping_updates() {
        let mut spec: CheckpointReplay = serde_json::from_str(include_str!(
            "../../../../tools/oracle/cases/classroom-return-keyboard.json"
        ))
        .unwrap();
        spec.validate().unwrap();
        let origin = spec.ambient_origin.as_ref().unwrap();
        let mut clock = PresentationClock::new(origin.effect_tick.unwrap());
        let mut play_time = PlayTime::default();
        // Consecutive source observations bracket both loading stalls.
        let observations = [
            (2411, 34186),
            (2412, 34186),
            (2427, 34186),
            (2428, 34187),
            (3091, 34850),
            (3092, 34850),
            (3121, 34850),
            (3122, 34851),
            (3337, 35066),
        ];
        for update in origin.update + 1..=spec.updates {
            spec.register_effect_clock(update, &mut clock);
            clock.advance();
            play_time.advance();
            if let Some((_, expected)) = observations.iter().find(|(tick, _)| *tick == update) {
                assert_eq!(clock.tick(), *expected, "update {update}");
            }
        }
        assert_eq!(play_time.total(), 1100);
        for (start, end) in [(0, 1), (4, 3), (3337, 3338)] {
            spec.effect_pauses = vec![PresentationStall { start, end }];
            assert!(spec.validate().is_err());
        }
        spec.effect_pauses = vec![
            PresentationStall { start: 1, end: 2 },
            PresentationStall { start: 2, end: 3 },
        ];
        assert!(spec.validate().is_err());
    }
}
