use super::{NativeHost, NativeResult, Wait, require};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Ticks,
    Resource,
    DialogueClosed,
    DialogueReady,
    ActorMotion,
    ActorAnimation,
    ActorHeading,
    Camera,
    CameraPath(u8),
    MediaReady,
    MediaComplete,
    MediaLoaded,
    MediaPosition,
}

impl Command {
    fn decode(code: i32) -> Option<Self> {
        Some(match code {
            0 => Self::Ticks,
            1 => Self::Resource,
            2 => Self::DialogueClosed,
            3 => Self::DialogueReady,
            4 => Self::ActorMotion,
            7 => Self::ActorAnimation,
            8 => Self::Camera,
            9..=13 => Self::CameraPath(code as u8),
            14 | 16 => Self::MediaReady,
            15 => Self::MediaComplete,
            17 => Self::MediaLoaded,
            18 => Self::ActorHeading,
            19 => Self::MediaPosition,
            _ => return None,
        })
    }
}

impl NativeHost<'_> {
    pub(super) fn yield_command(&mut self, code: i32, value: i32) -> Result<NativeResult, String> {
        use Command::*;
        let skit = self.world.skit.is_some();
        let unsupported = || {
            if skit {
                format!("unsupported skit wait {code}")
            } else {
                format!("unsupported wait condition {code}")
            }
        };
        let command = Command::decode(code).ok_or_else(unsupported)?;
        let condition = match (command, skit) {
            (Ticks, _) => {
                require(value >= 0, "negative wait duration")?;
                Wait::Tick(
                    self.world
                        .tick
                        .checked_add(value.max(1) as u32)
                        .ok_or(if skit {
                            "skit wait overflow"
                        } else {
                            "wait clock overflow"
                        })?,
                )
            }
            (Resource, _) => {
                if skit {
                    require(
                        self.resources
                            .skits
                            .as_ref()
                            .ok_or("skit catalog missing")?
                            .portraits
                            .contains_key(&(value as u32)),
                        "wait for uncooked portrait",
                    )?;
                } else {
                    require(
                        self.world.loaded_resources.contains_key(&value),
                        "wait refers to an unloaded resource",
                    )?;
                    if let Some(observations) = self.resource_waits {
                        let observation = observations.front().ok_or("unobserved resource wait")?;
                        require(
                            observation.request_tick == self.world.tick
                                && self.resources.bindings.get(&observation.resource)
                                    == self.world.loaded_resources.get(&value),
                            "resource request differs from the observed tick or resource",
                        )?;
                        *self.resource_wait = Some(*observation);
                        *self.wait = Some(Wait::Tick(observation.resume_tick));
                        return Ok(NativeResult::Suspend);
                    }
                }
                // Scene readiness already made cooked dependencies resident.
                return Ok(NativeResult::Continue(None));
            }
            (DialogueClosed | DialogueReady, _) => {
                let Some(dialogue) = self.world.dialogue.get(&(value as u8)) else {
                    return self.yield_update();
                };
                if command == DialogueClosed {
                    Wait::Complete(dialogue.operation.clone())
                } else {
                    Wait::Ready(dialogue.operation.clone())
                }
            }
            // Portrait scenes have no actor motion or media preparation delay.
            (ActorMotion | MediaReady, true) => return self.yield_update(),
            (MediaComplete | MediaLoaded | MediaPosition, true) => Wait::SkitMedia {
                id: (command == MediaLoaded).then_some(value as u32),
                position: (command == MediaPosition && value >= 0).then_some(value as u32),
            },
            (ActorMotion, false) => Wait::ActorMotion(value),
            (ActorAnimation, false) => Wait::ActorAnimation(value),
            (ActorHeading, false) => Wait::ActorHeading(value),
            (Camera, false) => Wait::Camera {
                after: self.world.tick,
            },
            (CameraPath(channel), false) => Wait::CameraPath {
                after: self.world.tick,
                channel,
            },
            (MediaReady | MediaComplete, false) if self.world.voice.is_some() => {
                if command == MediaReady {
                    return self.yield_update();
                }
                Wait::Voice
            }
            (MediaReady | MediaComplete | MediaPosition, false) => {
                let Some(movie) = &self.world.movie else {
                    return self.yield_update();
                };
                match command {
                    MediaReady => Wait::Ready(movie.operation.clone()),
                    MediaPosition if value >= 0 => {
                        Wait::Position(movie.operation.clone(), value as u32)
                    }
                    _ => Wait::Complete(movie.operation.clone()),
                }
            }
            _ => return Err(unsupported()),
        };
        let mut wait = if matches!(condition, Wait::Tick(_)) {
            condition
        } else {
            Wait::Service {
                condition: Box::new(condition),
                ready_at: None,
            }
        };
        // Field services observe readiness immediately. Skits first observe it
        // on the next update; both resume one update after that observation.
        if !skit && wait.poll(self.world)? {
            return Ok(NativeResult::Continue(None));
        }
        *self.wait = Some(wait);
        Ok(NativeResult::Suspend)
    }
}
