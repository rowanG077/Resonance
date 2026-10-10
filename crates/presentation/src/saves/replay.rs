//! Deterministic keyboard replay from an ordinary field save, with file-only audio.
use super::*;
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
    /// Controlled fixtures change live progress at free field control or Main.
    /// Initialization runs at the saved story, matching a live source-state edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    story_origin: Option<StoryOrigin>,
    pub inputs: Vec<KeyboardInput>,
    /// Update zero is the initialized field, before the first input/update.
    pub captures: BTreeMap<u32, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    expected: BTreeMap<u32, ExpectedField>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoryOrigin {
    update: u32,
    from: i32,
    to: i32,
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
    TestSpeed,
    Pause,
    SkipEvent,
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
            Self::TestSpeed => (KeyCode::F6, Logical::F6),
            Self::Pause => (KeyCode::F7, Logical::F7),
            Self::SkipEvent => (KeyCode::F8, Logical::F8),
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
        for expected in self.expected.values() {
            if let Some(slot) = &expected.saved_slot {
                SlotId::new(slot)?;
            }
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
    spec.validate()?;
    let mut app = probe::app(root, save, output, resolution)?;
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
    let began = Instant::now();
    for update in 0..=spec.updates {
        ensure!(
            began.elapsed().as_secs() < 600,
            "checkpoint replay timed out at update {update}"
        );
        if update > 0 {
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
                "audio_settings":app.world().get_resource::<crate::field_audio::Control>().map(|c|c.settings()),
                "presentation_counter":app.world().resource::<crate::Clock>().0.tick(),
                "effect_counter":field.effect_clock.tick(),
                "random_state":field.events.world.random_state,
                "gameplay_random_index":field.events.world.gameplay_random.index(),
                "paralysis":field.events.world.paralysis,
                "main_menu_fade":field.menu.as_ref().map(|m|m.main_fade),
                "eyes":field.events.world.actors.iter().filter_map(|(id,actor)|
                    actor.appearance.eyes.map(|eyes|(id.to_string(),eyes))).collect::<BTreeMap<_,_>>(),
                "actors":field.events.world.actors.iter().filter_map(|(id,actor)| {
                    actor.autonomy.map(|autonomy| (id.to_string(), serde_json::json!({
                        "autonomy":autonomy,"position":actor.position,"heading":actor.heading,
                        "target_heading":actor.target_heading,
                        "model_scale":actor.model_scale(),
                        "model_alpha":actor.opacity,
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
    let resolution = app.world().resource::<crate::display::Display>().0;
    fs::write(
        output.join("recording.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "complete":true,"audio_device":false,"keyboard_input":true,"width":resolution.width,"height":resolution.height,
            "output_stage":app.world().resource::<crate::display::OutputStage>(),
            "updates":spec.updates,"audio_frames":frames,"late_reads":late,"initial":initial,"captures":captures,
            "identity":app.world().resource::<new_game::Session>().identity,
            "audio_commands":app.world().resource::<crate::field_audio::Trace>().0,
        }))?,
    )?;
    Ok(())
}
fn attach(app: &mut App, mixer: &resonance_playback::Control) -> Result<()> {
    crate::playthrough::attach::<crate::field_audio::FieldSource>(app.world_mut(), mixer)?;
    crate::playthrough::attach::<crate::GameAudio>(app.world_mut(), mixer)?;
    crate::playthrough::attach::<crate::credits::Audio>(app.world_mut(), mixer)?;
    crate::testing::audio(app.world_mut());
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintained_checkpoint_replays_use_the_current_contract() -> Result<()> {
        let cases = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/oracle/cases");
        for entry in fs::read_dir(cases)? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
            if value.get("inputs").is_some() && value.get("captures").is_some() {
                let replay: CheckpointReplay =
                    serde_json::from_value(value).with_context(|| path.display().to_string())?;
                replay
                    .validate()
                    .with_context(|| path.display().to_string())?;
            }
        }
        Ok(())
    }
}
