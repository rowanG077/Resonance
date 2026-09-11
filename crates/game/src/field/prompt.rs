//! One action hint shared by nearby doors and memory circles.
use anyhow::Result;

const HOLD_TICKS: u8 = 30;
const FADE_TICKS: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FieldAction {
    Enter = 1,
    Leave = 18,
    Save = 23,
}
impl FieldAction {
    pub(super) fn from_id(id: u32) -> Result<Option<Self>> {
        Ok(match id {
            0 => None,
            1 => Some(Self::Enter),
            18 => Some(Self::Leave),
            23 => Some(Self::Save),
            _ => anyhow::bail!("unsupported field action hint {id}"),
        })
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
        assert!(FieldAction::from_id(99).is_err());
    }
}
