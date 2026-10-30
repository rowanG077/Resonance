use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, Battle, ButtonInput, Control, ControllerPort, Side, TechniqueId};
mod items;
pub use items::{ActorSelection, InventoryInput, ItemRow, ListFrame};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Input {
    pub controller: ControllerPort,
    /// Current physical-pad connections in stable local slots. Keyboard input
    /// does not imply an attached pad; the default preserves the no-pad route.
    pub connected: [bool; 4],
    pub open: ButtonInput,
    pub confirm_a: ButtonInput,
    pub cancel_b: ButtonInput,
    /// Actor selectors use physical Y, independent of the mapped menu action.
    pub cancel_y: ButtonInput,
    /// Independent left/right events (bits1/2), including digital repeat.
    pub picker_directions: u8,
    /// The shared menu ORs all four physical input channels.
    pub shared_menu: crate::menu::Input,
    /// Shared analog repeat event, -1/0/1.
    pub step: i8,
}

/// Command-owned post-Tech target selection. The page has already validated
/// the learned action; this state owns navigation, the selected enemy and its issuer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TechTarget {
    pub actor: ActorId,
    pub issuer: ActorId,
    pub technique: TechniqueId,
    pub selected: Option<ActorId>,
}

const UNISON_UNLOCK_STORY: i32 = 1_403_000;
const COLISEUM_EQUIPMENT_CHANGE_TICKS: u32 = 300;

pub fn enabled_rows(escape_restricted: bool, story: i32) -> u8 {
    let mut enabled = 0xff;
    if escape_restricted {
        enabled &= !Command::Escape.mask();
    }
    if story < UNISON_UNLOCK_STORY {
        enabled &= !Command::Unison.mask();
    }
    enabled
}

/// Formation order and base capabilities are prepared once; eligibility and
/// the coliseum equipment deadline are sampled when opening the menu.
#[derive(Debug, Clone)]
pub struct Setup {
    actors: Vec<ActorId>,
    enabled: u8,
    coliseum: bool,
}
impl Setup {
    pub fn new(
        actors: Vec<ActorId>,
        enabled: u8,
        roster: &[resonance_battle::Actor],
        coliseum: bool,
    ) -> Result<Self> {
        ensure!(
            (1..=resonance_battle::PARTY_CAPACITY).contains(&actors.len()),
            "invalid command roster size"
        );
        for (slot, &id) in actors.iter().enumerate() {
            let actor = roster.get(id.index()).context("command actor is absent")?;
            ensure!(
                actor.side == Side::Party && !actors[..slot].contains(&id),
                "invalid command party actor"
            );
        }
        Ok(Self {
            actors,
            enabled,
            coliseum,
        })
    }

    pub fn admission(&self, battle: &Battle, controller: ControllerPort) -> Option<Admission> {
        if !battle.command_admission_allowed() {
            return None;
        }
        let mut enabled = self.enabled;
        if self.coliseum && battle.ledger().elapsed_ticks >= COLISEUM_EQUIPMENT_CHANGE_TICKS {
            enabled &= !Command::Equipment.mask();
        }
        self.actors.iter().find_map(|&id| {
            let actor = battle.actors().get(id.index())?;
            let slot = actor.control_slot;
            if slot != controller
                || matches!(actor.control, Control::Auto | Control::Enemy)
                    && slot != ControllerPort::One
            {
                return None;
            }
            Some(Admission {
                actor: id,
                controller: slot,
                enabled,
            })
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admission {
    pub actor: ActorId,
    pub controller: ControllerPort,
    pub enabled: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub actor: ActorId,
    pub selected: Command,
    /// Static command capabilities projected through the sampled item lock.
    pub enabled: u8,
    pub controller: ControllerPort,
    /// Connection sample from this retained menu visit, shared by input and draw.
    pub connected: [bool; 4],
    pub view: View,
    pub escape_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Strip,
    User(ActorSelection),
    Inventory(ListFrame),
    Tech(crate::menu::techniques::Tech),
    Strategy(crate::menu::strategy::Strategy),
    Unison(crate::menu::unison::Unison),
    Equipment(crate::menu::equipment::Equipment),
    Ally(ActorSelection),
    Enemy { target: ActorId },
    TechTarget { target: Option<ActorId> },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputKind {
    #[default]
    Command,
    SharedMenu,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InventoryMemory {
    pub selected: usize,
    pub first: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Command {
    #[default]
    Tech,
    Unison,
    Strategy,
    Equipment,
    Items,
    Escape,
}
impl Command {
    pub const ALL: [Self; 6] = [
        Self::Tech,
        Self::Unison,
        Self::Strategy,
        Self::Equipment,
        Self::Items,
        Self::Escape,
    ];

    pub fn index(self) -> u8 {
        self as u8
    }
    pub fn mask(self) -> u8 {
        1 << self.index()
    }
    fn next(self, direction: i8) -> Self {
        Self::ALL[(i16::from(self.index()) + i16::from(direction.signum()))
            .rem_euclid(Self::ALL.len() as i16) as usize]
    }
    fn intent(self, input: Input, controller: ControllerPort, enabled: u8) -> StripIntent {
        if input.controller != controller {
            return StripIntent::Stay;
        }
        if input.confirm_a.pressed && enabled & self.mask() != 0 {
            StripIntent::Choose(self)
        } else if input.cancel_b.pressed || input.open.pressed {
            StripIntent::Close
        } else {
            StripIntent::Stay
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StripIntent {
    Stay,
    Close,
    Choose(Command),
}

/// Cursor memory transferred when a battle opens or closes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Memory {
    pub tech_character: usize,
    pub unison_character: usize,
    pub inventory: InventoryMemory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Cue(u16),
    CaptureBackdrop,
    VoiceStreamsPaused(bool),
}

#[derive(Debug, Default)]
pub struct Step {
    pub events: Vec<Event>,
    pub paused: bool,
}

#[derive(Debug)]
struct Session {
    admission: Admission,
    page: Page,
}

#[derive(Debug)]
enum Page {
    Strip,
    Items(items::Owner),
    Tech(Box<crate::menu::techniques::BattlePage>),
    TechTarget(TechTarget),
    Strategy(crate::menu::strategy::Strategy),
    Unison(crate::menu::unison::Unison),
    Equipment(crate::menu::equipment::Equipment),
}
impl Page {
    fn input_kind(&self) -> InputKind {
        match self {
            Self::Items(items) => items.input_kind(),
            Self::Tech(_) | Self::Strategy(_) | Self::Unison(_) | Self::Equipment(_) => {
                InputKind::SharedMenu
            }
            Self::Strip | Self::TechTarget(_) => InputKind::Command,
        }
    }
    fn view(&self) -> View {
        match self {
            Self::Strip => View::Strip,
            Self::TechTarget(target) => View::TechTarget {
                target: target.selected,
            },
            Self::Items(items) => items.view(),
            Self::Tech(tech) => View::Tech(tech.state.clone()),
            Self::Strategy(state) => View::Strategy(state.clone()),
            Self::Unison(state) => View::Unison(state.clone()),
            Self::Equipment(state) => View::Equipment(state.clone()),
        }
    }
}

#[derive(Debug, Default)]
pub struct Owner {
    session: Option<Session>,
    selected: Command,
    connected: [bool; 4],
    equipment_character: usize,
    tech_character: usize,
    unison_character: usize,
    item_memory: items::Memory,
    items_available: bool,
    escape_requested: bool,
}
impl Owner {
    pub(in crate::battle) fn capture_arte_binding(
        &mut self,
        candidate: &mut super::results::Candidate,
        held: &std::collections::BTreeSet<resonance_content::input::PhysicalInput>,
    ) -> bool {
        match self.session.as_mut().map(|s| &mut s.page) {
            Some(Page::Tech(page)) => candidate.capture_arte_binding(page, held),
            _ => false,
        }
    }

    /// Failed shared-page drawing returns input to the retained command strip.
    /// The caller completes or discards the departed page's edits.
    pub(super) fn return_to_strip(&mut self) -> Option<View> {
        let view = self.frame()?.view;
        let page = std::mem::replace(&mut self.session.as_mut()?.page, Page::Strip);
        self.remember_page(&page);
        Some(view)
    }

    pub fn equipment_character(&self) -> usize {
        self.equipment_character
    }

    pub fn memory(&self) -> Memory {
        Memory {
            tech_character: self.tech_character,
            unison_character: self.unison_character,
            inventory: self.item_memory.inventory(),
        }
    }

    pub fn restore_memory(&mut self, memory: Memory) {
        self.tech_character = memory.tech_character;
        self.unison_character = memory.unison_character;
        self.item_memory.restore(memory.inventory);
    }

    fn remember_page(&mut self, page: &Page) {
        match page {
            Page::Tech(tech) => self.tech_character = tech.state.character,
            Page::Unison(unison) => {
                self.unison_character = unison.character;
                self.tech_character = unison.character;
            }
            Page::Items(items) => items.remember(&mut self.item_memory),
            _ => {}
        }
    }

    pub(super) fn close(&mut self) {
        if let Some(session) = self.session.take() {
            self.remember_page(&session.page);
        }
        self.escape_requested = false;
    }

    pub fn controller(&self) -> Option<ControllerPort> {
        self.session
            .as_ref()
            .map(|session| session.admission.controller)
    }
    pub fn input_kind(&self) -> InputKind {
        self.session
            .as_ref()
            .map_or(InputKind::Command, |session| session.page.input_kind())
    }
    fn effective_enabled(&self, capabilities: u8) -> u8 {
        if self.items_available {
            capabilities
        } else {
            capabilities & !Command::Items.mask()
        }
    }

    pub fn frame(&self) -> Option<Frame> {
        let session = self.session.as_ref()?;
        Some(Frame {
            actor: session.admission.actor,
            controller: session.admission.controller,
            enabled: self.effective_enabled(session.admission.enabled),
            selected: self.selected,
            connected: self.connected,
            view: session.page.view(),
            escape_requested: self.escape_requested,
        })
    }

    pub fn step_with_candidate(
        &mut self,
        input: Input,
        admission: Option<Admission>,
        battle: &mut Battle,
        candidate: &mut super::results::Candidate,
    ) -> Result<Step> {
        self.connected = input.connected;
        self.items_available = battle.item_cooldown() == 0 && battle.pending_item().is_none();
        self.escape_requested = battle.escape_frame().is_some_and(|frame| frame.requested);
        let Some(mut session) = self.session.take() else {
            let Some(admission) = admission.filter(|_| input.open.pressed) else {
                return Ok(Step::default());
            };
            candidate.refresh_menu_roster(battle);
            self.session = Some(Session {
                admission,
                page: Page::Strip,
            });
            return Ok(Step {
                events: vec![Event::Cue(2), Event::VoiceStreamsPaused(true)],
                paused: true,
            });
        };
        let mut result = Step {
            paused: true,
            ..Default::default()
        };
        let was_shared = session.page.input_kind() == InputKind::SharedMenu;
        let next = match std::mem::replace(&mut session.page, Page::Strip) {
            Page::Strip => {
                let enabled = self.effective_enabled(session.admission.enabled);
                if input.controller == session.admission.controller {
                    if input.step != 0 {
                        self.selected = self.selected.next(input.step);
                        result.events.push(Event::Cue(1));
                    }
                    if input.confirm_a.pressed && enabled & self.selected.mask() == 0 {
                        result.events.push(Event::Cue(4));
                    }
                }
                match self
                    .selected
                    .intent(input, session.admission.controller, enabled)
                {
                    StripIntent::Stay => Some(Page::Strip),
                    StripIntent::Close => {
                        result.events.push(Event::Cue(3));
                        None
                    }
                    StripIntent::Choose(command) => {
                        result.events.push(Event::Cue(2));
                        match command {
                            Command::Tech => {
                                let tech = candidate.begin_tech(
                                    battle,
                                    self.tech_character,
                                    input.connected,
                                )?;
                                Some(Page::Tech(Box::new(tech)))
                            }
                            Command::Unison => {
                                let unison =
                                    candidate.begin_unison(battle, self.unison_character)?;
                                Some(Page::Unison(unison))
                            }
                            Command::Strategy => {
                                candidate.begin_strategy(battle)?;
                                Some(Page::Strategy(crate::menu::strategy::Strategy::opening()))
                            }
                            Command::Equipment => {
                                let (equipment, character) =
                                    candidate.begin_equipment(battle, self.equipment_character)?;
                                self.equipment_character = character;
                                Some(Page::Equipment(equipment))
                            }
                            Command::Items => {
                                let inventory = candidate.items();
                                let items =
                                    items::Owner::new(&mut self.item_memory, &inventory, battle)?;
                                Some(Page::Items(items))
                            }
                            Command::Escape => {
                                self.escape_requested =
                                    candidate.toggle_escape(battle, session.admission.actor)?;
                                if !self.escape_requested {
                                    *result.events.last_mut().unwrap() = Event::Cue(3);
                                }
                                None
                            }
                        }
                    }
                }
            }
            Page::Tech(mut tech) => {
                use crate::menu::techniques::TargetKind;

                tech.connected = input.connected;
                let visit = candidate.step_tech(
                    battle,
                    &mut tech,
                    session.admission.actor,
                    input.shared_menu,
                )?;
                result.events.extend(visit.cue.map(Event::Cue));
                match visit.exit {
                    Some(exit) => {
                        self.tech_character = tech.state.character;
                        match (exit.technique, exit.target_kind) {
                            (Some(technique), Some(TargetKind::Enemy)) => {
                                let actor = candidate.tech_target_actor(battle, exit.member)?;
                                let selected = battle.select_target_step(
                                    session.admission.actor,
                                    battle.target(session.admission.actor),
                                    0,
                                );
                                battle.project_command_target(
                                    selected.map(|target| (session.admission.actor, target)),
                                )?;
                                Some(Page::TechTarget(TechTarget {
                                    actor,
                                    issuer: session.admission.actor,
                                    technique: TechniqueId(technique),
                                    selected,
                                }))
                            }
                            (Some(_), Some(TargetKind::SelfTarget)) => None,
                            _ => Some(Page::Strip),
                        }
                    }
                    None => Some(Page::Tech(tech)),
                }
            }
            Page::TechTarget(mut target) => {
                let owns_input = input.controller == session.admission.controller;
                let previous = target.selected;
                target.selected = battle.select_target_step(
                    target.issuer,
                    target.selected,
                    if owns_input { input.step } else { 0 },
                );
                if owns_input
                    && input.step != 0
                    && target.selected != previous
                    && target.selected.is_some()
                {
                    result.events.push(Event::Cue(super::audio::TARGET_CHANGED));
                }
                let queued = owns_input
                    && input.confirm_a.pressed
                    && candidate.queue_tech_target(battle, target)?;
                if owns_input && (queued || input.cancel_b.pressed || input.cancel_y.pressed) {
                    battle.project_command_target(None)?;
                    result.events.push(Event::Cue(if queued { 2 } else { 3 }));
                    (!queued).then_some(Page::Strip)
                } else {
                    battle.project_command_target(
                        target.selected.map(|selected| (target.issuer, selected)),
                    )?;
                    result.paused = target.selected.is_none();
                    if owns_input && input.confirm_a.pressed {
                        result.events.push(Event::Cue(4));
                    }
                    Some(Page::TechTarget(target))
                }
            }
            Page::Unison(mut unison) => {
                let visit = candidate.step_unison(battle, &mut unison, input.shared_menu)?;
                if visit.closed {
                    self.unison_character = unison.character;
                    self.tech_character = unison.character;
                }
                result.events.extend(visit.cue.map(Event::Cue));
                Some(if visit.closed {
                    Page::Strip
                } else {
                    Page::Unison(unison)
                })
            }
            Page::Equipment(mut equipment) => {
                let visit = candidate.step_equipment(
                    battle,
                    &mut equipment,
                    &mut self.equipment_character,
                    input.shared_menu,
                )?;
                result.events.extend(visit.cue.map(Event::Cue));
                Some(if visit.closed {
                    Page::Strip
                } else {
                    Page::Equipment(equipment)
                })
            }
            Page::Strategy(mut strategy) => {
                let visit = candidate.step_strategy(&mut strategy, input.shared_menu)?;
                result.events.extend(visit.cue.map(Event::Cue));
                if visit.closed {
                    candidate.finish_strategy(battle)?;
                }
                Some(if visit.closed {
                    Page::Strip
                } else {
                    Page::Strategy(strategy)
                })
            }
            Page::Items(mut items) => {
                let inventory = candidate.items();
                let owned_input = if input.controller == session.admission.controller
                    || items.input_kind() == InputKind::SharedMenu
                {
                    input
                } else {
                    Input::default()
                };
                let visit = items.step(
                    owned_input,
                    session.admission.actor,
                    &mut self.item_memory,
                    &inventory,
                    battle,
                )?;
                result.events.extend(visit.cues.into_iter().map(Event::Cue));
                match visit.outcome {
                    items::Outcome::Stay => Some(Page::Items(items)),
                    items::Outcome::Strip => Some(Page::Strip),
                    items::Outcome::Close => None,
                }
            }
        };
        if let Some(page) = next {
            if page.input_kind() == InputKind::SharedMenu && !was_shared {
                result.events.push(Event::CaptureBackdrop);
            }
            session.page = page;
            self.session = Some(session);
        } else {
            result.events.push(Event::VoiceStreamsPaused(false));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{Actor, PreparedBattle};
    fn actor(side: Side) -> Actor {
        Actor {
            hp: 100,
            tp: 20,
            ..Actor::new(
                side,
                Default::default(),
                resonance_battle::EquipmentAttributes {
                    normal_combo_limit: 3,
                    ..resonance_battle::EquipmentAttributes::new(100, 30, Default::default())
                },
            )
        }
    }

    pub(super) fn battle() -> (Battle, Setup) {
        let mut actor = actor(Side::Party);
        actor.control = Control::SemiAuto;
        let prepared =
            PreparedBattle::new(vec![(actor, Default::default())], Default::default(), 1).unwrap();
        let id = prepared.actor_ids().next().unwrap();
        (
            prepared.finish().unwrap(),
            Setup {
                actors: vec![id],
                enabled: 0xdd,
                coliseum: false,
            },
        )
    }
    #[test]
    fn commands_wrap_and_admit_only_the_owner_and_enabled_choice() {
        assert_eq!(Command::Tech.next(-1), Command::Escape);
        assert_eq!(Command::Escape.next(1), Command::Tech);
        let pressed = ButtonInput {
            pressed: true,
            held: true,
            released: false,
        };
        for command in Command::ALL {
            let input = Input {
                controller: ControllerPort::Three,
                confirm_a: pressed,
                cancel_b: pressed,
                ..Default::default()
            };
            assert_eq!(
                command.intent(input, ControllerPort::Three, command.mask()),
                StripIntent::Choose(command)
            );
            assert_eq!(
                command.intent(input, ControllerPort::Three, 0),
                StripIntent::Close
            );
            assert_eq!(
                command.intent(input, ControllerPort::Two, command.mask()),
                StripIntent::Stay
            );
        }
    }
    #[test]
    fn story_and_encounter_gates_are_independent() {
        assert_eq!(enabled_rows(true, 2500) & Command::Escape.mask(), 0);
        assert_eq!(enabled_rows(false, 2500) & Command::Unison.mask(), 0);
        assert_ne!(
            enabled_rows(false, UNISON_UNLOCK_STORY) & Command::Unison.mask(),
            0
        );
        assert_ne!(enabled_rows(false, 2500) & Command::Escape.mask(), 0);
    }
    #[test]
    fn admission_uses_live_control_and_allows_defeated_party_members() {
        let mut ko = actor(Side::Party);
        ko.hp = 0;
        ko.control = Control::SemiAuto;
        let prepared =
            PreparedBattle::new(vec![(ko, Default::default())], Default::default(), 1).unwrap();
        let id = prepared.actor_ids().next().unwrap();
        let setup = Setup {
            actors: vec![id],
            enabled: 0xdd,
            coliseum: false,
        };
        let mut battle = prepared.finish().unwrap();
        assert!(setup.admission(&battle, ControllerPort::Two).is_none());
        assert!(setup.admission(&battle, ControllerPort::One).is_some());
        battle.recognize_result();
        assert!(setup.admission(&battle, ControllerPort::One).is_none());
    }

    #[test]
    fn nonzero_controller_slot_admits_manual_and_semi_auto_only() {
        for (control, allowed) in [
            (Control::Manual, true),
            (Control::SemiAuto, true),
            (Control::Auto, false),
        ] {
            let mut party = actor(Side::Party);
            party.control = control;
            party.control_slot = resonance_battle::ControllerPort::Two;
            let prepared =
                PreparedBattle::new(vec![(party, Default::default())], Default::default(), 1)
                    .unwrap();
            let id = prepared.actor_ids().next().unwrap();
            let setup = Setup {
                actors: vec![id],
                enabled: 0xdd,
                coliseum: false,
            };
            let battle = prepared.finish().unwrap();
            assert_eq!(
                setup.admission(&battle, ControllerPort::Two).is_some(),
                allowed
            );
            assert!(setup.admission(&battle, ControllerPort::One).is_none());
        }
    }

    #[test]
    fn coliseum_equipment_deadline_excludes_paused_updates() {
        let (mut battle, mut setup) = battle();
        setup.coliseum = true;
        let equipment_enabled = |battle: &Battle, setup: &Setup| {
            setup
                .admission(battle, ControllerPort::One)
                .unwrap()
                .enabled
                & Command::Equipment.mask()
                != 0
        };
        for _ in 1..COLISEUM_EQUIPMENT_CHANGE_TICKS {
            battle.step(Default::default()).unwrap();
        }
        assert!(equipment_enabled(&battle, &setup));
        for _ in 0..60 {
            battle
                .step(resonance_battle::BattleInput {
                    paused: true,
                    ..Default::default()
                })
                .unwrap();
        }
        assert!(equipment_enabled(&battle, &setup));
        battle.step(Default::default()).unwrap();
        assert!(!equipment_enabled(&battle, &setup));
        setup.coliseum = false;
        assert!(equipment_enabled(&battle, &setup));
    }
}
