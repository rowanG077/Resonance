//! Native scenarios: wait for a game event, hold input, and capture.
mod scene;
use super::*;
use crate::Clock;
use bevy::{app::PluginsState, time::TimeUpdateStrategy};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    thread,
};

pub(crate) fn recording_scene(world: &World) -> Result<serde_json::Value> {
    scene::diagnostic(world)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointReplay {
    pub version: u32,
    steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Step {
    Hold {
        keys: Vec<Key>,
        updates: u32,
    },
    Wait {
        until: Event,
        #[serde(default)]
        keys: Vec<Key>,
        /// Send a fresh one-update key press at this interval while waiting.
        tap_every: Option<u32>,
        max_updates: u32,
    },
    Capture {
        name: String,
    },
}

impl Step {
    /// Shared input semantics for offline recording and the live window probe.
    fn input<'a>(&'a self, world: &World, elapsed: u32) -> Result<Option<&'a [Key]>> {
        match self {
            Self::Hold { keys, updates } => Ok((elapsed < *updates).then_some(keys.as_slice())),
            Self::Wait {
                until,
                keys,
                tap_every,
                max_updates,
            } => {
                if until.matches(world)? {
                    return Ok(None);
                }
                ensure!(
                    elapsed < *max_updates,
                    "did not reach {until:?} within {max_updates} updates; scene: {:?}",
                    recording_scene(world)
                );
                Ok(Some(
                    if tap_every.is_some_and(|period| !elapsed.is_multiple_of(period)) {
                        &[]
                    } else {
                        keys
                    },
                ))
            }
            Self::Capture { .. } => Ok(None),
        }
    }

    pub(crate) fn wait(until: Event, max_updates: u32) -> Self {
        Self::Wait {
            until,
            keys: Vec::new(),
            tap_every: None,
            max_updates,
        }
    }
    pub(crate) fn select(until: Event, key: Key) -> Self {
        Self::Wait {
            until,
            keys: vec![key],
            tap_every: Some(2),
            max_updates: 120,
        }
    }
    pub(crate) fn capture(name: impl Into<String>) -> Self {
        Self::Capture { name: name.into() }
    }
    pub(crate) fn tap(key: Key) -> [Self; 2] {
        [
            Self::Hold {
                keys: vec![key],
                updates: 1,
            },
            Self::Hold {
                keys: Vec::new(),
                updates: 1,
            },
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Event {
    FieldReady,
    FieldEmote {
        map_id: u32,
        actor: i32,
        emote_kind: u16,
    },
    FieldBillboard {
        map_id: u32,
        recipe: u16,
    },
    Field {
        map_id: u32,
        #[serde(default)]
        free_control: bool,
        story: Option<i32>,
        max_x: Option<f32>,
    },
    Battle {
        encounter: u16,
        phase: BattleStage,
    },
    Menu {
        page: MenuPage,
    },
    /// Observe completed menu transitions without restricting ordinary input.
    MenuSettled {
        page: MenuPage,
    },
    ItemsFocus {
        focus: ItemsFocus,
    },
    MenuSelection {
        page: MenuPage,
        entry: MenuPage,
    },
    TitleLoad,
    SavePoint,
    Dialogue {
        contains: String,
    },
    Choice {
        slot: u8,
    },
    Movie {
        resource: u32,
        #[serde(default)]
        frame: u32,
    },
    Boot {
        tick: u32,
    },
    TitleTick {
        tick: u32,
    },
    Slots {
        mode: SlotMode,
        state: SlotState,
    },
    Title,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SlotMode {
    Save,
    Load,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SlotState {
    Bank,
    List,
    Confirm,
    Notice,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemsFocus {
    List,
    Target,
}

fn current_menu(world: &World) -> Option<&resonance_game::menu::Menu> {
    world
        .get_resource::<title::LoadMenu>()
        .map(|menu| &menu.0)
        .or_else(|| {
            world
                .get_resource::<new_game::Session>()?
                .field
                .menu
                .as_ref()
        })
}

fn menu_settled(world: &World, menu: &resonance_game::menu::Menu) -> bool {
    let tick = world.resource::<Clock>().0.tick();
    if world.contains_resource::<title::LoadMenu>() {
        world
            .get_resource::<crate::field_ui::MenuOverlay>()
            .is_some_and(|art| art.menu_settled(menu, tick))
    } else {
        world
            .get_resource::<crate::field_ui::Artwork>()
            .is_some_and(|art| art.menu_settled(menu, tick))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BattleStage {
    Entry,
    Combat,
    Ending,
    Results,
    Finished,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MenuPage {
    Main,
    Items,
    Equip,
    Tech,
    Unison,
    Strategy,
    Status,
    Cooking,
    System,
    Save,
    Load,
    Customize,
}
impl MenuPage {
    fn page(self) -> resonance_game::menu::Page {
        use resonance_game::menu::Page;
        match self {
            Self::Main => Page::Main,
            Self::Items => Page::Items,
            Self::Equip => Page::Equip,
            Self::Tech => Page::Tech,
            Self::Unison => Page::Unison,
            Self::Strategy => Page::Strategy,
            Self::Status => Page::Status,
            Self::Cooking => Page::Cooking,
            Self::System => Page::System,
            Self::Save => Page::Slots(resonance_game::menu::Mode::Save),
            Self::Load => Page::Slots(resonance_game::menu::Mode::Load),
            Self::Customize => Page::Customize,
        }
    }
}
impl Event {
    fn matches(&self, world: &World) -> Result<bool> {
        let session = world.get_resource::<new_game::Session>();
        Ok(match self {
            Self::FieldReady => {
                scene::phase(world)? == scene::Phase::Field
                    && session.is_some_and(|session| {
                        session.ready_for_field && session.field.player_has_control()
                    })
            }
            Self::FieldEmote { map_id, .. } | Self::FieldBillboard { map_id, .. } => {
                scene::phase(world)? == scene::Phase::Field
                    && session.is_some_and(|session| {
                        session.ready_for_field
                            && session.field.map_id == *map_id
                            && world
                                .get_resource::<crate::field_effects::Artwork>()
                                .is_some_and(|art| {
                                    let field = &session.field.events.world;
                                    match self {
                                        Self::FieldEmote {
                                            actor, emote_kind, ..
                                        } => field.emotes.iter().any(|(&id, emote)| {
                                            emote.actor == *actor
                                                && Some(emote.kind)
                                                    == resonance_events::emote::Kind::try_from(
                                                        i32::from(*emote_kind),
                                                    )
                                                    .ok()
                                                && art.drawn(
                                                    &crate::field_audit::Request::Emote(id),
                                                    field.tick,
                                                )
                                        }),
                                        Self::FieldBillboard { recipe, .. } => {
                                            field.billboards.iter().any(|(&id, effect)| {
                                                effect.recipe == *recipe
                                                    && art.drawn(
                                                        &crate::field_audit::Request::Billboard(id),
                                                        field.tick,
                                                    )
                                            })
                                        }
                                        _ => unreachable!(),
                                    }
                                })
                    })
            }
            Self::Field {
                map_id,
                free_control,
                story,
                max_x,
            } => session.is_some_and(|session| {
                session.field.map_id == *map_id
                    && session.ready_for_field
                    && !world.resource::<crate::movie::Playback>().active
                    && !world.contains_resource::<crate::battle::Owner>()
                    && (!free_control || session.field.player_has_control())
                    && story.is_none_or(|story| session.field.story_progress().ok() == Some(story))
                    && max_x.is_none_or(|x| {
                        session
                            .field
                            .events
                            .world
                            .actors
                            .get(&session.field.events.world.controlled_actor)
                            .is_some_and(|actor| actor.position[0] <= x)
                    })
            }),
            Self::Battle { encounter, phase } => world
                .get_resource::<crate::battle::Owner>()
                .and_then(|owner| owner.replay_phase())
                .is_some_and(|(actual, stage)| {
                    actual == *encounter
                        && match phase {
                            BattleStage::Entry => stage == resonance_battle::BattlePhase::Entry,
                            BattleStage::Combat => stage == resonance_battle::BattlePhase::Combat,
                            BattleStage::Ending => stage == resonance_battle::BattlePhase::Ending,
                            BattleStage::Results => stage == resonance_battle::BattlePhase::Results,
                            BattleStage::Finished => {
                                stage == resonance_battle::BattlePhase::Finished
                            }
                        }
                }),
            Self::Menu { page } => session
                .and_then(|session| session.field.menu.as_ref())
                .is_some_and(|menu| {
                    menu.page == page.page() && !menu.busy && !menu.main_animating()
                }),
            Self::MenuSettled { page } => current_menu(world)
                .is_some_and(|menu| menu.page == page.page() && menu_settled(world, menu)),
            Self::ItemsFocus { focus } => current_menu(world).is_some_and(|menu| {
                use resonance_game::menu::{Page, items::Focus};
                menu.page == Page::Items
                    && menu.inventory.focus
                        == match focus {
                            ItemsFocus::List => Focus::List,
                            ItemsFocus::Target => Focus::Target,
                        }
            }),
            Self::MenuSelection { page, entry } => current_menu(world).is_some_and(|menu| {
                use resonance_game::menu::{MAIN_ENTRIES, Mode, Page};
                let selected = match menu.page {
                    Page::Main => MAIN_ENTRIES.get(menu.selected).map(|(page, _)| *page),
                    Page::System => [
                        Page::Slots(Mode::Save),
                        Page::Slots(Mode::Load),
                        Page::Customize,
                    ]
                    .get(menu.selected)
                    .copied(),
                    _ => None,
                };
                menu.page == page.page()
                    && !menu.busy
                    && !menu.main_animating()
                    && selected == Some(entry.page())
            }),
            Self::TitleLoad => {
                scene::phase(world)? == scene::Phase::Title
                    && world.resource::<crate::Menu>().0.accepts_input()
                    && world.resource::<crate::Menu>().0.selected == 1
            }
            Self::SavePoint => session.is_some_and(|session| {
                session.field.player_has_control()
                    && session.field.events.world.save_points.iter().any(|point| {
                        point.active && crate::field_view::save_point_drawn(world, point.actor)
                    })
            }),
            Self::Dialogue { contains } => session.is_some_and(|session| {
                session.field.dialogue.iter().any(|(slot, page)| {
                    crate::field_ui::displayed_dialogue(
                        page,
                        session.field.events.world.dialogue.get(slot),
                    )
                    .is_some()
                        && page.fully_revealed()
                        && page.current().text().contains(contains)
                })
            }),
            Self::Choice { slot } => {
                session.is_some_and(|session| {
                    session
                        .field
                        .events
                        .world
                        .choices
                        .get(slot)
                        .is_some_and(|choice| choice.operation.is_pending())
                        && session.field.dialogue.values().all(|page| {
                            page.closed || page.fully_revealed() && page.accepts_input()
                        })
                })
            }
            Self::Movie { resource, frame } => {
                let movie = world.resource::<crate::movie::Playback>();
                movie.active
                    && movie.resource == Some(*resource)
                    && movie
                        .presented_frame
                        .is_some_and(|presented| presented >= *frame)
            }
            Self::Boot { tick } => world
                .resource::<crate::boot::Playback>()
                .logos
                .as_ref()
                .is_some_and(|logos| logos.active() && logos.tick >= *tick),
            Self::TitleTick { tick } => {
                scene::phase(world)? == scene::Phase::Title
                    && world.resource::<crate::Menu>().0.tick >= *tick
            }
            Self::Slots { mode, state } => current_menu(world).is_some_and(|menu| {
                use resonance_game::menu::{Mode, Page, SlotFocus};
                menu.page
                    == Page::Slots(match mode {
                        SlotMode::Save => Mode::Save,
                        SlotMode::Load => Mode::Load,
                    })
                    && !menu.busy
                    && !menu.main_animating()
                    && match state {
                        SlotState::Bank => {
                            menu.focus == SlotFocus::Bank
                                && menu.confirmation.is_none()
                                && menu.notice.is_none()
                        }
                        SlotState::List => {
                            menu.focus == SlotFocus::List
                                && menu.confirmation.is_none()
                                && menu.notice.is_none()
                        }
                        SlotState::Confirm => menu.confirmation == Some(true),
                        SlotState::Notice => menu.notice.is_some(),
                    }
            }),
            Self::Title => {
                scene::phase(world)? == scene::Phase::Title
                    && world.resource::<crate::Menu>().0.accepts_input()
            }
        })
    }
}
impl CheckpointReplay {
    pub(crate) fn new(steps: Vec<Step>) -> Self {
        Self { version: 2, steps }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 2 && !self.steps.is_empty(),
            "invalid native replay version or empty scenario"
        );
        let mut budget = 0u32;
        let mut names = BTreeSet::new();
        for step in &self.steps {
            let (keys, updates) = match step {
                Step::Hold { keys, updates } => (keys, *updates),
                Step::Wait {
                    keys,
                    tap_every,
                    max_updates,
                    until,
                } => {
                    ensure!(
                        tap_every.is_none_or(|period| period >= 2 && period <= *max_updates),
                        "invalid replay key repeat interval"
                    );
                    if let Event::Dialogue { contains } = until {
                        ensure!(!contains.is_empty(), "dialogue condition needs text");
                    }
                    (keys, *max_updates)
                }
                Step::Capture { name } => {
                    ensure!(
                        !name.is_empty()
                            && name.len() <= 64
                            && name
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                            && names.insert(name),
                        "invalid or duplicate capture name"
                    );
                    continue;
                }
            };
            ensure!(
                (1..=36_000).contains(&updates)
                    && keys.len() <= 10
                    && keys.iter().enumerate().all(|(i, k)| !keys[..i].contains(k)),
                "invalid replay duration or duplicate keys"
            );
            budget = budget
                .checked_add(updates)
                .context("replay duration overflow")?;
        }
        ensure!(
            budget <= 36_000 && (1..=512).contains(&names.len()),
            "replay exceeds recording limits"
        );
        Ok(())
    }
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

impl Key {
    fn button(self) -> Option<GamepadButton> {
        use GamepadButton::*;
        Some(match self {
            Self::Left => DPadLeft,
            Self::Right => DPadRight,
            Self::Up => DPadUp,
            Self::Down => DPadDown,
            Self::Interact => South,
            Self::Cancel => East,
            Self::Alternate => West,
            Self::Menu => North,
            Self::PreviousPage => LeftTrigger,
            Self::NextPage => RightTrigger,
            Self::Skit => Z,
            Self::Start => Start,
            _ => return None,
        })
    }
}
fn send_input(world: &mut World, held: &[Key], keys: &[Key], pad: Option<Entity>) {
    use bevy::input::{
        ButtonState,
        gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent},
    };
    let releases = held
        .iter()
        .filter(|key| !keys.contains(key))
        .map(|key| (key, ButtonState::Released));
    let presses = keys
        .iter()
        .filter(|key| !held.contains(key))
        .map(|key| (key, ButtonState::Pressed));
    for (key, state) in releases.chain(presses) {
        if let Some(pad) = pad {
            world.write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                pad,
                key.button().expect("validated controller key"),
                f32::from(state == ButtonState::Pressed),
            )));
        } else {
            world.write_message(key.event(state));
        }
    }
}

/// One cursor feeds raw device events immediately before each native update.
/// Recorders service capture boundaries; live probes omit those observations.
#[derive(Resource)]
pub(crate) struct ScenarioInput {
    steps: Vec<Step>,
    index: usize,
    elapsed: u32,
    updates: u32,
    consumed: u64,
    before: Option<(u32, u64)>,
    held: Vec<Key>,
    pad: Option<Entity>,
    failed: bool,
}
impl ScenarioInput {
    /// A native consumer accepted this fixed update. Overlapping ownership
    /// during a transition still acknowledges the input only once.
    pub(crate) fn acknowledge_input(&mut self) {
        if self
            .before
            .is_some_and(|(_, serial)| serial == self.consumed)
        {
            self.consumed += 1;
        }
    }

    pub(crate) fn complete(&self) -> bool {
        self.index == self.steps.len() && !self.failed
    }

    fn settle(&mut self, world: &World) -> Result<Option<Vec<Key>>> {
        while let Some(step) = self.steps.get(self.index) {
            if matches!(step, Step::Capture { .. }) {
                return Ok(None);
            }
            if let Some(keys) = step.input(world, self.elapsed)? {
                return Ok(Some(keys.to_vec()));
            }
            self.index += 1;
            self.elapsed = 0;
        }
        Ok(None)
    }

    fn capture(&self) -> Option<&str> {
        match self.steps.get(self.index) {
            Some(Step::Capture { name }) => Some(name),
            _ => None,
        }
    }
}

pub(crate) fn install_scenario_input(app: &mut App, mut scenario: CheckpointReplay) -> Result<()> {
    scenario.validate()?;
    scenario
        .steps
        .retain(|step| !matches!(step, Step::Capture { .. }));
    install_cursor(app, scenario.steps, None);
    Ok(())
}

fn install_cursor(app: &mut App, steps: Vec<Step>, pad: Option<Entity>) {
    use bevy::input::{
        InputSystems,
        gamepad::{gamepad_connection_system, gamepad_event_processing_system},
        keyboard::keyboard_input_system,
    };
    app.insert_resource(ScenarioInput {
        steps,
        index: 0,
        elapsed: 0,
        updates: 0,
        consumed: 0,
        before: None,
        held: Vec::new(),
        pad,
        failed: false,
    })
    // The same raw messages must not be read again by the render-frame systems.
    .configure_sets(
        PreUpdate,
        InputSystems.run_if(not(resource_exists::<ScenarioInput>)),
    )
    .add_systems(FixedPreUpdate, scenario_input.before(InputSystems))
    .add_systems(
        FixedPreUpdate,
        (
            keyboard_input_system,
            gamepad_connection_system,
            gamepad_event_processing_system.after(gamepad_connection_system),
        )
            .in_set(InputSystems),
    )
    .add_systems(FixedPostUpdate, scenario_consumed);
}

fn settle_cursor(world: &mut World) -> Option<Vec<Key>> {
    world.resource_scope(|world, mut input: Mut<ScenarioInput>| {
        while !input.failed {
            match input.settle(world) {
                Ok(keys) => return keys,
                Err(error) => {
                    if world
                        .get_resource::<crate::diagnostics::Diagnostics>()
                        .is_some_and(|diagnostics| {
                            diagnostics.0.report("native scenario", error).is_ok()
                        })
                    {
                        input.index += 1;
                        input.elapsed = 0;
                    } else {
                        input.failed = true;
                        world.write_message(AppExit::error());
                    }
                }
            }
        }
        None
    })
}

fn scenario_input(world: &mut World) {
    let next_keys = settle_cursor(world);
    world.resource_scope(|world, mut input: Mut<ScenarioInput>| {
        let phase = match scene::phase(world) {
            Ok(phase) => phase,
            Err(error) => {
                error!("Native scenario lost its input owner: {error:#}");
                input.failed = true;
                world.write_message(AppExit::error());
                return;
            }
        };
        let keys = if input.failed || input.complete() || input.capture().is_some() {
            Vec::new()
        } else if scene::ready(world, phase) {
            next_keys.unwrap_or_default()
        } else {
            input.before = None;
            return;
        };
        send_input(world, &input.held, &keys, input.pad);
        input.held = keys;
        input.before = (!input.complete() && !input.failed && input.capture().is_none())
            .then(|| (world.resource::<Clock>().0.tick(), input.consumed));
    });
}

pub(crate) fn scenario_consumed(world: &mut World) {
    let presentation = world.resource::<Clock>().0.tick();
    let mut input = world.resource_mut::<ScenarioInput>();
    if let Some((before_presentation, before_input)) = input.before.take() {
        input.updates += u32::from(before_presentation != presentation);
        input.elapsed += u32::from(input.consumed != before_input);
    }
}

/// Recording policy is shared with the live application's diagnostic session.
#[derive(Default)]
pub struct CheckpointRecordingOptions<'a> {
    pub resolution: crate::Resolution,
    pub save_directory: Option<&'a Path>,
    pub paranoid: bool,
    /// Send raw controller events through the ordinary input system.
    pub gamepad: bool,
}

pub fn record_new_game(
    root: &Path,
    output: &Path,
    spec: &CheckpointReplay,
    options: CheckpointRecordingOptions<'_>,
) -> Result<()> {
    record(root, None, output, spec, options)
}

pub fn record_checkpoint(
    root: &Path,
    save: &Path,
    output: &Path,
    spec: &CheckpointReplay,
    options: CheckpointRecordingOptions<'_>,
) -> Result<()> {
    record(root, Some(save), output, spec, options)
}

fn record(
    root: &Path,
    save: Option<&Path>,
    output: &Path,
    spec: &CheckpointReplay,
    options: CheckpointRecordingOptions<'_>,
) -> Result<()> {
    spec.validate()?;
    if options.gamepad {
        for step in &spec.steps {
            let (Step::Hold { keys, .. } | Step::Wait { keys, .. }) = step else {
                continue;
            };
            ensure!(
                keys.iter().all(|key| key.button().is_some()),
                "scenario uses a key without a controller binding"
            );
        }
    }
    let app = probe::app_with_saves_mode(
        root,
        output,
        SaveOptions {
            directory: Some(
                options
                    .save_directory
                    .map_or_else(|| output.join("slots"), Path::to_path_buf),
            ),
            quick_slot: Some("probe".into()),
            load: save.map(Path::to_path_buf),
        },
        options.resolution,
        options.paranoid,
    )?;
    record_app(app, output, spec, options.gamepad)
}

pub(crate) fn record_app(
    app: App,
    output: &Path,
    spec: &CheckpointReplay,
    gamepad: bool,
) -> Result<()> {
    run_app(app, Destination::Recording(output), spec, gamepad)
}

pub(crate) fn capture_image(app: App, path: &Path, spec: &CheckpointReplay) -> Result<()> {
    ensure!(
        spec.steps
            .iter()
            .filter(|step| matches!(step, Step::Capture { .. }))
            .count()
            == 1,
        "single-image scenario requires one capture"
    );
    run_app(app, Destination::Image(path), spec, false)
}

#[derive(Clone, Copy)]
enum Destination<'a> {
    Recording(&'a Path),
    Image(&'a Path),
}

fn run_app(
    mut app: App,
    output: Destination<'_>,
    spec: &CheckpointReplay,
    gamepad: bool,
) -> Result<()> {
    spec.validate()?;
    let pad = gamepad.then(|| app.world_mut().spawn(Gamepad::default()).id());
    install_cursor(&mut app, spec.steps.clone(), pad);
    let began = Instant::now();
    while app.plugins_state() == PluginsState::Adding {
        ensure!(
            began.elapsed().as_secs() < 60,
            "scenario renderer setup timed out"
        );
        bevy::tasks::tick_global_task_pools_on_main_thread();
        thread::sleep(std::time::Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    // Startup creates the live scene owners. This is the only unconditional
    // zero-time update; preparation waits pump only while genuinely unready.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    app.update();
    crate::playthrough::check_exit(&app)?;
    let (mixer, mut audio) = resonance_playback::Offline::new();
    record_live(&mut app, output, spec, &mixer, &mut audio, pad)
}

/// Continue an initialized application. Loading gets real preparation, never simulated disc time.
fn record_live(
    app: &mut App,
    output: Destination<'_>,
    spec: &CheckpointReplay,
    mixer: &resonance_playback::Control,
    audio: &mut impl Iterator<Item = f32>,
    pad: Option<Entity>,
) -> Result<()> {
    spec.validate()?;
    match output {
        Destination::Recording(directory) => {
            ensure!(
                !directory.join("replay.json").exists(),
                "replay output already exists"
            );
            fs::create_dir_all(directory)?;
            fs::write(
                directory.join("replay.json"),
                serde_json::to_vec_pretty(spec)?,
            )?;
        }
        Destination::Image(path) => {
            ensure!(
                !path.exists() && !path.with_extension("json").exists(),
                "capture output already exists"
            );
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent)?;
            }
        }
    }
    let diagnostics = app
        .world()
        .resource::<crate::diagnostics::Diagnostics>()
        .0
        .clone();
    crate::audio::validate_startup(app, true, true)?;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.init_resource::<crate::field_audio::Trace>();
    let mut timed_out = false;
    wait_ready(app, &mut timed_out)?;
    // This snapshot was taken at load publication, before the Restore entry
    // was queued. Save eligibility is gameplay state, not a loading fence.
    let initial = app
        .world()
        .get_resource::<new_game::Session>()
        .and_then(|session| session.restored_checkpoint.clone());
    diagnostics.attempt("checkpoint audio attachment", attach(app, mixer))?;
    let mut wave = match output {
        Destination::Recording(directory) => Some(hound::WavWriter::create(
            directory.join("audio.partial.wav"),
            crate::field_audio::PCM_SPEC,
        )?),
        Destination::Image(_) => None,
    };
    let failure = std::sync::Arc::new(AtomicBool::new(false));
    let written = std::sync::Arc::new(AtomicU32::new(0));
    let mut frames = 0u64;
    let mut captures = Vec::new();
    let began = Instant::now();
    loop {
        let _ = settle_cursor(app.world_mut());
        let cursor = app.world().resource::<ScenarioInput>();
        if cursor.complete() || cursor.failed {
            break;
        }
        if let Some(name) = cursor.capture().map(str::to_owned) {
            wait_ready(app, &mut timed_out)?;
            let result = crate::new_game_capture::screenshot_held(
                app,
                match output {
                    Destination::Recording(directory) => directory.join(format!("{name}.png")),
                    Destination::Image(path) => path.to_owned(),
                },
                failure.clone(),
                written.clone(),
            );
            if failure.load(Ordering::Acquire) {
                result?;
                anyhow::bail!("checkpoint capture write failed");
            }
            diagnostics.attempt("checkpoint capture", result)?;
            let updates = app.world().resource::<ScenarioInput>().updates;
            if let Some(capture) = diagnostics.attempt(
                "checkpoint observation",
                observe(app.world_mut(), &name, updates, frames),
            )? {
                captures.push(capture);
            }
            app.world_mut().resource_mut::<ScenarioInput>().index += 1;
            continue;
        }
        if began.elapsed().as_secs() >= 600 {
            diagnostics.report("checkpoint replay", anyhow::anyhow!("scenario timed out"))?;
            break;
        }
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            resonance_game::clock::UPDATE_STEP,
        ));
        app.update();
        crate::playthrough::check_exit(app)?;
        wait_ready(app, &mut timed_out)?;
        diagnostics.attempt("checkpoint audio attachment", attach(app, mixer))?;
        let updates = app.world().resource::<ScenarioInput>().updates;
        let end = u64::from(updates)
            * u64::from(crate::field_audio::PCM_SPEC.sample_rate)
            * resonance_game::clock::UPDATE_RATE_DENOMINATOR
            / resonance_game::clock::UPDATE_RATE_NUMERATOR;
        diagnostics.attempt(
            "replay movie audio",
            app.world()
                .resource::<crate::movie::Playback>()
                .wait_for_audio(end - frames),
        )?;
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
                if let Some(wave) = &mut wave {
                    wave.write_sample((sample * 32768.).round().clamp(-32768., 32767.) as i16)?;
                }
            }
        }
        frames = end;
        if let Some(control) = app.world().get_resource::<crate::field_audio::Control>() {
            diagnostics.attempt("checkpoint audio", control.check())?;
        }
    }
    let cursor = app.world().resource::<ScenarioInput>();
    let updates = cursor.updates;
    let completed_steps = cursor.index;
    ensure!(
        !failure.load(Ordering::Acquire),
        "checkpoint capture write failed"
    );
    let expected_captures = spec
        .steps
        .iter()
        .filter(|step| matches!(step, Step::Capture { .. }))
        .count();
    diagnostics.attempt(
        "checkpoint captures",
        (|| {
            ensure!(
                written.load(Ordering::Acquire) as usize == expected_captures,
                "scenario did not write every capture"
            );
            Ok(())
        })(),
    )?;
    let unprepared = app
        .world()
        .resource::<loading::Resident>()
        .unprepared_reads
        .load(Ordering::Relaxed);
    let resolution = app.world().resource::<crate::display::Display>().0;
    let final_scene =
        diagnostics.attempt("checkpoint final scene", recording_scene(app.world()))?;
    if let (Destination::Recording(directory), Some(wave)) = (output, wave) {
        finish_recording(
            directory,
            &diagnostics,
            wave,
            unprepared,
            serde_json::json!({
                "complete":completed_steps == spec.steps.len(), "completed_steps":completed_steps,
                "updates":updates, "audio_frames":frames, "audio_device":false, "keyboard_input":pad.is_none(), "gamepad_input":pad.is_some(),
                "width":resolution.width, "height":resolution.height, "unprepared_reads":unprepared,
                "initial":initial, "captures":captures, "final_scene":final_scene,
                "identity":app.world().get_resource::<new_game::Session>().map(|session|&session.identity),
                "audio_commands":app.world().resource::<crate::field_audio::Trace>().0,
            }),
        )?;
    } else if let Destination::Image(path) = output {
        if unprepared != 0 {
            diagnostics.report(
                "checkpoint unprepared reads",
                anyhow::anyhow!("checkpoint replay read an unprepared asset"),
            )?;
        }
        let sidecar = path.with_extension("json");
        let mut metadata = match fs::read(&sidecar) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
            Err(error) => return Err(error.into()),
        };
        metadata["complete"] = serde_json::json!(completed_steps == spec.steps.len());
        metadata["completed_steps"] = serde_json::json!(completed_steps);
        publish_metadata(&sidecar, &diagnostics, metadata)?;
    }
    Ok(())
}

fn observe(
    world: &mut World,
    name: &str,
    update: u32,
    audio_frame: u64,
) -> Result<serde_json::Value> {
    let quicksave_available = checkpoint(world).is_ok();
    let mut capture = serde_json::json!({"name":name, "update":update, "audio_frame":audio_frame,
        "quicksave_available":quicksave_available,
        "asset_reads":world.resource::<loading::Resident>().memory_reads.load(Ordering::Acquire),
        "scene":recording_scene(world)?, "presentation_counter":world.resource::<Clock>().0.tick(),
        "battle":world.get_resource::<crate::battle::Owner>().and_then(|owner|owner.diagnostic())});
    if let Some(session) = world.get_resource::<new_game::Session>() {
        let field = &session.field;
        let actor = field
            .events
            .world
            .actors
            .get(&field.events.world.controlled_actor);
        capture["active_save_point"] = serde_json::json!(
            field
                .events
                .world
                .save_points
                .iter()
                .any(|point| point.active)
        );
        capture["map_id"] = serde_json::json!(field.map_id);
        capture["story"] = serde_json::json!(field.story_progress()?);
        capture["tick"] = serde_json::json!(field.events.tick());
        capture["position"] = serde_json::json!(actor.map(|actor| actor.position));
        capture["heading"] = serde_json::json!(actor.map(|actor| actor.heading));
        capture["drawn_effects"] = world
            .get_resource::<crate::field_effects::Artwork>()
            .map_or(serde_json::Value::Null, |art| {
                art.diagnostic(&field.events.world)
            });
        capture["save_points"] = crate::field_view::save_point_diagnostic(world);
        capture["checkpoint"] = serde_json::to_value(field.checkpoint().ok())?;
        capture["restored_checkpoint"] = serde_json::to_value(&session.restored_checkpoint)?;
        capture["persistent_party"] = serde_json::to_value(&field.events.world.party)?;
        capture["gameplay_random"] = serde_json::to_value(field.events.world.gameplay_random)?;
        capture["menu"] = serde_json::json!(field.menu.as_ref().map(|menu| serde_json::json!({
            "page":format!("{:?}",menu.page), "focus":format!("{:?}",menu.focus),
            "visually_settled":menu_settled(world, menu),
            "selected":menu.selected, "character":menu.character, "busy":menu.busy,
            "checkpoint":menu.checkpoint,
            "inventory":{"category":menu.inventory.category,"row":menu.inventory.row,
                "first":menu.inventory.first,"focus":format!("{:?}",menu.inventory.focus)},
            "equipment":menu.equipment,"tech":menu.tech,"strategy":menu.strategy,"unison":menu.unison,
            "cooking":menu.cooking,"customize":menu.customize,"status":menu.status,
            "confirmation":menu.confirmation,"notice":menu.notice,"popup":menu.popup,
            "slots":menu.slots.iter().map(|slot| match slot {
                resonance_game::menu::Slot::Empty => "empty",
                resonance_game::menu::Slot::Saved { .. } => "saved",
                resonance_game::menu::Slot::Invalid(_) => "invalid",
            }).collect::<Vec<_>>()
        })));
    }
    Ok(capture)
}
fn finish_recording<W: std::io::Write + std::io::Seek>(
    output: &Path,
    diagnostics: &resonance_content::diagnostics::Diagnostics,
    wave: hound::WavWriter<W>,
    unprepared_reads: u64,
    metadata: serde_json::Value,
) -> Result<()> {
    wave.finalize()?;
    fs::rename(output.join("audio.partial.wav"), output.join("audio.wav"))?;
    if unprepared_reads != 0 {
        diagnostics.report(
            "checkpoint unprepared reads",
            anyhow::anyhow!("checkpoint replay read an unprepared asset"),
        )?;
    }
    publish_metadata(&output.join("recording.json"), diagnostics, metadata)
}

fn publish_metadata(
    path: &Path,
    diagnostics: &resonance_content::diagnostics::Diagnostics,
    mut metadata: serde_json::Value,
) -> Result<()> {
    metadata["mode"] = serde_json::json!(if diagnostics.paranoid() {
        "paranoid"
    } else {
        "tolerant"
    });
    metadata["paranoid"] = serde_json::json!(diagnostics.paranoid());
    let valid = metadata["complete"] == true && !diagnostics.has_errors();
    metadata["valid"] = serde_json::json!(valid);
    metadata["diagnostics"] = serde_json::to_value(diagnostics.entries())?;
    fs::write(path, serde_json::to_vec_pretty(&metadata)?)?;
    ensure!(
        valid,
        "scenario validation failed; diagnostics: {}",
        path.display()
    );
    Ok(())
}

pub(super) fn capture_from<'a>(
    report: &'a serde_json::Value,
    name: &str,
) -> Result<&'a serde_json::Value> {
    report["captures"]
        .as_array()
        .and_then(|captures| captures.iter().find(|capture| capture["name"] == name))
        .with_context(|| format!("missing scenario capture {name}"))
}

pub(crate) fn assert_checkpoint(
    actual: &FieldCheckpoint,
    expected: &FieldCheckpoint,
) -> Result<()> {
    ensure!(
        actual.progress.tick >= expected.progress.tick,
        "save/load moved the event clock backwards"
    );
    // Compare the published load before ordinary gameplay. Field construction
    // advances the event clock; every other saved value must survive exactly.
    let mut actual = actual.clone();
    actual.progress.tick = expected.progress.tick;
    ensure!(
        serde_json::to_value(actual)? == serde_json::to_value(expected)?,
        "save/load changed checkpoint state"
    );
    Ok(())
}

fn attach(app: &mut App, mixer: &resonance_playback::Control) -> Result<()> {
    crate::playthrough::attach::<crate::movie::MovieAudio>(app.world_mut(), mixer)?;
    crate::playthrough::attach::<crate::field_audio::FieldSource>(app.world_mut(), mixer)?;
    crate::playthrough::attach::<crate::GameAudio>(app.world_mut(), mixer)
}
pub(super) fn wait_ready(app: &mut App, timed_out: &mut bool) -> Result<()> {
    let diagnostics = app
        .world()
        .resource::<crate::diagnostics::Diagnostics>()
        .0
        .clone();
    let began = Instant::now();
    loop {
        let world = app.world_mut();
        let Some(phase) = diagnostics.attempt("checkpoint scene owner", scene::phase(world))?
        else {
            return Ok(());
        };
        if scene::ready(world, phase) && !world.resource::<Persistence>().is_writing() {
            *timed_out = false;
            return Ok(());
        }
        // A timed-out scene can still receive ordinary updates and diagnostics.
        // Retry the bounded preparation wait only after readiness has recovered.
        if *timed_out {
            return Ok(());
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
        let battle_clock = app
            .world()
            .get_resource::<crate::battle::Owner>()
            .and_then(|battle| battle.capture_clock());
        let entry_clock = app
            .world()
            .get_resource::<crate::battle::Owner>()
            .and_then(|battle| battle.entry_clock());
        let presentation_tick = app.world().resource::<Clock>().0.tick();
        let field_tick = (phase == scene::Phase::Field)
            .then(|| app.world().get_resource::<new_game::Session>())
            .flatten()
            .map(|s| (s.field.map_id, s.field.events.tick()));
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
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::ZERO,
        ));
        app.update();
        crate::playthrough::check_exit(app)?;
        let world = app.world_mut();
        if world.resource::<Clock>().0.tick() != presentation_tick {
            diagnostics.report(
                "checkpoint preparation",
                anyhow::anyhow!("presentation advanced during checkpoint preparation"),
            )?;
        }
        if let Some((map, tick)) = field_tick
            && let Some(session) = world.get_resource::<new_game::Session>()
            && session.field.map_id == map
            && session.field.events.tick() != tick
        {
            diagnostics.report(
                "checkpoint preparation",
                anyhow::anyhow!("field advanced during checkpoint preparation"),
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
        thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_scene_does_not_run_an_extra_update() {
        let mut app = App::new();
        let mut boot = crate::boot::Playback::default();
        boot.logos = Some(Default::default());
        app.insert_resource(crate::diagnostics::Diagnostics(
            resonance_content::diagnostics::Diagnostics::new(true),
        ))
        .insert_resource(crate::timing::Ready(true))
        .insert_resource(boot)
        .insert_resource(Persistence {
            store: Store::new(std::env::temp_dir()),
            slot: SlotId::new("unused").unwrap(),
            writing: Mutex::default(),
        })
        .add_systems(Update, || panic!("ready scene received an extra update"));
        let mut timed_out = true;
        wait_ready(&mut app, &mut timed_out).unwrap();
        assert!(!timed_out);
        assert_eq!(
            app.world()
                .resource::<crate::boot::Playback>()
                .logos
                .as_ref()
                .unwrap()
                .tick,
            0
        );
    }

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
    fn recording_publishes_artifacts_then_rejects_incomplete_or_degraded_runs() {
        for (complete, degraded) in [(true, true), (false, false), (true, false)] {
            let output = Output::new();
            let diagnostics = resonance_content::diagnostics::Diagnostics::new(false);
            if degraded {
                diagnostics
                    .report("battle effect", anyhow::anyhow!("missing texture"))
                    .unwrap();
            }
            let result = finish_recording(
                &output.0,
                &diagnostics,
                output.wave(),
                if degraded { 2 } else { 0 },
                serde_json::json!({"complete":complete}),
            );
            let valid = complete && !degraded;
            assert_eq!(result.is_ok(), valid);
            if let Err(error) = result {
                assert!(error.to_string().contains("recording.json"));
            }
            let metadata: serde_json::Value =
                serde_json::from_slice(&fs::read(output.0.join("recording.json")).unwrap())
                    .unwrap();
            assert_eq!(metadata["complete"], complete);
            assert_eq!(metadata["mode"], "tolerant");
            assert_eq!(metadata["valid"], valid);
            assert_eq!(
                metadata["diagnostics"].as_array().unwrap().len(),
                if degraded { 2 } else { 0 }
            );
            assert!(!output.0.join("audio.partial.wav").exists());
            let samples = hound::WavReader::open(output.0.join("audio.wav"))
                .unwrap()
                .into_samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(samples, [123, -123]);
        }
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
    fn checkpoint_comparison_at_publication_preserves_all_state_except_setup_tick() {
        use resonance_events::{SavedProgress, camera::CameraSettings, party::Party};
        let expected = FieldCheckpoint {
            map_id: 332,
            position: [0.; 3],
            heading: 0.,
            played_ticks: 100,
            allow_incomplete_scripts: false,
            camera: Some(CameraSettings {
                axes: [true; 3],
                fixed_position: [0.; 3],
                angles: [0.; 3],
                offset: [0.; 3],
                distance: 1000.,
                fov_degrees: 27.,
                position_bounds: [[-1000., 1000.]; 3],
                target_bounds: [[-1000., 1000.]; 3],
                position_rate: 1.,
                target_rate: 1.,
                fog: None,
            }),
            progress: SavedProgress {
                script_globals: vec![0; 256],
                script_state: Default::default(),
                event_flags: Default::default(),
                event_records: Default::default(),
                random_state: 1,
                gameplay_random: Default::default(),
                tick: 100,
                party: Party {
                    battle_rules: Default::default(),
                    new_game_plus: Default::default(),
                    game_clears: 0,
                    grade_hundredths: 0,
                    collectors_book_complete: false,
                    monster_book_complete: false,
                    figurine_book_complete: false,
                    unison_gauge: 0,
                    battles: Default::default(),
                    figurines: Default::default(),
                    monsters: Default::default(),
                    travel: Default::default(),
                    cooking: Default::default(),
                    strategy_presets: None,
                    encounter_modifier: None,
                    viewed_skits: Default::default(),
                    members: vec![],
                    formation: vec![],
                    field_leader: 1,
                    leader_locked: false,
                    items: Default::default(),
                    found_items: Default::default(),
                    recent_items: vec![],
                    gald: 0,
                    spent_gald: 0,
                    settings: Default::default(),
                },
            },
        };
        let mut advanced = expected.clone();
        advanced.progress.tick += 1;
        assert_checkpoint(&advanced, &expected).unwrap();
        let corruptions: [fn(&mut FieldCheckpoint); 9] = [
            |s| {
                s.progress.event_records.insert(
                    1,
                    resonance_events::EventRecord {
                        value: 1,
                        extra: 0,
                        tick: 100,
                        level: None,
                        recorded_at: None,
                    },
                );
            },
            |s| s.progress.random_state ^= 1,
            |s| {
                s.progress.gameplay_random.next_u32();
            },
            |s| s.progress.party.gald += 1,
            |s| s.position[0] += 1.,
            |s| s.camera.as_mut().unwrap().distance += 1.,
            |s| s.progress.tick -= 1,
            |s| s.played_ticks -= 1,
            |s| s.played_ticks += 1,
        ];
        for corrupt in corruptions {
            let mut actual = expected.clone();
            corrupt(&mut actual);
            assert!(assert_checkpoint(&actual, &expected).is_err());
        }
    }

    #[test]
    fn items_focus_requires_the_requested_panel() {
        use resonance_game::menu::{Menu, Page, items::Focus};
        let mut world = World::new();
        world.insert_resource(title::LoadMenu(Menu::new(Page::Items, None, false)));
        let list = Event::ItemsFocus {
            focus: ItemsFocus::List,
        };
        let target = Event::ItemsFocus {
            focus: ItemsFocus::Target,
        };
        assert!(list.matches(&world).unwrap());
        assert!(!target.matches(&world).unwrap());
        world.resource_mut::<title::LoadMenu>().0.inventory.focus = Focus::Target;
        assert!(!list.matches(&world).unwrap());
        assert!(target.matches(&world).unwrap());
        world.resource_mut::<title::LoadMenu>().0.page = Page::Main;
        assert!(!target.matches(&world).unwrap());
    }

    #[test]
    fn scenario_has_one_ordered_input_and_capture_language() {
        let mut replay: CheckpointReplay = serde_json::from_value(serde_json::json!({
            "version":2,"steps":[
                {"do":"wait","until":{"kind":"field","map_id":332,"free_control":true},"max_updates":120},
                {"do":"hold","keys":["menu"],"updates":1},
                {"do":"wait","until":{"kind":"menu","page":"main"},"max_updates":60},
                {"do":"wait","until":{"kind":"menu_settled","page":"main"},"max_updates":60},
                {"do":"capture","name":"main"}
            ]
        })).unwrap();
        replay.validate().unwrap();
        replay.steps.push(Step::Capture {
            name: "main".into(),
        });
        assert!(replay.validate().is_err());
        assert!(
            serde_json::from_value::<CheckpointReplay>(serde_json::json!({
                "version":2,"steps":[],"presentation_advances":{"1":80}
            }))
            .is_err()
        );
    }

    #[test]
    fn new_game_scenario_uses_the_same_events_and_controller_keys() {
        for scenario in [
            include_str!("../../examples/scenarios/new-game.json"),
            include_str!("../../examples/scenarios/field-route.json"),
            include_str!("../../../../tools/oracle/cases/native-navigation.json"),
            include_str!("../../examples/scenarios/field-menus.json"),
        ] {
            let spec: CheckpointReplay = serde_json::from_str(scenario).unwrap();
            spec.validate().unwrap();
            for step in spec.steps {
                if let Step::Hold { keys, .. } | Step::Wait { keys, .. } = step {
                    assert!(keys.into_iter().all(|key| key.button().is_some()));
                }
            }
        }
    }

    #[test]
    fn replay_input_releases_old_buttons_without_repeating_held_keys() {
        use bevy::input::{ButtonState, gamepad::RawGamepadEvent, keyboard::KeyboardInput};
        let mut world = World::new();
        world.init_resource::<Messages<KeyboardInput>>();
        world.init_resource::<Messages<RawGamepadEvent>>();
        send_input(
            &mut world,
            &[Key::Up, Key::Interact],
            &[Key::Down, Key::Interact],
            None,
        );
        let events: Vec<_> = world
            .resource_mut::<Messages<KeyboardInput>>()
            .drain()
            .map(|event| (event.key_code, event.state))
            .collect();
        assert_eq!(
            events,
            [
                (KeyCode::ArrowUp, ButtonState::Released),
                (KeyCode::ArrowDown, ButtonState::Pressed)
            ]
        );
        let pad = world.spawn(Gamepad::default()).id();
        send_input(
            &mut world,
            &[Key::Up, Key::Interact],
            &[Key::Down, Key::Interact],
            Some(pad),
        );
        let events: Vec<_> = world
            .resource_mut::<Messages<RawGamepadEvent>>()
            .drain()
            .map(|event| match event {
                RawGamepadEvent::Button(event) => (event.gamepad, event.button, event.value),
                _ => panic!("unexpected controller event"),
            })
            .collect();
        assert_eq!(
            events,
            [
                (pad, GamepadButton::DPadUp, 0.),
                (pad, GamepadButton::DPadDown, 1.)
            ]
        );
        assert!(world.resource::<Messages<KeyboardInput>>().is_empty());
    }

    #[test]
    fn scenario_input_is_consumed_once_per_native_update_at_irregular_render_rates() {
        #[derive(Resource, Default)]
        struct Consumed(Vec<([f32; 2], bool, bool)>);
        fn fixture(gamepad: bool) -> App {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
                .insert_resource(Time::<Fixed>::from_duration(
                    resonance_game::clock::UPDATE_STEP,
                ))
                .insert_resource(TimeUpdateStrategy::ManualDuration(
                    std::time::Duration::ZERO,
                ))
                .init_resource::<Clock>()
                .init_resource::<crate::PendingInput>()
                .init_resource::<crate::field_view::Controls>()
                .init_resource::<Consumed>()
                .insert_resource(crate::TitleActive)
                .insert_resource(crate::Menu(Default::default()))
                .insert_resource(crate::timing::Ready(false))
                .add_systems(
                    FixedPreUpdate,
                    (crate::gather_input, crate::field_view::gather_controls)
                        .after(bevy::input::InputSystems),
                )
                .add_systems(
                    FixedUpdate,
                    |ready: Res<crate::timing::Ready>,
                     mut clock: ResMut<Clock>,
                     mut title: ResMut<crate::Menu>,
                     mut pending: ResMut<crate::PendingInput>,
                     mut field: ResMut<crate::field_view::Controls>,
                     mut scenario: ResMut<ScenarioInput>,
                     mut consumed: ResMut<Consumed>| {
                        if !ready.0 {
                            return;
                        }
                        clock.0.advance();
                        title.0.tick += 1;
                        let title = pending.consume(clock.0);
                        let field = field.consume();
                        scenario.acknowledge_input();
                        consumed.0.push((
                            field.direction,
                            field.pressed(resonance_events::input::Button::Accept),
                            title.accept,
                        ));
                    },
                );
            let spec = CheckpointReplay::new(vec![
                Step::Hold {
                    keys: vec![Key::Up, Key::Interact],
                    updates: 1,
                },
                Step::Hold {
                    keys: vec![Key::Up],
                    updates: 1,
                },
                Step::Hold {
                    keys: vec![Key::Down],
                    updates: 1,
                },
                Step::Hold {
                    keys: vec![],
                    updates: 1,
                },
                Step::Wait {
                    until: Event::TitleTick { tick: 6 },
                    keys: vec![Key::Interact],
                    tap_every: Some(2),
                    max_updates: 2,
                },
                Step::capture("complete"),
            ]);
            if gamepad {
                let pad = app.world_mut().spawn(Gamepad::default()).id();
                install_cursor(
                    &mut app,
                    spec.steps
                        .into_iter()
                        .filter(|step| !matches!(step, Step::Capture { .. }))
                        .collect(),
                    Some(pad),
                );
            } else {
                install_scenario_input(&mut app, spec).unwrap();
            }
            app.update();
            app.insert_resource(TimeUpdateStrategy::ManualDuration(
                resonance_game::clock::UPDATE_STEP * 3,
            ));
            app.update();
            assert!(app.world().resource::<Consumed>().0.is_empty());
            assert_eq!(app.world().resource::<ScenarioInput>().updates, 0);
            app.world_mut().resource_mut::<crate::timing::Ready>().0 = true;
            app
        }
        let step = resonance_game::clock::UPDATE_STEP;
        let irregular = [
            step / 2,
            std::time::Duration::ZERO,
            step - step / 2,
            step * 3,
            std::time::Duration::ZERO,
            step * 3,
        ];
        let mut regular = fixture(false);
        let mut uneven = fixture(false);
        let mut controller = fixture(true);
        for duration in [step; 7] {
            regular.insert_resource(TimeUpdateStrategy::ManualDuration(duration));
            regular.update();
        }
        for app in [&mut uneven, &mut controller] {
            for duration in irregular {
                app.insert_resource(TimeUpdateStrategy::ManualDuration(duration));
                app.update();
            }
            assert!(app.world().resource::<ScenarioInput>().complete());
            assert_eq!(app.world().resource::<ScenarioInput>().updates, 6);
        }
        let expected = vec![
            ([0., 1.], true, true),
            ([0., 1.], false, false),
            ([0., -1.], false, false),
            ([0., 0.], false, false),
            ([0., 0.], true, true),
            ([0., 0.], false, false),
            ([0., 0.], false, false),
        ];
        assert_eq!(regular.world().resource::<Consumed>().0, expected);
        assert_eq!(uneven.world().resource::<Consumed>().0, expected);
        assert_eq!(controller.world().resource::<Consumed>().0, expected);
    }

    #[test]
    #[ignore = "requires current prepared opening field; no GPU or audio device"]
    fn quickload_restore_does_not_erase_consumed_scenario_input() {
        let directory = Output::new();
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
            PathBuf::from,
        );
        let mut cache = loading::Cache::default();
        let package = new_game::FieldPackage::prepare(&root, 332, &mut cache, || false).unwrap();
        let saved = crate::test_support::field_checkpoint(&package.files).unwrap();
        let mut session = new_game::Session::load_prepared(
            &root,
            package.files,
            Some(saved.clone()),
            None,
            &mut cache,
        )
        .unwrap();
        // Save the initialized field, including its camera bounds.
        let saved = session.field.checkpoint().unwrap();
        session.audio = None;
        // Restore the saved party while acknowledging the quickload input once.
        session.field.events.world.party.as_mut().unwrap().gald += 1;
        let store = Store::new(directory.0.clone());
        let slot = SlotId::new("scenario").unwrap();
        store
            .write_async(
                Kind::Quicksave,
                slot.clone(),
                resonance_persistence::encode(
                    &Header {
                        identity: session.identity.clone(),
                        label: "Scenario".into(),
                        location: "Opening".into(),
                        played_ticks: saved.played_ticks,
                        saved_unix_seconds: 0,
                    },
                    &saved,
                )
                .unwrap(),
            )
            .unwrap()
            .wait()
            .unwrap();

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::input::InputPlugin,
        ))
        .init_asset::<Image>()
        .init_resource::<Clock>()
        .init_resource::<crate::timing::Ready>()
        .init_resource::<crate::boot::Playback>()
        .init_resource::<field_view::Controls>()
        .init_resource::<loading::Resident>()
        .init_resource::<crate::movie::Playback>()
        .insert_resource(crate::display::Targets {
            source: Handle::default(),
            output: None,
        })
        .insert_resource(crate::RunOptions {
            assets: root,
            saves: SaveOptions::default(),
            script_root: None,
            capture: None,
            capture_at: None,
            reveal: false,
            selected: 0,
            silent: true,
            paranoid: true,
            skip_intro: true,
            record_playthrough: None,
            record_title_ticks: 0,
            skip_battles: false,
            allow_incomplete_scripts: false,
        })
        .insert_resource(Persistence {
            store,
            slot,
            writing: Mutex::default(),
        })
        .insert_resource(Time::<Fixed>::from_duration(
            resonance_game::clock::UPDATE_STEP,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::ZERO,
        ))
        .add_systems(
            FixedPreUpdate,
            field_view::gather_controls.after(bevy::input::InputSystems),
        )
        .add_systems(
            FixedUpdate,
            (
                crate::timing::advance_clock,
                |mut session: ResMut<new_game::Session>,
                 mut controls: ResMut<field_view::Controls>,
                 mut scenario: ResMut<ScenarioInput>| {
                    if session.audio.is_some() {
                        return;
                    }
                    session.field.step(controls.consume()).unwrap();
                    scenario.acknowledge_input();
                },
            )
                .chain(),
        )
        .add_systems(FixedPostUpdate, super::shortcuts.before(scenario_consumed));
        // Renderer readiness is injected; persistence and field restoration are real.
        let art =
            field_view::prepared_test_art(&session.assets, app.world().resource::<AssetServer>());
        app.insert_resource(art).insert_resource(session);
        app.world()
            .resource::<loading::Resident>()
            .active
            .store(true, Ordering::Release);
        install_cursor(
            &mut app,
            vec![
                Step::Hold {
                    keys: vec![Key::Quickload],
                    updates: 1,
                },
                Step::Hold {
                    keys: vec![],
                    updates: 1,
                },
                Step::capture("loaded"),
            ],
            None,
        );
        app.update();
        assert!(
            checkpoint(app.world_mut()).is_ok(),
            "real quickload admission must succeed"
        );
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            resonance_game::clock::UPDATE_STEP,
        ));
        app.update();
        let restored = app.world().resource::<new_game::Session>();
        assert_eq!(
            restored.field.events.world.party.as_ref().unwrap().gald,
            saved.progress.party.gald
        );
        assert_checkpoint(restored.restored_checkpoint.as_ref().unwrap(), &saved).unwrap();
        assert_eq!(app.world().resource::<ScenarioInput>().elapsed, 1);
        assert_eq!(app.world().resource::<ScenarioInput>().updates, 1);
        assert_eq!(app.world().resource::<Clock>().0.tick(), 1);
        // Publication would prepare these assets before the next native update.
        app.world_mut().resource_mut::<new_game::Session>().audio = None;
        app.update();
        let _ = settle_cursor(app.world_mut());
        let cursor = app.world().resource::<ScenarioInput>();
        assert_eq!(cursor.capture(), Some("loaded"));
        assert_eq!(cursor.consumed, 2);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::F9)
        );
    }

    #[test]
    fn scenario_wait_repeats_input_and_checks_its_event_at_the_deadline() {
        let mut world = World::new();
        world.insert_resource(crate::TitleActive);
        world.insert_resource(crate::Menu(Default::default()));
        let step = Step::Wait {
            until: Event::TitleTick { tick: 10 },
            keys: vec![Key::Interact],
            tap_every: Some(2),
            max_updates: 4,
        };
        assert_eq!(step.input(&world, 0).unwrap(), Some(&[Key::Interact][..]));
        assert_eq!(step.input(&world, 1).unwrap(), Some(&[][..]));
        assert_eq!(step.input(&world, 2).unwrap(), Some(&[Key::Interact][..]));
        assert!(
            step.input(&world, 4).is_err(),
            "an unmet event must time out"
        );
        world.resource_mut::<crate::Menu>().0.tick = 10;
        assert!(
            step.input(&world, 4).unwrap().is_none(),
            "an event reached on the last permitted update succeeds"
        );
    }
}
