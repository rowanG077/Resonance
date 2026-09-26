//! Shared actor flash latch (26980), sampled by 5200C before actor callbacks.
use crate::{
    ActorId, Battle, Cue, EffectAppearance, Element, GuardResult, HitResult, PreparedBattle,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct ContactElementFeedback {
    /// None is an original empty element entry, not a missing prepared resource.
    pub effect: Option<EffectAppearance>,
    pub color: [u8; 3],
}

#[derive(Debug, Clone)]
pub struct ContactFeedback {
    /// First contact, then another contact within four actor updates.
    pub ordinary: [EffectAppearance; 2],
    pub guard: [EffectAppearance; 2],
    pub critical: EffectAppearance,
    pub guard_break: EffectAppearance,
    pub overlimit: EffectAppearance,
    /// The neutral entry and every element reachable from the prepared actors.
    pub elements: BTreeMap<Option<Element>, ContactElementFeedback>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct State {
    repeated: u8,
    flash: u8,
    color: [u8; 3],
}

impl State {
    pub(crate) fn flash(&mut self, color: [u8; 3]) {
        self.color = color;
        self.flash = 2;
    }

    pub(crate) fn appearance(&self, tint: &mut [u8; 4], held_ambient: Option<[u8; 3]>) {
        if self.flash != 0 {
            // 5200C keeps alpha; a global transition substitutes stage RGB.
            tint[..3].copy_from_slice(&held_ambient.unwrap_or(self.color));
        }
    }

    pub(crate) fn step(&mut self) {
        self.repeated = self.repeated.saturating_sub(1);
        self.flash = self.flash.saturating_sub(1);
    }

    pub(crate) fn clear_flash(&mut self) {
        self.flash = 0;
    }
}

impl PreparedBattle {
    /// Enabled actor admissions share the contact flash latch. Casting probes
    /// and resident/effect admissions must not acquire this appearance.
    pub fn with_admission_flashes(mut self, flashes: BTreeMap<u16, [u8; 3]>) -> Result<Self> {
        for action in flashes.keys() {
            ensure!(
                self.actions.iter().any(|definition| {
                    definition.id == *action && definition.phase == crate::ActionPhase::Actor
                }),
                "admission flash requires a prepared enabled actor action"
            );
        }
        self.admission_flashes = flashes;
        Ok(self)
    }

    pub fn with_contact_feedback(mut self, feedback: ContactFeedback) -> Result<Self> {
        ensure!(
            feedback.elements.contains_key(&None),
            "missing neutral contact feedback"
        );
        for appearance in feedback
            .ordinary
            .into_iter()
            .chain(feedback.guard)
            .chain([feedback.critical, feedback.guard_break, feedback.overlimit])
            .chain(
                feedback
                    .elements
                    .values()
                    .filter_map(|element| element.effect),
            )
        {
            ensure!(
                self.effects
                    .get(&appearance.resource)
                    .is_some_and(|bank| bank.members.contains_key(&appearance.member)),
                "unprepared common contact effect"
            );
        }
        self.contact_feedback = Some(feedback);
        Ok(self)
    }
}

impl Battle {
    // Keep the resolved contact operands together at the feedback boundary.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn common_contact_feedback(
        &mut self,
        action: crate::ActionId,
        owner: ActorId,
        target: ActorId,
        position: [f32; 3],
        heading: f32,
        element: Option<Element>,
        result: HitResult,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let Some(feedback) = &self.prepared.contact_feedback else {
            return Ok(());
        };
        let element_feedback = *feedback
            .elements
            .get(&element)
            .context("unprepared contact element")?;
        let actor = &self.actors[target.index()];
        let repeated = usize::from(self.contact_feedback[target.index()].repeated != 0);
        let mut emissions = Vec::with_capacity(4);
        let mut emit = |appearance, origin, follow, scale, owner| {
            emissions.push(crate::effect::Spawn {
                action,
                scene: None,
                owner,
                target,
                appearance,
                origin,
                heading,
                follow,
                scale,
                late: false,
                tint: Default::default(),
            })
        };
        if actor.overlimit_active {
            emit(feedback.overlimit, position, None, 1., target);
        } else if result.guard != GuardResult::None {
            if matches!(result.guard, GuardResult::Blocked { special: true, .. }) {
                if let Some(appearance) = element_feedback.effect {
                    let origin = if element == Some(Element::Lightning) {
                        actor.body.center
                    } else {
                        position
                    };
                    emit(
                        appearance,
                        origin,
                        Some(crate::effect::Follow::Center(target)),
                        1.,
                        target,
                    );
                } else if element.is_none() {
                    emit(feedback.guard[0], position, None, 1., target);
                }
            } else {
                emit(feedback.guard[repeated], position, None, 1., target);
            }
        } else {
            if element.is_some() {
                if let Some(appearance) = element_feedback.effect {
                    let origin = if element == Some(Element::Lightning) {
                        actor.body.center
                    } else {
                        position
                    };
                    emit(appearance, origin, None, 1., target);
                }
                emit(feedback.ordinary[1], position, None, 1., target);
            } else {
                emit(feedback.ordinary[repeated], position, None, 1., target);
            }
        }
        // The first branch's model tint is independent of any critical/break
        // overlay and is not requested by guarded or active Over Limit hits.
        let flashes = !actor.overlimit_active && result.guard == GuardResult::None;
        if result.critical {
            emit(feedback.critical, position, None, 1., owner);
        }
        if result.guard == GuardResult::Broken {
            emit(
                feedback.guard_break,
                actor.body.center,
                Some(crate::effect::Follow::Center(target)),
                actor.effect_scale,
                target,
            );
        }
        for emission in emissions {
            self.show_effect(emission, cues)?;
        }
        let state = &mut self.contact_feedback[target.index()];
        if flashes {
            state.flash(element_feedback.color);
        }
        state.repeated = 4;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionId, Affinity, BattleInput, HitProtection, Side};
    use std::sync::Arc;

    fn battle() -> Battle {
        let mut prepared = crate::tests::prepared(
            "pub task run() {}",
            vec![
                crate::tests::actor(Side::Party),
                crate::tests::actor(Side::Enemy),
            ],
            1,
        );
        Arc::get_mut(&mut prepared).unwrap().effects.insert(
            4,
            crate::tests::effect_binding(4, [0, 1, 2, 11, 12, 16, 29, 33, 47]),
        );
        let effect = |member| EffectAppearance {
            resource: 4,
            member,
        };
        let feedback = ContactFeedback {
            ordinary: [effect(11), effect(12)],
            guard: [effect(1), effect(2)],
            critical: effect(16),
            guard_break: effect(0),
            overlimit: effect(47),
            elements: BTreeMap::from([
                (
                    None,
                    ContactElementFeedback {
                        effect: None,
                        color: [192, 128, 128],
                    },
                ),
                (
                    Some(Element::Fire),
                    ContactElementFeedback {
                        effect: Some(effect(29)),
                        color: [192, 64, 64],
                    },
                ),
                (
                    Some(Element::Lightning),
                    ContactElementFeedback {
                        effect: Some(effect(33)),
                        color: [160, 160, 192],
                    },
                ),
                (
                    Some(Element::Darkness),
                    ContactElementFeedback {
                        effect: None,
                        color: [48; 3],
                    },
                ),
            ]),
        };
        let prepared = Arc::try_unwrap(prepared)
            .unwrap()
            .with_contact_feedback(feedback)
            .unwrap();
        Battle::new(Arc::new(prepared))
    }

    fn hit(guard: GuardResult, critical: bool) -> HitResult {
        HitResult {
            amount: 10,
            hp_change: -10,
            critical,
            affinity: Affinity::Normal,
            guard,
            auto_guard: false,
            armored: false,
            protection: HitProtection::None,
        }
    }

    fn contact(
        battle: &mut Battle,
        element: Option<Element>,
        result: HitResult,
    ) -> Result<Vec<Cue>> {
        let mut cues = vec![];
        battle.common_contact_feedback(
            ActionId(9),
            ActorId(0),
            ActorId(1),
            [2., 3., 4.],
            37.,
            element,
            result,
            &mut cues,
        )?;
        Ok(cues)
    }

    fn members(cues: &[Cue]) -> Vec<u16> {
        cues.iter()
            .filter_map(|cue| match cue {
                Cue::Effect { member, .. } => Some(*member),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn element_flash_and_repeated_contact_use_independent_actor_timers() -> Result<()> {
        let mut battle = battle();
        let ordinary = hit(GuardResult::None, false);
        assert_eq!(
            members(&contact(&mut battle, Some(Element::Fire), ordinary)?),
            [29, 12]
        );
        let base = [47, 51, 55, 173];
        for expected in [[192, 64, 64, 173], [192, 64, 64, 173], base] {
            let mut tint = base;
            battle.contact_feedback[1].appearance(&mut tint, None);
            assert_eq!(tint, expected);
            battle.step(BattleInput::default())?;
        }
        assert_eq!(members(&contact(&mut battle, None, ordinary)?), [12]);
        for _ in 0..4 {
            battle.step(BattleInput::default())?;
        }
        assert_eq!(members(&contact(&mut battle, None, ordinary)?), [11]);
        assert_eq!(members(&contact(&mut battle, None, ordinary)?), [12]);
        Ok(())
    }

    #[test]
    fn guard_overlimit_critical_and_empty_element_entries_keep_original_dispatch_order()
    -> Result<()> {
        let normal_guard = GuardResult::Blocked {
            first: true,
            special: false,
        };
        let special_guard = GuardResult::Blocked {
            first: true,
            special: true,
        };
        for (element, guard, critical, overlimit, expected) in [
            (Some(Element::Fire), normal_guard, false, false, vec![1]),
            (Some(Element::Fire), special_guard, false, false, vec![29]),
            (None, special_guard, false, false, vec![1]),
            (Some(Element::Darkness), special_guard, false, false, vec![]),
            (
                Some(Element::Darkness),
                GuardResult::None,
                false,
                false,
                vec![12],
            ),
            (None, GuardResult::None, true, false, vec![11, 16]),
            (None, GuardResult::Broken, false, false, vec![1, 0]),
            (
                Some(Element::Fire),
                GuardResult::None,
                true,
                true,
                vec![47, 16],
            ),
        ] {
            let mut battle = battle();
            battle.actors[1].overlimit_active = overlimit;
            battle.actors[1].body.center = [7., 13., 17.];
            battle.actors[1].effect_scale = 2.5;
            let cues = contact(&mut battle, element, hit(guard, critical))?;
            assert_eq!(members(&cues), expected);
            assert_eq!(
                battle.contact_feedback[1].flash != 0,
                !overlimit && guard == GuardResult::None
            );
            if guard == GuardResult::Broken {
                let context = battle
                    .sequences
                    .values()
                    .find_map(|sequence| {
                        sequence
                            .effect
                            .as_ref()
                            .filter(|context| context.scale == 2.5)
                    })
                    .unwrap();
                assert_eq!(context.origin, [7., 13., 17.]);
                assert!(matches!(
                    context.follow,
                    Some(crate::effect::Follow::Center(ActorId(1)))
                ));
            }
        }
        let mut battle = battle();
        battle.actors[1].body.center = [7., 13., 17.];
        let cues = contact(
            &mut battle,
            Some(Element::Lightning),
            hit(special_guard, false),
        )?;
        assert!(matches!(
            cues[0],
            Cue::Effect {
                member: 33,
                position: [7., 13., 17.],
                heading: 37.,
                ..
            }
        ));
        assert!(
            battle
                .sequences
                .values()
                .any(
                    |sequence| sequence.effect.as_ref().is_some_and(|context| matches!(
                        context.follow,
                        Some(crate::effect::Follow::Center(ActorId(1)))
                    ))
                )
        );
        Ok(())
    }

    #[test]
    fn death_clears_an_existing_flash_before_the_lethal_contact_requests_its_own() -> Result<()> {
        let mut battle = battle();
        contact(
            &mut battle,
            Some(Element::Fire),
            hit(GuardResult::None, false),
        )?;
        battle.actors[1].hp = 0;
        battle.enter_death(ActorId(1), &mut vec![])?;
        assert_eq!(battle.contact_feedback[1].flash, 0);
        contact(&mut battle, None, hit(GuardResult::None, false))?;
        assert_eq!(battle.contact_feedback[1].flash, 2);
        assert_eq!(battle.contact_feedback[1].color, [192, 128, 128]);
        Ok(())
    }
}
