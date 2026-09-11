//! Deterministic keyboard replay from an ordinary field save, with file-only audio.
use super::*;
use crate::Clock;
use bevy::{app::PluginsState, time::TimeUpdateStrategy};
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
    /// Controlled menu fixtures change progress once the main menu owns input.
    /// Field initialization and free-control services run at the saved story.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    story_origin: Option<StoryOrigin>,
    pub inputs: Vec<KeyboardInput>,
    /// Update zero is the initialized field, before the first input/update.
    pub captures: BTreeMap<u32, String>,
    /// Updates during which the source produced no presentation (loading).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presentation_pauses: Vec<PresentationStall>,
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
    /// Running effect phase, independent of paused field animation and UI time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effect_tick: Option<u32>,
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
            self.inputs.len() <= self.updates as usize
                && self.inputs.windows(2).all(|w| w[0].update < w[1].update)
                && self
                    .inputs
                    .iter()
                    .all(|i| (1..=self.updates).contains(&i.update) && i.keys.len() <= 10),
            "invalid checkpoint replay inputs"
        );
        ensure!(
            !self.captures.is_empty()
                && self.captures.len() <= 256
                && self
                    .captures
                    .iter()
                    .all(|(&tick, name)| tick <= self.updates
                        && !name.is_empty()
                        && name.len() <= 64
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')),
            "invalid checkpoint replay captures"
        );
        let names: std::collections::BTreeSet<_> = self.captures.values().collect();
        ensure!(
            names.len() == self.captures.len(),
            "duplicate checkpoint capture name"
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
            self.presentation_pauses
                .iter()
                .all(|pause| { pause.start <= pause.end && pause.end <= self.updates })
                && self
                    .presentation_pauses
                    .windows(2)
                    .all(|w| w[0].end < w[1].start),
            "invalid presentation pause ranges"
        );
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
                origin.update == *self.captures.first_key_value().unwrap().0
                    && origin.samples.len() <= 32
                    && origin.eyes.len() <= 8
                    && origin.actors.len() <= 8
                    && origin.background_waits.len() <= 32
                    && origin
                        .flutters
                        .as_ref()
                        .is_none_or(|leaves| leaves.len() <= 32)
                    && (origin.random_state.is_none() || origin.save_sparks.is_some())
                    && (!origin.samples.is_empty()
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
    spec.validate()?;
    let mut app = probe::app(root, save, output)?;
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
    wait_ready(app, true)?;
    if let Some(tick) = spec.presentation_origin {
        *app.world_mut().resource_mut::<Clock>() =
            Clock(resonance_game::clock::PresentationClock::new(tick));
    }
    if let Some(session) = spec.session_origin {
        let field = &mut app.world_mut().resource_mut::<new_game::Session>().field;
        field.play_time =
            resonance_game::clock::PlayTime::with_session(field.play_time.total(), session)
                .context("observed session time exceeds total play time")?;
    }
    let mut initial = serde_json::to_value(checkpoint(app.world_mut())?)?;
    initial["presentation_counter"] =
        serde_json::json!(app.world().resource::<crate::Clock>().0.tick());
    attach(app, mixer)?;
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
    let mut frames = 0;
    let mut held = Vec::<Key>::new();
    let mut inputs = spec.inputs.iter().peekable();
    let mut preview_instance = None;
    let mut preview_registered = false;
    let began = Instant::now();
    for update in 0..=spec.updates {
        ensure!(
            began.elapsed().as_secs() < 600,
            "checkpoint replay timed out at update {update}"
        );
        if update > 0 {
            if let Some(&ticks) = spec.presentation_advances.get(&update) {
                let mut clock = app.world_mut().resource_mut::<Clock>();
                clock.0 = resonance_game::clock::PresentationClock::new(
                    clock.0.tick().wrapping_add(ticks),
                );
            }
            if let Some(&ticks) = spec.effect_advances.get(&update) {
                let field = &mut app.world_mut().resource_mut::<new_game::Session>().field;
                field.effect_clock = resonance_game::clock::PresentationClock::new(
                    field.effect_clock.tick().wrapping_add(ticks),
                );
            }
            app.world_mut().resource_mut::<crate::PresentationPause>().0 = spec
                .presentation_pauses
                .iter()
                .any(|pause| (pause.start..=pause.end).contains(&update));
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
            wait_ready(app, false)?;
            attach(app, mixer)?;
            let end = u64::from(update) * 32028 * resonance_game::clock::UPDATE_RATE_DENOMINATOR
                / resonance_game::clock::UPDATE_RATE_NUMERATOR;
            for _ in frames..end {
                for _ in 0..2 {
                    let sample = audio.next().context("checkpoint audio stopped")?;
                    ensure!(sample.is_finite(), "nonfinite checkpoint audio");
                    wave.write_sample((sample * 32768.).round().clamp(-32768., 32767.) as i16)?;
                }
            }
            frames = end;
            if let Some(control) = app.world().get_resource::<crate::field_audio::Control>() {
                control.check()?;
            }
        }
        if let Some(origin) = &spec.story_origin
            && origin.update == update
        {
            let mut session = app.world_mut().resource_mut::<new_game::Session>();
            let menu = session
                .field
                .menu
                .as_mut()
                .context("story fixture requires an open menu")?;
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
            session.field.events.set_global(16, origin.to)?;
        }
        if let Some(origin) = &spec.ambient_origin
            && origin.update == update
        {
            origin.apply(&mut app.world_mut().resource_mut::<new_game::Session>().field)?;
        }
        let preview = app
            .world()
            .resource::<new_game::Session>()
            .field
            .menu
            .as_ref()
            .and_then(|m| m.preview().map(|p| p.id));
        if preview != preview_instance {
            preview_instance = preview;
            preview_registered = false;
        }
        if let Some(origin) = spec.preview_origins.get(&update) {
            ensure!(
                !preview_registered,
                "preview animation can only be registered once per selection"
            );
            let mut session = app.world_mut().resource_mut::<new_game::Session>();
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
        }
        if let Some(name) = spec.captures.get(&update) {
            crate::new_game_capture::screenshot(
                app,
                output.join(format!("{name}.png")),
                failure.clone(),
                written.clone(),
            )?;
            let session = app.world().resource::<new_game::Session>();
            let field = &session.field;
            if let Some(expected) = spec.expected.get(&update) {
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
            }
            let actor = &field.events.world.actors[&field.events.world.controlled_actor];
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
            captures.push(
                serde_json::json!({"name":name, "update":update, "audio_frame":frames,
                "audio_settings":app.world().get_resource::<crate::field_audio::Control>().map(|c|c.settings()),
                "presentation_counter":app.world().resource::<crate::Clock>().0.tick(),
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
                "map_id":field.map_id, "story":field.story_progress()?, "tick":field.events.tick(),
                "position":actor.position, "heading":actor.heading,
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
                "menu":menu,
                "checkpoint":field.checkpoint().ok()}),
            );
        }
    }
    ensure!(
        !failure.load(Ordering::Acquire)
            && written.load(Ordering::Acquire) as usize == spec.captures.len(),
        "checkpoint replay did not capture every requested frame"
    );
    wave.finalize()?;
    fs::rename(output.join("audio.partial.wav"), output.join("audio.wav"))?;
    let late = app
        .world()
        .resource::<loading::Resident>()
        .late_reads
        .load(Ordering::Relaxed);
    ensure!(late == 0, "checkpoint replay read an unprepared asset");
    fs::write(
        output.join("recording.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "complete":true,"audio_device":false,"keyboard_input":true,"width":640,"height":480,
            "updates":spec.updates,"audio_frames":frames,"late_reads":late,"initial":initial,"captures":captures,
            "identity":app.world().resource::<new_game::Session>().identity,
            "audio_commands":app.world().resource::<crate::field_audio::Trace>().0,
        }))?,
    )?;
    Ok(())
}
fn attach(app: &mut App, mixer: &resonance_playback::Control) -> Result<()> {
    crate::playthrough::attach::<crate::field_audio::FieldSource>(app.world_mut(), mixer)?;
    crate::playthrough::attach::<crate::GameAudio>(app.world_mut(), mixer)
}
fn wait_ready(app: &mut App, initial: bool) -> Result<()> {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    let began = Instant::now();
    let mut settled = 0;
    while settled < 2 {
        ensure!(
            began.elapsed().as_secs() < 60,
            "checkpoint field preparation timed out"
        );
        app.update();
        crate::playthrough::check_exit(app)?;
        let world = app.world_mut();
        let ready = field_view::ready(world)
            && world
                .resource::<loading::Resident>()
                .active
                .load(Ordering::Acquire)
            && world.get_resource::<new_game::Session>().is_some_and(|s| {
                s.ready_for_field
                    && s.audio.is_none()
                    && s.field.events.world.field_transition.is_none()
                    && s.field.menu.as_ref().is_none_or(|m| !m.busy)
            })
            && !world.resource::<Persistence>().is_writing()
            && (!initial || checkpoint(world).is_ok());
        settled = if ready { settled + 1 } else { 0 };
        if !ready {
            thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    Ok(())
}
