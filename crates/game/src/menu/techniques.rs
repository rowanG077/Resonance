//! Shared field and battle Tech page. The caller owns the party.
pub use super::Input;
use super::MenuAction;
use anyhow::{Result, ensure};
pub use resonance_battle::TechniqueTarget as TargetKind;
use resonance_battle::{ActorId, Battle, PreparedTechnique};
use resonance_content::{
    menu_data::{MenuData, TechniqueUse},
    session::SessionData,
};
use resonance_events::party::{Member, Party, TechniqueShortcut};

pub const VISIBLE_SHORTCUT_CHOICES: usize = 8;

#[derive(Clone, Copy)]
pub enum Context<'a> {
    Field {
        at_save_point: bool,
        connected: &'a [bool; 4],
    },
    Battle {
        battle: &'a Battle,
        actors: &'a [(ActorId, u8)],
        connected: &'a [bool; 4],
    },
}

impl<'a> Context<'a> {
    fn battle(self) -> bool {
        matches!(self, Self::Battle { .. })
    }
    fn connected(self) -> &'a [bool; 4] {
        match self {
            Self::Field { connected, .. } | Self::Battle { connected, .. } => connected,
        }
    }
    fn prepared(self, member: usize, catalogue: u16) -> Option<(ActorId, &'a PreparedTechnique)> {
        let Self::Battle { battle, actors, .. } = self else {
            return None;
        };
        let actor = actors
            .iter()
            .find(|(_, character)| usize::from(*character) == member + 1)?
            .0;
        Some((actor, battle.prepared_technique(actor, catalogue)?))
    }
}

#[derive(Debug, Clone)]
pub struct BattlePage {
    pub state: Tech,
    pub connected: [bool; 4],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    Character,
    Control,
    #[default]
    Shortcuts,
    List,
    AssistCharacter,
    AssistList,
    Target,
    CannotForget,
    Forget {
        yes: bool,
    },
    Closed {
        technique: Option<u16>,
    },
}
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Tech {
    pub character: usize,
    pub focus: Focus,
    pub unison: bool,
    pub slot: usize,
    pub row: usize,
    pub first: usize,
    pub assist: usize,
    pub target: usize,
    pub return_to: Focus,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Control {
        slot: usize,
        value: u8,
    },
    Shortcut {
        member: usize,
        slot: usize,
        selected: Option<TechniqueShortcut>,
    },
    Enabled {
        member: usize,
        technique: u16,
        enabled: bool,
    },
    FieldForget {
        member: usize,
        technique: u16,
    },
    FieldCast {
        member: usize,
        target: usize,
        technique: u16,
        at_save_point: bool,
    },
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditResult {
    pub changed: bool,
    pub cue: Option<u16>,
}

pub(crate) fn shortcut_selection(
    party: &Party,
    member: usize,
    slot: usize,
) -> Option<TechniqueShortcut> {
    let owner = party.members.get(member)?;
    if slot >= 4 {
        return owner.assist_shortcuts.get(slot - 4).copied().flatten();
    }
    let technique = owner.shortcuts[slot];
    (technique != 0).then_some(TechniqueShortcut {
        character: member,
        technique,
    })
}

pub(crate) fn shortcut_choice(
    choices: &[u16],
    selected: Option<TechniqueShortcut>,
) -> Option<(usize, usize)> {
    if choices.is_empty() {
        return None;
    }
    let row = selected
        .and_then(|selected| choices.iter().position(|&id| id == selected.technique))
        .unwrap_or(0);
    Some((row, row.saturating_sub(VISIBLE_SHORTCUT_CHOICES - 1)))
}

impl Edit {
    /// Field convenience; battle commits through its prevalidated live binding join.
    pub fn apply_field(self, party: &mut Party, data: &MenuData) -> Result<EditResult> {
        let changed = match self {
            Self::Control { slot, value } => {
                ensure!(
                    slot < party.formation.len() && slot < 4 && value < 3,
                    "invalid Tech control edit"
                );
                let old = &mut party.settings.battle_controls[slot];
                let changed = *old != value;
                *old = value;
                changed
            }
            Self::Shortcut {
                member,
                slot,
                selected,
            } => party
                .assign_technique(member, slot, selected)
                .map_err(anyhow::Error::msg)?,
            Self::Enabled {
                member,
                technique,
                enabled,
            } => {
                let member = party
                    .members
                    .get_mut(member)
                    .ok_or_else(|| anyhow::anyhow!("unknown Tech member"))?;
                ensure!(
                    member.techniques.contains(&technique),
                    "cannot enable an unlearned technique"
                );
                if enabled {
                    member.disabled_techniques.remove(&technique)
                } else {
                    member.disabled_techniques.insert(technique)
                }
            }
            Self::FieldForget { member, technique } => party
                .forget_technique(data, member, technique)
                .map_err(anyhow::Error::msg)?,
            Self::FieldCast {
                member,
                target,
                technique,
                at_save_point,
            } => {
                let cue = party
                    .cast_technique(data, member, target, technique, at_save_point)
                    .map_err(anyhow::Error::msg)?;
                return Ok(EditResult {
                    changed: cue.is_some(),
                    cue: Some(cue.map_or(4, |v| v as u16)),
                });
            }
        };
        Ok(EditResult { changed, cue: None })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    pub character: usize,
    pub member: usize,
    pub technique: Option<u16>,
    /// Formation slot selected by the party target panel.
    pub target: usize,
    pub target_kind: Option<TargetKind>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub cue: Option<u16>,
    pub changed: bool,
    pub exit: Option<Exit>,
}
/// Navigation completes immediately; edits leave page state untouched until committed.
pub enum Step {
    Navigate(Visit),
    Edit(Edit),
}

#[derive(Clone, Copy)]
pub struct Page<'a> {
    pub state: &'a Tech,
    pub party: &'a Party,
    pub session: &'a SessionData,
    pub data: &'a MenuData,
    pub context: Context<'a>,
}
impl<'a> Page<'a> {
    pub fn battle(self) -> bool {
        self.context.battle()
    }
    pub fn at_save_point(self) -> bool {
        matches!(
            self.context,
            Context::Field {
                at_save_point: true,
                ..
            }
        )
    }
    pub fn party_count(self) -> usize {
        match self.context {
            Context::Battle { actors, .. } => actors.len(),
            Context::Field { .. } => self.party.formation.len(),
        }
    }
    pub fn member_index(self) -> usize {
        usize::from(self.party.formation[self.state.character] - 1)
    }
    pub fn member(self) -> &'a Member {
        &self.party.members[self.member_index()]
    }
    pub fn tech_target_visible(self) -> bool {
        self.state.focus == Focus::Target
    }
    pub fn tech_member_index(self) -> usize {
        let assisted = matches!(self.state.focus, Focus::AssistCharacter | Focus::AssistList)
            || self.state.focus == Focus::Target && self.state.return_to == Focus::AssistList;
        usize::from(
            self.party.formation[if assisted {
                self.state.assist
            } else {
                self.state.character
            }] - 1,
        )
    }
    pub fn tech_auto(self) -> bool {
        self.party
            .settings
            .battle_controls
            .get(self.state.character)
            .is_none_or(|&v| v == 2)
    }
    pub fn tech_unison_available(self) -> bool {
        self.state.character < 4
            && self.tech_auto()
            && !self.context.connected()[self.state.character]
    }
    pub fn tech_columns(self) -> usize {
        if self.state.unison {
            return 1;
        }
        let slot = self
            .party
            .formation
            .iter()
            .position(|&id| usize::from(id - 1) == self.tech_member_index())
            .unwrap();
        if self
            .party
            .settings
            .battle_controls
            .get(slot)
            .is_none_or(|&v| v == 2)
        {
            2
        } else {
            1
        }
    }
    pub fn technique_list(self) -> Vec<u16> {
        let index = self.tech_member_index();
        let member = &self.party.members[index];
        self.session.characters[index]
            .allowed_techniques
            .iter()
            .copied()
            .filter(|&id| {
                member.techniques.contains(&id)
                    || self
                        .data
                        .techniques
                        .get(usize::from(id))
                        .is_some_and(|tech| {
                            tech.level <= u16::from(member.level)
                                && match tech.route {
                                    0 => true,
                                    1 => member.technique_balance <= 0,
                                    2 => member.technique_balance > 0,
                                    _ => false,
                                }
                        })
            })
            .collect()
    }
    pub fn selected_technique(self) -> Option<TechniqueShortcut> {
        if self.state.focus == Focus::Shortcuts {
            shortcut_selection(self.party, self.member_index(), self.state.slot)
        } else if matches!(
            self.state.focus,
            Focus::Character | Focus::Control | Focus::AssistCharacter
        ) {
            None
        } else {
            self.technique_list()
                .get(self.state.row)
                .map(|&technique| TechniqueShortcut {
                    character: self.tech_member_index(),
                    technique,
                })
        }
    }
    pub fn tech_description(self) -> Option<TechniqueShortcut> {
        self.selected_technique()
            .filter(|_| !self.tech_target_visible())
    }
    pub fn tech_targets_all(self) -> bool {
        self.selected_technique().is_some_and(|s| {
            matches!(
                self.data.techniques[usize::from(s.technique)].field_use,
                Some(
                    TechniqueUse::Recover { party: true, .. } | TechniqueUse::Cure { party: true }
                )
            )
        })
    }
    pub fn battle_party_target(self) -> bool {
        self.selected_technique().is_some_and(|selected| {
            self.context
                .prepared(selected.character, selected.technique)
                .is_some_and(|(_, row)| row.capabilities.target == TargetKind::Ally)
        })
    }
    pub fn character_name(self, member: usize) -> &'a str {
        self.party.members[member]
            .name
            .as_deref()
            .unwrap_or(&self.data.initial_names[member])
    }
    pub fn technique_cost(self, member: usize, id: u16) -> Option<u32> {
        if !self.battle() {
            return Some(u32::from(self.party.members.get(member)?.technique_cost(
                self.data,
                id,
                self.at_save_point(),
            )));
        }
        let (actor, prepared) = self.context.prepared(member, id)?;
        let Context::Battle { battle, .. } = self.context else {
            return None;
        };
        battle.technique_tp_cost(actor, prepared.action)
    }
    pub fn ready(self, member: usize, id: u16) -> bool {
        if let Context::Battle { battle, .. } = self.context {
            self.context
                .prepared(member, id)
                .is_some_and(|(actor, prepared)| {
                    battle
                        .technique_queue_admitted(actor, prepared.action)
                        .unwrap_or(false)
                })
        } else {
            self.technique_cost(member, id)
                .is_some_and(|cost| cost <= u32::from(self.party.members[member].tp))
        }
    }
    pub fn available(self, member: usize, id: u16) -> bool {
        !self.battle() || self.context.prepared(member, id).is_some()
    }
}
impl Tech {
    pub fn opening(
        character: usize,
        context: Context<'_>,
        party: &Party,
        session: &SessionData,
        data: &MenuData,
    ) -> Result<Self> {
        let mut state = Self {
            character,
            ..Self::default()
        };
        state.validate(party, context)?;
        state.reset_focus(party, session, data, context);
        Ok(state)
    }
    pub fn page<'a>(
        &'a self,
        party: &'a Party,
        session: &'a SessionData,
        data: &'a MenuData,
        context: Context<'a>,
    ) -> Page<'a> {
        Page {
            state: self,
            party,
            session,
            data,
            context,
        }
    }
    fn validate(&self, party: &Party, context: Context<'_>) -> Result<()> {
        let count = party.formation.len();
        ensure!(
            (1..=8).contains(&count)
                && party.members.len() == 9
                && self.character < count
                && (!context.battle() || self.character < 4),
            "invalid Tech party selection"
        );
        for (slot, &id) in party.formation.iter().enumerate() {
            ensure!(
                (1..=9).contains(&id) && !party.formation[..slot].contains(&id),
                "invalid Tech formation"
            );
            ensure!(
                slot >= 4 || party.settings.battle_controls[slot] < 3,
                "invalid Tech control setting"
            );
        }
        Ok(())
    }
    fn reset_focus(
        &mut self,
        party: &Party,
        session: &SessionData,
        data: &MenuData,
        context: Context<'_>,
    ) {
        self.unison = false;
        self.slot = 0;
        self.row = 0;
        self.first = 0;
        self.focus = if self.page(party, session, data, context).tech_auto() {
            Focus::List
        } else {
            Focus::Shortcuts
        };
    }
    fn cycle_character(
        &mut self,
        previous: bool,
        skip_ko: bool,
        party: &Party,
        session: &SessionData,
        data: &MenuData,
        context: Context<'_>,
    ) {
        let count = self.page(party, session, data, context).party_count();
        let old = self.character;
        for _ in 0..count {
            self.character = (self.character + if previous { count - 1 } else { 1 }) % count;
            if !skip_ko
                || !self
                    .page(party, session, data, context)
                    .member()
                    .knocked_out()
            {
                break;
            }
        }
        if old != self.character {
            self.reset_focus(party, session, data, context);
        }
        if skip_ko {
            self.focus = Focus::Character;
        }
    }
    pub fn step(
        &mut self,
        input: Input,
        party: &mut Party,
        session: &SessionData,
        data: &MenuData,
        context: Context<'_>,
    ) -> Result<Visit> {
        ensure!(
            !context.battle(),
            "battle Tech edits require prepared core bindings"
        );
        match self.request_step(input, party, session, data, context)? {
            Step::Navigate(visit) => Ok(visit),
            Step::Edit(edit) => {
                let result = edit.apply_field(party, data)?;
                Ok(self.finish_edit(edit, result, party, session, data, context))
            }
        }
    }

    pub fn request_step(
        &mut self,
        input: Input,
        party: &Party,
        session: &SessionData,
        data: &MenuData,
        context: Context<'_>,
    ) -> Result<Step> {
        let (cue, edit) = self.dispatch(input, party, session, data, context)?;
        Ok(match edit {
            Some(edit) => Step::Edit(edit),
            None => Step::Navigate(self.visit(cue, false, party, session, data, context)),
        })
    }

    /// Called only after the requested edit has succeeded in the field or battle core.
    pub fn finish_edit(
        &mut self,
        edit: Edit,
        result: EditResult,
        party: &Party,
        session: &SessionData,
        data: &MenuData,
        context: Context<'_>,
    ) -> Visit {
        let cue = match edit {
            Edit::Control { .. } => {
                self.first = 0;
                Some(1)
            }
            Edit::Shortcut {
                selected: Some(_), ..
            } => {
                self.focus = Focus::Shortcuts;
                Some(2)
            }
            Edit::Shortcut { selected: None, .. } | Edit::Enabled { .. } => Some(1),
            Edit::FieldForget { .. } => {
                self.focus = self.return_to;
                let count = self
                    .page(party, session, data, context)
                    .technique_list()
                    .len();
                self.row = self.row.min(count.saturating_sub(1));
                self.reveal(self.page(party, session, data, context).tech_columns());
                Some(2)
            }
            Edit::FieldCast {
                member, technique, ..
            } => {
                if result.changed
                    && !self
                        .page(party, session, data, context)
                        .ready(member, technique)
                {
                    self.focus = self.return_to;
                }
                None
            }
        };
        self.visit(
            result.cue.or(cue),
            result.changed,
            party,
            session,
            data,
            context,
        )
    }

    fn visit(
        &self,
        cue: Option<u16>,
        changed: bool,
        party: &Party,
        session: &SessionData,
        data: &MenuData,
        context: Context<'_>,
    ) -> Visit {
        let exit = if let Focus::Closed { technique } = self.focus {
            let member = self.page(party, session, data, context).member_index();
            Some(Exit {
                character: self.character,
                member,
                technique,
                target: self.target,
                target_kind: technique.and_then(|technique| {
                    context
                        .prepared(member, technique)
                        .map(|(_, row)| row.capabilities.target)
                }),
            })
        } else {
            None
        };
        Visit { cue, changed, exit }
    }
    fn dispatch(
        &mut self,
        input: Input,
        party: &Party,
        session: &SessionData,
        data: &MenuData,
        context: Context<'_>,
    ) -> Result<(Option<u16>, Option<Edit>)> {
        let Some(action) = input else {
            return Ok((None, None));
        };
        let page = self.page(party, session, data, context);
        let member = page.member_index();
        let selected = (action != MenuAction::Cancel)
            .then(|| page.selected_technique())
            .flatten();
        let battle = page.battle();
        let at_save_point = page.at_save_point();
        let mut cue = None;
        let mut requested = None;
        match self.focus {
            Focus::Closed { .. } => {}
            Focus::CannotForget => {
                if matches!(action, MenuAction::Confirm | MenuAction::Cancel) {
                    self.focus = self.return_to;
                    cue = Some(1);
                }
            }
            Focus::Forget { yes } => {
                if action == MenuAction::Cancel {
                    self.focus = self.return_to;
                    cue = Some(3);
                } else if action == MenuAction::Confirm {
                    if yes {
                        let selected = selected
                            .ok_or_else(|| anyhow::anyhow!("missing Tech forget selection"))?;
                        return Ok((
                            None,
                            Some(Edit::FieldForget {
                                member,
                                technique: selected.technique,
                            }),
                        ));
                    } else {
                        self.focus = self.return_to;
                        cue = Some(
                            if self.page(party, session, data, context).tech_columns() == 2 {
                                2
                            } else {
                                3
                            },
                        );
                    }
                } else if matches!(action, MenuAction::Left | MenuAction::Right) {
                    self.focus = Focus::Forget { yes: !yes };
                    cue = Some(1);
                }
            }
            Focus::Control => {
                if action == MenuAction::Cancel {
                    self.close(None);
                    cue = Some(3);
                } else if matches!(action, MenuAction::Left | MenuAction::Right) {
                    let value = (party.settings.battle_controls[self.character]
                        + if action == MenuAction::Left { 2 } else { 1 })
                        % 3;
                    return Ok((
                        None,
                        Some(Edit::Control {
                            slot: self.character,
                            value,
                        }),
                    ));
                } else if action == MenuAction::Confirm || action == MenuAction::Down {
                    self.focus = Focus::Character;
                    cue = Some(1);
                }
            }
            Focus::Character => {
                if action == MenuAction::Cancel {
                    self.close(None);
                    cue = Some(3);
                } else if action == MenuAction::Details {
                    if self.character < 4 {
                        let value = (party.settings.battle_controls[self.character] + 1) % 3;
                        return Ok((
                            None,
                            Some(Edit::Control {
                                slot: self.character,
                                value,
                            }),
                        ));
                    }
                } else if action == MenuAction::Menu {
                    if page.tech_unison_available() {
                        self.unison = true;
                        self.focus = Focus::Shortcuts;
                        self.slot = 0;
                        self.row = 0;
                        self.first = 0;
                        cue = Some(1);
                    }
                } else if action == MenuAction::Up {
                    if self.character < 4 {
                        self.focus = Focus::Control;
                        cue = Some(1);
                    }
                } else if action == MenuAction::Confirm || action == MenuAction::Down {
                    if page.tech_auto() {
                        self.reset_focus(party, session, data, context);
                    } else {
                        self.focus = Focus::Shortcuts;
                        self.slot = 0;
                    }
                    cue = Some(1);
                } else if action == MenuAction::Left || action == MenuAction::PreviousTab {
                    self.cycle_character(true, true, party, session, data, context);
                    cue = Some(1);
                } else if action == MenuAction::Right || action == MenuAction::NextTab {
                    self.cycle_character(false, true, party, session, data, context);
                    cue = Some(1);
                }
            }
            Focus::Shortcuts => {
                if action == MenuAction::Cancel {
                    if self.unison {
                        self.focus = Focus::Character;
                        self.unison = false;
                    } else if battle {
                        self.close(None);
                    } else {
                        self.focus = Focus::Character;
                    }
                    cue = Some(3);
                } else if matches!(action, MenuAction::PreviousTab | MenuAction::NextTab) {
                    if !self.unison && party.formation.len() > 1 {
                        self.cycle_character(
                            action == MenuAction::PreviousTab,
                            false,
                            party,
                            session,
                            data,
                            context,
                        );
                        cue = Some(1);
                    }
                } else if action == MenuAction::Confirm {
                    if self.slot < 4 {
                        let list = self.page(party, session, data, context).technique_list();
                        if let Some((row, first)) = shortcut_choice(&list, selected) {
                            self.focus = Focus::List;
                            self.row = row;
                            self.first = first;
                            cue = Some(2);
                        } else {
                            cue = Some(4);
                        }
                    } else {
                        self.focus = Focus::AssistCharacter;
                        self.assist = self.character;
                        cue = Some(2);
                    }
                } else if action == MenuAction::Alternate {
                    if selected.is_some() {
                        return Ok((
                            None,
                            Some(Edit::Shortcut {
                                member,
                                slot: self.slot,
                                selected: None,
                            }),
                        ));
                    }
                } else if action == MenuAction::Up {
                    if self.slot == 0 {
                        if self.unison {
                            self.slot = 3;
                        } else {
                            self.focus = Focus::Character;
                        }
                    } else {
                        self.slot -= 1;
                    }
                    cue = Some(1);
                } else if action == MenuAction::Down {
                    let count = if self.unison || self.character >= 4 {
                        4
                    } else {
                        6
                    };
                    if self.slot + 1 < count {
                        self.slot += 1;
                    } else if self.unison {
                        self.slot = 0;
                    } else {
                        self.focus = Focus::Character;
                    }
                    cue = Some(1);
                }
            }
            Focus::AssistCharacter => {
                if action == MenuAction::Cancel {
                    self.focus = Focus::Shortcuts;
                    cue = Some(3);
                } else if action == MenuAction::Left || action == MenuAction::PreviousTab {
                    let count = party.formation.len().min(4);
                    if party.formation.len() > 1 {
                        self.assist = (self.assist + count - 1) % count;
                        self.row = 0;
                        self.first = 0;
                        cue = Some(1);
                    }
                } else if action == MenuAction::Right || action == MenuAction::NextTab {
                    let count = party.formation.len().min(4);
                    if party.formation.len() > 1 {
                        self.assist = (self.assist + 1) % count;
                        self.row = 0;
                        self.first = 0;
                        cue = Some(1);
                    }
                } else if action == MenuAction::Confirm {
                    self.focus = Focus::AssistList;
                    self.row = 0;
                    self.first = 0;
                    cue = Some(2);
                }
            }
            Focus::Target => {
                if action == MenuAction::Cancel {
                    self.focus = self.return_to;
                    cue = Some(3);
                } else if action == MenuAction::Confirm {
                    let selected = selected
                        .ok_or_else(|| anyhow::anyhow!("missing Tech field target selection"))?;
                    if battle {
                        self.close(Some(selected.technique));
                        cue = Some(2);
                        return Ok((cue, None));
                    }
                    let target = usize::from(party.formation[self.target] - 1);
                    return Ok((
                        None,
                        Some(Edit::FieldCast {
                            member: selected.character,
                            target,
                            technique: selected.technique,
                            at_save_point,
                        }),
                    ));
                } else if !page.tech_targets_all() {
                    let old = self.target;
                    match action {
                        MenuAction::Left if self.target >= 4 => self.target -= 4,
                        MenuAction::Right if self.target + 4 < page.party_count() => {
                            self.target += 4
                        }
                        MenuAction::Up if self.target != 0 => self.target -= 1,
                        MenuAction::Down if self.target + 1 < page.party_count() => {
                            self.target += 1
                        }
                        _ => {}
                    }
                    cue = (old != self.target).then_some(1);
                }
            }
            Focus::List | Focus::AssistList => {
                let two = page.tech_columns() == 2;
                if two && self.focus == Focus::List {
                    if action == MenuAction::Alternate {
                        if !battle {
                            cue = self.begin_forget(party, session, data, context)?;
                        }
                    } else if matches!(action, MenuAction::PreviousTab | MenuAction::NextTab)
                        && party.formation.len() > 1
                    {
                        self.cycle_character(
                            action == MenuAction::PreviousTab,
                            false,
                            party,
                            session,
                            data,
                            context,
                        );
                        cue = Some(1);
                    }
                }
                let (next_cue, requested_edit) =
                    self.list_pass((action, two), party, session, data, context)?;
                cue = next_cue.or(cue);
                requested = requested_edit;
            }
        }
        Ok((cue, requested))
    }
    fn close(&mut self, technique: Option<u16>) {
        self.focus = Focus::Closed { technique };
    }
    fn begin_forget(
        &mut self,
        party: &Party,
        session: &SessionData,
        data: &MenuData,
        context: Context<'_>,
    ) -> Result<Option<u16>> {
        let page = self.page(party, session, data, context);
        let selected = page
            .selected_technique()
            .ok_or_else(|| anyhow::anyhow!("missing Tech forget row"))?;
        if !party.members[selected.character]
            .techniques
            .contains(&selected.technique)
        {
            return Ok(Some(4));
        }
        self.return_to = self.focus;
        self.focus = if data.techniques[usize::from(selected.technique)].alternatives[0] == 0 {
            Focus::CannotForget
        } else {
            Focus::Forget { yes: false }
        };
        Ok(Some(if self.focus == Focus::CannotForget {
            4
        } else {
            2
        }))
    }
    fn list_pass(
        &mut self,
        (action, two): (MenuAction, bool),
        party: &Party,
        session: &SessionData,
        data: &MenuData,
        context: Context<'_>,
    ) -> Result<(Option<u16>, Option<Edit>)> {
        let page = self.page(party, session, data, context);
        let member = page.member_index();
        let battle = page.battle();
        let assist = self.focus == Focus::AssistList;
        if action == MenuAction::Cancel {
            if assist {
                self.focus = Focus::AssistCharacter;
            } else if two && battle {
                self.close(None);
            } else if two {
                self.focus = Focus::Character;
            } else {
                self.focus = Focus::Shortcuts;
            }
            return Ok((Some(3), None));
        }
        let selected = page.selected_technique();
        if action == MenuAction::Confirm {
            let Some(selected) = selected else {
                return Ok((Some(4), None));
            };
            if !party.members[selected.character]
                .techniques
                .contains(&selected.technique)
                || !page.available(selected.character, selected.technique)
            {
                return Ok((Some(4), None));
            }
            ensure!(
                selected.technique != 0
                    && data
                        .techniques
                        .get(usize::from(selected.technique))
                        .is_some(),
                "selected Tech technique description is missing"
            );
            if assist || !two {
                return Ok((
                    None,
                    Some(Edit::Shortcut {
                        member,
                        slot: self.slot,
                        selected: Some(selected),
                    }),
                ));
            }
            if !battle
                && ((party.members[selected.character].knocked_out()
                    || party.members[selected.character].ailments.petrified
                    || party.members[selected.character].ailments.curse)
                    || !page.ready(selected.character, selected.technique))
            {
                return Ok((Some(4), None));
            }
            if battle {
                if page.battle_party_target() {
                    self.begin_target(party, session, data, context);
                } else {
                    self.close(Some(selected.technique));
                }
            } else {
                if data.techniques[usize::from(selected.technique)]
                    .field_use
                    .is_none()
                {
                    return Ok((Some(4), None));
                }
                self.begin_target(party, session, data, context);
            }
            return Ok((Some(2), None));
        }
        if action == MenuAction::Menu {
            if two {
                if self.focus != Focus::List {
                    return Ok((None, None));
                }
                let Some(selected) = selected else {
                    return Ok((Some(4), None));
                };
                if !party.members[selected.character]
                    .techniques
                    .contains(&selected.technique)
                    || !page.available(selected.character, selected.technique)
                {
                    return Ok((Some(4), None));
                }
                let enabled = party.members[selected.character]
                    .disabled_techniques
                    .contains(&selected.technique);
                return Ok((
                    None,
                    Some(Edit::Enabled {
                        member: selected.character,
                        technique: selected.technique,
                        enabled,
                    }),
                ));
            }
            if self.unison || battle {
                return Ok((None, None));
            }
            let Some(selected) = selected else {
                return Ok((Some(4), None));
            };
            if !party.members[selected.character]
                .techniques
                .contains(&selected.technique)
                || (party.members[selected.character].knocked_out()
                    || party.members[selected.character].ailments.petrified
                    || party.members[selected.character].ailments.curse)
                || !page.ready(selected.character, selected.technique)
                || data.techniques[usize::from(selected.technique)]
                    .field_use
                    .is_none()
            {
                return Ok((Some(4), None));
            }
            self.begin_target(party, session, data, context);
            return Ok((Some(2), None));
        }
        if !two && action == MenuAction::Alternate {
            return Ok((
                if self.focus == Focus::List && !self.unison && !battle {
                    self.begin_forget(party, session, data, context)?
                } else {
                    None
                },
                None,
            ));
        }
        let count = self
            .page(party, session, data, context)
            .technique_list()
            .len();
        Ok((
            self.navigate_list(action, if two { 2 } else { 1 }, count),
            None,
        ))
    }
    fn begin_target(
        &mut self,
        party: &Party,
        session: &SessionData,
        data: &MenuData,
        context: Context<'_>,
    ) {
        self.return_to = self.focus;
        self.focus = Focus::Target;
        if self.page(party, session, data, context).tech_targets_all() {
            self.target = 0;
        }
    }
    fn reveal(&mut self, columns: usize) {
        let visible = if columns == 2 { 12 } else { 8 };
        self.first = self
            .first
            .min(self.row / columns * columns)
            .max(self.row.saturating_sub(visible - 1).div_ceil(columns) * columns);
    }
    fn navigate_list(&mut self, action: MenuAction, columns: usize, count: usize) -> Option<u16> {
        if count == 0 {
            return None;
        }
        let old = self.row;
        let visible = if columns == 2 { 12 } else { 8 };
        match action {
            MenuAction::PageDown => self.row = (old + visible).min(count - 1),
            MenuAction::PageUp => self.row = old.saturating_sub(visible),
            MenuAction::Up if columns == 2 && old < 2 => {
                if self.focus == Focus::List {
                    self.focus = Focus::Character;
                }
                return Some(1);
            }
            MenuAction::Left => self.row = self.row.saturating_sub(1),
            MenuAction::Right if old + 1 < count => self.row += 1,
            MenuAction::Up => self.row = self.row.saturating_sub(columns),
            MenuAction::Down if old + columns < count => self.row += columns,
            _ => return None,
        }
        self.reveal(columns);
        (old != self.row).then_some(
            if matches!(action, MenuAction::PageUp | MenuAction::PageDown) {
                38
            } else {
                1
            },
        )
    }
}

impl super::Menu {
    pub fn tech_page(&self) -> Page<'_> {
        let resources = self.resources.as_ref().unwrap();
        self.tech.page(
            self.party(),
            &resources.session,
            &resources.data,
            Context::Field {
                at_save_point: self.at_save_point,
                connected: &self.tech_connected,
            },
        )
    }
    pub fn tech_description(&self) -> Option<TechniqueShortcut> {
        self.tech_page().tech_description()
    }
    pub fn tech_target_visible(&self) -> bool {
        self.tech_page().tech_target_visible()
    }
    pub fn tech_unison_available(&self) -> bool {
        self.tech_page().tech_unison_available()
    }
    pub fn tech_targets_all(&self) -> bool {
        self.tech_page().tech_targets_all()
    }
    pub fn tech_auto(&self) -> bool {
        self.tech_page().tech_auto()
    }
    pub fn tech_member_index(&self) -> usize {
        self.tech_page().tech_member_index()
    }
    pub fn tech_columns(&self) -> usize {
        self.tech_page().tech_columns()
    }
    pub fn technique_list(&self) -> Vec<u16> {
        self.tech_page().technique_list()
    }
    pub fn selected_technique(&self) -> Option<TechniqueShortcut> {
        self.tech_page().selected_technique()
    }
    pub(super) fn open_techniques(&mut self) -> bool {
        let Some(resources) = self.resources.as_ref() else {
            return false;
        };
        match Tech::opening(
            self.character,
            Context::Field {
                at_save_point: self.at_save_point,
                connected: &self.tech_connected,
            },
            self.party(),
            &resources.session,
            &resources.data,
        ) {
            Ok(state) => {
                self.tech = state;
                true
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                false
            }
        }
    }
    pub(super) fn step_techniques(&mut self, input: Input) -> Option<i16> {
        let resources = self.resources.as_ref()?;
        let visit = self.tech.step(
            input,
            &mut self.checkpoint.as_mut()?.progress.party,
            &resources.session,
            &resources.data,
            Context::Field {
                at_save_point: self.at_save_point,
                connected: &self.tech_connected,
            },
        );
        match visit {
            Ok(visit) => {
                self.party_changed |= visit.changed;
                self.character = self.tech.character;
                if visit.exit.is_some() {
                    self.return_to_main();
                }
                visit.cue.map(|v| v as i16)
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                Some(4)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_reveals_rows_and_accepts_consecutive_inputs() {
        for columns in [1, 2] {
            let visible = if columns == 2 { 12 } else { 8 };
            let mut state = Tech {
                focus: Focus::List,
                row: visible - 1,
                ..Default::default()
            };
            for _ in 0..2 {
                let before = state.row;
                assert_eq!(state.navigate_list(MenuAction::Down, columns, 20), Some(1));
                assert_eq!(state.row, before + columns);
                assert!((state.first..state.first + visible).contains(&state.row));
            }
            assert_eq!(
                state.navigate_list(MenuAction::PageDown, columns, 20),
                Some(38)
            );
            assert_eq!(state.row, if columns == 1 { 17 } else { 19 });
            assert!((state.first..state.first + visible).contains(&state.row));
            if columns == 1 {
                assert_eq!(
                    state.navigate_list(MenuAction::PageDown, columns, 20),
                    Some(38)
                );
                assert_eq!(state.row, 19);
            }
            assert_eq!(state.navigate_list(MenuAction::PageDown, columns, 20), None);
            assert_eq!(
                state.navigate_list(MenuAction::PageUp, columns, 20),
                Some(38)
            );
            assert_eq!(state.row, 19 - visible);
            assert!((state.first..state.first + visible).contains(&state.row));
        }
    }

    #[test]
    fn up_from_the_first_row_keeps_assist_ownership() {
        let mut state = Tech {
            focus: Focus::AssistList,
            ..Default::default()
        };
        state.navigate_list(MenuAction::Up, 2, 20);
        assert_eq!(state.focus, Focus::AssistList);
        state.focus = Focus::List;
        state.navigate_list(MenuAction::Up, 2, 20);
        assert_eq!(state.focus, Focus::Character);
    }
}
