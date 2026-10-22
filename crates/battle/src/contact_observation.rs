//! Contact-time inputs for independent oracle validation. No damage calculation
//! or random draw is repeated by this observation hook.
use crate::{Activity, Actor, ActorId, Battle, ContactSource, HitRule};

#[derive(Debug, Clone)]
pub struct ContactObservation {
    pub source: ContactSource,
    pub owner_id: ActorId,
    pub target_id: ActorId,
    pub owner: Actor,
    pub target: Actor,
    pub activity: Activity,
    pub rule: HitRule,
    pub attack_power: u16,
    pub all_divide: bool,
    pub ordinary_mode: bool,
}

impl Battle {
    pub fn observe_fixture_contact_damage(&mut self) {
        self.contact_observations = Some(Vec::new());
    }

    pub fn take_fixture_contact_damage(&mut self) -> Vec<ContactObservation> {
        self.contact_observations
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }
}
