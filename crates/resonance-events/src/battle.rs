//! Field-to-battle handoff. The field keeps its VM and clocks while the game
//! prepares and runs the encounter, then resumes the original caller once.
use crate::Operation;

/// Native battle-entry counters queried by inn/skit conditions.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct History {
    pub total: u16,
    pub participation: [u16; 9],
}

impl History {
    pub fn count(&self, member: i32) -> Result<u16, String> {
        match member {
            0 => Ok(self.total),
            1..=9 => Ok(self.participation[(member - 1) as usize]),
            _ => Err("invalid battle history member".into()),
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.total <= 9999 && self.participation.iter().all(|&n| n <= 9999),
            "invalid battle history"
        );
        Ok(())
    }

    fn record(&mut self, formation: &[u8]) {
        self.total = self.total.saturating_add(1).min(9999);
        for &member in formation.iter().take(4) {
            let count = &mut self.participation[usize::from(member - 1)];
            *count = count.saturating_add(1).min(9999);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battle_history_counts_active_participants_and_survives_saves() {
        let mut history = History::default();
        history.record(&[1, 2, 3, 4, 9]);
        history.record(&[9, 1]);
        assert_eq!(history.count(0).unwrap(), 2);
        assert_eq!(history.participation, [2, 1, 1, 1, 0, 0, 0, 0, 1]);
        assert!(history.count(-1).is_err());
        assert!(history.count(10).is_err());
        let mut restored: History =
            serde_json::from_slice(&serde_json::to_vec(&history).unwrap()).unwrap();
        assert_eq!(restored.participation, history.participation);
        for _ in 0..10_000 {
            restored.record(&[1]);
        }
        assert_eq!(restored.total, 9999);
        assert_eq!(restored.count(1).unwrap(), 9999);
        restored.validate().unwrap();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefeatPolicy {
    GameOver,
    ResumeEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        if self.party.is_none() {
            return Err("battle requires a party".into());
        }
        let request = Request {
            setup,
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
        if let Some(party) = &mut self.party {
            party.battles.record(&party.formation);
        }
        self.battle_request = None;
        Ok(true)
    }
}
