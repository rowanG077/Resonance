//! Fatal-defeat mode12 (7202C). No retained field operation can complete here.
use crate::{DirectionRepeat, field::FieldInput};
use anyhow::{Result, ensure};
use resonance_content::{
    font::BitmapFont,
    game_over::{Art, PATH},
    prepared::{Cache, Files},
};
use std::path::Path;

#[derive(Clone)]
pub struct Assets {
    pub art: Art,
    pub font: BitmapFont,
    pub files: Files,
}
impl Assets {
    pub fn load(
        root: &Path,
        files: Files,
        cache: &mut Cache,
        cancelled: impl Fn() -> bool,
    ) -> Result<(Files, Self)> {
        let art: Art = files.json(PATH)?;
        art.validate()?;
        let files = files.with_dependencies(root, art.files.clone(), cache, cancelled)?;
        let font: BitmapFont = files.json(&art.font)?;
        font.validate()?;
        ensure!(
            art.files.contains_key(&font.texture),
            "unlisted game-over font image"
        );
        files
            .diagnostics()
            .attempt("game-over font image", files.read(&font.texture))?;
        ensure!(
            std::iter::once(&art.caption)
                .chain(&art.choices)
                .flat_map(|s| s.chars())
                .all(|c| font.glyphs.contains_key(&c)),
            "uncooked game-over text"
        );
        Ok((files.clone(), Self { art, font, files }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    Load,
    Title,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Navigate,
    Confirm,
}
#[derive(Default)]
pub struct Update {
    pub cue: Option<Cue>,
    pub destination: Option<Destination>,
}
pub struct Screen {
    pub selected: usize,
    pub alpha: u8,
    closing: bool,
    repeat: [DirectionRepeat; 2],
    held: [bool; 2],
}
impl Default for Screen {
    fn default() -> Self {
        Self {
            selected: 0,
            alpha: 255,
            closing: false,
            repeat: Default::default(),
            held: [false; 2],
        }
    }
}
impl Screen {
    pub fn closing(&self) -> bool {
        self.closing
    }

    /// The committed route is available only after the closing fade completes.
    pub fn destination(&self) -> Option<Destination> {
        (self.closing && self.alpha == 255).then_some(if self.selected == 0 {
            Destination::Load
        } else {
            Destination::Title
        })
    }

    /// 184A0 advances the fade before 72784 examines input. Navigation remains
    /// available during entry; only A-confirm waits for the fade to reach zero.
    pub fn step(&mut self, input: FieldInput, phase: u32) -> Update {
        self.alpha = if self.closing {
            self.alpha.saturating_add(8)
        } else {
            self.alpha.saturating_sub(8)
        };
        let mut result = Update::default();
        if !self.closing {
            let held = [input.direction[1] > 0.5, input.direction[1] < -0.5];
            let directions = std::array::from_fn::<_, 2, _>(|i| {
                self.repeat[i].step(held[i], held[i] && !self.held[i], phase)
            });
            self.held = held;
            if directions[0] || directions[1] {
                self.selected ^= 1;
                result.cue = Some(Cue::Navigate);
            }
            if self.alpha == 0 && input.interact {
                self.closing = true;
                self.alpha = 1;
                result.cue = Some(Cue::Confirm);
            }
        }
        result.destination = self.destination();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entry_navigation_confirmation_and_both_routes_follow_source_fades() {
        for choice in 0..2 {
            let mut screen = Screen::default();
            let first = screen.step(
                FieldInput {
                    direction: [0., choice as f32],
                    interact: true,
                    ..Default::default()
                },
                1,
            );
            assert_eq!(screen.selected, choice);
            assert_eq!(screen.alpha, 247);
            assert!(first.destination.is_none());
            for phase in 2..32 {
                screen.step(FieldInput::default(), phase);
            }
            let confirming = screen.step(
                FieldInput {
                    interact: true,
                    ..Default::default()
                },
                32,
            );
            assert_eq!(confirming.cue, Some(Cue::Confirm));
            assert_eq!(screen.alpha, 1);
            for phase in 33..64 {
                assert!(
                    screen
                        .step(
                            FieldInput {
                                direction: [0., 1.],
                                interact: true,
                                ..Default::default()
                            },
                            phase
                        )
                        .destination
                        .is_none()
                );
                assert_eq!(screen.selected, choice);
            }
            assert_eq!(
                screen.step(FieldInput::default(), 64).destination,
                Some(if choice == 0 {
                    Destination::Load
                } else {
                    Destination::Title
                })
            );
        }
    }
    #[test]
    fn cancel_and_start_do_not_confirm() {
        let mut screen = Screen::default();
        for phase in 0..100 {
            assert!(
                screen
                    .step(
                        FieldInput {
                            cancel: true,
                            start: true,
                            ..Default::default()
                        },
                        phase
                    )
                    .destination
                    .is_none()
            );
        }
        assert_eq!(screen.alpha, 0);
    }
}
