//! One action hint shared by nearby actors, doors and memory circles.
use anyhow::Result;

const HOLD_TICKS: u8 = 30;
const FADE_TICKS: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FieldAction {
    Enter = 1,
    Talk = 2,
    Shop = 4,
    Examine = 5,
    Open = 6,
    Push = 7,
    Pull = 8,
    Operate = 9,
    Switch = 10,
    Climb = 11,
    Descend = 12,
    Jump = 13,
    Hit = 14,
    Read = 15,
    Rest = 16,
    Eat = 17,
    Leave = 18,
    Move = 19,
    Grab = 20,
    Question = 21,
    Exclamation = 22,
    Save = 23,
    Warp = 24,
}
impl FieldAction {
    pub(super) fn from_id(id: u32) -> Result<Option<Self>> {
        Ok(match id {
            0 => None,
            1 => Some(Self::Enter),
            2 => Some(Self::Talk),
            4 => Some(Self::Shop),
            5 => Some(Self::Examine),
            6 => Some(Self::Open),
            7 => Some(Self::Push),
            8 => Some(Self::Pull),
            9 => Some(Self::Operate),
            10 => Some(Self::Switch),
            11 => Some(Self::Climb),
            12 => Some(Self::Descend),
            13 => Some(Self::Jump),
            14 => Some(Self::Hit),
            15 => Some(Self::Read),
            16 => Some(Self::Rest),
            17 => Some(Self::Eat),
            3 | 18 => Some(Self::Leave),
            19 => Some(Self::Move),
            20 => Some(Self::Grab),
            21 => Some(Self::Question),
            22 => Some(Self::Exclamation),
            23 => Some(Self::Save),
            24 => Some(Self::Warp),
            _ => anyhow::bail!("unsupported field action hint {id}"),
        })
    }
}

impl super::FieldSession {
    pub(super) fn interaction_action(&self) -> Result<Option<FieldAction>> {
        if super::save_point::SavePoints::sealed_target(&self.events.world).is_some() {
            return Ok(Some(FieldAction::Examine));
        }
        if super::treasure::Treasures::target(&self.events.world).is_some() {
            return Ok(Some(FieldAction::Examine));
        }
        if super::blocks::Blocks::target(&self.events.world).is_some() {
            return Ok(Some(FieldAction::Grab));
        }
        let Some(id) = self.interaction_target() else {
            return Ok(None);
        };
        let actor = &self.events.world.actors[&id];
        // Actor property 17 selects its interaction label; zero suppresses it.
        FieldAction::from_id(actor.interaction_label as u32)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ActionPrompt {
    pub action: FieldAction,
    pub opacity: u8,
    pub text_opacity: u8,
}

#[derive(Default)]
pub(super) struct ActionHints {
    pub prompt: Option<ActionPrompt>,
    action: Option<FieldAction>,
    remaining: u8,
    opacity: u8,
}
impl ActionHints {
    pub fn step(&mut self, action: Option<FieldAction>, visible: bool) {
        if let Some(action) = action {
            self.action = Some(action);
            self.remaining = HOLD_TICKS;
        }
        if self.remaining == 0 {
            self.opacity = 0;
        }
        self.prompt = self
            .action
            .filter(|_| visible && self.remaining > 0)
            .map(|action| {
                let fading = self.remaining < FADE_TICKS;
                self.opacity = if fading {
                    self.opacity.saturating_sub(12)
                } else {
                    self.opacity.saturating_add(8)
                };
                ActionPrompt {
                    action,
                    opacity: self.opacity,
                    text_opacity: if fading { self.remaining * 12 } else { 255 },
                }
            });
        self.remaining = self.remaining.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_hint_changes_label_without_restarting_opacity_and_expires_during_events() {
        let mut hints = ActionHints::default();
        for _ in 0..10 {
            hints.step(Some(FieldAction::Enter), true);
        }
        assert_eq!(hints.prompt.unwrap().opacity, 80);
        hints.step(Some(FieldAction::Save), true);
        let prompt = hints.prompt.unwrap();
        assert_eq!(prompt.action, FieldAction::Save);
        assert_eq!(prompt.opacity, 88);
        for _ in 0..11 {
            hints.step(None, true);
        }
        assert_eq!(hints.prompt.unwrap().text_opacity, 19 * 12);
        for _ in 0..HOLD_TICKS {
            hints.step(None, false);
            assert!(hints.prompt.is_none());
        }
        hints.step(None, true);
        assert!(hints.prompt.is_none());
        hints.step(Some(FieldAction::Leave), true);
        assert_eq!(hints.prompt.unwrap().opacity, 8);
        assert!(FieldAction::from_id(0).unwrap().is_none());
        assert_eq!(FieldAction::from_id(6).unwrap(), Some(FieldAction::Open));
        for id in 1..=24 {
            assert!(FieldAction::from_id(id).unwrap().is_some(), "hint {id}");
        }
        assert!(FieldAction::from_id(99).is_err());
    }
}
