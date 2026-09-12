//! One action hint shared by nearby actors, doors and memory circles.
use anyhow::{Context, Result, ensure};

const HOLD_TICKS: u8 = 30;
const FADE_TICKS: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FieldAction {
    Enter = 1,
    Talk = 2,
    Shop = 4,
    Examine = 5,
    Leave = 18,
    Save = 23,
}
impl FieldAction {
    pub(super) fn from_id(id: u32) -> Result<Option<Self>> {
        Ok(match id {
            0 => None,
            1 => Some(Self::Enter),
            2 => Some(Self::Talk),
            4 => Some(Self::Shop),
            5 => Some(Self::Examine),
            18 => Some(Self::Leave),
            23 => Some(Self::Save),
            _ => anyhow::bail!("unsupported field action hint {id}"),
        })
    }
}

impl super::FieldSession {
    /// Register a transient hint observed by an oracle replay, outside save data.
    pub fn apply_action_prompt_origin(&mut self, id: u8, opacity: u8, remaining: u8) -> Result<()> {
        let free_control = self.player_has_control();
        self.action_hints
            .apply_origin(id, opacity, remaining, free_control)
    }

    pub(super) fn interaction_action(&self) -> Result<Option<FieldAction>> {
        let Some(id) = self.interaction_target() else {
            return Ok(None);
        };
        let actor = &self.events.world.actors[&id];
        // Actor property 17 selects its interaction label; zero suppresses it.
        FieldAction::from_id(actor.properties.get(&17).copied().unwrap_or(2) as u32)
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
    fn apply_origin(
        &mut self,
        id: u8,
        opacity: u8,
        remaining: u8,
        free_control: bool,
    ) -> Result<()> {
        let action = FieldAction::from_id(u32::from(id))?.context("empty action hint origin")?;
        ensure!(
            free_control
                && self
                    .prompt
                    .is_some_and(|p| p.action == action && p.opacity > 0)
                && opacity > 0
                && (1..HOLD_TICKS).contains(&remaining),
            "action hint origin requires matching visible action at free control"
        );
        self.action = Some(action);
        self.remaining = remaining;
        self.opacity = opacity;
        self.prompt = Some(ActionPrompt {
            action,
            opacity,
            // Observations retain the counter after drawing and decrementing it.
            text_opacity: if remaining < FADE_TICKS - 1 {
                (remaining + 1) * 12
            } else {
                255
            },
        });
        Ok(())
    }

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
    fn observed_hint_preserves_fade_continuity_and_requires_matching_control() {
        let mut hints = ActionHints::default();
        assert!(hints.apply_origin(2, 255, 29, true).is_err());
        hints.step(Some(FieldAction::Talk), true);
        hints.apply_origin(2, 255, 29, true).unwrap();
        assert_eq!(
            (
                hints.opacity,
                hints.remaining,
                hints.prompt.unwrap().text_opacity
            ),
            (255, 29, 255)
        );
        for (id, opacity, remaining, control) in [
            (2, 255, 29, false),
            (1, 255, 29, true),
            (99, 255, 29, true),
            (0, 255, 29, true),
            (2, 0, 29, true),
            (2, 255, 0, true),
            (2, 255, 30, true),
        ] {
            assert!(hints.apply_origin(id, opacity, remaining, control).is_err());
            assert_eq!((hints.opacity, hints.remaining), (255, 29));
        }
        hints.apply_origin(2, 200, 18, true).unwrap();
        assert_eq!(hints.prompt.unwrap().text_opacity, 228);
        hints.step(None, true);
        let prompt = hints.prompt.unwrap();
        assert_eq!(
            (prompt.opacity, prompt.text_opacity, hints.remaining),
            (188, 216, 17)
        );
    }

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
