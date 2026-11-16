//! Three independent break gauges and timed, exclusively claimed finishing openings.
use crate::{ActorId, Battle, BattlePhase, Cue, HitResult, HitRule, Side, Species};
use anyhow::{Result, ensure};
pub use resonance_content::fatal_strike::{
    Color as FatalColor, EnemyRules as FatalEnemyRules, Outcome as FatalOutcome,
};

mod scene;
pub use scene::{FatalPerformance, FatalSceneFrame, FatalStyle, FatalSuccess};

const OPENING_TICKS: u16 = 90;
const CHAIN_TICKS: u16 = 300;
const RESTORE_DELAY: u16 = 180;
const MAX_CHAIN: u8 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FatalOpening {
    pub color: FatalColor,
    pub remaining: u16,
    pub chain: u8,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatalTargetFrame {
    pub actor: ActorId,
    pub remaining: [u16; 3],
    pub capacity: [u16; 3],
    pub opening: Option<FatalOpening>,
    pub chain: u8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FatalStrikeFrame {
    pub targets: Vec<FatalTargetFrame>,
    pub scene: Option<FatalSceneFrame>,
}

#[derive(Debug, Clone)]
struct Gauge {
    rules: FatalEnemyRules,
    remaining: [u16; 3],
    opening: Option<FatalOpening>,
    chain: u8,
    chain_remaining: u16,
    quiet: u16,
}

impl Gauge {
    fn new(rules: FatalEnemyRules) -> Self {
        Self {
            remaining: rules.capacity,
            rules,
            opening: None,
            chain: 0,
            chain_remaining: 0,
            quiet: 0,
        }
    }

    fn tick(&mut self) {
        self.quiet = self.quiet.saturating_add(1);
        self.chain_remaining = self.chain_remaining.saturating_sub(1);
        if self.chain_remaining == 0 && self.opening.is_none() {
            self.chain = 0;
        }
        if let Some(opening) = &mut self.opening {
            opening.remaining -= 1;
            if opening.remaining == 0 {
                self.opening = None;
                self.remaining = self.rules.capacity;
                self.chain_remaining = CHAIN_TICKS;
            }
        } else if self.quiet > RESTORE_DELAY && self.quiet.is_multiple_of(6) {
            for (value, capacity) in self.remaining.iter_mut().zip(self.rules.capacity) {
                *value = value.saturating_add(1).min(capacity);
            }
            self.quiet = RESTORE_DELAY;
        }
    }

    fn apply(
        &mut self,
        damage: [u16; 3],
        mitigation: u32,
        generation: &mut u64,
    ) -> Option<FatalOpening> {
        if self.rules.outcome == FatalOutcome::Disabled
            || self.opening.is_some()
            || damage == [0; 3]
        {
            return None;
        }
        let mut broken = None;
        for color in FatalColor::ALL {
            let index = color.index();
            let amount =
                u32::from(damage[index]) * u32::from(self.rules.vulnerability[index]) / mitigation;
            self.remaining[index] =
                self.remaining[index].saturating_sub(amount.min(u32::from(u16::MAX)) as u16);
            if self.remaining[index] == 0 && broken.is_none() {
                broken = Some(color);
            }
        }
        if damage != [0; 3] {
            self.quiet = 0;
        }
        let color = broken?;
        self.chain = self.chain.saturating_add(1).min(MAX_CHAIN);
        self.chain_remaining = CHAIN_TICKS;
        *generation += 1;
        let opening = FatalOpening {
            color,
            remaining: OPENING_TICKS,
            chain: self.chain,
            generation: *generation,
        };
        self.opening = Some(opening);
        Some(opening)
    }
}

#[derive(Debug, Default)]
pub(crate) struct State {
    enabled: bool,
    gauges: Vec<Option<Gauge>>,
    generation: u64,
    performances: std::collections::BTreeMap<ActorId, [std::sync::Arc<FatalPerformance>; 3]>,
    pub(crate) scene: Option<scene::Scene>,
}

impl State {
    pub(crate) fn active(&self) -> bool {
        self.scene.is_some()
    }
    pub(crate) fn camera(&self) -> Option<crate::CameraPose> {
        self.scene.as_ref().map(|scene| scene.camera)
    }
    pub(crate) fn fov(&self) -> Option<f32> {
        self.scene.as_ref().map(|scene| scene.fov)
    }
    pub(crate) fn frame(&self, actors: &[crate::Actor]) -> Option<FatalStrikeFrame> {
        self.enabled.then(|| FatalStrikeFrame {
            scene: self.scene.as_ref().map(|scene| scene.frame()),
            targets: self
                .gauges
                .iter()
                .enumerate()
                .filter_map(|(index, gauge)| {
                    let gauge = gauge.as_ref()?;
                    (actors[index].available() && gauge.rules.outcome != FatalOutcome::Disabled)
                        .then_some(FatalTargetFrame {
                            actor: ActorId(index as u8),
                            remaining: gauge.remaining,
                            capacity: gauge.rules.capacity,
                            opening: gauge.opening,
                            chain: gauge.chain,
                        })
                })
                .collect(),
        })
    }
}

impl Battle {
    /// The setting is battle-local; the game host persists its Customize preference.
    pub fn set_fatal_strikes(&mut self, enabled: bool) {
        if self.fatal_strikes.enabled == enabled {
            return;
        }
        if let Some(scene) = self.fatal_strikes.scene.take() {
            let mut cues = Vec::new();
            self.retire_fatal_scene(scene, false, &mut cues);
            self.pending_cues.extend(cues);
        }
        self.fatal_strikes.enabled = enabled;
        self.fatal_strikes.gauges = self
            .actors
            .iter()
            .enumerate()
            .map(|(index, actor)| {
                (enabled
                    && actor.side == Side::Enemy
                    && !self.prepared.actor_setup[index].target_excluded)
                    .then(|| {
                        let rules = FatalEnemyRules {
                            vulnerability: match actor.species {
                                Species::Beast | Species::Magical => [100, 65, 140],
                                Species::Bird | Species::Insect => [65, 140, 100],
                                Species::Plant | Species::Aquatic => [140, 100, 65],
                                Species::Machine | Species::Dragon | Species::Immaterial => [70; 3],
                                _ => [100; 3],
                            },
                            ..Default::default()
                        };
                        Gauge::new(rules)
                    })
            })
            .collect();
    }

    pub fn fatal_strikes_enabled(&self) -> bool {
        self.fatal_strikes.enabled
    }

    /// Encounter and authored enemy rules override the species defaults before combat.
    pub fn configure_fatal_enemy(&mut self, actor: ActorId, rules: FatalEnemyRules) -> Result<()> {
        rules.validate()?;
        let mut rules = rules;
        if self
            .prepared
            .actor_setup
            .get(actor.index())
            .is_some_and(|setup| setup.target_excluded)
        {
            rules.outcome = FatalOutcome::Disabled;
        }
        ensure!(
            self.actor(actor)?.side == Side::Enemy,
            "Fatal Strike rules require an enemy"
        );
        ensure!(self.fatal_strikes.enabled, "Fatal Strikes are disabled");
        self.fatal_strikes.gauges[actor.index()] = Some(Gauge::new(rules));
        Ok(())
    }

    pub(crate) fn advance_fatal_gauges(&mut self) {
        if !self.fatal_strikes.enabled || self.phase() != BattlePhase::Combat {
            return;
        }
        for (gauge, actor) in self.fatal_strikes.gauges.iter_mut().zip(&self.actors) {
            if let Some(gauge) = gauge {
                if !actor.available() {
                    *gauge = Gauge::new(gauge.rules);
                } else if actor.time_stop == 0 && actor.hit_stop == 0 {
                    gauge.tick();
                }
            }
        }
    }

    pub(crate) fn record_fatal_contact(
        &mut self,
        owner: ActorId,
        target: ActorId,
        rule: HitRule,
        hit: HitResult,
        cues: &mut Vec<Cue>,
    ) {
        if !self.fatal_strikes.enabled
            || self.phase() != BattlePhase::Combat
            || self.actors[owner.index()].side != Side::Party
            || self.actors[target.index()].side != Side::Enemy
            || self.actors[target.index()].hp <= 0
            || !hit.is_damage()
        {
            return;
        }
        let Some(gauge) = self.fatal_strikes.gauges[target.index()].as_mut() else {
            return;
        };
        let damage = rule.fatal_damage.unwrap_or_else(|| {
            if rule.kind == crate::DamageKind::Magic {
                return [4; 3];
            }
            let color = if rule.reaction.recoil.launch {
                FatalColor::Blue
            } else if rule.reaction.recoil.knock_down {
                FatalColor::Red
            } else {
                FatalColor::Green
            };
            let mut damage = [0; 3];
            damage[color.index()] = if rule.arte { 12 } else { 8 };
            damage
        });
        let mitigation = if matches!(hit.guard, crate::GuardResult::Blocked { .. }) {
            400
        } else if hit.protection == crate::HitProtection::Armored {
            200
        } else {
            100
        };
        if let Some(opening) = gauge.apply(damage, mitigation, &mut self.fatal_strikes.generation) {
            cues.push(Cue::FatalOpening {
                actor: target,
                opening,
            });
        }
    }
}

#[cfg(test)]
mod tests;
