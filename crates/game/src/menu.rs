//! Player menus own input while the field remains at a controllable checkpoint.
use crate::{DirectionRepeat, field::FieldCheckpoint};
use std::sync::Arc;
pub mod collection;
pub mod cooking;
pub mod customize;
pub mod equipment;
pub mod ex_skills;
pub mod figurines;
pub mod items;
pub mod manual;
pub mod monsters;
mod party;
pub mod preview;
pub mod rename;
pub mod status;
pub mod strategy;
pub mod synopsis;
pub mod techniques;
pub mod unison;
pub mod world_map;

pub struct Resources {
    pub session: Arc<resonance_content::session::SessionData>,
    pub data: Arc<resonance_content::menu_data::MenuData>,
}

pub const SLOTS_PER_BANK: usize = 127;
pub const VISIBLE_SLOTS: usize = 6;
pub const VISIBLE_PARTY: usize = 4;
pub const MAIN_COLUMNS: usize = 5;
const MAIN_SLIDE_STEP: u8 = 25;
const SYSTEM_SLIDE_STEP: u8 = 32;
const DESCRIPTION_FADE_START: u8 = 240;

/// Return the incoming text opacity before advancing the crossfade.
fn fade_description(fade: &mut u8, changed: bool) -> u8 {
    if *fade == 0 && changed {
        *fade = DESCRIPTION_FADE_START;
    }
    let opacity = 255 - *fade;
    *fade = fade.saturating_sub(16);
    opacity
}

/// A submenu's slide is independent of the retained Main-menu backdrop.
#[derive(Debug, Default, serde::Serialize)]
pub struct Transition {
    pub page_fade: u8,
    pub page_closing: bool,
}
impl Transition {
    pub fn animating(&self) -> bool {
        self.page_fade != 0 || self.page_closing
    }
    fn opening() -> Self {
        Self {
            page_fade: 231 - MAIN_SLIDE_STEP,
            page_closing: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Save,
    Load,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotFocus {
    Bank,
    List,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterMenu {
    Status,
    Equip,
    Tech,
    ExSkills,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Main,
    Party,
    Character(CharacterMenu),
    Status,
    Equip,
    Tech,
    ExSkills,
    Titles,
    Rename,
    Unison,
    Strategy,
    Synopsis,
    Cooking,
    Customize,
    Items,
    Collection,
    WorldMap,
    Monsters,
    Figurines,
    Manual,
    System,
    Slots(Mode),
}
/// Authored order shared by navigation and the menu renderer.
pub const MAIN_ENTRIES: [(Page, &str); 10] = [
    (Page::Tech, "tech"),
    (Page::Unison, "unison"),
    (Page::Strategy, "strategy"),
    (Page::Status, "status"),
    (Page::Synopsis, "synopsis"),
    (Page::Items, "items"),
    (Page::ExSkills, "ex_skill"),
    (Page::Equip, "equip"),
    (Page::Cooking, "cooking"),
    (Page::System, "system"),
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    ReadSlots,
    Save(usize),
    Load(usize),
}
#[derive(Debug, Clone, Default)]
pub enum Slot {
    #[default]
    Empty,
    Saved {
        location: String,
        played_ticks: u64,
        checkpoint: Box<FieldCheckpoint>,
    },
    Invalid(String),
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationKind {
    Save,
    Overwrite,
    Load,
}
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PopupContent {
    Confirmation {
        kind: ConfirmationKind,
        bank: usize,
        yes: bool,
    },
    Notice(String),
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct Popup {
    pub content: PopupContent,
    pub opacity: u8,
}

pub struct Menu {
    pub resources: Option<Arc<Resources>>,
    /// Index into the party's formation, independent of the current list row.
    pub character: usize,
    pub first_character: usize,
    pub swap_character: Option<usize>,
    pub status: status::Status,
    pub party_statistics: bool,
    pub inventory: items::Inventory,
    pub collection: collection::Collection,
    pub world_map: world_map::WorldMap,
    pub monsters: monsters::MonsterList,
    pub figurines: figurines::Figurines,
    pub manual: manual::Manual,
    pub equipment: equipment::Equipment,
    pub tech: techniques::Tech,
    pub ex_skills: ex_skills::ExSkills,
    pub unison: unison::Unison,
    pub strategy: strategy::Strategy,
    pub rename: rename::Rename,
    pub synopsis: synopsis::Synopsis,
    pub cooking: cooking::Cooking,
    pub customize: customize::Customize,
    party_changed: bool,
    pub play_time: crate::clock::PlayTime,
    pub page: Page,
    pub selected: usize,
    pub bank: usize,
    pub slot: usize,
    pub first_slot: usize,
    pub focus: SlotFocus,
    pub slots: Vec<Slot>,
    pub confirmation: Option<bool>,
    pub notice: Option<String>,
    /// Retains the displayed content while a dismissed popup fades out.
    pub popup: Option<Popup>,
    pub busy: bool,
    pub closed: bool,
    pub main_fade: u8,
    pub system_opacity: u8,
    pub system_closing: bool,
    closing: bool,
    entering: Option<Page>,
    returning: bool,
    pub tick: u32,
    pub checkpoint: Option<FieldCheckpoint>,
    pub at_save_point: bool,
    command: Option<Command>,
    direct: bool,
    held: [bool; 6],
    repeat: [DirectionRepeat; 6],
}
impl Menu {
    fn select_main(&mut self, page: Page) {
        self.selected = MAIN_ENTRIES
            .iter()
            .position(|&(entry, _)| entry == page)
            .expect("page has no Main-menu entry");
    }

    pub fn main_entry_available(&self, page: Page) -> bool {
        match page {
            Page::System => true,
            Page::Synopsis => self.has_synopsis(),
            Page::ExSkills => self.has_ex_skills(),
            Page::Unison => self.has_unison(),
            _ => self.resources.is_some(),
        }
    }

    pub fn new(page: Page, checkpoint: Option<FieldCheckpoint>, at_save_point: bool) -> Self {
        let direct = matches!(page, Page::Slots(_));
        Self {
            resources: None,
            character: 0,
            first_character: 0,
            swap_character: None,
            status: Default::default(),
            party_statistics: false,
            inventory: Default::default(),
            collection: Default::default(),
            world_map: Default::default(),
            monsters: Default::default(),
            figurines: Default::default(),
            manual: Default::default(),
            equipment: Default::default(),
            tech: Default::default(),
            ex_skills: Default::default(),
            unison: Default::default(),
            strategy: Default::default(),
            rename: Default::default(),
            synopsis: Default::default(),
            cooking: Default::default(),
            customize: Default::default(),
            party_changed: false,
            play_time: Default::default(),
            page,
            selected: 0,
            bank: 0,
            slot: 0,
            first_slot: 0,
            focus: SlotFocus::Bank,
            slots: vec![Slot::Empty; SLOTS_PER_BANK * 2],
            confirmation: None,
            notice: None,
            popup: None,
            busy: direct,
            closed: false,
            main_fade: 0,
            system_opacity: if page == Page::System { 255 } else { 0 },
            system_closing: false,
            closing: false,
            entering: None,
            returning: false,
            tick: 0,
            checkpoint,
            at_save_point,
            command: direct.then_some(Command::ReadSlots),
            direct,
            held: [false; 6],
            repeat: Default::default(),
        }
    }
    pub fn index(&self) -> usize {
        self.bank * SLOTS_PER_BANK + self.slot
    }
    pub fn begin_opening(&mut self) {
        if self.page == Page::Main {
            // Entry advances once before presenting the first sliding pose.
            self.main_fade = 231 - MAIN_SLIDE_STEP;
        }
    }
    pub fn main_animating(&self) -> bool {
        self.main_fade != 0
            || self.closing
            || self.entering.is_some()
            || self.page == Page::System && (self.system_opacity != 255 || self.system_closing)
    }
    pub fn background_fade(&self) -> u8 {
        if self.entering.is_some() || self.returning {
            0
        } else {
            self.main_fade
        }
    }
    pub fn foreground_fade(&self) -> u8 {
        match self.page {
            Page::Synopsis => self.synopsis.transition.page_fade,
            Page::Strategy => self.strategy.transition.page_fade,
            _ => self.main_fade,
        }
    }
    fn advance_submenu_slide(&mut self) -> bool {
        let transition = match self.page {
            Page::Synopsis => &mut self.synopsis.transition,
            Page::Strategy => &mut self.strategy.transition,
            Page::Equip => &mut self.equipment.transition,
            Page::Tech => &mut self.tech.transition,
            Page::Unison => &mut self.unison.transition,
            Page::Cooking => &mut self.cooking.transition,
            Page::ExSkills => &mut self.ex_skills.transition,
            Page::Customize => &mut self.customize.transition,
            _ => return false,
        };
        let Transition {
            page_fade: fade,
            page_closing: closing,
        } = transition;
        if *closing {
            if *fade == 255 {
                *closing = false;
                self.return_to_main();
            } else {
                *fade = fade.saturating_add(MAIN_SLIDE_STEP);
            }
            return true;
        }
        *fade = fade.saturating_sub(MAIN_SLIDE_STEP);
        *fade != 0
    }
    fn return_to_main(&mut self) {
        self.page = Page::Main;
        self.clamp_party_view();
        self.returning = true;
        self.begin_opening();
    }
    fn advance_system(&mut self) -> bool {
        if self.page != Page::System {
            return false;
        }
        if self.system_closing {
            self.system_opacity = self.system_opacity.saturating_sub(SYSTEM_SLIDE_STEP);
            if self.system_opacity == 0 {
                self.system_closing = false;
                self.page = Page::Main;
                self.select_main(Page::System);
                return false;
            }
        } else {
            self.system_opacity = self.system_opacity.saturating_add(SYSTEM_SLIDE_STEP);
            if self.system_opacity == 255 {
                return false;
            }
        }
        true
    }
    pub(crate) fn take_party_changes(
        &mut self,
    ) -> Option<(
        resonance_events::party::Party,
        resonance_events::GameplayRandom,
    )> {
        std::mem::take(&mut self.party_changed).then(|| {
            let progress = &self.checkpoint.as_ref().unwrap().progress;
            (progress.party.clone(), progress.gameplay_random.clone())
        })
    }
    pub(crate) fn set_play_time(&mut self, time: crate::clock::PlayTime) {
        self.play_time = time;
        if let Some(checkpoint) = &mut self.checkpoint {
            checkpoint.played_ticks = Some(time.total());
        }
    }
    pub fn take_command(&mut self) -> Option<Command> {
        self.command.take()
    }
    fn party_result(&mut self, result: Result<bool, String>) -> Option<i16> {
        match result {
            Ok(changed) => {
                self.party_changed |= changed;
                Some(2)
            }
            Err(error) => {
                self.notice = Some(error);
                Some(4)
            }
        }
    }
    pub fn finish(&mut self, notice: Option<String>) {
        self.busy = false;
        self.notice = notice;
        self.step_popup();
    }
    /// Returns a menu cue ID. Selection repeats on the menu clock, while field
    /// scripts, actor animations and camera motion remain paused.
    pub fn step(&mut self, input: crate::field::FieldInput) -> Option<i16> {
        if self.busy && matches!(self.page, Page::Monsters | Page::Figurines) {
            return None;
        }
        self.tick = self.tick.wrapping_add(1);
        if self.advance_system() {
            return None;
        }
        if let Some(page) = self.entering {
            if self.main_fade == 255 {
                self.page = page;
                self.entering = None;
                self.main_fade = 0;
                match page {
                    Page::Status => self.animate_status_portrait(),
                    Page::Items => self.fade_item_description(),
                    Page::Equip => self.fade_equipment_description(),
                    Page::Tech => self.fade_tech_description(),
                    Page::Unison => self.fade_unison_description(),
                    Page::ExSkills => {
                        self.remember_ex_description();
                        self.fade_ex_description();
                    }
                    Page::Cooking => self.fade_cooking_description(),
                    Page::Customize => self.step_customize_preview(),
                    Page::Slots(_) => {
                        self.command = Some(Command::ReadSlots);
                        self.focus = SlotFocus::Bank;
                        self.busy = true;
                    }
                    _ => {}
                }
            } else {
                self.main_fade = self.main_fade.saturating_add(MAIN_SLIDE_STEP);
            }
            return None;
        }
        if self.closing {
            self.main_fade = self.main_fade.saturating_add(MAIN_SLIDE_STEP);
            self.closed = self.main_fade == 255;
            return None;
        }
        self.main_fade = self.main_fade.saturating_sub(MAIN_SLIDE_STEP);
        if self.main_fade != 0 {
            return None;
        }
        self.returning = false;
        let page = self.page;
        match page {
            Page::Rename => self.advance_rename(),
            Page::Tech => {
                self.remember_tech_description();
                self.step_tech_preview();
                self.fade_tech_target();
            }
            Page::Cooking => self.remember_cooking_description(),
            Page::ExSkills => self.remember_ex_description(),
            Page::Unison => self.remember_unison_description(),
            Page::Equip => self.remember_equipment_description(),
            Page::Items => {
                self.remember_item_description();
                self.fade_item_target();
                if self.advance_item_page() {
                    return None;
                }
            }
            Page::Strategy => self.remember_strategy_description(),
            Page::Collection if self.advance_collection() => return None,
            Page::Manual if self.advance_manual() => return None,
            Page::WorldMap if self.advance_world_map() => return None,
            Page::Monsters | Page::Figurines if self.advance_catalogue() => return None,
            Page::Status | Page::Titles if self.advance_status() => return None,
            _ => {}
        }
        let cue = self.step_input(input);
        self.step_catalogue_animation();
        match self.page {
            Page::Items => {
                self.step_item_preview();
                self.fade_item_description();
            }
            Page::Collection => self.fade_collection_description(),
            Page::WorldMap => self.fade_world_map_description(),
            Page::Strategy => self.fade_strategy_description(),
            Page::Equip => self.fade_equipment_description(),
            Page::Tech => self.fade_tech_description(),
            Page::Cooking => self.fade_cooking_description(),
            Page::ExSkills => self.fade_ex_description(),
            Page::Unison => self.fade_unison_description(),
            Page::Customize => self.step_customize_preview(),
            Page::Status | Page::Titles => self.animate_status_portrait(),
            _ => {}
        }
        self.clamp_party_view();
        self.step_popup();
        cue
    }

    fn step_popup(&mut self) {
        const FADE_STEP: u8 = 32;
        let content = if let Some(yes) = self.confirmation {
            Some(PopupContent::Confirmation {
                kind: match self.page {
                    Page::Slots(Mode::Load) => ConfirmationKind::Load,
                    _ if matches!(self.slots[self.index()], Slot::Empty) => ConfirmationKind::Save,
                    _ => ConfirmationKind::Overwrite,
                },
                bank: self.bank,
                yes,
            })
        } else {
            self.notice.clone().map(PopupContent::Notice)
        };
        if let Some(content) = content {
            let opacity = self
                .popup
                .as_ref()
                .map_or(0, |p| p.opacity)
                .saturating_add(FADE_STEP);
            self.popup = Some(Popup { content, opacity });
        } else if let Some(popup) = &mut self.popup {
            popup.opacity = popup.opacity.saturating_sub(FADE_STEP);
            if popup.opacity == 0 {
                self.popup = None;
            }
        }
    }

    fn step_input(&mut self, mut input: crate::field::FieldInput) -> Option<i16> {
        let held = [
            input.direction[0] < -0.5,
            input.direction[0] > 0.5,
            input.direction[1] > 0.5,
            input.direction[1] < -0.5,
            input.scroll_direction > 0,
            input.scroll_direction < 0,
        ];
        let [left, right, up, down, page_up, page_down] = std::array::from_fn(|i| {
            self.repeat[i].step(held[i], held[i] && !self.held[i], self.tick)
        });
        self.held = held;
        if matches!(self.page, Page::Status | Page::Cooking) {
            input.previous_page |= page_up;
            input.next_page |= page_down;
        }
        if self.busy || self.closed {
            return None;
        }
        if self.advance_submenu_slide() {
            return None;
        }
        if self.page == Page::Rename {
            return self.step_rename(input, [left, right, up, down]);
        }
        if self.page == Page::Items
            && (self.inventory.page_closing || self.inventory.page_fade != 0)
        {
            return None;
        }
        if self.page == Page::Collection
            && (self.collection.page_closing || self.collection.page_fade != 0)
        {
            return None;
        }
        if self.page == Page::Manual && (self.manual.page_closing || self.manual.page_fade != 0) {
            return None;
        }
        if matches!(self.page, Page::Status | Page::Titles) && self.status.animating() {
            return None;
        }
        if self.notice.is_some() {
            if input.interact || input.cancel {
                self.notice = None;
                return Some(3);
            }
            return None;
        }
        if input.start
            && matches!(
                self.page,
                Page::Main | Page::Party | Page::Character(_) | Page::System
            )
        {
            self.party_statistics = !self.party_statistics;
            return Some(1);
        }
        if let Some(yes) = &mut self.confirmation {
            if input.cancel {
                self.confirmation = None;
                return Some(3);
            }
            if input.interact {
                let cue = if *yes { 2 } else { 3 };
                if *yes {
                    self.command = Some(match self.page {
                        Page::Slots(Mode::Save) => Command::Save(self.index()),
                        Page::Slots(Mode::Load) => Command::Load(self.index()),
                        _ => unreachable!("confirmation requires a slot page"),
                    });
                    self.busy = true;
                }
                self.confirmation = None;
                return Some(cue);
            }
            if up && !*yes || down && *yes {
                *yes = up;
                return Some(1);
            }
            return None;
        }
        let directions = [left, right, up, down, page_up, page_down];
        match self.page {
            Page::Items => return self.step_items(input, directions),
            Page::Collection => return self.step_collection(input, directions),
            Page::WorldMap => return self.step_world_map(input, [up, down, page_up, page_down]),
            Page::Monsters => return self.step_monsters(input, directions),
            Page::Figurines => return self.step_figurines(input, [up, down, page_up, page_down]),
            Page::Manual => return self.step_manual(input, [up, down, page_up, page_down]),
            Page::Equip => return self.step_equipment(input, directions),
            Page::Tech => return self.step_techniques(input, directions),
            Page::Unison => return self.step_unison(input, directions),
            Page::ExSkills => return self.step_ex_skills(input, directions),
            Page::Strategy => return self.step_strategy(input, [left, right, up, down]),
            Page::Synopsis => return self.step_synopsis(input, up, down),
            Page::Cooking => return self.step_cooking(input, [left, right, up, down]),
            Page::Customize => return self.step_customize(input, directions),
            Page::Party => return self.step_party(input, up, down),
            Page::Rename => unreachable!("rename input was already dispatched"),
            Page::Main
            | Page::Character(_)
            | Page::Status
            | Page::Titles
            | Page::System
            | Page::Slots(_) => {}
        }
        if input.cancel || input.menu {
            match self.page {
                Page::Main => self.closing = true,
                Page::Character(_) => self.page = Page::Main,
                Page::Status => {
                    self.status.closing = true;
                }
                Page::Titles => self.status.title_closing = true,
                Page::System => {
                    self.system_closing = true;
                }
                Page::Slots(_) if self.focus == SlotFocus::List => self.focus = SlotFocus::Bank,
                Page::Slots(_) if self.direct => self.closed = true,
                Page::Slots(_) => {
                    self.select_main(Page::System);
                    self.return_to_main();
                }
                _ => unreachable!("submenu input was already dispatched"),
            }
            return Some(3);
        }
        let navigated = left || right || up || down;
        match self.page {
            Page::Main => {
                let row = self.selected / MAIN_COLUMNS;
                let col = self.selected % MAIN_COLUMNS;
                if down && row == 1 && self.checkpoint.is_some() {
                    self.page = Page::Party;
                    self.swap_character = None;
                    return Some(1);
                }
                if let Some(cue) = self.page_party(input) {
                    return Some(cue);
                }
                self.selected = if up || down {
                    (1 - row) * MAIN_COLUMNS + col
                } else if left {
                    (self.selected + MAIN_ENTRIES.len() - 1) % MAIN_ENTRIES.len()
                } else if right {
                    (self.selected + 1) % MAIN_ENTRIES.len()
                } else {
                    self.selected
                };
                if input.interact {
                    let available = self.resources.is_some() && self.checkpoint.is_some();
                    match MAIN_ENTRIES[self.selected].0 {
                        Page::Unison if self.has_unison() => self.open_unison(),
                        Page::Cooking if available => self.open_cooking(),
                        Page::Synopsis if self.has_synopsis() => {
                            self.entering = Some(Page::Synopsis);
                            self.synopsis = synopsis::Synopsis {
                                row: self.synopsis.row,
                                first: self.synopsis.first,
                                transition: Transition::opening(),
                                ..Default::default()
                            };
                        }
                        Page::Strategy if available => {
                            self.entering = Some(Page::Strategy);
                            self.strategy = strategy::Strategy {
                                transition: Transition::opening(),
                                ..Default::default()
                            };
                        }
                        Page::Items if available => {
                            self.open_items();
                            self.inventory.focus = items::Focus::List;
                            self.entering = Some(Page::Items);
                            self.inventory.clamp(self.inventory_items().len());
                        }
                        page @ (Page::Tech | Page::Status | Page::ExSkills | Page::Equip)
                            if available && self.main_entry_available(page) =>
                        {
                            self.page = Page::Character(match page {
                                Page::Tech => CharacterMenu::Tech,
                                Page::Status => CharacterMenu::Status,
                                Page::ExSkills => CharacterMenu::ExSkills,
                                _ => CharacterMenu::Equip,
                            });
                        }
                        Page::System => {
                            self.page = Page::System;
                            self.system_opacity = 0;
                            self.system_closing = false;
                            self.selected = 0;
                        }
                        _ => return Some(4),
                    }
                    return Some(2);
                }
            }
            Page::Character(destination) => {
                let cue = self.move_party_cursor(input, up, down);
                if input.interact {
                    if matches!(destination, CharacterMenu::Tech | CharacterMenu::Equip)
                        && self.member().knocked_out()
                    {
                        return Some(4);
                    }
                    self.entering = Some(match destination {
                        CharacterMenu::ExSkills => {
                            self.ex_skills = ex_skills::ExSkills::opening();
                            Page::ExSkills
                        }
                        CharacterMenu::Tech => {
                            self.tech = techniques::Tech::opening();
                            self.reset_tech_focus();
                            Page::Tech
                        }
                        CharacterMenu::Status => {
                            self.status = status::Status::opening();
                            Page::Status
                        }
                        CharacterMenu::Equip => {
                            let by_parameter = self.equipment.by_parameter;
                            self.equipment = equipment::Equipment::opening();
                            self.equipment.by_parameter = by_parameter;
                            Page::Equip
                        }
                    });
                    return Some(2);
                }
                return cue;
            }
            Page::Status | Page::Titles => {
                return self.step_status(input, [left, right, up, down]);
            }
            Page::System => {
                if up {
                    self.selected = (self.selected + 2) % 3;
                }
                if down {
                    self.selected = (self.selected + 1) % 3;
                }
                if input.interact
                    && self.selected == 2
                    && self.resources.is_some()
                    && self.checkpoint.is_some()
                {
                    self.open_customize();
                    self.system_closing = true;
                    return Some(2);
                }
                if input.interact
                    && (self.selected == 1 || self.selected == 0 && self.at_save_point)
                {
                    self.entering = Some(Page::Slots(if self.selected == 0 {
                        Mode::Save
                    } else {
                        Mode::Load
                    }));
                    self.system_closing = true;
                    return Some(2);
                }
                if input.interact {
                    return Some(4);
                }
            }
            Page::Slots(mode) => {
                if self.focus == SlotFocus::Bank {
                    if input.interact {
                        self.focus = SlotFocus::List;
                        return Some(2);
                    }
                    if left && self.bank == 1 || right && self.bank == 0 {
                        self.bank = usize::from(right);
                        return Some(1);
                    }
                    return None;
                }
                let previous = self.slot;
                if up {
                    self.slot = self.slot.saturating_sub(1);
                } else if down {
                    self.slot = (self.slot + 1).min(SLOTS_PER_BANK - 1);
                }
                self.first_slot = self
                    .first_slot
                    .min(self.slot)
                    .max(self.slot.saturating_sub(VISIBLE_SLOTS - 1));
                if input.interact {
                    if mode == Mode::Save || matches!(self.slots[self.index()], Slot::Saved { .. })
                    {
                        self.confirmation = Some(
                            mode == Mode::Load || matches!(self.slots[self.index()], Slot::Empty),
                        );
                        return Some(2);
                    }
                    if let Slot::Invalid(error) = &self.slots[self.index()] {
                        self.notice = Some(error.clone());
                    }
                    return Some(4);
                }
                return (self.slot != previous).then_some(1);
            }
            _ => unreachable!("submenu input was already dispatched"),
        }
        navigated.then_some(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::FieldInput;

    #[test]
    fn dismissed_popup_fades_with_its_original_content_and_reopening_preserves_opacity() {
        let mut menu = Menu::new(Page::Slots(Mode::Save), None, true);
        menu.finish(None);
        menu.focus = SlotFocus::List;
        let accept = FieldInput {
            interact: true,
            ..Default::default()
        };
        menu.step(accept);
        assert_eq!(menu.popup.as_ref().unwrap().opacity, 32);
        for _ in 0..7 {
            menu.step(Default::default());
        }
        assert_eq!(menu.popup.as_ref().unwrap().opacity, 255);
        menu.step(FieldInput {
            cancel: true,
            ..Default::default()
        });
        assert!(menu.confirmation.is_none());
        assert_eq!(menu.popup.as_ref().unwrap().opacity, 223);
        menu.slots[0] = Slot::Invalid("Unreadable".into());
        menu.step(Default::default());
        assert!(matches!(
            menu.popup.as_ref().unwrap().content,
            PopupContent::Confirmation {
                kind: ConfirmationKind::Save,
                yes: true,
                ..
            }
        ));
        menu.step(accept);
        let popup = menu.popup.as_ref().unwrap();
        assert_eq!(popup.opacity, 223);
        assert!(matches!(
            popup.content,
            PopupContent::Confirmation {
                kind: ConfirmationKind::Overwrite,
                yes: false,
                ..
            }
        ));
        menu.step(FieldInput {
            cancel: true,
            ..Default::default()
        });
        for _ in 0..6 {
            menu.step(Default::default());
        }
        assert!(menu.popup.is_none());
    }

    #[test]
    fn a_slot_needs_explicit_confirmation_and_a_write_holds_input() {
        let mut menu = Menu::new(Page::Slots(Mode::Save), None, true);
        assert_eq!(menu.take_command(), Some(Command::ReadSlots));
        menu.finish(None);
        let accept = FieldInput {
            interact: true,
            ..Default::default()
        };
        menu.step(accept);
        assert_eq!(menu.focus, SlotFocus::List);
        menu.step(accept);
        assert_eq!(menu.confirmation, Some(true));
        assert!(menu.take_command().is_none());
        menu.step(FieldInput {
            direction: [0., -1.],
            ..Default::default()
        });
        assert_eq!(menu.step(accept), Some(3));
        assert!(menu.take_command().is_none());
        menu.step(accept);
        menu.step(FieldInput {
            direction: [0., -1.],
            ..accept
        });
        assert_eq!(menu.take_command(), Some(Command::Save(0)));
        menu.step(FieldInput {
            cancel: true,
            ..Default::default()
        });
        assert!(!menu.closed);
        menu.finish(Some("Save successful.".into()));
        menu.step(accept);
        assert!(menu.notice.is_none());
        menu.step(FieldInput {
            cancel: true,
            ..Default::default()
        });
        assert!(!menu.closed);
        menu.step(FieldInput {
            cancel: true,
            ..Default::default()
        });
        assert!(menu.closed);
    }

    #[test]
    fn overwriting_an_unreadable_slot_defaults_to_no() {
        let mut menu = Menu::new(Page::Slots(Mode::Save), None, true);
        menu.take_command();
        menu.finish(None);
        menu.slots[0] = Slot::Invalid("Invalid save".into());
        let accept = FieldInput {
            interact: true,
            ..Default::default()
        };
        menu.step(accept);
        menu.step(accept);
        assert_eq!(menu.confirmation, Some(false));
        assert_eq!(
            menu.step(FieldInput {
                direction: [-1., 0.],
                ..Default::default()
            }),
            None
        );
        assert_eq!(menu.step(accept), Some(3));
        assert!(menu.take_command().is_none());
    }

    #[test]
    fn slot_and_choice_navigation_stop_at_their_edges_without_a_cue() {
        let mut menu = Menu::new(Page::Slots(Mode::Save), None, true);
        menu.take_command();
        menu.finish(None);
        let input = |direction| FieldInput {
            direction,
            ..Default::default()
        };
        assert_eq!(menu.step(input([-1., 0.])), None);
        assert_eq!(menu.bank, 0);
        assert_eq!(menu.step(input([1., 0.])), Some(1));
        menu.step(FieldInput::default());
        assert_eq!(menu.step(input([1., 0.])), None);
        menu.focus = SlotFocus::List;
        assert_eq!(menu.step(input([0., 1.])), None);
        assert_eq!(menu.slot, 0);
        menu.slot = SLOTS_PER_BANK - 1;
        assert_eq!(menu.step(input([0., -1.])), None);
        menu.confirmation = Some(true);
        assert_eq!(menu.step(input([0., 1.])), None);
        assert_eq!(menu.confirmation, Some(true));
        assert_eq!(menu.step(input([0., -1.])), Some(1));
        menu.step(FieldInput::default());
        assert_eq!(menu.step(input([0., -1.])), None);
        assert_eq!(menu.confirmation, Some(false));
    }
}
