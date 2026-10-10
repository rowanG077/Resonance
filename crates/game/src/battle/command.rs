use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, Battle, ButtonInput, Control, Side};
mod items;
pub use items::{ActorSelection, InventoryInput, ItemRow, ListFrame};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Input {
    pub controller: u8,
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
    pub technique: u16,
    pub selected: Option<ActorId>,
}

const UNISON_UNLOCK_STORY: i32 = 1_403_000;

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

/// Formation order and capability are prepared once; actor eligibility is live.
#[derive(Debug, Clone)]
pub struct Setup {
    actors: Vec<ActorId>,
    enabled: u8,
}
impl Setup {
    pub fn new(
        actors: Vec<ActorId>,
        enabled: u8,
        roster: &[resonance_battle::Actor],
    ) -> Result<Self> {
        ensure!(
            (1..=resonance_battle::PARTY_CAPACITY).contains(&actors.len()),
            "invalid command roster size"
        );
        for (slot, &id) in actors.iter().enumerate() {
            let actor = roster.get(id.index()).context("command actor is absent")?;
            ensure!(
                actor.side == Side::Party
                    && actor.control_slot < 4
                    && !actors[..slot].contains(&id),
                "invalid command party actor"
            );
        }
        Ok(Self { actors, enabled })
    }

    pub fn admission(&self, battle: &Battle, controller: u8) -> Result<Option<Admission>> {
        ensure!(
            controller < 4,
            "invalid command controller slot {controller}"
        );
        if !battle.command_admission_allowed() {
            return Ok(None);
        }
        Ok(self.actors.iter().find_map(|&id| {
            let actor = battle.actors().get(id.index())?;
            let slot = actor.control_slot;
            if slot != controller
                || matches!(actor.control, Control::Auto | Control::Enemy) && slot != 0
            {
                return None;
            }
            Some(Admission {
                actor: id,
                controller: slot,
                enabled: self.enabled,
            })
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admission {
    pub actor: ActorId,
    pub controller: u8,
    pub enabled: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub actor: ActorId,
    pub selected: Command,
    /// Static command capabilities projected through the sampled item lock.
    pub enabled: u8,
    pub controller: u8,
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
    fn intent(self, input: Input, controller: u8, enabled: u8) -> StripIntent {
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

    pub fn controller(&self) -> Option<u8> {
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
                tech.connected = input.connected;
                let visit = candidate.step_tech(
                    battle,
                    &mut tech,
                    session.admission.actor,
                    input.shared_menu,
                )?;
                if visit.exit.is_some() {
                    self.tech_character = tech.state.character;
                }
                result.events.extend(visit.cue.map(Event::Cue));
                match visit.exit {
                    Some(exit)
                        if exit.technique.is_some()
                            && exit.target_kind
                                == Some(crate::menu::techniques::TargetKind::Enemy) =>
                    {
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
                            technique: exit.technique.unwrap(),
                            selected,
                        }))
                    }
                    Some(exit)
                        if exit.technique.is_some()
                            && exit.target_kind
                                == Some(crate::menu::techniques::TargetKind::SelfTarget) =>
                    {
                        None
                    }
                    Some(_) => Some(Page::Strip),
                    None => Some(Page::Tech(tech)),
                }
            }
            Page::TechTarget(mut target) => {
                let owns_input = input.controller == session.admission.controller;
                target.selected = battle.select_target_step(
                    target.issuer,
                    target.selected,
                    if owns_input { input.step } else { 0 },
                );
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
    use resonance_battle::{Actor, ActorAvailability, PreparedBattle};
    fn actor(side: Side) -> Actor {
        Actor {
            side,
            species: 0,
            equipment: resonance_battle::EquipmentAttributes {
                max_hp: 100,
                max_tp: 30,
                tp_cost_reduction: false,
                quick_escape: false,
                taunt_enabled: false,
                taunt_guard: false,
                taunt_cancel: false,
                control_ex: Default::default(),
                quick_turn: false,
                backstep_guard: false,
                casting: Default::default(),
                dagger_reach: false,
                contact: Default::default(),
                normal_combo_limit: 3,
                luck: 0,
                stats: Default::default(),
                affinities: [resonance_battle::Affinity::Normal; 9],
                damage: Default::default(),
                recovery: Default::default(),
                base_element: None,
                combo_traits: Default::default(),
                normal_guard: false,
                speed_multiplier: 1.,
                reaction_ex: Default::default(),
                stun_ex_bonus: false,
                spell_revenge: false,
            },
            control: Default::default(),
            availability: ActorAvailability::Active,
            guard: Default::default(),
            hp: 100,
            tp: 20,
            control_ex_state: Default::default(),
            casting_state: Default::default(),
            stored_spell: None,
            control_slot: 0,
            overlimit: Default::default(),
            proficiency: 0,
            input: Default::default(),
            elements: Default::default(),
            attack_power: 100,
            conditions: Default::default(),
            position: [0.; 3],
            heading: 0.,
            facing_direction: [0., 0., 1.],
            effect_scale: 1.,
            body: Default::default(),
            movement: Default::default(),
            reaction: Default::default(),
            hit_stop: 0,
            time_stop: 0,
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
                controller: 2,
                confirm_a: pressed,
                cancel_b: pressed,
                ..Default::default()
            };
            assert_eq!(
                command.intent(input, 2, command.mask()),
                StripIntent::Choose(command)
            );
            assert_eq!(command.intent(input, 2, 0), StripIntent::Close);
            assert_eq!(command.intent(input, 1, command.mask()), StripIntent::Stay);
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
        };
        let mut battle = prepared.finish().unwrap();
        assert!(setup.admission(&battle, 1).unwrap().is_none());
        assert!(setup.admission(&battle, 0).unwrap().is_some());
        battle.recognize_result();
        assert!(setup.admission(&battle, 0).unwrap().is_none());
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
            party.control_slot = 1;
            let prepared =
                PreparedBattle::new(vec![(party, Default::default())], Default::default(), 1)
                    .unwrap();
            let id = prepared.actor_ids().next().unwrap();
            let setup = Setup {
                actors: vec![id],
                enabled: 0xdd,
            };
            let battle = prepared.finish().unwrap();
            assert_eq!(setup.admission(&battle, 1).unwrap().is_some(), allowed);
            assert!(setup.admission(&battle, 0).unwrap().is_none());
        }
    }

    #[test]
    fn command_setup_rejects_invalid_rosters_before_admission() {
        let (battle, setup) = battle();
        let actor = setup.actors[0];
        assert!(Setup::new(vec![], 0xff, battle.actors()).is_err());
        assert!(Setup::new(vec![actor, actor], 0xff, battle.actors()).is_err());
        assert!(Setup::new(vec![actor], 0xff, &[]).is_err());
        let mut enemy = battle.actors()[0].clone();
        enemy.side = Side::Enemy;
        assert!(Setup::new(vec![actor], 0xff, &[enemy]).is_err());
        let prepared = Setup::new(vec![actor], 0xff, battle.actors()).unwrap();
        assert!(prepared.admission(&battle, 4).is_err());
        assert!(prepared.admission(&battle, 0).unwrap().is_some());
    }
}
