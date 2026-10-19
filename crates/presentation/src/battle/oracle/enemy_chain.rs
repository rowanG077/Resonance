//! Finite observed starting choices; authored successors execute through ordinary chaining.
use super::{Binding, Case, Trace, random_inputs::Pin};
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActionId, ActionRequest, ActorId, Battle, BattleInput, Cue, FixtureEnemyChainInput,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::VecDeque;

#[derive(Deserialize)]
struct Request {
    row: usize,
    target: usize,
    after_updates: u32,
    bound_after_updates: u32,
    approach: bool,
}

#[derive(Deserialize)]
struct Admission {
    row: usize,
    after_updates: u32,
}

#[derive(Deserialize)]
struct Branch {
    row: usize,
    after_updates: u32,
    before: u32,
    after: u32,
    values: Vec<u16>,
    chance_index: usize,
    selection_index: Option<usize>,
}

#[derive(Deserialize)]
struct Proof {
    format: String,
    case: String,
    capture: Value,
    requests: VecDeque<Request>,
    admissions: VecDeque<Admission>,
    branches: Vec<Branch>,
}

pub(super) struct Plan {
    owner: ActorId,
    target_row: usize,
    origin: u32,
    requests: VecDeque<Request>,
    admissions: VecDeque<Admission>,
    branches: Vec<FixtureEnemyChainInput>,
    pending_start: Option<Cue>,
    recent: VecDeque<Value>,
}

impl Plan {
    pub(super) fn admit(case: &Case, source: &Trace) -> Result<Option<Self>> {
        let Some(pin) = source.setup.get("enemy_chain_inputs") else {
            return Ok(None);
        };
        let Binding::Enemy {
            row: target_row, ..
        } = case.binding
        else {
            anyhow::bail!("enemy chain input belongs to a party action");
        };
        let pin: Pin = serde_json::from_value(pin.clone())?;
        let proof: Proof = serde_json::from_slice(&pin.read_bytes()?)?;
        ensure!(
            proof.format == "source-enemy-chain-inputs-v1"
                && proof.case == case.id
                && proof.capture == source.setup["capture"],
            "enemy chain proof belongs to another capture"
        );
        let owner = ActorId::from_index(super::field(&source.setup, "owner")?)?;
        ensure!(
            !proof.requests.is_empty() && proof.admissions.len() > proof.requests.len(),
            "enemy chain proof contains no predecessor and successor"
        );
        let target: Vec<_> = proof
            .admissions
            .iter()
            .filter(|entry| entry.row == target_row)
            .collect();
        ensure!(
            target.len() == 1
                && target[0].after_updates
                    == super::field::<u32>(
                        &source.setup["input"],
                        "bound_admission_after_updates"
                    )?,
            "enemy chain target admission differs"
        );
        let mut previous = 0;
        for admission in &proof.admissions {
            ensure!(
                admission.after_updates > previous,
                "enemy chain admissions are unordered"
            );
            previous = admission.after_updates;
        }
        previous = 0;
        for request in &proof.requests {
            ensure!(
                request.after_updates > previous
                    && request.after_updates <= request.bound_after_updates
                    && (request.approach || request.after_updates == request.bound_after_updates)
                    && proof.admissions.iter().any(|entry| entry.row == request.row
                        && entry.after_updates == request.bound_after_updates),
                "unwitnessed enemy chain start"
            );
            previous = request.after_updates;
        }
        let mut branches = Vec::new();
        for branch in proof.branches {
            ensure!(
                !branch.values.is_empty() && branch.values.len() <= 128,
                "unbounded chain RNG interval"
            );
            let mut state = branch.before;
            for value in &branch.values {
                state = state.wrapping_mul(0x41c64e6d).wrapping_add(0x12d687);
                ensure!(
                    *value == (state >> 16) as u16,
                    "chain RNG interval has a different draw"
                );
            }
            ensure!(
                state == branch.after
                    && branch
                        .selection_index
                        .is_none_or(|index| index > branch.chance_index),
                "chain RNG endpoint or call order differs"
            );
            let chance = *branch
                .values
                .get(branch.chance_index)
                .context("missing observed chain chance")?;
            let selection = branch
                .selection_index
                .map(|index| {
                    branch
                        .values
                        .get(index)
                        .copied()
                        .context("missing observed chain choice")
                })
                .transpose()?;
            branches.push(FixtureEnemyChainInput {
                clock: branch.after_updates,
                actor: owner,
                row: branch.row,
                chance,
                selection,
            });
        }
        Ok(Some(Self {
            owner,
            target_row,
            origin: 0,
            requests: proof.requests,
            admissions: proof.admissions,
            branches,
            pending_start: None,
            recent: VecDeque::new(),
        }))
    }

    pub(super) fn begin(&mut self, core: &mut Battle) -> Result<()> {
        self.origin = core.ledger().combat_ticks;
        core.install_fixture_enemy_chains(
            self.owner,
            self.origin,
            std::mem::take(&mut self.branches),
        )
    }

    pub(super) fn submit(&mut self, core: &mut Battle, input: &mut BattleInput) -> Result<()> {
        let elapsed = core.ledger().combat_ticks + 1 - self.origin;
        let Some(request) = self.requests.front() else {
            return Ok(());
        };
        ensure!(
            request.after_updates >= elapsed,
            "observed enemy start was not submitted"
        );
        if request.after_updates != elapsed {
            return Ok(());
        }
        let request = self.requests.pop_front().unwrap();
        let action = core.select_fixture_enemy_row(self.owner, request.row)?;
        let target = ActorId::from_index(request.target)?;
        let action = ActionRequest {
            actor: self.owner,
            action,
            target,
        };
        if request.approach {
            ensure!(
                core.request_enemy_approach(action, target)?,
                "observed chain approach was rejected"
            );
        } else {
            input.actions.push(action);
        }
        Ok(())
    }

    pub(super) fn pending_action(&self) -> Option<ActionId> {
        match self.pending_start {
            Some(Cue::Started { action, .. }) => Some(action),
            _ => None,
        }
    }

    pub(super) fn observe_dispatch(
        &mut self,
        core: &Battle,
        frame: &mut resonance_battle::BattleFrame,
    ) -> Result<()> {
        let actor = &core.actors()[self.owner.index()];
        self.recent.push_back(json!({
            "combat":core.ledger().combat_ticks - self.origin,
            "position":actor.position,"forward":actor.movement.forward,
            "activity":format!("{:?}", core.activity(self.owner)),
            "action_ages":frame.actions.iter().filter_map(|(_, owner, age)|
                (*owner == self.owner).then_some(*age)).collect::<Vec<_>>()
        }));
        if self.recent.len() > 8 {
            self.recent.pop_front();
        }
        let cues = &mut frame.cues;
        // A chained action can be allocated after this update's action visits.
        // Compare its first executed visit, independently of the expected clock.
        if let Some(action) = self.pending_action() {
            let age = core
                .action_age(action)
                .context("chain action vanished before dispatch")?;
            if age > 0 {
                cues.insert(0, self.pending_start.take().unwrap());
            }
        }
        for cue in std::mem::take(cues) {
            if let Cue::Started { actor, action, .. } = cue
                && actor == self.owner
                && core.action_age(action) == Some(0)
            {
                ensure!(
                    self.pending_start.is_none(),
                    "overlapping undispatched chain actions"
                );
                self.pending_start = Some(cue);
            } else {
                cues.push(cue);
            }
        }
        Ok(())
    }

    pub(super) fn observe(&mut self, core: &Battle) -> Result<()> {
        let expected = self
            .admissions
            .pop_front()
            .context("unobserved action in enemy chain")?;
        ensure!(
            core.fixture_enemy_choice(self.owner) == Some(expected.row)
                && core.ledger().combat_ticks - self.origin == expected.after_updates,
            "enemy chain admission differs: expected row {}, combat offset {}; observed row {:?}, combat offset {}; recent native visits {:?}",
            expected.row,
            expected.after_updates,
            core.fixture_enemy_choice(self.owner),
            core.ledger().combat_ticks - self.origin,
            self.recent
        );
        Ok(())
    }

    pub(super) fn target(&self, core: &Battle) -> bool {
        core.fixture_enemy_choice(self.owner) == Some(self.target_row)
    }

    pub(super) fn invocation(&self, core: &Battle) -> Result<Value> {
        Ok(json!({
            "row": core.fixture_enemy_choice(self.owner).context("missing chain choice")?,
            "bound_after_updates": core.ledger().combat_ticks - self.origin,
        }))
    }

    pub(super) fn finish(&self, core: &Battle) -> Result<()> {
        ensure!(
            self.requests.is_empty() && self.admissions.is_empty() && self.pending_start.is_none(),
            "unused enemy chain history"
        );
        core.finish_fixture_enemy_chains()
    }
}
