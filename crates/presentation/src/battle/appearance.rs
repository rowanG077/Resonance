//! Colors derived from the completed battle view, with brief presentation-owned flashes.
use resonance_battle::{
    Actor, BattleFrame, Control, Cue, ModelMaterial, Side,
    conditions::{Condition, POISON},
};

const FLASH_TICKS: u8 = 6; // One tenth of a second at 60 updates/second.

#[derive(Clone, Copy)]
struct Flash {
    color: [u8; 3],
    remaining: u8,
}

#[derive(Default)]
pub(super) struct Appearance {
    flashes: Vec<Option<Flash>>,
}

impl Appearance {
    pub fn advance(&mut self, frame: &mut BattleFrame, paused: bool) {
        let combat = frame.recognized_result.is_none() && frame.outcome.is_none();
        if !combat {
            self.flashes.clear();
        }
        self.flashes.resize(frame.actors.len(), None);
        if !paused {
            for flash in &mut self.flashes {
                if let Some(active) = flash {
                    active.remaining -= 1;
                    if active.remaining == 0 {
                        *flash = None;
                    }
                }
            }
        }
        if combat {
            for cue in &frame.cues {
                if let Cue::ActorFlash { actor, color } = *cue
                    && let Some(flash) = self.flashes.get_mut(actor.index())
                {
                    *flash = Some(Flash {
                        color,
                        remaining: FLASH_TICKS,
                    });
                }
            }
        }
        let selector = frame.target_selector.map(|actor| actor.index());
        let selected = combat
            .then(|| {
                selector
                    .or_else(|| {
                        frame.actors.iter().position(|actor| {
                            actor.side == Side::Party && actor.control != Control::Auto
                        })
                    })
                    .and_then(|index| frame.targets[index])
            })
            .flatten();
        for model in &mut frame.models {
            let index = model.actor.index();
            let actor = &frame.actors[index];
            let mut tint = model.tint;
            let material = if actor.time_stop != 0
                || actor.conditions.effective().contains(Condition::Petrified)
            {
                tint[..3].fill(64);
                ModelMaterial::RedChannel
            } else {
                if actor.available() && actor.conditions.effective().intersects(POISON) {
                    tint[..3].copy_from_slice(&[32, 64, 32]);
                }
                if let Some(flash) = self.flashes[index] {
                    for (base, color) in tint[..3].iter_mut().zip(flash.color) {
                        *base = ((u16::from(color) * u16::from(flash.remaining)
                            + u16::from(*base) * u16::from(FLASH_TICKS - flash.remaining))
                            / u16::from(FLASH_TICKS)) as u8;
                    }
                }
                ModelMaterial::Normal
            };
            if actor.available() && selected.is_some_and(|target| target.index() != index) {
                let scale = if selector.is_some() {
                    0.5
                } else if actor.side == Side::Enemy {
                    0.8
                } else {
                    1.
                };
                for channel in &mut tint[..3] {
                    *channel = (f32::from(*channel) * scale) as u8;
                }
            }
            (model.tint, model.material) = (tint, material);
        }
        for weapon in &mut frame.weapons {
            if let Some(model) = frame
                .models
                .iter()
                .find(|model| model.actor == weapon.owner)
            {
                (weapon.tint, weapon.material) = (model.tint, model.material);
            }
        }
    }
}

/// Low health and a nearly full or active Over Limit flash the body's outline.
pub(crate) fn outline(actor: &Actor, combat: bool, update: u64, alpha: u8) -> [u8; 4] {
    let warning = actor.overlimit.is_active()
        || actor.overlimit.charge() >= 900
        || (actor.side == Side::Enemy && actor.hp_percent() <= 10);
    let red = if combat && actor.available() && warning && update % 4 >= 2 {
        128
    } else {
        0
    };
    [red, 0, 0, alpha / 2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{BattleResult, ModelFrame, PreparedBattle, WeaponFrame};

    #[test]
    fn appearance_follows_conditions_targets_holds_and_results_without_changing_actors() {
        let mut actors = vec![crate::test_support::actor(Side::Party, 100, 20)];
        actors.extend((0..2).map(|_| crate::test_support::actor(Side::Enemy, 100, 20)));
        actors[0].control = Control::Manual;
        let core = PreparedBattle::new(
            (actors)
                .into_iter()
                .map(|actor| (actor, Default::default()))
                .collect(),
            Default::default(),
            1,
        )
        .unwrap()
        .finish()
        .unwrap();
        let mut frame = core.snapshot();
        let ids: Vec<_> = core.actor_ids().collect();
        frame.models = ids
            .iter()
            .map(|&actor| ModelFrame {
                actor,
                visible: true,
                depth_write: true,
                tint: [0; 4],
                material: ModelMaterial::Normal,
                texture_layers: [0; 4],
                light: None,
                shadow: None,
                resource: 0,
                clip: 0,
                frame: 0.,
                blend_weight: 0.,
                root_translation: [0.; 3],
                world: [[0.; 4]; 4],
                bones: Default::default(),
            })
            .collect();
        frame.weapons.push(WeaponFrame {
            texture_scroll: None,
            owner: ids[0],
            slot: 0,
            visible: true,
            tint: [0; 4],
            material: ModelMaterial::Normal,
            resource: 1,
            clip: None,
            frame: 0.,
            world: [[0.; 4]; 4],
            bones: Default::default(),
            links: vec![],
        });
        frame.targets[0] = Some(ids[1]);
        frame.actors[1].conditions =
            resonance_battle::conditions::Conditions::new(resonance_battle::conditions::Layers {
                base: Condition::PoisonMild.into(),
                ..Default::default()
            });
        frame.actors[2].time_stop = 30;
        let actors = frame.actors.clone();
        let mut appearance = Appearance::default();
        let mut render = |frame: &mut BattleFrame, paused| {
            // Each publication arrives with fresh base colors from model playback.
            for model in &mut frame.models {
                model.tint = if model.actor == ids[0] {
                    [40, 50, 60, 173]
                } else {
                    [64, 64, 64, 173]
                };
            }
            appearance.advance(frame, paused);
        };
        frame.cues.push(Cue::ActorFlash {
            actor: ids[0],
            color: [192, 128, 128],
        });
        render(&mut frame, false);
        assert_eq!(frame.models[0].tint, [192, 128, 128, 173]);
        assert_eq!(frame.weapons[0].tint, frame.models[0].tint);
        assert_eq!(frame.models[1].tint, [32, 64, 32, 173]);
        assert_eq!(frame.models[2].material, ModelMaterial::RedChannel);
        let held = frame.models.clone();
        frame.cues.clear();
        for _ in 0..20 {
            render(&mut frame, true);
        }
        assert_eq!(frame.models, held);
        for _ in 0..20 {
            render(&mut frame, false);
        }
        assert_eq!(frame.models[0].tint, [40, 50, 60, 173]);
        assert_eq!(frame.weapons[0].tint, frame.models[0].tint);
        frame.target_selector = Some(ids[0]);
        frame.targets[0] = Some(ids[2]);
        render(&mut frame, true);
        assert_eq!(frame.models[1].tint, [16, 32, 16, 173]);
        assert_eq!(frame.models[2].tint, [64, 64, 64, 173]);
        assert_eq!(frame.actors, actors);
        frame.target_selector = None;
        frame.cues.push(Cue::ActorFlash {
            actor: ids[0],
            color: [128, 192, 128],
        });
        render(&mut frame, false);
        frame.recognized_result = Some(BattleResult::Victory);
        render(&mut frame, true);
        assert_eq!(frame.models[0].tint, [40, 50, 60, 173]);
        assert_eq!(frame.weapons[0].tint, frame.models[0].tint);
        frame.actors[0].overlimit = resonance_battle::OverLimit::active(100).unwrap();
        assert_eq!(outline(&frame.actors[0], true, 2, 173), [128, 0, 0, 86]);
        assert_eq!(outline(&frame.actors[0], false, 2, 173), [0, 0, 0, 86]);
        frame.actors[2].hp = 10;
        for (update, red) in [(0, 0), (1, 0), (2, 128), (3, 128), (4, 0)] {
            assert_eq!(
                outline(&frame.actors[2], true, update, 173),
                [red, 0, 0, 86]
            );
        }
        frame.actors[2].hp = 11;
        assert_eq!(outline(&frame.actors[2], true, 2, 173), [0, 0, 0, 86]);
        for (charge, red) in [(899, 0), (900, 128)] {
            frame.actors[2].overlimit = resonance_battle::OverLimit::new(charge).unwrap();
            assert_eq!(outline(&frame.actors[2], true, 2, 173), [red, 0, 0, 86]);
        }
        assert_eq!(outline(&frame.actors[2], true, 2, 0)[3], 0);
    }
}
