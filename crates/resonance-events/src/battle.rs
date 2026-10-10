//! Field-to-battle handoff. The field keeps its VM and clocks while the game
//! prepares and runs the encounter, then resumes the original caller once.
use crate::Operation;

/// Script-selected restrictions and percentage adjustments for the next encounter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rules {
    pub modifiers: u16,
    pub disabled_commands: u8,
    pub coliseum: bool,
    pub attack_adjustment: i8,
    pub defense_adjustment: i8,
    pub intelligence_adjustment: i8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefeatPolicy {
    GameOver,
    ResumeEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Encounter {
    Formation(u16),
    /// Weighted BTLusual encounter pool; selection belongs to the combat owner.
    Pool(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setup {
    pub encounter: Encounter,
    pub arena: u16,
    pub defeat: DefeatPolicy,
    /// An explicit music selection; None uses the current world's battle theme.
    pub music: Option<u16>,
    /// Native callback/route overrides retained for the encounter owner.
    pub route: [i32; 5],
}

impl Setup {
    pub fn formation(&self) -> anyhow::Result<u16> {
        match self.encounter {
            Encounter::Formation(id) => Ok(id),
            Encounter::Pool(id) => {
                anyhow::bail!("encounter pool {id} has no prepared formation selector")
            }
        }
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Escaped = 1,
    Victory = 2,
    Defeat = 3,
}

impl TryFrom<i32> for Outcome {
    type Error = String;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Escaped),
            2 => Ok(Self::Victory),
            3 => Ok(Self::Defeat),
            _ => Err(format!("invalid battle result {value}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub setup: Setup,
    pub rules: Rules,
    /// Mode 5 overrides the next scene's initial black/white clear.
    pub transition_white: Option<bool>,
    pub(crate) operation: Operation,
}

impl Request {
    pub fn id(&self) -> u64 {
        self.operation.id()
    }

    pub fn is_pending(&self) -> bool {
        self.operation.is_pending()
    }

    /// Scene owners retain a clone while combat owns the published request.
    pub fn result(&self) -> Result<Option<Outcome>, String> {
        match self.operation.progress().outcome {
            None => Ok(None),
            Some(crate::Outcome::Completed(Some(value))) => Outcome::try_from(value).map(Some),
            Some(crate::Outcome::Completed(None)) => {
                Err("battle completed without a result".into())
            }
            Some(crate::Outcome::Cancelled) => Err("battle request was cancelled".into()),
        }
    }

    /// Complete after writeback and results presentation. Fatal defeat retires
    /// the field instead, cancelling its caller and outstanding callbacks.
    pub fn complete(&self, outcome: Outcome) -> Result<(), String> {
        if outcome == Outcome::Defeat && self.setup.defeat == DefeatPolicy::GameOver {
            return Err("battle defeat requires game over".into());
        }
        self.operation.complete(Some(outcome as i32))
    }
}

impl crate::GameWorld {
    /// Publish an encounter from a non-scripted scene, with cancellation owned
    /// by the same runtime as scripted services. Combat may take the request;
    /// the world retains the returned clone until results have been applied.
    pub fn request_battle(&mut self, setup: Setup) -> Result<Request, String> {
        if self.battle_request.is_some() {
            return Err("nested battle request".into());
        }
        let rules = self
            .party
            .as_ref()
            .ok_or("battle requires a party")?
            .battle_rules;
        let request = Request {
            setup,
            rules,
            transition_white: self.next_transition_white.take(),
            operation: self.operations.begin()?,
        };
        self.battle_request = Some(request.clone());
        Ok(request)
    }
}

impl crate::GameWorld {
    /// Temporary exploration mode: preserve the victory result expected by the
    /// suspended script without constructing a combat scene or awarding loot.
    pub fn skip_battle_as_victory(&mut self) -> Result<bool, String> {
        let Some(request) = self.battle_request.as_ref() else {
            return Ok(false);
        };
        request.complete(Outcome::Victory)?;
        if let Some(white) = request.transition_white {
            self.fade = Some(crate::Fade {
                start_tick: self.tick,
                duration: 0,
                from: 255.,
                to: 255.,
                white,
            });
        }
        if let Some(party) = &mut self.party {
            let active: Vec<_> = party
                .formation
                .iter()
                .take(4)
                .map(|&id| (id, party.members[usize::from(id - 1)].technique_balance))
                .collect();
            party
                .begin_battle(&active)
                .map_err(|error| error.to_string())?;
            party.battles.combat_ticks = 0;
        }
        // Restore after the resumed caller has applied its battle-transition
        // fade, so that fade cannot leave the retained field track muted.
        self.restore_battle_music = true;
        self.battle_request = None;
        Ok(true)
    }
}
