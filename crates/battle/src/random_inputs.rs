//! Strict, finite random inputs for a directly admitted validation action.
use crate::{ActionId, ActorId, Battle, HammerRing, ProjectileScatter};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Contact {
    Attached,
    Projectile(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub clock: u32,
    pub owner: usize,
    pub target: usize,
    pub action: u64,
    pub contact: Contact,
}

#[derive(Debug, Clone)]
pub struct Scatter {
    pub key: Key,
    pub values: [u16; 3],
    pub distribution: ProjectileScatter,
}

#[derive(Debug, Clone)]
pub struct HammerRingLaunch {
    pub key: Key,
    pub index: u8,
    pub values: [u16; 3],
    pub distribution: HammerRing,
}

#[derive(Debug, Clone)]
pub struct Velocity {
    pub key: Key,
    pub axis: usize,
    pub amplitude: f32,
    pub value: u16,
}

#[derive(Debug, Clone)]
pub struct Stun {
    pub key: Key,
    pub value: u16,
    pub chance_percent: u16,
}

#[derive(Debug, Clone)]
pub struct Ailment {
    pub key: Key,
    pub condition: crate::HitCondition,
    pub resistance: bool,
    pub value: u16,
}

#[derive(Debug, Clone)]
pub struct Seal {
    pub key: Key,
    pub condition: crate::conditions::Condition,
    pub chance_percent: u16,
    pub percentile: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CriticalRole {
    Chance,
    Roll,
}

#[derive(Debug, Clone)]
pub struct Critical {
    pub key: Key,
    pub role: CriticalRole,
    pub value: u16,
}

#[derive(Debug, Clone)]
pub struct TargetChoice {
    pub key: Key,
    pub candidates: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Positioning {
    pub clock: u32,
    pub owner: usize,
    pub target: usize,
    pub action: u64,
    pub values: Vec<u16>,
}

#[derive(Default)]
pub struct RecordedInputs {
    pub scatter: Vec<Scatter>,
    pub hammer_ring: Vec<HammerRingLaunch>,
    pub velocity: Vec<Velocity>,
    pub stun: Vec<Stun>,
    pub ailments: Option<Vec<Ailment>>,
    pub seals: Option<Vec<Seal>>,
    pub criticals: Option<Vec<Critical>>,
    pub targets: Option<Vec<TargetChoice>>,
    pub positioning: Option<Vec<Positioning>>,
}

pub(crate) struct Inputs {
    origin: u32,
    owner: usize,
    action: u64,
    admission: ActionId,
    actions: BTreeSet<ActionId>,
    admitted: bool,
    scatter: VecDeque<Scatter>,
    hammer_ring: VecDeque<HammerRingLaunch>,
    velocity: VecDeque<Velocity>,
    stun: VecDeque<Stun>,
    ailments: Option<VecDeque<Ailment>>,
    seals: Option<VecDeque<Seal>>,
    criticals: Option<VecDeque<Critical>>,
    targets: Option<VecDeque<TargetChoice>>,
    positioning: Option<VecDeque<Positioning>>,
}

impl Inputs {
    pub(crate) fn positioning(
        &mut self,
        clock: u32,
        owner: ActorId,
        target: ActorId,
    ) -> Result<Option<Vec<u16>>> {
        if owner.index() != self.owner {
            return Ok(None);
        }
        let Some(rows) = &mut self.positioning else {
            return Ok(None);
        };
        let row = rows.front().context("extra positioning search")?;
        ensure!(
            self.admitted
                && clock.checked_sub(self.origin) == Some(row.clock)
                && target.index() == row.target,
            "positioning search clock or target differs"
        );
        Ok(Some(rows.pop_front().unwrap().values))
    }

    pub(crate) fn target(
        &mut self,
        clock: u32,
        owner: ActorId,
        action: ActionId,
        projectile: u64,
        candidates: &[ActorId],
    ) -> Result<Option<usize>> {
        let Some(inputs) = &self.targets else {
            return Ok(None);
        };
        let input = inputs.front().context("extra target choice")?;
        let key = self.key(Key {
            clock,
            owner: owner.index(),
            action: action.0,
            target: input.key.target,
            contact: Contact::Projectile(projectile),
        })?;
        ensure!(
            input.key == key
                && candidates
                    .iter()
                    .map(|actor| actor.index())
                    .eq(input.candidates.iter().copied()),
            "target choice action or candidates differ"
        );
        let choice = candidates
            .iter()
            .position(|actor| actor.index() == input.key.target)
            .context("recorded target is unavailable")?;
        self.targets.as_mut().unwrap().pop_front();
        Ok(Some(choice))
    }

    pub(crate) fn owns_action(&self, action: ActionId) -> bool {
        self.actions.contains(&action)
    }

    pub(crate) fn observes_damage(&self, draw: crate::damage::Draw) -> bool {
        match draw {
            crate::damage::Draw::Ailment | crate::damage::Draw::AilmentResistance => {
                self.ailments.is_some()
            }
            crate::damage::Draw::CriticalChance | crate::damage::Draw::Critical => {
                self.criticals.is_some()
            }
            _ => false,
        }
    }

    fn key(&self, mut actual: Key) -> Result<Key> {
        ensure!(
            self.actions.contains(&ActionId(actual.action)),
            "random input action identity differs"
        );
        actual.action = self.action;
        actual.clock = actual
            .clock
            .checked_sub(self.origin)
            .context("random input requested before action admission")?;
        Ok(actual)
    }

    pub(crate) fn released(&mut self, owner: ActorId, parent: Option<ActionId>, action: ActionId) {
        if owner.index() == self.owner && parent.is_some_and(|id| self.actions.contains(&id)) {
            self.actions.insert(action);
        }
    }

    pub(crate) fn scatter(
        &mut self,
        key: Key,
        distribution: ProjectileScatter,
    ) -> Result<[u16; 3]> {
        let key = self.key(key)?;
        let input = self.scatter.front().context("extra scatter draw")?;
        ensure!(
            input.key == key,
            "scatter input key differs: {:?} / {key:?}",
            input.key
        );
        ensure!(
            input.distribution.position_range == distribution.position_range
                && input.distribution.position_step == distribution.position_step
                && input.distribution.lateral_range == distribution.lateral_range
                && input.distribution.lateral_step == distribution.lateral_step,
            "scatter input distribution differs"
        );
        Ok(self.scatter.pop_front().unwrap().values)
    }

    pub(crate) fn hammer_ring(
        &mut self,
        key: Key,
        index: u8,
        distribution: HammerRing,
    ) -> Result<[u16; 3]> {
        let key = self.key(key)?;
        let input = self.hammer_ring.front().context("extra hammer ring draw")?;
        ensure!(
            input.key == key && input.index == index && input.distribution == distribution,
            "hammer ring input key, index or distribution differs"
        );
        Ok(self.hammer_ring.pop_front().unwrap().values)
    }

    pub(crate) fn stun(&mut self, key: Key, chance_percent: u16) -> Result<u16> {
        let key = self.key(key)?;
        let input = self.stun.front().context("extra stun draw")?;
        ensure!(
            input.key == key,
            "stun input key differs: {:?} / {key:?}",
            input.key
        );
        ensure!(
            input.chance_percent == chance_percent,
            "stun input chance differs"
        );
        Ok(self.stun.pop_front().unwrap().value)
    }

    pub(crate) fn velocity(&mut self, key: Key, axis: usize, amplitude: f32) -> Result<u16> {
        let key = self.key(key)?;
        let input = self
            .velocity
            .front()
            .context("extra velocity jitter draw")?;
        ensure!(
            input.key == key && input.axis == axis && input.amplitude == amplitude,
            "velocity jitter input key or distribution differs"
        );
        Ok(self.velocity.pop_front().unwrap().value)
    }

    pub(crate) fn seal(
        &mut self,
        key: Key,
        condition: crate::conditions::Condition,
        chance_percent: u16,
    ) -> Result<Option<u16>> {
        if self.seals.is_none() {
            return Ok(None);
        }
        let key = self.key(key)?;
        let inputs = self.seals.as_mut().unwrap();
        let input = inputs.front().context("extra seal draw")?;
        ensure!(
            input.key == key
                && input.condition == condition
                && input.chance_percent == chance_percent,
            "seal input contact, condition or chance differs"
        );
        Ok(Some(inputs.pop_front().unwrap().percentile))
    }

    pub(crate) fn ailment(
        &mut self,
        key: Key,
        condition: Option<crate::HitCondition>,
        resistance: bool,
    ) -> Result<Option<u16>> {
        if self.ailments.is_none() {
            return Ok(None);
        }
        let key = self.key(key)?;
        let inputs = self.ailments.as_mut().unwrap();
        let input = inputs.front().context("extra ailment draw")?;
        ensure!(
            input.key == key
                && Some(input.condition) == condition
                && input.resistance == resistance,
            "ailment input contact, condition or draw role differs"
        );
        Ok(Some(inputs.pop_front().unwrap().value))
    }

    pub(crate) fn critical(&mut self, key: Key, role: CriticalRole) -> Result<Option<u16>> {
        if self.criticals.is_none() {
            return Ok(None);
        }
        let key = self.key(key)?;
        let inputs = self.criticals.as_mut().unwrap();
        let input = inputs.front().context("extra critical draw")?;
        ensure!(
            input.key == key && input.role == role,
            "critical input contact or draw role differs"
        );
        Ok(Some(inputs.pop_front().unwrap().value))
    }
}

impl Battle {
    pub fn install_random_inputs(&mut self, origin: u32, recorded: RecordedInputs) -> Result<()> {
        let RecordedInputs {
            scatter,
            hammer_ring,
            velocity,
            stun,
            ailments,
            seals,
            criticals,
            targets,
            positioning,
        } = recorded;
        ensure!(
            self.random_inputs.is_none(),
            "random inputs already installed"
        );
        let first = scatter
            .first()
            .map(|row| row.key)
            .or_else(|| hammer_ring.first().map(|row| row.key))
            .or_else(|| velocity.first().map(|row| row.key))
            .or_else(|| stun.first().map(|row| row.key))
            .or_else(|| ailments.as_ref()?.first().map(|row| row.key))
            .or_else(|| seals.as_ref()?.first().map(|row| row.key))
            .or_else(|| criticals.as_ref()?.first().map(|row| row.key))
            .or_else(|| targets.as_ref()?.first().map(|row| row.key));
        let (owner, action) = first
            .map(|key| (key.owner, key.action))
            .or_else(|| {
                positioning
                    .as_ref()?
                    .first()
                    .map(|row| (row.owner, row.action))
            })
            .context("empty random input fixture")?;
        for key in scatter
            .iter()
            .map(|row| row.key)
            .chain(hammer_ring.iter().map(|row| row.key))
            .chain(velocity.iter().map(|row| row.key))
            .chain(stun.iter().map(|row| row.key))
            .chain(ailments.iter().flatten().map(|row| row.key))
            .chain(seals.iter().flatten().map(|row| row.key))
            .chain(criticals.iter().flatten().map(|row| row.key))
            .chain(targets.iter().flatten().map(|row| row.key))
        {
            ensure!(
                key.owner == owner
                    && key.action == action
                    && key.owner < self.actors.len()
                    && key.target < self.actors.len()
                    && key.action != 0
                    && key.contact != Contact::Projectile(0),
                "random inputs must belong to one valid action"
            );
        }
        ensure!(
            scatter
                .iter()
                .all(|row| matches!(row.key.contact, Contact::Projectile(_)))
                && scatter
                    .windows(2)
                    .all(|rows| rows[0].key.clock <= rows[1].key.clock)
                && scatter
                    .iter()
                    .map(|row| row.key)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == scatter.len(),
            "unordered or duplicate scatter inputs"
        );
        ensure!(
            hammer_ring.iter().all(|row| {
                matches!(row.key.contact, Contact::Projectile(_))
                    && row.index < row.distribution.count
                    && row.distribution.jitter_range > 0
            }) && hammer_ring
                .windows(2)
                .all(|rows| rows[0].key.clock <= rows[1].key.clock)
                && hammer_ring
                    .iter()
                    .map(|row| row.key)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == hammer_ring.len(),
            "invalid, unordered or duplicate hammer ring inputs"
        );
        ensure!(
            velocity.iter().all(|row| {
                matches!(row.key.contact, Contact::Projectile(_))
                    && row.axis < 3
                    && row.amplitude.is_finite()
                    && row.amplitude.abs() >= 1.
            }) && velocity
                .windows(2)
                .all(|rows| rows[0].key.clock <= rows[1].key.clock)
                && velocity
                    .iter()
                    .map(|row| (row.key, row.axis))
                    .collect::<BTreeSet<_>>()
                    .len()
                    == velocity.len(),
            "invalid, unordered or duplicate velocity jitter inputs"
        );
        ensure!(
            stun.windows(2)
                .all(|rows| rows[0].key.clock <= rows[1].key.clock)
                && stun
                    .iter()
                    .map(|row| row.key)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == stun.len(),
            "unordered or duplicate stun inputs"
        );
        if let Some(rows) = &ailments {
            ensure!(
                rows.windows(2)
                    .all(|pair| pair[0].key.clock <= pair[1].key.clock)
                    && rows
                        .iter()
                        .map(|row| (row.key, row.resistance))
                        .collect::<BTreeSet<_>>()
                        .len()
                        == rows.len()
                    && rows.iter().enumerate().all(|(index, row)| {
                        row.condition.chance != 0
                            && (!row.resistance
                                || index > 0
                                    && rows[index - 1].key == row.key
                                    && rows[index - 1].condition == row.condition
                                    && !rows[index - 1].resistance)
                    }),
                "invalid, unordered or duplicate ailment inputs"
            );
        }
        if let Some(rows) = &seals {
            ensure!(
                rows.iter()
                    .all(|row| row.percentile < 100 && row.chance_percent <= 100)
                    && rows
                        .windows(2)
                        .all(|pair| pair[0].key.clock <= pair[1].key.clock)
                    && rows
                        .iter()
                        .map(|row| row.key)
                        .collect::<BTreeSet<_>>()
                        .len()
                        == rows.len(),
                "invalid, unordered or duplicate seal inputs"
            );
        }
        if let Some(rows) = &criticals {
            ensure!(
                rows.windows(2)
                    .all(|pair| pair[0].key.clock <= pair[1].key.clock)
                    && rows
                        .iter()
                        .map(|row| (row.key, row.role))
                        .collect::<BTreeSet<_>>()
                        .len()
                        == rows.len(),
                "unordered or duplicate critical inputs"
            );
        }
        if let Some(rows) = &targets {
            ensure!(
                rows.windows(2)
                    .all(|pair| pair[0].key.clock <= pair[1].key.clock)
                    && rows
                        .iter()
                        .map(|row| row.key)
                        .collect::<BTreeSet<_>>()
                        .len()
                        == rows.len()
                    && rows
                        .iter()
                        .all(|row| row.candidates.contains(&row.key.target)),
                "invalid, unordered or duplicate target choices"
            );
        }
        if let Some(rows) = &positioning {
            ensure!(
                rows.windows(2).all(|pair| pair[0].clock < pair[1].clock)
                    && rows.iter().all(|row| {
                        row.owner == owner
                            && row.action == action
                            && action != 0
                            && owner < self.actors.len()
                            && row.target < self.actors.len()
                            && !row.values.is_empty()
                            && row.values.len() % 2 == 0
                    }),
                "invalid or unordered positioning inputs"
            );
        }
        // The selected request must own the next allocation. Its source ordinal
        // stays independent of this native ID and any explicitly linked releases.
        self.random_inputs = Some(Inputs {
            origin,
            owner,
            action,
            admission: ActionId(self.next_action),
            actions: BTreeSet::from([ActionId(self.next_action)]),
            admitted: false,
            scatter: scatter.into(),
            hammer_ring: hammer_ring.into(),
            velocity: velocity.into(),
            stun: stun.into(),
            ailments: ailments.map(Into::into),
            seals: seals.map(Into::into),
            criticals: criticals.map(Into::into),
            targets: targets.map(Into::into),
            positioning: positioning.map(Into::into),
        });
        Ok(())
    }

    /// Bind observed inputs to a chained action allocated before its first visit.
    pub fn install_allocated_random_inputs(
        &mut self,
        origin: u32,
        recorded: RecordedInputs,
        action: ActionId,
    ) -> Result<()> {
        let sequence = self
            .sequence(&action)
            .context("random input action is absent")?;
        let owner = sequence.actor;
        ensure!(
            origin == self.ledger.elapsed_ticks + 1
                && sequence.age == 0
                && matches!(
                    sequence.execution,
                    crate::action::Execution::Attack(crate::attack::AttackRun::Starting)
                        | crate::action::Execution::Casting(crate::casting::CastingRun::Starting)
                ),
            "random input action has already dispatched"
        );
        self.install_random_inputs(origin, recorded)?;
        let inputs = self.random_inputs.as_mut().unwrap();
        ensure!(
            inputs.owner == owner.index(),
            "random input action owner differs"
        );
        inputs.admission = action;
        inputs.actions = BTreeSet::from([action]);
        Ok(())
    }

    pub fn verify_random_input_admission(
        &mut self,
        owner: ActorId,
        action: ActionId,
    ) -> Result<()> {
        if let Some(inputs) = &mut self.random_inputs {
            ensure!(
                !inputs.admitted
                    && inputs.origin == self.ledger.elapsed_ticks
                    && inputs.owner == owner.index()
                    && inputs.admission == action,
                "random input action admission differs: expected tick {}, owner {}, action {:?}, admitted {}; got tick {}, owner {}, action {:?}",
                inputs.origin,
                inputs.owner,
                inputs.admission,
                inputs.admitted,
                self.ledger.elapsed_ticks,
                owner.index(),
                action,
            );
            inputs.admitted = true;
        }
        Ok(())
    }

    pub fn finish_random_inputs(&self) -> Result<()> {
        if let Some(inputs) = &self.random_inputs {
            ensure!(inputs.admitted, "random input action was not admitted");
            ensure!(
                inputs.scatter.is_empty()
                    && inputs.hammer_ring.is_empty()
                    && inputs.velocity.is_empty()
                    && inputs.stun.is_empty()
                    && inputs.ailments.as_ref().is_none_or(VecDeque::is_empty)
                    && inputs.seals.as_ref().is_none_or(VecDeque::is_empty)
                    && inputs.criticals.as_ref().is_none_or(VecDeque::is_empty)
                    && inputs.targets.as_ref().is_none_or(VecDeque::is_empty)
                    && inputs.positioning.as_ref().is_none_or(VecDeque::is_empty),
                "unused random inputs: {} scatter, {} hammer ring, {} velocity, {} stun, {} ailment, {} seal, {} critical, {} target, {} positioning",
                inputs.scatter.len(),
                inputs.hammer_ring.len(),
                inputs.velocity.len(),
                inputs.stun.len(),
                inputs.ailments.as_ref().map_or(0, VecDeque::len),
                inputs.seals.as_ref().map_or(0, VecDeque::len),
                inputs.criticals.as_ref().map_or(0, VecDeque::len),
                inputs.targets.as_ref().map_or(0, VecDeque::len),
                inputs.positioning.as_ref().map_or(0, VecDeque::len)
            );
        }
        Ok(())
    }
}
