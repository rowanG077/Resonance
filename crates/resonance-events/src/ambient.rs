//! The field's two spatial ambience channels (native channels 5 and 6).
use crate::{AudioCommand, GameWorld};

#[derive(Debug, Clone, Copy)]
pub struct AmbientSound {
    pub id: i16,
    pub volume: u8,
    pub radius: f32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Voice {
    id: i16,
    volume: u8,
    pan: u8,
}
impl GameWorld {
    pub(crate) fn actor_sound(&mut self, actor: i32, sound: AmbientSound) {
        if let Some(voice) = self
            .actors
            .get(&actor)
            .and_then(|a| self.spatial_voice(sound, a.position))
        {
            self.audio_commands.push(AudioCommand::Sound {
                id: voice.id,
                volume: voice.volume,
                pan: voice.pan,
                slot: None,
            });
        }
    }

    fn spatial_voice(&self, sound: AmbientSound, position: [f32; 3]) -> Option<Voice> {
        let listener = self.actors.get(&self.controlled_actor)?;
        let distance = position
            .iter()
            .zip(listener.position)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            .sqrt();
        let volume = if distance >= sound.radius {
            0
        } else {
            (f64::from(sound.volume)
                - f64::from(sound.volume) / f64::from(sound.radius) * f64::from(distance))
                as u8
        };
        let pan = self.field_camera.as_ref().map_or(64, |camera| {
            let direction: [f32; 3] =
                std::array::from_fn(|i| camera.target[i] - camera.position[i]);
            let horizontal = direction[0].hypot(direction[1]);
            let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
            if horizontal == 0. || length == 0. {
                return 64;
            }
            let offset: [f32; 3] = std::array::from_fn(|i| position[i] - camera.position[i]);
            let depth = offset
                .iter()
                .zip(direction)
                .map(|(a, b)| a * b / length)
                .sum::<f32>();
            let right = (offset[0] * direction[1] - offset[1] * direction[0]) / horizontal;
            let screen_x =
                320. + right / (depth * (camera.fov_degrees().to_radians() * 0.5).tan()) * 240.;
            (0.2 * screen_x).clamp(0., 127.) as u8
        });
        Some(Voice {
            id: sound.id,
            volume,
            pan,
        })
    }

    pub(crate) fn step_ambient_sound(&mut self) {
        if self.tick & 1 != 0 {
            return;
        }
        let mut candidates = std::collections::BTreeMap::<i16, Voice>::new();
        for actor in self.actors.values() {
            let Some(sound) = actor.ambient_sound else {
                continue;
            };
            let Some(voice) = self
                .spatial_voice(sound, actor.position)
                .filter(|v| v.volume > 0)
            else {
                continue;
            };
            if candidates
                .get(&sound.id)
                .is_none_or(|old| old.volume < voice.volume)
            {
                candidates.insert(sound.id, voice);
            }
        }
        let mut ranked: Vec<_> = candidates.into_values().collect();
        ranked.sort_by_key(|voice| (std::cmp::Reverse(voice.volume), voice.id));
        ranked.truncate(2);
        let mut next = [None; 2];
        // Keep channel ownership stable when the two loudest emitters trade rank.
        for (index, old) in self.ambient_voices.iter().enumerate() {
            if let Some(old) = old
                && let Some(i) = ranked.iter().position(|v| v.id == old.id)
            {
                next[index] = Some(ranked.remove(i));
            }
        }
        for voice in ranked {
            *next.iter_mut().find(|slot| slot.is_none()).unwrap() = Some(voice);
        }
        for (index, (old, new)) in self.ambient_voices.iter().zip(next).enumerate() {
            let slot = index as u16 + 5;
            match (old, new) {
                (Some(old), Some(new)) if old.id == new.id => {
                    if old.volume != new.volume {
                        self.audio_commands.push(AudioCommand::SoundVolume {
                            slot,
                            volume: new.volume,
                        });
                    }
                    if old.pan != new.pan {
                        self.audio_commands
                            .push(AudioCommand::SoundPan { slot, pan: new.pan });
                    }
                }
                (_, Some(new)) => self.audio_commands.push(AudioCommand::RepeatSound {
                    id: new.id,
                    volume: new.volume,
                    pan: new.pan,
                    slot: slot as u8,
                }),
                (Some(_), None) => self.audio_commands.push(AudioCommand::StopSound(slot)),
                _ => {}
            }
        }
        self.ambient_voices = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Actor;
    #[test]
    fn emitters_deduplicate_preserve_channels_and_stop_out_of_range() {
        let mut world = GameWorld {
            controlled_actor: 1,
            ..Default::default()
        };
        world.actors.insert(1, Actor::new(1, [0.; 3]));
        for (actor, id, x) in [(10, 70, 0.), (11, 70, 90.), (12, 71, 50.)] {
            let mut emitter = Actor::new(0, [x, 0., 0.]);
            emitter.ambient_sound = Some(AmbientSound {
                id,
                volume: 100,
                radius: 100.,
            });
            world.actors.insert(actor, emitter);
        }
        world.step_ambient_sound();
        assert_eq!(world.audio_commands.len(), 2);
        assert!(matches!(
            world.audio_commands[0],
            AudioCommand::RepeatSound {
                id: 70,
                slot: 5,
                volume: 100,
                ..
            }
        ));
        world.audio_commands.clear();
        world.actors.get_mut(&10).unwrap().position[0] = 80.;
        world.step_ambient_sound();
        assert!(matches!(
            world.audio_commands.as_slice(),
            [AudioCommand::SoundVolume {
                slot: 5,
                volume: 19
            }] | [AudioCommand::SoundVolume {
                slot: 5,
                volume: 20
            }]
        ));
        world.audio_commands.clear();
        world.actors.get_mut(&1).unwrap().position[0] = 1000.;
        world.step_ambient_sound();
        assert!(matches!(
            world.audio_commands.as_slice(),
            [AudioCommand::StopSound(5), AudioCommand::StopSound(6)]
        ));
    }
}
