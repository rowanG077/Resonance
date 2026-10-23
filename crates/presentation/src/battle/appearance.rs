//! Colors derived from the completed battle view, with brief presentation-owned flashes.
use resonance_battle::{
    Actor, BattleFrame, Control, Cue, ModelMaterial, Side,
    conditions::{Condition, PHYSICAL_AILMENTS, POISON},
};

#[derive(Default)]
pub(super) struct Appearance {
    colors: Vec<ActorColors>,
}

#[derive(Clone, Copy, Default)]
struct ActorColors {
    flash: Option<[u8; 3]>,
    pending_flash: Option<[u8; 3]>,
    pulse: Option<([u8; 3], u8)>,
}

impl Appearance {
    pub fn advance(&mut self, frame: &mut BattleFrame, paused: bool, actor_ticks: &[u16]) {
        let combat = frame.recognized_result.is_none() && frame.outcome.is_none();
        self.colors
            .resize(frame.actors.len(), ActorColors::default());
        if !paused {
            for model in &frame.models {
                if frame.paused_actors.contains(&model.actor) {
                    continue;
                }
                let index = model.actor.index();
                let actor = &frame.actors[index];
                let base = body_tint(actor, model.tint, actor_ticks.get(index).copied());
                let colors = &mut self.colors[index];
                if let Some((_, remaining)) = &mut colors.pulse {
                    *remaining -= 1;
                    if *remaining == 0 {
                        colors.pulse = None;
                    }
                }
                let flash = &mut colors.flash;
                if actor.conditions.effective().intersects(PHYSICAL_AILMENTS) {
                    *flash = None;
                    continue;
                }
                if let Some(color) = flash {
                    fade_color(color, base);
                    if color[..] == base[..3] {
                        *flash = None;
                    }
                }
            }
        }
        for (colors, actor) in self.colors.iter_mut().zip(&frame.actors) {
            if let Some(color) = colors.pending_flash.take()
                && combat
                && !actor.conditions.effective().intersects(PHYSICAL_AILMENTS)
            {
                colors.flash = Some(color);
            }
        }
        if combat {
            for cue in &frame.cues {
                match *cue {
                    Cue::ActorFlash { actor, color }
                        if !frame.actors[actor.index()]
                            .conditions
                            .effective()
                            .intersects(PHYSICAL_AILMENTS) =>
                    {
                        self.colors[actor.index()].flash = Some(color);
                        self.colors[actor.index()].pending_flash = None;
                    }
                    Cue::DeferredFlash {
                        actor,
                        mut color,
                        fade_first,
                    } => {
                        if fade_first
                            && let Some(model) =
                                frame.models.iter().find(|model| model.actor == actor)
                        {
                            fade_color(
                                &mut color,
                                body_tint(
                                    &frame.actors[actor.index()],
                                    model.tint,
                                    actor_ticks.get(actor.index()).copied(),
                                ),
                            );
                        }
                        self.colors[actor.index()].pending_flash = Some(color);
                    }
                    Cue::ActorPulse { actor, color } => {
                        const PULSE_TICKS: u8 = 2;
                        self.colors[actor.index()].pulse = Some((color, PULSE_TICKS));
                    }
                    _ => {}
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
            let mut tint = body_tint(actor, model.tint, actor_ticks.get(index).copied());
            let material = if actor.time_stop != 0
                || actor.conditions.effective().contains(Condition::Petrified)
            {
                tint[..3].fill(64);
                ModelMaterial::RedChannel
            } else {
                if let Some(color) = self.colors[index].flash {
                    tint[..3].copy_from_slice(&color);
                }
                ModelMaterial::Normal
            };
            if actor.available() && selected.is_some_and(|target| target.index() != index) {
                for channel in &mut tint[..3] {
                    if selector.is_some() {
                        *channel /= 2;
                    } else if actor.side == Side::Enemy {
                        *channel -= *channel / 6;
                    }
                }
            }
            if material == ModelMaterial::Normal
                && selector.is_none()
                && let Some((color, _)) = self.colors[index].pulse
            {
                tint[..3].copy_from_slice(&color);
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

fn fade_color(color: &mut [u8; 3], base: [u8; 4]) {
    for (channel, target) in color.iter_mut().zip(base) {
        *channel = match (*channel).cmp(&target) {
            std::cmp::Ordering::Less => *channel + 1,
            std::cmp::Ordering::Greater => *channel - 1,
            std::cmp::Ordering::Equal => *channel,
        };
    }
}

fn body_tint(actor: &Actor, mut tint: [u8; 4], next_tick: Option<u16>) -> [u8; 4] {
    if actor.available() && actor.conditions.effective().intersects(POISON) {
        const POISON_PERIOD: u16 = 120;
        let phase = next_tick.map_or(0, |tick| (tick + POISON_PERIOD - 1) % POISON_PERIOD);
        let angle = f32::from(phase) * std::f32::consts::TAU / f32::from(POISON_PERIOD);
        let green = (56. + 8. * angle.sin()) as u8;
        tint[..3].copy_from_slice(&[32, green, 32]);
    }
    tint
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
                texture_tick: 0,
                light: None,
                shadow: None,
                resource: resonance_battle::ResourceId(0),
                clip: 0,
                frame: 0.,
                blend_weight: 0.,
                root_translation: [0.; 3],
                world: [[0.; 4]; 4],
                particle_world: [[0.; 4]; 4],
                bones: Default::default(),
            })
            .collect();
        frame.weapons.push(WeaponFrame {
            texture_scroll: None,
            owner: ids[0],
            slot: Some(resonance_battle::WeaponSlot(0)),
            visible: true,
            tint: [0; 4],
            material: ModelMaterial::Normal,
            resource: resonance_battle::ResourceId(1),
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
            appearance.advance(frame, paused, &[31; 3]);
        };
        frame.cues.push(Cue::ActorFlash {
            actor: ids[0],
            color: [192, 128, 128],
        });
        frame.cues.push(Cue::ActorFlash {
            actor: ids[1],
            color: [112, 40, 40],
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
        assert_eq!(frame.models[0].tint, [172, 108, 108, 173]);
        frame.paused_actors.push(ids[0]);
        render(&mut frame, false);
        assert_eq!(frame.models[0].tint, [172, 108, 108, 173]);
        frame.paused_actors.clear();
        for _ in 0..132 {
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
        frame.actors[2].time_stop = 0;
        render(&mut frame, false);
        assert_eq!(frame.models[1].tint, [27, 54, 27, 173]);
        assert_eq!(frame.models[2].tint, [64, 64, 64, 173]);
        frame.recognized_result = Some(BattleResult::Victory);
        render(&mut frame, true);
        assert_eq!(frame.models[0].tint, [128, 192, 128, 173]);
        for _ in 0..142 {
            render(&mut frame, false);
        }
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
