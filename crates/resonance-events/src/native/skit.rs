use super::*;
use crate::skit::{Media, Portrait, Request};

impl NativeHost<'_> {
    pub(super) fn request_skit(
        &mut self,
        op: NativeCall,
        a: &[i32],
        _: &mut Memory,
    ) -> Result<NativeResult, String> {
        let id = u16::try_from(a[0]).map_err(|_| "invalid skit id")?;
        let preview = op == NativeCall::PreviewSkit;
        if !preview
            && a[2] == 0
            && self
                .world
                .party
                .as_ref()
                .is_some_and(|p| p.viewed_skits.contains(&id))
        {
            return Ok(NativeResult::Continue(None));
        }
        require(
            self.world.skit_request.is_none() && self.world.skit.is_none(),
            "nested skit request",
        )?;
        require(
            self.resources
                .skits
                .as_ref()
                .is_some_and(|c| c.resources.contains_key(&id)),
            "skit is not cooked",
        )?;
        let operation = self.world.operations.begin()?;
        *self.wait = Some(Wait::Complete(operation.clone()));
        self.world.skit_request = Some(Request {
            id,
            preview,
            skippable: preview || a[1] != 0,
            operation,
        });
        Ok(NativeResult::Suspend)
    }

    pub(super) fn skit(
        &mut self,
        op: NativeCall,
        a: &[i32],
        memory: &mut Memory,
    ) -> Result<NativeResult, String> {
        let catalog = self
            .resources
            .skits
            .as_ref()
            .ok_or("skit catalog missing")?;
        // Draw random phases before borrowing the portrait scene.
        let random = if op == NativeCall::CreatePortrait {
            [
                self.world.random() as u16 % 100,
                self.world.random() as u16 % 100,
                0,
            ]
        } else {
            [0; 3]
        };
        let scene = self
            .world
            .skit
            .as_mut()
            .ok_or("portrait service outside a skit")?;
        match op {
            NativeCall::LoadPortrait => {
                require(
                    catalog.portraits.contains_key(&(a[0] as u32)),
                    "portrait is not cooked",
                )?;
                return Ok(NativeResult::Continue(Some(a[0])));
            }
            NativeCall::CreatePortrait => {
                let resource = a[1] as u32;
                let asset = catalog
                    .portraits
                    .get(&resource)
                    .ok_or("portrait handle is not loaded")?;
                let timeline = catalog
                    .portraits
                    .get(&(a[12] as u32))
                    .ok_or("portrait timeline is not cooked")?;
                require(
                    asset.layout_sha256 == timeline.layout_sha256,
                    "portrait timeline layout differs from cooked atlas",
                )?;
                let slot = (0..32)
                    .find(|slot| !scene.portraits.contains_key(slot))
                    .ok_or("too many portraits")?;
                let color = std::array::from_fn(|i| a[7 + i] as u8 as f32 / 255.);
                let fade = a[11].max(1) as f32;
                scene.portraits.insert(
                    slot,
                    Portrait {
                        id: a[0],
                        resource,
                        position: [a[2] as f32, a[3] as f32],
                        size: std::array::from_fn(|i| {
                            if a[4 + i] == -1 {
                                asset.size[i] as f32
                            } else {
                                a[4 + i] as f32
                            }
                        }),
                        angle: a[6] as f32,
                        tilt: [0.; 2],
                        color,
                        opacity: if fade <= 1. { color[3] } else { 0. },
                        fade_step: color[3] / fade,
                        scale: 1.,
                        scale_target: 1.,
                        scale_step: 0.,
                        talking: false,
                        images: std::array::from_fn(|i| {
                            asset.tracks[i].first().map_or(0, |f| f.image)
                        }),
                        cursors: [0; 3],
                        forced: [None; 2],
                        counters: random,
                    },
                );
            }
            NativeCall::DespawnActor => {
                scene.portraits.retain(|_, p| p.id != a[0]);
            }
            NativeCall::SetActorProperty | NativeCall::GetActorProperty => {
                require(
                    matches!(a[1], 1 | 2 | 35..=37 | 64 | 65),
                    "unsupported portrait property",
                )?;
                if let Some(portrait) = scene.portraits.values_mut().find(|p| p.id == a[0]) {
                    let scalar = match a[1] {
                        1 | 2 => Some(&mut portrait.position[(a[1] - 1) as usize]),
                        35 | 36 => Some(&mut portrait.tilt[(a[1] - 35) as usize]),
                        37 => Some(&mut portrait.angle),
                        _ => None,
                    };
                    if let Some(scalar) = scalar {
                        let old = *scalar as i32;
                        if op == NativeCall::SetActorProperty {
                            *scalar = a[2] as f32;
                        }
                        return Ok(NativeResult::Continue(Some(old)));
                    }
                    let channel = (a[1] - 64) as usize;
                    if op == NativeCall::SetActorProperty {
                        let track = &catalog.portraits[&portrait.resource].tracks[channel];
                        if track.is_empty() {
                            return Ok(NativeResult::Continue(Some(0)));
                        }
                        require(
                            a[2] == -1 || (0..track.len() as i32).contains(&a[2]),
                            "portrait expression index out of range",
                        )?;
                        portrait.forced[channel] = (a[2] >= 0).then_some(a[2] as usize);
                    }
                }
                return Ok(NativeResult::Continue(Some(0)));
            }
            NativeCall::ScalePortrait => {
                let Some(portrait) = scene.portraits.values_mut().find(|p| p.id == a[0]) else {
                    return Ok(NativeResult::Continue(None));
                };
                portrait.scale_target = a[1] as f32 / 100.;
                portrait.scale_step = (portrait.scale_target - portrait.scale) / a[2].max(1) as f32;
            }
            NativeCall::SetPortraitTalking => {
                const EXCLUSIVE: i32 = 0xae71b;
                const STOP_ALL: i32 = 0x604f1;
                if matches!(a[1], EXCLUSIVE | STOP_ALL) {
                    for portrait in scene.portraits.values_mut() {
                        portrait.talking = false;
                    }
                }
                if a[1] != STOP_ALL
                    && let Some(portrait) = scene.portraits.values_mut().find(|p| p.id == a[0])
                {
                    portrait.talking = true;
                }
            }
            NativeCall::SetSkitSubtitle => {
                let message = self
                    .resources
                    .messages
                    .get(usize::try_from(a[2]).map_err(|_| "invalid subtitle index")?)
                    .ok_or("skit subtitle missing")?;
                let resolved = crate::dialogue::resolve(
                    message,
                    memory,
                    &self.resources.names(self.world.party.as_ref()),
                    &self.resources.text,
                    self.world.controlled_actor,
                )?;
                scene.subtitle.clear();
                for token in resolved.tokens {
                    match token {
                        crate::dialogue::TextToken::Text { text } => scene.subtitle.push_str(&text),
                        other => return Err(format!("unsupported skit subtitle token {other:?}")),
                    }
                }
                scene.subtitle_started = self.world.tick;
                scene.panel_started.get_or_insert(self.world.tick);
            }
            NativeCall::PlayMovie => {
                self.world
                    .audio_commands
                    .push(crate::AudioCommand::StopVoice);
                if a[0] == -1 {
                    scene.media = None;
                } else {
                    let id = a[0] as u32;
                    let media = catalog.media.get(&id).ok_or("skit media is not cooked")?;
                    scene.media = Some(Media {
                        id,
                        started: self.world.tick,
                        frames: media.frames,
                        sample_rate: media.sample_rate,
                    });
                    if media.voice.is_some() {
                        self.world
                            .audio_commands
                            .push(crate::AudioCommand::Voice(id));
                    }
                    return self.yield_update();
                }
            }
            NativeCall::WaitMediaPosition | NativeCall::YieldCommand => {
                let (command, value) = if op == NativeCall::WaitMediaPosition {
                    (19, a[0])
                } else {
                    (a[0], a[1])
                };
                return self.yield_command(command, value);
            }
            _ => return Err(format!("unsupported skit native {op:?}")),
        }
        Ok(NativeResult::Continue(None))
    }
}
