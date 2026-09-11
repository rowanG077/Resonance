use crate::Operation;

/// Dialogue style and behavior bits supplied by script bindings.
pub mod flags {
    pub const PERSISTENT: u16 = 0x02;
    pub const FRAMELESS: u16 = 0x04;
    pub const RED: u16 = 0x08;
    pub const GREEN: u16 = 0x10;
    pub const AUTO_PAGES: u16 = 0x20;
    pub const AUTO_SIDE: u16 = 0x40;
    pub const ABOVE: u16 = 0x80;
    pub const BELOW: u16 = 0x100;
    pub const POINTER: u16 = 0x400;
    pub const INSTANT: u16 = 0x1000;
    pub const PLACEMENT: u16 = AUTO_SIDE | ABOVE | BELOW;
}
pub const DIALOGUE_SLOTS: u8 = 3;
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::message::Message;
use symphonia_script_vm::{Host, Memory, RunEvent, Vm};

#[derive(Debug, Clone)]
pub enum TextToken {
    Text { text: String },
    Control { opcode: u8, value: i32 },
}
#[derive(Debug, Clone)]
pub struct ResolvedMessage {
    pub tokens: Vec<TextToken>,
}
impl ResolvedMessage {
    pub fn from_spans(spans: &[resonance_content::font::TextSpan]) -> Self {
        Self {
            tokens: spans
                .iter()
                .flat_map(|span| {
                    [
                        TextToken::Control {
                            opcode: 3,
                            value: i32::from(span.color),
                        },
                        TextToken::Text {
                            text: span.text.clone(),
                        },
                    ]
                })
                .collect(),
        }
    }
}

impl crate::GameWorld {
    /// Open a centered notice using the same renderer and cancellation lifetime
    /// as script dialogue. The owning game service controls player input.
    pub fn show_notice(&mut self, body: ResolvedMessage, flags: u16) -> Result<Operation, String> {
        let slot = (0..DIALOGUE_SLOTS)
            .find(|slot| {
                self.dialogue
                    .get(slot)
                    .is_none_or(|d| !d.operation.is_pending())
            })
            .ok_or("all dialogue slots are occupied")?;
        let operation = self.operations.begin()?;
        self.dialogue.insert(
            slot,
            Dialogue {
                operation: operation.clone(),
                speaker: ResolvedMessage { tokens: Vec::new() },
                body,
                anchor: DialogueAnchor::Screen([0., 0.]),
                speaker_actor: None,
                opening_actor: None,
                flags,
                dimensions: None,
                height_offset: 0,
            },
        );
        Ok(operation)
    }
}
struct ExpressionHost;
impl Host for ExpressionHost {}

/// Evaluate message expressions against shared scenario memory using the same
/// VM arithmetic and reference rules as event scripts.
pub(crate) fn resolve(
    message: &Message,
    memory: &mut Memory,
    names: &BTreeMap<i32, String>,
    text: &resonance_content::session::GameText,
    controlled: i32,
) -> Result<ResolvedMessage, String> {
    use symphonia_script::message::Token;
    let mut tokens = Vec::new();
    for token in &message.tokens {
        match token {
            Token::Text { text } => tokens.push(TextToken::Text { text: text.clone() }),
            Token::Raw { value } => return Err(format!("undecoded message byte {value:#x}")),
            Token::Control { opcode, expression } => {
                if expression.len() > 4096 || !expression.len().is_multiple_of(2) {
                    return Err("invalid message expression length".into());
                }
                let mut bytes = vec![0, 4, 0, 0, 0, 0, 0, 0];
                bytes.extend(expression);
                let program =
                    Arc::new(symphonia_script::Program::decode(&bytes).map_err(|e| e.to_string())?);
                let mut vm = Vm::new(program, 0).map_err(|e| e.to_string())?;
                let result = vm
                    .run(&mut ExpressionHost, memory, 1024)
                    .map_err(|e| e.to_string())?;
                if result.event != RunEvent::Halted {
                    return Err("message expression suspended".into());
                }
                let value = vm.expression().ok_or("message expression has no result")?;
                match opcode {
                    1 => {
                        let id = if value == crate::CONTROLLED_ACTOR {
                            controlled
                        } else {
                            value
                        };
                        tokens.push(TextToken::Text {
                            text: names
                                .get(&id)
                                .ok_or("message character name is missing")?
                                .clone(),
                        });
                    }
                    4 | 0x11 => tokens.push(TextToken::Text {
                        text: (if *opcode == 4 {
                            &text.items
                        } else {
                            &text.titles
                        })
                        .get(&u16::try_from(value).map_err(|_| "invalid message label index")?)
                        .ok_or("message item/title name is not cooked")?
                        .clone(),
                    }),
                    5 => tokens.push(TextToken::Text {
                        text: value.to_string(),
                    }),
                    2 | 3 | 7 | 8 | 9 => tokens.push(TextToken::Control {
                        opcode: *opcode,
                        value,
                    }),
                    6 => {} // The original evaluates this expression without emitting text.
                    _ => {
                        return Err(format!(
                            "message substitution {opcode:#x} is not implemented"
                        ));
                    }
                }
            }
        }
    }
    Ok(ResolvedMessage { tokens })
}

#[derive(Debug, Clone)]
pub enum DialogueAnchor {
    Actor(i32),
    ScreenGrid(u8),
    Screen([f32; 2]),
}

#[derive(Debug, Clone)]
pub struct Dialogue {
    pub operation: Operation,
    pub speaker: ResolvedMessage,
    pub body: ResolvedMessage,
    pub anchor: DialogueAnchor,
    pub speaker_actor: Option<i32>,
    /// Wait for the speaker’s current turn before opening. Clear once;
    /// a later turn must not close and reopen the same window.
    pub opening_actor: Option<i32>,
    pub flags: u16,
    /// Explicit body dimensions, or measure every page using the bitmap font.
    pub dimensions: Option<[u16; 2]>,
    /// Added to the speaker's head attachment for an automatically sized box.
    pub height_offset: i16,
}

impl Dialogue {
    /// Labels and descriptions stay open until the owning script closes them.
    pub fn persistent(&self) -> bool {
        self.flags & (flags::PERSISTENT | flags::AUTO_PAGES) != 0
    }
}

/// A selection over a contiguous range of lines in an existing dialogue.
/// Line indices are zero based here; the native binding returns one based lines.
#[derive(Debug, Clone)]
pub struct Choice {
    pub operation: Operation,
    pub first_line: u8,
    pub last_line: u8,
    pub selected_line: u8,
    pub cancel_allowed: bool,
    pub timeout_ticks: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceExit {
    Confirm,
    Cancel,
    Timeout,
}

impl Choice {
    pub fn finish(&self, reason: ChoiceExit) -> Result<(), String> {
        if reason == ChoiceExit::Cancel && !self.cancel_allowed {
            return Err("this choice cannot be cancelled".into());
        }
        // The completion position carries the reason to the native adapter.
        // The result itself remains a normal deferred VM value.
        self.operation.advance(match reason {
            ChoiceExit::Confirm => 0,
            ChoiceExit::Cancel => 1,
            ChoiceExit::Timeout => 2,
        })?;
        self.operation
            .complete(Some(i32::from(self.selected_line) + 1))
    }
}

#[derive(Debug, Clone)]
pub struct Movie {
    pub resource: u32,
    /// Full-screen story playback owns the scene until it completes.
    pub blocking: bool,
    /// Ready means decoded frames can be presented; position is the presented
    /// frame index. Complete only after playback ends or the player skips.
    pub operation: Operation,
}
