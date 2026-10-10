//! Read-only action admission and the history of the active combo.
use crate::conditions::Condition;
use crate::technique_command::{ArteFamily, RegalArteFamily, TechniqueCapabilities};
use crate::{ActorId, Battle, PreparedTechnique, Rejection};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComboTraits {
    pub sky_combo: bool,
    /// Allows compatible techniques while airborne.
    pub aerial_arte: bool,
    pub ability_plus: bool,
    pub super_chain: bool,
    pub flash: bool,
    pub counter_combo: bool,
    pub jump_combo: bool,
    pub landing: bool,
    pub super_blast: bool,
    pub combo_force: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ComboHistory {
    pub basic: bool,
    pub advanced: bool,
    pub arcane: bool,
    pub ground: bool,
    pub anti_air: bool,
    pub aerial: bool,
}

impl ComboHistory {
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
    fn record(&mut self, capabilities: TechniqueCapabilities) {
        match capabilities.family {
            Some(ArteFamily::Basic) => self.basic = true,
            Some(ArteFamily::Advanced) => self.advanced = true,
            Some(ArteFamily::Arcane) => self.arcane = true,
            Some(ArteFamily::Finisher) => {
                self.basic = true;
                self.advanced = true;
                self.arcane = true;
            }
            None => {}
        }
        match capabilities.regal_family {
            Some(RegalArteFamily::Ground) => self.ground = true,
            Some(RegalArteFamily::AntiAir) => self.anti_air = true,
            Some(RegalArteFamily::Aerial) => self.aerial = true,
            None => {}
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum GroundMotion {
    #[default]
    Idle,
    Walk,
    Run,
    Stop,
    Guard,
    Taunt,
}

impl GroundMotion {
    pub(crate) fn locomotion(self) -> Option<crate::Locomotion> {
        Some(match self {
            Self::Idle => crate::Locomotion::Idle,
            Self::Walk => crate::Locomotion::Walk,
            Self::Run => crate::Locomotion::Run,
            Self::Stop => crate::Locomotion::Stop,
            Self::Guard | Self::Taunt => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct InputIntent {
    pub(crate) motion: GroundMotion,
    pub(crate) action: Option<ActionCandidate>,
    pub(crate) jump: bool,
    pub(crate) face_target: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ActionCandidate {
    pub action: crate::ActionKey,
    pub range: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BufferedAction {
    Normal(crate::NormalAttack),
    Technique(crate::ActionKey),
}

/// History and pending successor of one active combo. Idle and interruption reset it together.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Combo {
    pub history: ComboHistory,
    pub ability_plus_used: bool,
    pub last: Option<crate::ActionKey>,
    pub normal_links: u8,
    /// Bitset of NormalAttack variants admitted during this combo.
    pub normal_kinds: u8,
    pub confirmed_contact: bool,
    pub buffered: Option<BufferedAction>,
}

impl Combo {
    pub fn record(&mut self, action: crate::ActionKey, technique: Option<TechniqueCapabilities>) {
        if let Some(capabilities) = technique {
            if capabilities.family == Some(ArteFamily::Basic) && self.history.basic {
                self.ability_plus_used = true;
            }
            self.history.record(capabilities);
        } else {
            self.history = ComboHistory::default();
            self.ability_plus_used = false;
        }
        self.last = Some(action);
        self.confirmed_contact = false;
        self.buffered = None;
    }

    fn basic_family_available(self, action: crate::ActionKey, traits: ComboTraits) -> bool {
        if self.history.basic && !self.history.advanced && !self.history.arcane {
            traits.ability_plus && !self.ability_plus_used && self.last != Some(action)
        } else {
            !self.history.basic
                && (traits.super_chain || !self.history.advanced && !self.history.arcane)
        }
    }
}

impl crate::Actor {
    /// Subtract 40 from the range, with a minimum of 25.
    pub(crate) fn weapon_reach(&self, maximum: f32) -> f32 {
        if self.equipment.dagger_reach {
            (maximum - 40.).max(25.)
        } else {
            maximum
        }
    }
}

impl Battle {
    /// Normal Guard protects the newly admitted normal attack.
    pub(crate) fn apply_normal_guard(&mut self, actor: ActorId, duration: u16) {
        let chained = self.runtime[actor.index()].combo.normal_links != 0;
        let actor = &mut self.actors[actor.index()];
        if actor.side == crate::Side::Party && !chained && actor.equipment.normal_guard {
            actor.reaction.protection.armor(u32::from(duration));
        }
    }

    pub(crate) fn selection_definition(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> Option<PreparedTechnique> {
        self.prepared.technique(actor, action).copied()
    }

    pub(crate) fn selection_range(&self, actor: ActorId, action: crate::ActionKey) -> [f32; 2] {
        if let Some(row) = self.prepared.technique(actor, action) {
            return row.player_range;
        }
        self.prepared.actor_setup[actor.index()]
            .control
            .as_ref()
            .and_then(|control| {
                self.prepared
                    .actions
                    .get(action)?
                    .normal
                    .map(|kind| &control.normals[kind as usize])
            })
            .map_or([0., 0.], |row| [row.minimum_reach, row.reach])
    }

    pub(crate) fn combo_technique(&self, actor: ActorId) -> Option<PreparedTechnique> {
        self.selection_definition(actor, self.runtime[actor.index()].combo.last?)
    }

    pub(crate) fn action_family_available(&self, actor: ActorId, action: crate::ActionKey) -> bool {
        let Some(row) = self.selection_definition(actor, action) else {
            return true;
        };
        let owner = &self.actors[actor.index()];
        let selection = self.runtime[actor.index()].combo;
        let history = selection.history;
        if let Some(family) = row.capabilities.regal_family {
            match family {
                RegalArteFamily::Ground => !history.ground,
                RegalArteFamily::AntiAir => !history.anti_air,
                RegalArteFamily::Aerial => !history.aerial,
            }
        } else {
            match row.capabilities.family {
                Some(ArteFamily::Basic) => {
                    selection.basic_family_available(action, owner.equipment.combo_traits)
                }
                Some(ArteFamily::Advanced) => {
                    !history.advanced
                        && (owner.equipment.combo_traits.super_chain || !history.arcane)
                }
                Some(ArteFamily::Arcane) => !history.arcane,
                _ => true,
            }
        }
    }

    fn aerial_arte_available(&self, actor: ActorId, capabilities: TechniqueCapabilities) -> bool {
        let owner = &self.actors[actor.index()];
        let grounded = !owner.airborne();
        if let Some(family) = capabilities.regal_family {
            match family {
                RegalArteFamily::Ground | RegalArteFamily::AntiAir => grounded,
                RegalArteFamily::Aerial => !grounded,
            }
        } else {
            grounded || capabilities.aerial && owner.equipment.combo_traits.aerial_arte
        }
    }

    pub(crate) fn chain_descriptor_available(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
    ) -> bool {
        let Some(next) = self.selection_definition(actor, action) else {
            return true;
        };
        if !self.aerial_arte_available(actor, next.capabilities)
            || !self.action_family_available(actor, action)
        {
            return false;
        }
        let Some(current) = self.combo_technique(actor).map(|row| row.capabilities) else {
            return true;
        };
        if next.capabilities.regal_family.is_some() {
            !matches!(
                (current.regal_family, next.capabilities.regal_family),
                (
                    Some(RegalArteFamily::Ground),
                    Some(RegalArteFamily::Ground | RegalArteFamily::Aerial)
                ) | (
                    Some(RegalArteFamily::AntiAir),
                    Some(RegalArteFamily::AntiAir)
                ) | (Some(RegalArteFamily::Aerial), Some(RegalArteFamily::Aerial))
            )
        } else {
            current.family != next.capabilities.family
                || next.capabilities.family == Some(ArteFamily::Basic)
                    && self.runtime[actor.index()].combo.basic_family_available(
                        action,
                        self.actors[actor.index()].equipment.combo_traits,
                    )
                || next.capabilities.family.is_none()
        }
    }

    pub(crate) fn chain_count_available(&self, actor: ActorId, count: u8) -> bool {
        self.prepared.actor_setup[actor.index()]
            .arte_chain_limit
            .is_none_or(|limit| count < limit)
            || self.actors[actor.index()]
                .equipment
                .combo_traits
                .super_blast
    }

    pub(crate) fn action_candidate(
        &self,
        actor: ActorId,
        action: crate::ActionKey,
        range: [f32; 2],
    ) -> Result<ActionCandidate, Rejection> {
        let owner = &self.actors[actor.index()];
        let definition = self
            .prepared
            .actions
            .get(action)
            .ok_or(Rejection::Unavailable)?;
        let technique = self
            .selection_definition(actor, action)
            .map(|row| row.capabilities);
        let normal = definition.normal;
        if let Some(capabilities) = technique {
            if !self.technique_available(actor, action)
                || !owner.conditions.arte_queue_allowed()
                || !self.aerial_arte_available(actor, capabilities)
                || !self.action_family_available(actor, action)
                || self.prepared.special_guard(actor).is_some_and(|row| {
                    row == action && !self.special_guard_admission_allowed(actor, action)
                })
            {
                return Err(Rejection::Unavailable);
            }
        } else if normal == Some(crate::NormalAttack::Rising)
            && owner.conditions.effective().contains(Condition::Heavy)
        {
            return Err(Rejection::Unavailable);
        }
        if u32::from(owner.tp) < self.action_quote(actor, action) {
            return Err(Rejection::InsufficientTp);
        }
        let range = if normal.is_some()
            || technique.is_some_and(|capabilities| capabilities.uses_weapon_reach)
        {
            [range[0], owner.weapon_reach(range[1])]
        } else {
            range
        };
        Ok(ActionCandidate { action, range })
    }
}

#[cfg(test)]
mod tests;
