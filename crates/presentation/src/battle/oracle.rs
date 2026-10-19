//! Catalogue execution observations through real preparation, models and effects.
//! Comparison with independently observed source traces belongs to `compare`.
use super::feedback::Feedback;
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActionId, ActionRequest, Activity, ActorId, BattleInput, BattlePhase, ContactSource, Cue,
    EffectFollow, Effects, ParticleId, ProjectileId, Side,
};
use resonance_content::{
    arte::{Catalogue, RegalArteFamily, TechniqueTarget},
    battle_action::{EnemyEvent, EnemyExecution, TargetPolicy},
    battle_enemy,
    diagnostics::Diagnostics,
    menu_data::MenuData,
    prepared::{Cache, Files},
    session::SessionData,
};
use resonance_events::{
    battle::{DefeatPolicy, Encounter, Setup},
    party::Party,
};
use resonance_game::battle::encounter::{Inputs, PrepareOptions};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
mod compare;
mod effect_inputs;
mod effect_pitch;
mod enemy_chain;
mod projectile_random;
mod random_inputs;
mod secondary_body;
mod support;
mod tests;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "side", rename_all = "snake_case")]
pub(super) enum Binding {
    Party { character: u8, arte: u16 },
    Enemy { enemy: u8, row: usize },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContextKind {
    Attack,
    Aerial,
    Support,
    Revival,
    Counter,
    Capture,
    Departure,
    Movement,
    Presentation,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Case {
    pub id: String,
    pub binding: Binding,
    pub context: ContextKind,
}
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Trace {
    pub case: String,
    pub setup: Value,
    pub frames: Vec<Value>,
    pub events: Vec<Value>,
    pub assessment: Value,
    pub incomplete_observations: Vec<String>,
}

const SACRED_LIGHT: u16 = 222;

fn presentation_requirements(
    attack: &resonance_content::battle_action::EnemyAttack,
    body: &resonance_content::battle_model::Enemy,
    null_motions: &BTreeSet<u16>,
) -> Option<Value> {
    let mut poses = Vec::new();
    let mut effects = Vec::new();
    let mut empty_groups = BTreeSet::<u8>::new();
    let mut retained_pose = false;
    for (_, event) in &attack.events {
        match event {
            EnemyEvent::Pose(pose) if null_motions.contains(&pose.clip) => retained_pose = true,
            EnemyEvent::Pose(pose) => poses.push(json!({"clip":pose.clip,"first_bone":null})),
            EnemyEvent::PartialPose { first_bone, pose } => {
                poses.push(json!({"clip":pose.clip,"first_bone":first_bone}))
            }
            EnemyEvent::Effect(member) => effects.push(*member),
            EnemyEvent::Contact(contact) => {
                let resonance_content::battle_action::ContactAttachments::Groups(groups) =
                    &contact.attachments
                else {
                    return None;
                };
                if groups.is_empty()
                    || groups.iter().any(|&group| group >= 12)
                    || body.attachments.values().any(|part| {
                        // A one-node decorative rig has no melee markers.
                        part.rig.skeleton.bones.len() != 1
                            || part.rig.skeleton.bones[0].name != "default"
                            || !part.rig.attachments.is_empty()
                            || !part.layer_skeletons.is_empty()
                    })
                    || !body.weapon_variants.is_empty()
                    || body.secondary_body.is_some()
                    || body.body.skeleton.bones.is_empty()
                    || body.body.skeleton.bones.iter().any(|bone| {
                        let name = bone.name.as_bytes();
                        name.get(..2)
                            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"at"))
                            && name
                                .get(3)
                                .and_then(|digit| digit.checked_sub(b'0'))
                                .filter(|group| *group <= 9)
                                .is_none_or(|group| groups.contains(&group))
                    })
                {
                    return None;
                }
                empty_groups.extend(groups);
            }
            EnemyEvent::OffsetPosition {
                height: 0,
                retreat: 0,
            }
            | EnemyEvent::Vertical(0.0)
            | EnemyEvent::Gravity(0.0)
            | EnemyEvent::Sound(_)
            | EnemyEvent::Voice { .. } => {}
            _ => return None,
        }
    }
    if poses.is_empty() && effects.is_empty() && !retained_pose {
        return None;
    }
    let mut requirements = json!({"poses":poses,"effects":effects});
    if retained_pose && poses.is_empty() {
        requirements["retained_pose"] = json!(true);
    }
    if !empty_groups.is_empty() {
        requirements["empty_contact_groups"] = json!(empty_groups);
    }
    Some(requirements)
}

pub(super) struct Oracle {
    root: PathBuf,
    cache: Cache,
    retained: Files,
    menus: MenuData,
    session: SessionData,
    catalogue: Catalogue,
}
/// Reconstruct effects from the first initialized source visit.
fn initial_effect_entry(setup: &Value) -> Result<bool> {
    let births = setup.get("ready_particle_births");
    let Some(lists) = setup
        .get("ready_effect_lists")
        .or_else(|| births.and_then(|_| setup.get("observed_ready_effect_lists")))
    else {
        return Ok(false);
    };
    let battle = &setup["initial_battle"];
    let completed: u16 = field(battle, "presentation_tick")?;
    ensure!(
        completed <= 1 && field::<u32>(battle, "combat_tick")? == 0,
        "effect entry must be an initial visit"
    );
    let births = if completed == 1 {
        let births = births
            .and_then(Value::as_array)
            .context("first effect visit lacks observed births")?;
        ensure!(
            !births.is_empty()
                && births.iter().all(|birth| birth["age"] == 1)
                && setup.get("ready_ambient_effect").is_none()
                && ["ready_attachment_effects", "ready_idle_effects"]
                    .iter()
                    .all(|key| { setup[*key].as_array().is_some_and(Vec::is_empty) }),
            "first effect visit is not a complete body-emitter observation"
        );
        births.as_slice()
    } else {
        &[]
    };
    for key in [
        "ready_passive_effects",
        "ready_attachment_effects",
        "ready_idle_effects",
    ] {
        for clock in setup[key]
            .as_array()
            .context("missing pristine effect clocks")?
        {
            ensure!(
                field::<u16>(clock, "counter")? == completed,
                "effect entry clock has advanced beyond its observed visits"
            );
        }
    }
    let pool: u32 = field(battle, "pool")?;
    let lists = lists.as_array().context("invalid ready effect lists")?;
    ensure!(lists.len() == 4, "pristine effect lists are incomplete");
    for (list, (kind, offset)) in lists.iter().zip([
        ("projectile", 0x75c40),
        ("program", 0x75ec8),
        ("particle", 0x763d8),
        ("alternate", 0x76660),
    ]) {
        let sentinel = pool
            .checked_add(offset)
            .context("effect list address overflow")?;
        let (first, last) = match (kind, births.first(), births.last()) {
            ("program", Some(birth), _) => {
                let parent: u32 = field(birth, "parent")?;
                ensure!(
                    births.iter().all(|birth| birth["parent"] == parent),
                    "first effect visit has multiple programs"
                );
                let program = parent.checked_sub(0x28).context("invalid effect parent")?;
                (program, program)
            }
            ("particle", Some(first), Some(last)) => {
                (field(first, "address")?, field(last, "address")?)
            }
            _ => (sentinel, sentinel),
        };
        ensure!(
            [sentinel, first, last]
                .iter()
                .all(|address| (0x80000000..0x81800000).contains(address))
                && list["kind"] == kind
                && field::<u32>(list, "sentinel")? == sentinel
                && field::<u32>(list, "next")? == first
                && field::<u32>(list, "previous")? == last,
            "initial source effect list differs from its observed births"
        );
    }
    let actors = setup["source_actor_map"]
        .as_array()
        .context("missing source actors")?;
    let poses = setup["ready_models"]
        .as_array()
        .context("missing source poses")?;
    let models = setup["ready_model_observations"]
        .as_array()
        .context("missing bound model observations")?;
    ensure!(
        models.len() == actors.len() && poses.len() == actors.len(),
        "pristine model observations are incomplete"
    );
    for (index, model) in models.iter().enumerate() {
        ensure!(
            field::<usize>(model, "actor")? == index
                && model["source_slot"] == actors[index]["source_slot"]
                && field::<u32>(model, "child")? == 0
                && (0x80000000..0x81800000).contains(&field::<u32>(model, "controller")?)
                && (0x80000000..0x81800000).contains(&field::<u32>(model, "resource")?)
                && model["frame"] == poses[index]["frame"],
            "pristine source model is not bound at its observed frame"
        );
    }
    Ok(true)
}

impl Oracle {
    pub fn load(root: &Path) -> Result<Self> {
        let mut cache = Cache::default();
        let retained = Files::load_with_diagnostics(
            root,
            &["fields/map-332.preload.json"],
            &mut cache,
            || false,
            Diagnostics::new(true),
        )?;
        let menus: MenuData = retained.json("game/menu-data.json")?;
        let mut session: SessionData = retained.json("game/session-data.json")?;
        session.rules = Some(Arc::new(menus.clone()));
        Ok(Self {
            root: root.into(),
            cache,
            menus,
            session,
            catalogue: retained.json(resonance_content::arte::PATH)?,
            retained,
        })
    }

    /// Enumerate authored entries, never the subset the native dispatcher accepts.
    pub fn cases(&self) -> Result<Vec<Case>> {
        let mut cases = Vec::new();
        for (index, entries) in self
            .catalogue
            .learning
            .iter()
            .take(self.session.characters.len())
            .enumerate()
        {
            let character = u8::try_from(index + 1)?;
            for &id in entries {
                if id == 0 {
                    continue;
                }
                let arte = u16::from(id);
                let capability = self.catalogue.definition(usize::from(arte))?.capabilities;
                let context = if arte == SACRED_LIGHT {
                    ContextKind::Presentation
                } else if capability.revives {
                    ContextKind::Revival
                } else if capability.healing
                    || matches!(
                        capability.target,
                        TechniqueTarget::Ally | TechniqueTarget::SelfTarget
                    )
                {
                    ContextKind::Support
                } else if capability.regal_family == Some(RegalArteFamily::Aerial) {
                    ContextKind::Aerial
                } else if !capability.offensive
                    && !capability.spell
                    && capability.target == TechniqueTarget::Enemy
                {
                    ContextKind::Movement
                } else {
                    ContextKind::Attack
                };
                cases.push(Case {
                    id: format!("party:{character}:{arte}"),
                    binding: Binding::Party { character, arte },
                    context,
                });
            }
        }
        let mut enemies = Vec::new();
        for entry in fs::read_dir(self.root.join("battle/enemies"))? {
            let entry = entry?;
            if let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u8>().ok())
                && entry.path().join("definition.json").is_file()
            {
                enemies.push(id);
            }
        }
        enemies.sort_unstable();
        let placements: resonance_content::battle_unison::Catalogue =
            self.read(resonance_content::battle_unison::PATH)?;
        for enemy in enemies {
            let definition: battle_enemy::Definition = self.read(&battle_enemy::path(enemy))?;
            let body: resonance_content::battle_model::Enemy =
                self.read(&resonance_content::battle_model::enemy_path(enemy))?;
            ensure!(
                body.source_sha256 == definition.source_sha256,
                "enemy action and body packages differ"
            );
            for (row, action) in definition.actions.rows.iter().enumerate() {
                let attack = match &action.execution {
                    Some(
                        EnemyExecution::Attack(attack) | EnemyExecution::Physical { attack, .. },
                    ) => Some(attack),
                    _ => None,
                };
                let offensive_spell = match &action.execution {
                    Some(EnemyExecution::Spell(native)) => placements
                        .placements
                        .iter()
                        .position(|placement| placement.native_id == *native)
                        .map(|id| self.catalogue.definition(id))
                        .transpose()?
                        .is_some_and(|definition| definition.capabilities.offensive),
                    _ => false,
                };
                let context = if matches!(action.execution, Some(EnemyExecution::LeaveArena)) {
                    ContextKind::Departure
                } else if offensive_spell {
                    ContextKind::Attack
                } else if action.target_policy == Some(TargetPolicy::NearestIncapacitated) {
                    ContextKind::Revival
                } else if attack.is_some_and(|attack| {
                    attack
                        .events
                        .iter()
                        .any(|(_, event)| matches!(event, EnemyEvent::ReleaseCaptured { .. }))
                }) {
                    ContextKind::Capture
                } else if action.target_allies
                    || action.target_policy == Some(TargetPolicy::SelfTarget)
                    || attack.is_some_and(|attack| {
                        attack.events.iter().any(|(_, event)| {
                            matches!(
                                event,
                                EnemyEvent::StatUp { .. } | EnemyEvent::UseItem { .. }
                            )
                        })
                    })
                {
                    ContextKind::Support
                } else if attack.is_some_and(|attack| {
                    attack.events.iter().any(|(_, event)| {
                        matches!(
                            event,
                            EnemyEvent::Forward(_)
                                | EnemyEvent::SetPosition(_)
                                | EnemyEvent::PositionFromTarget { .. }
                        ) || matches!(event, EnemyEvent::OffsetPosition { height, retreat } if *height != 0 || *retreat != 0)
                    }) && attack.events.iter().all(|(_, event)| {
                        matches!(
                            event,
                            EnemyEvent::Effect(_)
                                | EnemyEvent::Pose(_)
                                | EnemyEvent::Forward(_)
                                | EnemyEvent::Vertical(_)
                                | EnemyEvent::Acceleration(_)
                                | EnemyEvent::Gravity(_)
                                | EnemyEvent::Braking(_)
                                | EnemyEvent::SetPosition(_)
                                | EnemyEvent::PositionFromTarget { .. }
                                | EnemyEvent::OffsetPosition { .. }
                                | EnemyEvent::TurnMotion(_)
                                | EnemyEvent::Sound(_)
                                | EnemyEvent::Voice { .. }
                        )
                    })
                }) {
                    ContextKind::Movement
                } else if attack
                    .and_then(|attack| {
                        presentation_requirements(attack, &body, &definition.null_motions)
                    })
                    .is_some()
                {
                    ContextKind::Presentation
                } else {
                    ContextKind::Attack
                };
                cases.push(Case {
                    id: format!("enemy:{enemy}:{row}"),
                    binding: Binding::Enemy { enemy, row },
                    context,
                });
            }
        }
        Ok(cases)
    }

    fn read<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        Ok(serde_json::from_slice(
            &fs::read(self.root.join(path)).with_context(|| path.to_owned())?,
        )?)
    }

    pub fn run(&mut self, case: &Case, source: &Trace, max_updates: u32) -> Result<Trace> {
        ensure!(max_updates > 0, "empty observation budget");
        ensure!(
            source.case == case.id,
            "source trace belongs to another catalogue entry"
        );
        ensure!(
            !source.frames.is_empty(),
            "source trace has no execution window"
        );
        let setup = &source.setup;
        let mut enemy_chain = enemy_chain::Plan::admit(case, source)?;
        if case.context == ContextKind::Presentation {
            let expected = match case.binding {
                Binding::Party {
                    arte: SACRED_LIGHT, ..
                } => {
                    json!({"poses":[], "effects":[1]})
                }
                Binding::Enemy { enemy, row } => {
                    let definition: battle_enemy::Definition =
                        self.read(&battle_enemy::path(enemy))?;
                    let attack = match &definition.actions.rows[row].execution {
                        Some(
                            EnemyExecution::Attack(attack)
                            | EnemyExecution::Physical { attack, .. },
                        ) => attack,
                        _ => anyhow::bail!("presentation requires an authored attack timeline"),
                    };
                    let body: resonance_content::battle_model::Enemy =
                        self.read(&resonance_content::battle_model::enemy_path(enemy))?;
                    ensure!(
                        body.source_sha256 == definition.source_sha256,
                        "presentation action and body packages differ"
                    );
                    presentation_requirements(attack, &body, &definition.null_motions)
                        .context("action has non-presentation events")?
                }
                _ => anyhow::bail!("presentation requires an authored presentation action"),
            };
            if let Some(groups) = expected.get("empty_contact_groups") {
                let groups: Vec<u8> = serde_json::from_value(groups.clone())?;
                let counts = vec![0u8; groups.len()];
                ensure!(
                    setup["presentation_contact_bindings"]
                        == json!({"groups":groups,
                    "samples":[{"stage":"ready","counts":counts},
                               {"stage":"terminal","counts":counts}]}),
                    "inert presentation contacts lack initialized boundary observations"
                );
            }
            ensure!(
                setup["presentation"] == expected,
                "presentation requirements differ from authored timeline"
            );
        }
        #[derive(Deserialize)]
        struct StickInterval {
            press_after_updates: u32,
            release_after_updates: u32,
            stick: [i8; 2],
        }
        #[derive(Deserialize)]
        struct RecipientInput {
            actor: usize,
            mode: String,
            port: u8,
            press_after_updates: Option<u32>,
            down_after_updates: Option<u32>,
            release_after_updates: u32,
            stick: Option<[i8; 2]>,
            #[serde(default)]
            stick_intervals: Vec<StickInterval>,
        }
        let recipient_input: Option<RecipientInput> = setup
            .get("recipient_input")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        if let Some(control) = &recipient_input {
            ensure!(
                matches!(case.binding, Binding::Enemy { .. })
                    && control.actor == 0
                    && setup["target"] == 0
                    && control.port == 0
                    && matches!(
                        control.mode.as_str(),
                        "none" | "normal" | "special" | "attack" | "stick"
                    )
                    && control.release_after_updates > 0
                    && matches!(control.mode.as_str(), "attack" | "stick")
                        == control.press_after_updates.is_some()
                    && if control.mode == "stick" {
                        matches!(
                            control.stick,
                            Some([127, 0] | [-127, 0] | [0, 127] | [127, 127] | [-127, 127])
                        )
                    } else {
                        control.stick.is_none()
                    }
                    && control
                        .press_after_updates
                        .is_none_or(|press| { press > 0 && press < control.release_after_updates })
                    && (control.mode == "special") == control.down_after_updates.is_some()
                    && control.down_after_updates.is_none_or(|down| {
                        down > 4 && down.saturating_add(2) < control.release_after_updates
                    }),
                "invalid bounded recipient input"
            );
            let mut previous_release = control.release_after_updates;
            for interval in &control.stick_intervals {
                ensure!(
                    interval.press_after_updates >= previous_release
                        && interval.press_after_updates < interval.release_after_updates
                        && matches!(interval.stick, [127, 0] | [-127, 0]),
                    "invalid bounded recipient movement interval"
                );
                previous_release = interval.release_after_updates;
            }
        }
        let support: Option<support::Observation> = setup
            .get("support")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        let observe_time_stop = support.is_some_and(|observation| observation.time_stop);
        ensure!(
            !observe_time_stop
                || setup["actors"]
                    .as_array()
                    .is_some_and(|actors| { actors.iter().all(|actor| actor["time_stop"] == 0) }),
            "time-stop fixture lacks an observed zero initial state"
        );
        ensure!(
            support.is_none()
                || matches!(case.context, ContextKind::Support | ContextKind::Revival),
            "support observations require a support action"
        );
        let scripted_defeat = setup.get("self_defeat").is_some();
        let self_stun = setup.get("self_stun").is_some();
        if scripted_defeat || self_stun {
            let Binding::Enemy { enemy, row } = case.binding else {
                anyhow::bail!("scripted interruption requires an enemy action");
            };
            let definition: battle_enemy::Definition = self.read(&battle_enemy::path(enemy))?;
            let attack = match &definition.actions.rows[row].execution {
                Some(EnemyExecution::Attack(attack) | EnemyExecution::Physical { attack, .. }) => {
                    attack
                }
                _ => anyhow::bail!("scripted interruption requires an authored attack"),
            };
            ensure!(
                !scripted_defeat
                    || (attack
                        .events
                        .iter()
                        .any(|(_, event)| matches!(event, EnemyEvent::Defeat { .. }))
                        && setup["self_defeat"]["retain_body"]
                            == definition.profile.traits.retain_defeated_body),
                "self-defeat terminal differs from the authored action"
            );
            ensure!(
                setup["self_defeat"]["terminal_only"] != true
                    || attack.events.iter().all(|(_, event)| {
                        matches!(
                            event,
                            EnemyEvent::Pose(_)
                                | EnemyEvent::PartialPose { .. }
                                | EnemyEvent::Sound(_)
                                | EnemyEvent::Voice { .. }
                                | EnemyEvent::Defeat { .. }
                        )
                    }),
                "terminal-only defeat contains another authored action outcome"
            );
            ensure!(
                !self_stun
                    || (setup["self_stun"] == true
                        && attack
                            .events
                            .iter()
                            .any(|(_, event)| matches!(event, EnemyEvent::Stun))),
                "self-stun terminal differs from the authored action"
            );
        }
        let self_defeated = scripted_defeat
            || support
                .is_some_and(|observation| observation.terminal == support::Terminal::SelfDefeated);
        let guard_input = setup["input"]["button"] == "guard_down";
        ensure!(
            !guard_input || support.is_some(),
            "guard input needs support-state observations"
        );
        let formation: u16 = field(setup, "formation")?;
        let arena: u16 = field(setup, "arena")?;
        let source_seed: u64 = field(setup, "seed")?;
        // Independent random streams can use explicit seeds to exercise the same
        // stochastic branch. Keep the captured source seed unchanged.
        let seed = setup
            .get("native_seed")
            .cloned()
            .map(serde_json::from_value::<u64>)
            .transpose()?
            .unwrap_or(source_seed);
        let input_arte = match case.binding {
            Binding::Party { arte, .. } => Some(
                setup["input"]
                    .get("selection_arte")
                    .cloned()
                    .map(serde_json::from_value::<u16>)
                    .transpose()?
                    .unwrap_or(arte),
            ),
            Binding::Enemy { .. } => None,
        };
        let support_input = input_arte
            .map(|arte| self.catalogue.definition(usize::from(arte)))
            .transpose()?
            .is_some_and(|definition| {
                definition.capabilities.healing
                    || matches!(
                        definition.capabilities.target,
                        TechniqueTarget::Ally | TechniqueTarget::SelfTarget
                    )
            });
        let request_delay: u32 = field(&setup["input"], "bound_admission_after_updates")?;
        let technique_selection = if matches!(case.binding, Binding::Party { .. })
            && case.context == ContextKind::Attack
            && !support_input
            && setup["input"]["button"] == "b"
        {
            let selection: u32 = field(&setup["input"], "request_after_updates")?;
            let polls: u32 = field(&setup["input"], "polls")?;
            ensure!(
                selection > 0 && selection <= request_delay && polls > 0 && polls.is_multiple_of(2),
                "source technique selection is outside its admission interval"
            );
            Some((selection, polls / 2))
        } else {
            None
        };
        let (guard_lead_updates, guard_selection_before_admission) = if guard_input {
            let polls: u32 = field(&setup["input"], "guard_lead_polls")?;
            let selection: u32 = field(&setup["input"], "request_after_updates")?;
            // Each combat update consumes two controller polls. Keep guarding
            // through the pending selection before native bound admission.
            ensure!(
                polls > 0
                    && polls.is_multiple_of(2)
                    && selection > polls / 2
                    && selection <= request_delay,
                "source guard lead is not a complete input interval"
            );
            (
                request_delay - selection + polls / 2,
                request_delay - selection,
            )
        } else {
            (0, 0)
        };
        let aerial_input = if case.context == ContextKind::Aerial {
            let lead: u32 = field(&setup["input"], "aerial_lead_polls")?;
            let up: u32 = field(&setup["input"], "aerial_up_polls")?;
            let selection: u32 = field(&setup["input"], "request_after_updates")?;
            ensure!(
                up > 0
                    && up < lead
                    && up.is_multiple_of(2)
                    && lead.is_multiple_of(2)
                    && selection > lead / 2
                    && selection <= request_delay,
                "source jump lead is not a complete input interval"
            );
            Some((request_delay - selection + lead / 2, up / 2))
        } else {
            None
        };
        let pending_entry: Option<u32> = setup["input"]
            .get("pending_entry_after_updates")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        ensure!(
            request_delay > 0,
            "source request must follow its ready snapshot"
        );
        ensure!(
            pending_entry.is_none_or(|delay| delay > 0 && delay <= request_delay)
                && (pending_entry.is_none() || matches!(case.binding, Binding::Enemy { .. })),
            "source pending entry is not an observed enemy admission interval"
        );
        #[derive(Deserialize)]
        struct ApproachTargetChange {
            after_updates: u32,
            target: usize,
        }
        let approach_target_changes: Vec<ApproachTargetChange> = setup
            .get("approach_target_changes")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        #[derive(Deserialize)]
        struct MenuControlChange {
            actor: usize,
            after_updates: u32,
            previous: u8,
            mode: u8,
        }
        #[derive(Deserialize)]
        struct MenuPrelude {
            frozen_updates: u32,
            command_after_updates: u32,
            control_changes: Vec<MenuControlChange>,
        }
        let menu_prelude: Option<MenuPrelude> = setup
            .get("menu_prelude")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        if let Some(menu) = &menu_prelude {
            ensure!(
                matches!(case.binding, Binding::Party { .. })
                    && setup["input"]["button"] == "tech_menu"
                    && menu.frozen_updates > 0
                    && menu.frozen_updates <= max_updates
                    && menu.command_after_updates > 0
                    && menu.command_after_updates <= request_delay
                    && menu.control_changes.iter().all(|change| {
                        change.actor == setup["owner"].as_u64().unwrap_or(u64::MAX) as usize
                            && change.after_updates < menu.command_after_updates
                            && change.previous <= 2
                            && change.mode <= 2
                    }),
                "source menu input is outside its observed prelude"
            );
        }
        let defense_deployed: Option<bool> = setup
            .get("defense_deployed")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        let initial_effect_entry = initial_effect_entry(setup)?;
        let ready_combat: u32 = field(&setup["initial_battle"], "combat_tick")?;
        let ready_presentation: u32 = field(&setup["initial_battle"], "presentation_tick")?;
        let onset_combat: u32 = field(&source.frames[0], "combat_tick")?;
        let onset_presentation: u32 = field(&source.frames[0], "presentation_tick")?;
        ensure!(
            onset_combat.checked_sub(ready_combat) == Some(request_delay),
            "source admission delay disagrees with its observed clocks"
        );
        let entry_pose_updates = onset_presentation
            .checked_sub(ready_presentation)
            .and_then(|elapsed| elapsed.checked_sub(request_delay))
            .and_then(|elapsed| {
                elapsed.checked_sub(menu_prelude.as_ref().map_or(0, |menu| menu.frozen_updates))
            })
            .context("source presentation clock precedes its qualified combat/menu clocks")?;
        ensure!(
            entry_pose_updates <= max_updates && (entry_pose_updates == 0 || ready_combat == 0),
            "source non-combat prelude is not an observed entry interval"
        );
        #[derive(Deserialize)]
        struct ReadyConditionVisit {
            actor: usize,
            position: [f32; 3],
        }
        let ready_condition_visits: Vec<ReadyConditionVisit> = serde_json::from_value(
            setup
                .get("ready_condition_visits")
                .cloned()
                .unwrap_or_else(|| json!([])),
        )?;
        #[derive(Deserialize)]
        struct EntryPlayback {
            after_updates: u32,
            actor: usize,
            clip: u16,
            frame: f32,
            end: f32,
            rate: f32,
            repeat: bool,
            blend: u8,
            loop_start: f32,
        }
        let entry_playback: Vec<EntryPlayback> = setup
            .get("entry_model_transitions")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        ensure!(
            entry_playback
                .iter()
                .all(|pose| { pose.after_updates > 0 && pose.after_updates <= entry_pose_updates })
                && entry_playback.windows(2).all(|pair| {
                    (pair[0].after_updates, pair[0].actor) < (pair[1].after_updates, pair[1].actor)
                }),
            "source entry model transitions are outside the noncombat prelude or unordered"
        );
        #[derive(Deserialize)]
        struct EntryPlacement {
            after_updates: u32,
            actor: usize,
            position: [f32; 3],
        }
        let entry_placements: Vec<EntryPlacement> = setup
            .get("entry_placements")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        ensure!(
            entry_placements.iter().all(|placement| {
                placement.after_updates > 0 && placement.after_updates <= entry_pose_updates
            }) && entry_placements.windows(2).all(|pair| {
                (pair[0].after_updates, pair[0].actor) < (pair[1].after_updates, pair[1].actor)
            }),
            "source entry placements are outside the noncombat prelude or unordered"
        );
        #[derive(Deserialize)]
        struct CarriedClock {
            actor: usize,
            slot: u8,
            interval: u16,
            counter: u16,
        }
        let carried_clocks: Vec<CarriedClock> = setup
            .get("ready_attachment_effects")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        #[derive(Deserialize)]
        struct PassiveClock {
            actor: usize,
            program: u16,
            interval: u16,
            counter: u16,
        }
        let passive_clocks: Vec<PassiveClock> = setup
            .get("ready_passive_effects")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let idle_clocks: Vec<PassiveClock> = setup
            .get("ready_idle_effects")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        #[derive(Deserialize)]
        struct AmbientClock {
            bank: String,
            program: u16,
            interval: u16,
            counter: u16,
            elapsed_updates: u16,
        }
        let ambient_clock: Option<AmbientClock> = setup
            .get("ready_ambient_effect")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        if let Some(observed) = &ambient_clock {
            ensure!(
                observed.interval > 0
                    && observed.counter < observed.interval
                    && observed.elapsed_updates % observed.interval == observed.counter
                    && u32::from(observed.elapsed_updates) <= ready_presentation
                    && (ready_combat != 0
                        || ready_presentation == u32::from(observed.elapsed_updates)),
                "source ambient phase differs from the observed ready clock"
            );
        }
        let mut party = Party::new(&self.session, Default::default())?;
        party.formation = field(setup, "party")?;
        party.field_leader = *party.formation.first().context("source party is empty")?;
        party.settings.battle_controls = [0; 4];
        party.settings.preferences.battle_rank = field(setup, "battle_rank")?;
        for member in &mut party.members {
            member.techniques.clear();
            member.disabled_techniques.clear();
            member.shortcuts = [0; 4];
            member.assist_shortcuts = [None; 2];
        }
        for member in setup["party_members"]
            .as_array()
            .context("source party equipment/progression is missing")?
        {
            let character: u8 = field(member, "character")?;
            let prepared = party
                .members
                .get_mut(usize::from(
                    character
                        .checked_sub(1)
                        .context("invalid source character")?,
                ))
                .context("invalid source character")?;
            prepared.equipment = field(member, "equipment")?;
            prepared.costume = field(member, "costume")?;
            prepared.level = field(member, "level")?;
            prepared.base_stats = field(member, "base_stats")?;
            prepared.luck = field(member, "luck")?;
            prepared.technique_balance = field(member, "technique_balance")?;
            prepared.ex_skills = field(member, "ex_skills")?;
            prepared.ex_gems = field(member, "ex_gems")?;
            prepared.compound_ex_skills = field(member, "compound_ex_skills")?;
            prepared.techniques = field(member, "techniques")?;
            if let Some(counts) = member.get("technique_uses") {
                prepared.technique_uses = serde_json::from_value(counts.clone())?;
            }
            prepared.recent_compound_ex_skills.clear();
            prepared.hp = prepared.base_stats[0];
            prepared.tp = prepared.base_stats[1];
        }
        if let Binding::Party { character, .. } = case.binding {
            let arte = input_arte.unwrap();
            ensure!(
                party.members[usize::from(character - 1)]
                    .techniques
                    .contains(&arte),
                "source owner has not learned the requested arte"
            );
            if technique_selection.is_some() || menu_prelude.is_some() {
                party.settings.battle_controls = field(&setup["initial_battle"], "party_controls")?;
                party.members[usize::from(character - 1)].shortcuts[0] = arte;
            }
        }
        party.validate(&self.session)?;
        #[derive(Deserialize)]
        struct EventRoute {
            route: [i32; 5],
            music: u16,
        }
        let event_route: Option<EventRoute> = setup
            .get("event_route")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        let options = || -> Result<PrepareOptions> {
            Ok(PrepareOptions {
                event_flags: field(setup, "event_flags")?,
                devils_arms_unlocked: field(setup, "devils_arms_unlocked")?,
                random_seed: seed,
                map: field(setup, "map")?,
                world_music: field(setup, "world_music")?,
                story: field(setup, "story")?,
                colette_state: field(setup, "colette_state")?,
                victory_story_flags: field(setup, "victory_story_flags")?,
                overlimit_boost: field(setup, "overlimit_boost")?,
            })
        };
        let inputs = Inputs::load(
            &self.root,
            &self.retained,
            &self.menus,
            &self.session,
            &party,
            Setup {
                route: event_route.as_ref().map_or([0; 5], |event| event.route),
                encounter: Encounter::Formation(formation),
                arena,
                defeat: if event_route.is_some() {
                    DefeatPolicy::GameOver
                } else {
                    DefeatPolicy::ResumeEvent
                },
                music: event_route.as_ref().map(|event| event.music),
            },
            &mut self.cache,
            || false,
        )?;
        let completed_events = if let Some(event) = &event_route {
            ensure!(
                ready_combat > u32::try_from(event.route[2])?,
                "event fixture precedes its observed combat interruption"
            );
            let mut opening = inputs.prepare(&self.menus, options()?, |sound| Ok(Some(sound)))?;
            ensure!(
                opening.lifecycle.has_events(),
                "event route prepared no events"
            );
            opening.core.suppress_automatic_decisions();
            let mut candidate = resonance_game::battle::results::Candidate::new(
                opening.results,
                opening.core.actors(),
                party.clone(),
                Default::default(),
                Arc::new(self.session.clone()),
                Arc::new(self.menus.clone()),
                Arc::new(self.catalogue.clone()),
            )?;
            for _ in 0..max_updates {
                opening.lifecycle.step(
                    &mut opening.core,
                    resonance_game::battle::lifecycle::Input {
                        confirm: true,
                        ..Default::default()
                    },
                    &mut candidate,
                    &mut Display,
                )?;
                if opening.core.ledger().combat_ticks >= ready_combat
                    && opening.lifecycle.event().is_none()
                {
                    break;
                }
            }
            ensure!(
                opening.core.ledger().combat_ticks >= ready_combat
                    && opening.lifecycle.event().is_none(),
                "prepared event prelude did not return combat within the observation budget"
            );
            Some(opening.lifecycle)
        } else {
            None
        };
        let mut selected = None;
        let mut departure_companions = Vec::new();
        let mut prepared = inputs
            .prepare_fixture(
                &self.menus,
                options()?,
                |sound| Ok(Some(sound)),
                |participants| {
                    let owner: usize = field(setup, "owner")?;
                    let target: usize = field(setup, "target")?;
                    ensure!(
                        owner < participants.len() && target < participants.len(),
                        "source actor mapping is outside the prepared roster"
                    );
                    match case.binding {
                        Binding::Party { character, .. } => ensure!(
                            party.formation.get(owner) == Some(&character),
                            "source owner is not the selected character"
                        ),
                        Binding::Enemy { enemy, .. } => ensure!(
                            participants[owner]
                                .1
                                .enemy_decision
                                .as_ref()
                                .is_some_and(|decision| decision.enemy_id == enemy),
                            "source owner is not the selected enemy"
                        ),
                    }
                    let action = match case.binding {
                        Binding::Party { arte, .. } => participants[owner]
                            .1
                            .techniques
                            .iter()
                            .find(|technique| technique.catalogue == arte)
                            .map(|technique| technique.action)
                            .context("catalogue arte has no prepared action")?,
                        Binding::Enemy { row, .. } => {
                            participants[owner]
                                .1
                                .enemy_decision
                                .as_ref()
                                .unwrap()
                                .choices
                                .get(row)
                                .context("authored enemy row has no prepared action")?
                                .action
                        }
                    };
                    let side = participants[owner].0.side;
                    let opponent = participants
                        .iter()
                        .position(|(actor, _)| actor.side != side)
                        .context("fixture has no opposing actor")?;
                    let incoming = participants[opponent]
                        .1
                        .control
                        .as_ref()
                        .and_then(|control| control.normals.first().map(|normal| normal.action))
                        .or_else(|| {
                            participants[opponent]
                                .1
                                .enemy_decision
                                .as_ref()
                                .and_then(|decision| decision.choices.first())
                                .map(|choice| choice.action)
                        });
                    if case.context == ContextKind::Departure {
                        let choice = participants[owner].1.enemy_decision.as_ref().unwrap()
                            .choices.iter().find(|choice| choice.action == action)
                            .context("departure has no prepared enemy row")?;
                        for predicate in &choice.requirements.predicates {
                            let resonance_content::battle_action::EnemyPredicate::AllyAbsent(enemy) = predicate else {
                                anyhow::bail!("departure prerequisite observer does not support this predicate");
                            };
                            departure_companions.extend(participants.iter().enumerate()
                                .filter(|(index, (_, setup))| *index != owner && setup.enemy_decision
                                    .as_ref().is_some_and(|definition| u16::from(definition.enemy_id) == *enemy))
                                .map(|(index, _)| index));
                        }
                        departure_companions.sort_unstable();
                        departure_companions.dedup();
                        ensure!(!departure_companions.is_empty(), "departure lacks a prepared companion prerequisite");
                    }
                    let initial = setup["actors"]
                        .as_array()
                        .context("source initial actors are missing")?;
                    ensure!(
                        initial.len() == participants.len(),
                        "source and native actor counts differ"
                    );
                    for (index, (actor, actor_setup)) in participants.iter_mut().enumerate() {
                        let observed = initial
                            .iter()
                            .find(|row| row["actor"].as_u64() == Some(index as u64))
                            .context("source actor mapping is incomplete")?;
                        actor.equipment.max_hp = field(observed, "max_hp")?;
                        actor.hp = field(observed, "hp")?;
                        actor.equipment.max_tp = field(observed, "max_tp")?;
                        actor.tp = field(observed, "tp")?;
                        // Older accepted fixtures required every observed charge to be zero.
                        let charge = observed
                            .get("overlimit_charge")
                            .cloned()
                            .map(serde_json::from_value)
                            .transpose()?
                            .unwrap_or(0);
                        if observed.get("overlimit_remaining").is_some() {
                            ensure!(charge == 0, "active Over Limit retains saved charge");
                            resonance_battle::OverLimit::active(field(
                                observed,
                                "overlimit_remaining",
                            )?)?;
                        }
                        // Active duration belongs to the observed ready boundary,
                        // after this fixture's synthetic camera entry.
                        actor.overlimit = resonance_battle::OverLimit::new(charge)?;
                        actor.position = field(observed, "position")?;
                        actor.heading = field(observed, "heading")?;
                        actor.facing_direction =
                            resonance_battle::direction_from_heading(actor.heading);
                        actor.movement.direction = actor.facing_direction;
                        actor.movement.target_direction = actor.facing_direction;
                        for (name, direction) in [
                            ("facing_direction", &mut actor.facing_direction),
                            ("movement_direction", &mut actor.movement.direction),
                            ("target_direction", &mut actor.movement.target_direction),
                        ] {
                            if observed.get(name).is_some() {
                                *direction = field(observed, name)?;
                            }
                        }
                        if observed.get("reaction_direction").is_some() {
                            actor.reaction.direction = field(observed, "reaction_direction")?;
                        }
                        let stats = &observed["stats"];
                        actor.equipment.stats = resonance_battle::CombatStats {
                            slash: field(stats, "slash")?,
                            thrust: field(stats, "thrust")?,
                            defense: field(stats, "defense")?,
                            intelligence: field(stats, "intelligence")?,
                            accuracy: field(stats, "accuracy")?,
                            evasion: field(stats, "evasion")?,
                            level: field(stats, "level")?,
                        };
                        if actor.side == Side::Party {
                            actor.equipment.contact.technique_balance =
                                field(stats, "technique_balance")?;
                        }
                        let mut layers = actor.conditions.layers();
                        layers.base = field(observed, "conditions")?;
                        actor.conditions.reload_layers(layers);
                        if support.is_some() {
                            field::<support::State>(observed, "support")?
                                .validate_initial(actor)?;
                        }
                        if actor.side == Side::Party {
                            let character = party.formation[index];
                            ensure!(
                                actor.equipment.stats.level
                                    == party.members[usize::from(character - 1)].level,
                                "source actor level differs from its party progression"
                            );
                        }
                        if index != owner {
                            let control = &setup["opponent_control"];
                            actor.guard.auto_disabled = field(control, "automatic_guard_disabled")?;
                            let mode: String = field(control, "mode")?;
                            ensure!(
                                mode == "manual",
                                "source opponent is not a neutral manual recipient"
                            );
                            // Enemy recovery still needs its common controller;
                            // the fixture suppresses automatic decisions separately.
                            if actor.control != resonance_battle::Control::Enemy {
                                actor.control = resonance_battle::Control::Manual;
                            }
                            actor.control_slot = field(control, "port")?;
                            if let Some(recipient) = &recipient_input
                                && index == recipient.actor
                            {
                                actor.control_slot = recipient.port;
                            }
                            actor_setup.recovery_return = None;
                        }
                        actor.availability = if actor.hp <= 0 {
                            resonance_battle::ActorAvailability::Dead
                        } else {
                            resonance_battle::ActorAvailability::Active
                        };
                        actor_setup.decision = None;
                        if menu_prelude.as_ref().is_none_or(|menu| {
                            !menu
                                .control_changes
                                .iter()
                                .any(|change| change.actor == index)
                        }) {
                            actor_setup.companion = None;
                        }
                        if let Some(decision) = &mut actor_setup.enemy_decision {
                            decision.disabled_choices = (0..decision.choices.len())
                                .map(u8::try_from)
                                .collect::<std::result::Result<_, _>>()?;
                        }
                    }
                    selected = Some((
                        ActorId::from_index(owner)?,
                        ActorId::from_index(target)?,
                        action,
                        ActorId::from_index(opponent)?,
                        incoming,
                    ));
                    Ok(())
                },
            )
            .with_context(|| format!("prepare {}", case.id))?;
        if let Some(completed) = completed_events {
            // The observed combat snapshot is installed only after the real route finishes.
            prepared.lifecycle = completed;
        } else {
            ensure!(
                !prepared.lifecycle.has_events(),
                "story-owned encounter requires an explicit event prelude"
            );
        }
        let (owner, target, action, opponent, incoming) =
            selected.context("fixture setup did not run")?;
        let banks: BTreeMap<_, _> = prepared
            .effects
            .iter()
            .map(|effect| (effect.resource, effect.source.source_sha256.clone()))
            .collect();
        let common_effect_bank = banks
            .get(&prepared.poison_effect.resource)
            .context("prepared common effect bank is missing")?
            .clone();
        let overlimit_anchors = prepared
            .feedback
            .actors
            .iter()
            .map(|actor| {
                actor
                    .overlimit
                    .as_ref()
                    .map(|effect| (effect.bone, effect.scale))
            })
            .collect();
        let ambient_effect = prepared.ambient_effect;
        if let Some(observed) = &ambient_clock {
            let (appearance, interval) =
                ambient_effect.context("source ambient effect is not prepared")?;
            ensure!(
                banks.get(&appearance.resource) == Some(&observed.bank)
                    && appearance.member == observed.program
                    && interval == observed.interval,
                "source ambient identity differs from the prepared stage"
            );
        }
        let critical_feedback = prepared
            .feedback
            .contact_art
            .as_ref()
            .map(|art| art.critical);
        let mut feedback = Feedback::new(prepared.feedback, !seed);
        let diagnostics = Diagnostics::new(true);
        let mut effects = Effects::new(
            prepared.effect_banks,
            Some(prepared.poison_effect),
            !seed,
            diagnostics.clone(),
        )?
        .with_stun_particle(prepared.stun_particle)?
        .with_head_anchors(prepared.head_anchors)
        .with_holy_song(prepared.holy_song_effect)?
        .with_overlimit_aura(
            resonance_battle::EffectAppearance {
                member: 46,
                ..prepared.holy_song_effect
            },
            overlimit_anchors,
        )?
        .with_ambient(if ambient_clock.is_some() {
            None
        } else {
            ambient_effect
        })?
        .with_passive(prepared.passive_effects)?;
        let mut candidate = resonance_game::battle::results::Candidate::new(
            prepared.results,
            prepared.core.actors(),
            party,
            Default::default(),
            Arc::new(self.session.clone()),
            Arc::new(self.menus.clone()),
            Arc::new(self.catalogue.clone()),
        )?;
        let mut lifecycle = prepared.lifecycle;
        let mut display = Display;
        let mut core = prepared.core;
        let input_action = input_arte
            .map(|arte| {
                core.prepared_technique(owner, arte)
                    .map(|row| row.action)
                    .context("requested input arte is not prepared")
            })
            .transpose()?
            .unwrap_or(action);
        if let Some(hosts) = setup.get("ready_released_hosts") {
            for host in serde_json::from_value::<Vec<usize>>(hosts.clone())? {
                core.restore_fixture_released_hosted(ActorId::from_index(host)?)?;
            }
        }
        if let Some(groups) = setup["presentation"].get("empty_contact_groups") {
            let groups: Vec<u8> = serde_json::from_value(groups.clone())?;
            let model = core
                .models()
                .and_then(|models| models.definition(owner))
                .context("inert presentation has no prepared body")?;
            ensure!(
                groups
                    .iter()
                    .all(|&group| model.contact_group_anchors(group).is_empty()),
                "inert presentation has prepared melee bindings"
            );
        }
        let secondary_bodies = secondary_body::admit(setup, &core)?;
        let initial_actors = setup["initial_battle"]["actors"]
            .as_array()
            .context("source initial actor navigation is missing")?;
        ensure!(
            initial_actors.len() == core.actors().len(),
            "source navigation roster differs from the prepared actors"
        );
        let mut input_target = target;
        for (index, observed) in initial_actors.iter().enumerate() {
            let target: u64 = field(observed, "target")?;
            let target = initial_actors
                .iter()
                .position(|actor| actor["address"].as_u64() == Some(target))
                .context("source initial target is outside its actor roster")?;
            if index == owner.index() && support_input && input_action != action {
                input_target = ActorId::from_index(target)?;
            }
            let home = observed
                .get("return_position")
                .map(|_| -> Result<_> {
                    Ok((
                        field(observed, "return_position")?,
                        field(observed, "formation_offset")?,
                    ))
                })
                .transpose()?;
            core.restore_fixture_navigation(
                ActorId::from_index(index)?,
                ActorId::from_index(target)?,
                home,
            )?;
        }
        core.set_diagnostics(diagnostics.clone());
        core.suppress_automatic_decisions();
        let mut observed_setup = setup.clone();
        observed_setup["native_seed"] = json!(seed);
        observed_setup["actors"] = setup_actors(core.actors(), &setup["actors"]);
        observed_setup["initial_action_power"] = json!(core.actors().iter().enumerate().map(|(actor, state)|
            json!({"actor":actor,"attack_power":state.attack_power,"casting_strength":state.casting_state.power_percent})).collect::<Vec<_>>());
        observed_setup["effect_banks"] = json!(banks);
        observed_setup["common_effect_bank"] = json!(common_effect_bank);
        observed_setup["native_opponent_controls"] = json!(
            core.actors()
                .iter()
                .enumerate()
                .filter(|(id, _)| *id != owner.index())
                .map(
                    |(id, actor)| json!({"actor":id,"controller":format!("{:?}", actor.control),
                "port":actor.control_slot,"ordinary_action_choices_disabled":true})
                )
                .collect::<Vec<_>>()
        );
        observed_setup["scope"] = json!({"execution":if enemy_chain.is_some() { "observed_enemy_chain" } else if technique_selection.is_some() { "controller_technique" } else { "explicit_action_request" }, "natural_selection":false,"natural_successors":enemy_chain.is_some(),"audio_output":false,"rendered_pixels":false});
        if input_action != action {
            observed_setup["scope"]["requested_arte"] = json!(input_arte);
            observed_setup["scope"]["requested_target"] = json!(input_target.index());
            observed_setup["scope"]["natural_cast_replacement"] = json!(true);
        }
        let mut trace = Trace {
            case: case.id.clone(),
            setup: observed_setup,
            frames: Vec::new(),
            events: Vec::new(),
            assessment: Value::Null,
            incomplete_observations: Vec::new(),
        };
        let observe_chain_history = setup.get("chain_history").is_some();
        if observe_chain_history {
            ensure!(
                enemy_chain.is_some(),
                "contact history requires observed chaining"
            );
            trace.setup["chain_history"] = json!({"frames":[], "events":[]});
        }
        let mut invocations = BTreeMap::<ActionId, Value>::new();
        let mut selected_action = None;
        let mut actions = BTreeMap::<ActionId, usize>::new();
        let mut projectiles = BTreeMap::<ProjectileId, usize>::new();
        let mut particles = BTreeMap::<ParticleId, usize>::new();
        let mut completed = BTreeSet::new();
        let mut expired = BTreeSet::new();
        let mut onset = None;
        let mut pending_cast_start = None;
        let mut random_inputs = random_inputs::admit(source)?;
        let mut effect_inputs = effect_inputs::Inputs::admit(source, &banks, &mut effects)?;
        let mut onset_combat = None;
        let mut raw_events = Vec::new();
        let mut model_ordinals = BTreeMap::new();
        let mut model_counts = BTreeMap::<(ActorId, u32, u16), usize>::new();
        let observe_model_births =
            |shown: &[resonance_battle::ParticleFrame],
             ordinals: &mut BTreeMap<ParticleId, usize>,
             counts: &mut BTreeMap<(ActorId, u32, u16), usize>| {
                for particle in shown {
                    if particle.model.is_some() && !ordinals.contains_key(&particle.id) {
                        let next = counts
                            .entry((particle.owner, particle.resource, particle.member))
                            .or_default();
                        *next += 1;
                        ordinals.insert(particle.id, *next);
                    }
                }
            };
        let mut recovering = false;
        let mut idle_observed = false;
        let mut requested = false;
        let mut running = false;
        let mut request_at = None;
        let mut approach_at = None;
        let mut incoming_sent = false;
        let mut departure_fatals = Vec::new();
        if case.context == ContextKind::Departure {
            ensure!(
                recipient_input
                    .as_ref()
                    .is_some_and(|input| input.mode == "attack")
                    && setup["departure_prerequisite"]["fatal_contacts"].is_array()
                    && departure_companions
                        .iter()
                        .all(|&index| core.actors()[index].hp > 0
                            && core.actors()[index].available()),
                "departure requires a living companion and an ordinary physical input prelude"
            );
        }
        let mut damage = 0_i64;
        let drain_kind = match case.binding {
            Binding::Party {
                character: 5,
                arte: 135,
            } => Some(resonance_battle::RecoveryKind::Hp),
            Binding::Party {
                character: 5,
                arte: 136,
            } => Some(resonance_battle::RecoveryKind::Tp),
            _ => None,
        };
        let mut drain_contact = None;
        let mut contact_stun = Vec::new();
        let observe_impacts = setup["knockdown_impacts"] == true;
        let mut impact_contacts = BTreeMap::new();
        let mut recovery = 0_i64;
        let mut finished = false;
        let mut previous_support: Vec<_> =
            core.actors().iter().map(support::State::observe).collect();
        let mut projectile_actions = BTreeMap::new();
        let owner_side = core.actors()[owner.index()].side;
        ensure!(
            approach_target_changes.is_empty()
                || pending_entry == Some(approach_target_changes[0].after_updates)
                    && approach_target_changes.last().is_some_and(|change| {
                        change.after_updates < request_delay && change.target == target.index()
                    })
                    && approach_target_changes.windows(2).all(|pair| {
                        pair[0].after_updates < pair[1].after_updates
                            && pair[0].target != pair[1].target
                    })
                    && approach_target_changes.iter().all(|change| {
                        core.actors()
                            .get(change.target)
                            .is_some_and(|actor| actor.side != owner_side)
                    }),
            "observed approach target sequence is outside its hostile pending interval"
        );

        let recipient = |index: usize, actor: &resonance_battle::Actor| {
            if drain_kind.is_some() {
                index == owner.index()
            } else if support.is_some() {
                actor.side == owner_side
            } else {
                index == target.index()
            }
        };
        let mut support_attribution = core
            .actors()
            .iter()
            .enumerate()
            .filter(|(index, actor)| recipient(*index, actor))
            .all(|(_, actor)| support::attributable(actor));
        let mut defeated_observed = false;
        let mut self_interrupted = BTreeSet::new();
        for _visit in 0..max_updates {
            let mut input = BattleInput::default();
            if running
                && core.phase() == BattlePhase::Combat
                && request_at.is_some()
                && let Some(chain) = &mut enemy_chain
            {
                chain.submit(&mut core, &mut input)?;
                requested = true;
                if onset.is_none()
                    && request_at == Some(core.ledger().combat_ticks + 1)
                    && let Some(inputs) = random_inputs.take()
                {
                    let origin = core.ledger().elapsed_ticks + 1;
                    if let Some(action) = chain.pending_action() {
                        core.install_allocated_random_inputs(origin, inputs, action)?;
                    } else {
                        core.install_random_inputs(origin, inputs)?;
                    }
                }
            }
            if running
                && core.phase() == BattlePhase::Combat
                && let Some(menu) = &menu_prelude
                && let Some(at) = request_at
            {
                let elapsed: u32 = core.ledger().combat_ticks - (at - request_delay);
                let control_mode = |mode| match mode {
                    0 => resonance_battle::Control::Manual,
                    1 => resonance_battle::Control::SemiAuto,
                    _ => resonance_battle::Control::Auto,
                };
                for change in &menu.control_changes {
                    if change.after_updates == elapsed {
                        let actor = ActorId::from_index(change.actor)?;
                        ensure!(
                            core.actors()[change.actor].control == control_mode(change.previous),
                            "source menu control change has a different initial mode"
                        );
                        core.set_control_mode(actor, control_mode(change.mode))?;
                    }
                }
                if !requested && elapsed == menu.command_after_updates {
                    if let Some(inputs) = random_inputs.take() {
                        core.install_random_inputs(
                            core.ledger()
                                .elapsed_ticks
                                .checked_add(at - core.ledger().combat_ticks)
                                .context("menu random input clock overflow")?,
                            inputs,
                        )?;
                    }
                    ensure!(
                        core.queue_technique_target(owner, input_action, input_target)?,
                        "observed menu command was rejected"
                    );
                    requested = true;
                }
            }
            if running
                && core.phase() == BattlePhase::Combat
                && let Some((selection, held_updates)) = technique_selection
                && let Some(at) = request_at
            {
                let update = core.ledger().combat_ticks.saturating_add(1);
                let press = at - (request_delay - selection);
                if (press..=press + held_updates).contains(&update) {
                    if let Some(inputs) = random_inputs.take() {
                        core.install_random_inputs(
                            core.ledger()
                                .elapsed_ticks
                                .checked_add(at - core.ledger().combat_ticks)
                                .context("random input clock overflow")?,
                            inputs,
                        )?;
                    }
                    let mut control = resonance_battle::ControlInput::neutral(owner);
                    control.technique.pressed = update == press;
                    control.technique.held = update < press + held_updates;
                    control.technique.released = update == press + held_updates;
                    input.controllers.push(control);
                    requested = true;
                }
            }
            if running
                && core.phase() == BattlePhase::Combat
                && let Some(recipient) = &recipient_input
                && let Some(at) = request_at
            {
                let elapsed = core.ledger().combat_ticks.saturating_add(1) - (at - request_delay);
                let mut control =
                    resonance_battle::ControlInput::neutral(ActorId::from_index(recipient.actor)?);
                if matches!(recipient.mode.as_str(), "normal" | "special") {
                    control.guard.held = elapsed < recipient.release_after_updates;
                    control.guard.pressed = elapsed == 1;
                    control.guard.released = elapsed == recipient.release_after_updates;
                }
                if let Some(press) = recipient.press_after_updates {
                    if let Some(stick) = recipient.stick {
                        if (press..recipient.release_after_updates).contains(&elapsed) {
                            control.stick = stick;
                            control.vertical_pressed = i8::from(elapsed == press && stick[1] > 0);
                        }
                    } else {
                        control.attack.held =
                            (press..recipient.release_after_updates).contains(&elapsed);
                        control.attack.pressed = elapsed == press;
                        control.attack.released = elapsed == recipient.release_after_updates;
                    }
                }
                if let Some(down) = recipient.down_after_updates
                    && (down..down + 2).contains(&elapsed)
                {
                    control.stick = [0, -127];
                    control.vertical_pressed = if elapsed == down { -1 } else { 0 };
                }
                for interval in &recipient.stick_intervals {
                    if (interval.press_after_updates..interval.release_after_updates)
                        .contains(&elapsed)
                    {
                        control.stick = interval.stick;
                    }
                }
                input.controllers.push(control);
            }
            if running
                && guard_input
                && onset.is_none()
                && core.phase() == BattlePhase::Combat
                && let Some(at) = request_at
            {
                let update = core.ledger().combat_ticks.saturating_add(1);
                let guard_start = at - guard_lead_updates;
                if (guard_start..=at).contains(&update) {
                    let mut control = resonance_battle::ControlInput::neutral(owner);
                    control.guard.held = true;
                    control.guard.pressed = update == guard_start;
                    if update == at - guard_selection_before_admission {
                        control.stick = [0, -127];
                        control.vertical_pressed = -1;
                        requested = true;
                    }
                    input.controllers.push(control);
                }
            }
            if running
                && onset.is_none()
                && let Some(at) = request_at
            {
                let elapsed = core.ledger().combat_ticks.saturating_add(1) - (at - request_delay);
                for change in approach_target_changes
                    .iter()
                    .filter(|change| change.after_updates == elapsed)
                {
                    core.restore_fixture_target(owner, ActorId::from_index(change.target)?)?;
                }
            }
            if running
                && enemy_chain.is_none()
                && approach_at.is_some_and(|at| core.ledger().combat_ticks.saturating_add(1) >= at)
                && !requested
                && core.phase() == BattlePhase::Combat
                && core.activity(owner) == Activity::Idle
            {
                if let Some(inputs) = random_inputs.take() {
                    // Inputs are relative to bound admission, after the observed
                    // approach interval. The Started cue still verifies that tick.
                    let until_admission = request_at
                        .context("pending entry lacks bound admission")?
                        .checked_sub(core.ledger().combat_ticks)
                        .context("pending entry follows bound admission")?;
                    core.install_random_inputs(
                        core.ledger()
                            .elapsed_ticks
                            .checked_add(until_admission)
                            .context("random input clock overflow")?,
                        inputs,
                    )?;
                }
                let observed_entry_target = approach_target_changes
                    .first()
                    .map(|change| ActorId::from_index(change.target))
                    .transpose()?;
                ensure!(
                    core.request_enemy_approach(
                        ActionRequest {
                            actor: owner,
                            action,
                            target: observed_entry_target.unwrap_or(target),
                        },
                        observed_entry_target
                            .or(setup
                                .get("requested_target")
                                .map(|_| field::<usize>(setup, "requested_target"))
                                .transpose()?
                                .map(ActorId::from_index)
                                .transpose()?)
                            .unwrap_or(target),
                    )?,
                    "observed enemy entry was rejected"
                );
                requested = true;
            }
            if running
                && onset.is_none()
                && core.phase() == BattlePhase::Combat
                && let Some((lead, up)) = aerial_input
                && let Some(at) = request_at
            {
                let update = core.ledger().combat_ticks.saturating_add(1);
                let start = at - lead;
                if (start..start + up).contains(&update) {
                    let mut control = resonance_battle::ControlInput::neutral(owner);
                    control.stick = [0, 127];
                    control.vertical_pressed = i8::from(update == start);
                    input.controllers.push(control);
                }
            }
            if running
                && enemy_chain.is_none()
                && !guard_input
                && technique_selection.is_none()
                && menu_prelude.is_none()
                && pending_entry.is_none()
                && request_at.is_some_and(|at| core.ledger().combat_ticks.saturating_add(1) >= at)
                && onset.is_none()
                && core.phase() == BattlePhase::Combat
                && if aerial_input.is_some() {
                    core.activity(owner) == Activity::Jumping
                        && core.actors()[owner.index()].position[1] > 0.
                } else {
                    core.activity(owner) == Activity::Idle
                }
            {
                if let Some(inputs) = random_inputs.take() {
                    core.install_random_inputs(
                        core.ledger()
                            .elapsed_ticks
                            .checked_add(1)
                            .context("random input clock overflow")?,
                        inputs,
                    )?;
                }
                if case.context == ContextKind::Departure {
                    ensure!(
                        json!(departure_fatals)
                            == setup["departure_prerequisite"]["fatal_contacts"]
                            && departure_companions
                                .iter()
                                .all(|&index| core.actors()[index].hp == 0
                                    && core.actors()[index].availability
                                        == resonance_battle::ActorAvailability::Dead),
                        "departure prerequisite was not reproduced by the ordinary attack prelude"
                    );
                }
                input.actions.push(ActionRequest {
                    actor: owner,
                    action: input_action,
                    target: input_target,
                });
                requested = true;
            }
            if running && case.context == ContextKind::Counter && onset.is_some() && !incoming_sent
            {
                input.actions.push(ActionRequest {
                    actor: opponent,
                    action: incoming.context("counter lacks a prepared incoming action")?,
                    target: owner,
                });
            }
            let previous_tp = core.actors()[owner.index()].tp;
            let previous_random = core.random_state();
            let previous_target_availability = core.actors()[target.index()].availability;
            let mut frame = lifecycle.step(
                &mut core,
                resonance_game::battle::lifecycle::Input {
                    battle: input,
                    ..Default::default()
                },
                &mut candidate,
                &mut display,
            )?;
            feedback.resolve(&mut frame);
            if input_action != action {
                frame.cues.retain(|cue| {
                    if let Cue::Started {
                        actor,
                        action: id,
                        definition: Some(key),
                    } = cue
                        && *actor == owner
                        && *key == input_action
                    {
                        pending_cast_start = Some(*id);
                        false
                    } else {
                        true
                    }
                });
                if let Some(selected) = frame.cues.iter().find_map(|cue| match cue {
                    Cue::Casting {
                        actor,
                        action,
                        phase: resonance_battle::CastPhase::Started,
                    } if *actor == owner => Some(*action),
                    _ => None,
                }) {
                    ensure!(
                        selected == action,
                        "ordinary input did not produce the observed cast replacement: requested {input_action:?}, selected {selected:?}, expected {action:?}, random {previous_random} -> {}, TP {}, uses {:?}",
                        core.random_state(),
                        core.actors()[owner.index()].tp,
                        core.technique_counts(owner),
                    );
                    frame.cues.push(Cue::Started {
                        actor: owner,
                        action: pending_cast_start
                            .take()
                            .context("replacement lacks its admitted input action")?,
                        definition: Some(selected),
                    });
                }
            }
            if let Some(chain) = &mut enemy_chain {
                chain.observe_dispatch(&core, &mut frame)?;
            }
            let clock = frame.clock;
            running = clock == resonance_battle::BattleClock::Running;
            let presentation = core.ledger().elapsed_ticks;
            let combat = core.ledger().combat_ticks;
            let mut replay_effect_entry = false;
            let mut sampled_entry_poses = Vec::new();
            let ready_boundary = request_at.is_none()
                && running
                && core.phase() == BattlePhase::Combat
                && core.activity(owner) == Activity::Idle;
            if ready_boundary {
                if let Some(camera) = setup.get("ready_camera") {
                    ensure!(
                        field::<f32>(camera, "fov_degrees")?
                            == resonance_battle::VERTICAL_FOV_DEGREES,
                        "recorded camera lens differs from the prepared battle"
                    );
                    frame.camera = Some(core.restore_fixture_camera(
                        resonance_battle::CameraPose {
                            eye: field(camera, "eye")?,
                            focus: field(camera, "focus")?,
                            pitch: field(camera, "pitch")?,
                            yaw: field(camera, "yaw")?,
                            radius: field(camera, "radius")?,
                        },
                        field(camera, "pending_radius")?,
                        field(camera, "target_settle_remaining")?,
                    )?);
                }
                // Restore the observed ready state at this idle boundary. Input
                // delays begin here; none can target the already completed visit.
                replay_effect_entry = !carried_clocks.is_empty()
                    || !passive_clocks.is_empty()
                    || !idle_clocks.is_empty()
                    || ambient_clock.is_some()
                    || !entry_playback.is_empty()
                    || !entry_placements.is_empty();
                let poses = setup["ready_models"]
                    .as_array()
                    .context("source ready body poses are missing")?;
                ensure!(
                    poses.len() == frame.actors.len(),
                    "source ready pose count differs"
                );
                for (index, pose) in poses.iter().enumerate() {
                    ensure!(
                        field::<usize>(pose, "actor")? == index,
                        "source ready pose order differs"
                    );
                    // A saved model can already have sampled part of its first
                    // observed visit. Keep that phase separate from scene time.
                    let model_entry_updates: u32 = pose
                        .get("entry_updates")
                        .cloned()
                        .map(serde_json::from_value)
                        .transpose()?
                        .unwrap_or(entry_pose_updates);
                    let secondary = secondary_body::ready(&secondary_bodies, index);
                    ensure!(
                        model_entry_updates <= entry_pose_updates
                            && (model_entry_updates == entry_pose_updates
                                || secondary.is_none()
                                    && (!replay_effect_entry
                                        || entry_pose_updates - model_entry_updates == 1)),
                        "source ready model phase exceeds the observed entry visits"
                    );
                    let actor = ActorId::from_index(index)?;
                    if recipient_input.as_ref().is_some_and(|input| {
                        input.actor == index && matches!(input.mode.as_str(), "attack" | "stick")
                    }) {
                        let source_slot: u64 =
                            field(&setup["source_actor_map"][index], "source_slot")?;
                        let source_actors = setup["initial_battle"]["actors"]
                            .as_array()
                            .context("missing initial actors")?;
                        let observed = source_actors
                            .iter()
                            .find(|actor| actor["slot"] == source_slot)
                            .context("missing recipient")?;
                        let chosen = source_actors
                            .iter()
                            .find(|actor| actor["address"] == observed["target"])
                            .context("recipient target was not observed")?;
                        let target = setup["source_actor_map"]
                            .as_array()
                            .context("missing source actor map")?
                            .iter()
                            .position(|actor| actor["source_slot"] == chosen["slot"])
                            .context("unmapped recipient target")?;
                        core.restore_fixture_target(actor, ActorId::from_index(target)?)?;
                    }
                    if setup["actors"][index].get("motion").is_some() {
                        core.restore_fixture_motion(
                            actor,
                            field(&setup["actors"][index], "motion")?,
                        )?;
                        frame.actors[index].state = core.actors()[index].clone();
                        trace.setup["actors"][index]["motion"] = json!(
                            resonance_battle::FixtureMotion::observe(&core.actors()[index])
                        );
                    }
                    if setup["actors"][index].get("movement_speed").is_some() {
                        core.restore_fixture_forward_speed(
                            actor,
                            field(&setup["actors"][index], "movement_speed")?,
                        )?;
                        trace.setup["actors"][index]["movement_speed"] =
                            json!(core.actors()[index].movement.forward);
                    }
                    let model = core
                        .models()
                        .and_then(|models| models.definition(actor))
                        .context("source ready pose has no prepared body")?;
                    let ready_pose = [
                        resonance_battle::ModelRequest::Play {
                            actor,
                            motion: resonance_battle::MotionBinding {
                                model: model.resource,
                                clip: field(pose, "clip")?,
                            },
                            pose: resonance_battle::Pose {
                                frame: field(pose, "frame")?,
                                rate: field(pose, "rate")?,
                                repeat: field(pose, "repeat")?,
                                blend: 0,
                                ..Default::default()
                            },
                        },
                        resonance_battle::ModelRequest::FixtureClock {
                            actor,
                            loop_start: field(pose, "loop_start")?,
                            secondary,
                            elapsed_updates: if replay_effect_entry {
                                0
                            } else {
                                model_entry_updates
                            },
                        },
                    ];
                    if replay_effect_entry && model_entry_updates < entry_pose_updates {
                        sampled_entry_poses.extend_from_slice(&ready_pose);
                    }
                    frame.model_requests.extend(ready_pose);
                }
                if let Some(deployed) = defense_deployed {
                    ensure!(
                        !deployed,
                        "fixture deployment must begin from its initial phase"
                    );
                    core.restore_fixture_defense_deployment()?;
                }
                if let Some(chain) = &mut enemy_chain {
                    chain.begin(&mut core)?;
                }
                request_at = Some(
                    combat
                        .checked_add(request_delay)
                        .context("source input delay overflow")?,
                );
                approach_at = pending_entry
                    .map(|delay| {
                        combat
                            .checked_add(delay)
                            .context("source entry delay overflow")
                    })
                    .transpose()?;
                trace.setup["native_ready"] = json!({"presentation_tick":presentation,"combat_tick":combat,
                    "bound_admission_after_updates":request_delay,"entry_pose_updates":entry_pose_updates,
                    "pending_entry_after_updates":pending_entry});
                ensure!(
                    ready_condition_visits.is_empty()
                        || (!replay_effect_entry
                            && !initial_effect_entry
                            && entry_pose_updates == 0),
                    "completed ready condition visit overlaps an entry replay"
                );
                for visit in &ready_condition_visits {
                    let observed = setup["actors"]
                        .get(visit.actor)
                        .context("completed ready condition actor is absent")?;
                    ensure!(
                        visit.position.iter().all(|v| v.is_finite()),
                        "invalid ready placement"
                    );
                    let actor = ActorId::from_index(visit.actor)?;
                    // Replay the completed visit at its pre-fixture placement.
                    core.restore_fixture_overlimit(actor, field(observed, "overlimit_remaining")?)?;
                    core.restore_fixture_position(actor, visit.position)?;
                    frame.actors[visit.actor].state = core.actors()[visit.actor].clone();
                    effects.restore_fixture_actor_clock(
                        actor,
                        field::<u16>(observed, "effect_clock")?
                            .checked_sub(1)
                            .context("completed ready condition clock is zero")?,
                    )?;
                }
            }
            core.advance_models(
                &mut frame,
                clock,
                clock == resonance_battle::BattleClock::Running
                    && core.phase() == BattlePhase::Combat
                    && !replay_effect_entry,
            )?;
            if replay_effect_entry {
                for observed in &carried_clocks {
                    let actor = ActorId::from_index(observed.actor)?;
                    let weapon = frame
                        .weapons
                        .iter()
                        .find(|weapon| weapon.owner == actor && weapon.slot == observed.slot)
                        .context("observed carried clock has no prepared weapon")?;
                    effects.restore_fixture_passive_phase(
                        actor,
                        Some(weapon.resource),
                        resonance_content::battle_profile::RecurringEffect {
                            program: u16::from(observed.slot),
                            interval: observed.interval,
                        },
                        observed.counter,
                        entry_pose_updates + request_delay,
                        false,
                    )?;
                }
                // Initial entry uses the ordinary newly initialized emitter clocks.
                for observed in passive_clocks.iter().filter(|_| !initial_effect_entry) {
                    effects.restore_fixture_passive_phase(
                        ActorId::from_index(observed.actor)?,
                        None,
                        resonance_content::battle_profile::RecurringEffect {
                            program: observed.program,
                            interval: observed.interval,
                        },
                        observed.counter,
                        entry_pose_updates + request_delay,
                        false,
                    )?;
                }
                for observed in &idle_clocks {
                    effects.restore_fixture_passive_phase(
                        ActorId::from_index(observed.actor)?,
                        None,
                        resonance_content::battle_profile::RecurringEffect {
                            program: observed.program,
                            interval: observed.interval,
                        },
                        observed.counter,
                        entry_pose_updates + request_delay,
                        ready_combat == 0,
                    )?;
                }
                let update = frame.update;
                let cues = std::mem::take(&mut frame.cues);
                if let Some(observed) = &ambient_clock {
                    let (appearance, interval) = ambient_effect.unwrap();
                    effects.restore_fixture_ambient_phase(
                        appearance,
                        interval,
                        observed.elapsed_updates,
                        &frame,
                    )?;
                }
                let combat_active = frame.combat_active;
                frame.combat_active = false;
                if initial_effect_entry && ready_presentation == 1 {
                    // The saved bodies include this visit; effects start from their own clocks.
                    effects.advance(&frame, false)?;
                    observe_model_births(&effects.frames(), &mut model_ordinals, &mut model_counts);
                }
                // Reconstruct noncombat entry from the observed ready state.
                // No particle births are imported.
                for elapsed in 1..=entry_pose_updates {
                    frame.update = frame
                        .update
                        .checked_add(1)
                        .context("entry update overflow")?;
                    // The first resumed visit may already be represented by a saved body
                    // sample. Replay that sample while effect clocks still advance.
                    frame.model_requests.append(&mut sampled_entry_poses);
                    // Entry movement is independent of the selected combat action.
                    for placement in entry_placements
                        .iter()
                        .filter(|placement| placement.after_updates == elapsed)
                    {
                        let actor = ActorId::from_index(placement.actor)?;
                        core.restore_fixture_position(actor, placement.position)?;
                        frame.actors[placement.actor].state =
                            core.actors()[placement.actor].clone();
                    }
                    // Entry can release a held pose and replace it before combat.
                    // Replaying those transitions preserves sparse animation channels.
                    for pose in entry_playback
                        .iter()
                        .filter(|pose| pose.after_updates == elapsed)
                    {
                        let actor = ActorId::from_index(pose.actor)?;
                        let model = core
                            .models()
                            .and_then(|models| models.definition(actor))
                            .context("source entry transition has no prepared body")?;
                        frame
                            .model_requests
                            .push(resonance_battle::ModelRequest::Play {
                                actor,
                                motion: resonance_battle::MotionBinding {
                                    model: model.resource,
                                    clip: pose.clip,
                                },
                                pose: resonance_battle::Pose {
                                    frame: pose.frame,
                                    end: Some(pose.end),
                                    rate: pose.rate,
                                    repeat: pose.repeat,
                                    blend: pose.blend,
                                    loop_start: pose.loop_start,
                                    ..Default::default()
                                },
                            });
                    }
                    if defense_deployed.is_some() {
                        core.advance_fixture_defense_deployment(&mut frame)?;
                    }
                    core.advance_models(&mut frame, resonance_battle::BattleClock::Running, false)?;
                    effects.advance(&frame, false)?;
                    if initial_effect_entry {
                        observe_model_births(
                            &effects.frames(),
                            &mut model_ordinals,
                            &mut model_counts,
                        );
                    }
                }
                frame.combat_active = combat_active;
                frame.update = update;
                frame.cues = cues;
            }
            feedback.resolve_model_effects(&mut frame)?;
            let effect_onset = onset.or_else(|| {
                frame
                    .cues
                    .iter()
                    .any(|cue| {
                        matches!(cue, Cue::Started { actor, definition: Some(key), .. }
                    if *actor == owner && *key == action)
                    })
                    .then_some(presentation)
            });
            effect_inputs.prepare(effect_onset.map(|start| presentation - start), &mut effects)?;
            let effect_cues =
                if replay_effect_entry || (initial_effect_entry && request_at.is_none()) {
                    vec![]
                } else {
                    effects.advance(&frame, clock.feedback_paused())?
                };
            effects.finish_fixture_birth_inputs()?;
            if ready_boundary {
                for visit in &ready_condition_visits {
                    ensure!(
                        effects.actor_effect_ticks()[visit.actor]
                            == field::<u16>(&setup["actors"][visit.actor], "effect_clock")?,
                        "completed ready condition visit did not advance its saved clock"
                    );
                    let actor = ActorId::from_index(visit.actor)?;
                    core.restore_fixture_position(
                        actor,
                        field(&setup["actors"][visit.actor], "position")?,
                    )?;
                    frame.actors[visit.actor].state = core.actors()[visit.actor].clone();
                }
            }
            frame.cues.extend(effect_cues);
            feedback.advance(
                &mut frame,
                clock.feedback_paused(),
                effects.actor_effect_ticks(),
            );
            if ready_boundary {
                for observed in setup["actors"].as_array().unwrap() {
                    let actor = ActorId::from_index(field(observed, "actor")?)?;
                    if observed.get("overlimit_remaining").is_some() {
                        core.restore_fixture_overlimit(
                            actor,
                            field(observed, "overlimit_remaining")?,
                        )?;
                        trace.setup["actors"][actor.index()]["overlimit_remaining"] =
                            json!(core.actors()[actor.index()].overlimit.remaining());
                    }
                    if observed.get("effect_clock").is_some() {
                        effects
                            .restore_fixture_actor_clock(actor, field(observed, "effect_clock")?)?;
                        trace.setup["actors"][actor.index()]["effect_clock"] =
                            json!(effects.actor_effect_ticks()[actor.index()]);
                    }
                }
            }
            for cue in &frame.cues {
                if case.context == ContextKind::Departure
                    && onset.is_none()
                    && let Cue::Hit {
                        source: ContactSource::Melee { .. },
                        owner: actor,
                        actor: recipient,
                        result,
                        ..
                    } = cue
                    && departure_companions.contains(&recipient.index())
                    && result.hp_change < 0
                    && core.actors()[recipient.index()].hp == 0
                    && core.actors()[recipient.index()].availability
                        == resonance_battle::ActorAvailability::Dead
                {
                    ensure!(
                        recipient_input
                            .as_ref()
                            .is_some_and(|input| input.actor == actor.index()),
                        "departure companion was defeated by another actor"
                    );
                    let ready = request_at.context("departure contact preceded ready input")?
                        - request_delay;
                    departure_fatals.push(json!({"actor":actor.index(),"target":recipient.index(),
                        "after_updates":combat-ready}));
                }
                if matches!(cue, Cue::Started { actor, .. } if *actor == owner)
                    && let Some(chain) = &mut enemy_chain
                {
                    chain.observe(&core)?;
                }
                if observe_impacts && let Cue::Hit { actor, .. } = cue {
                    impact_contacts.remove(actor);
                }
                if matches!(cue, Cue::Started { actor, definition, .. } if *actor == opponent && *definition == incoming)
                {
                    incoming_sent = true;
                }
                if matches!(cue, Cue::Started { actor, .. } | Cue::Released { actor, .. } if *actor != owner)
                {
                    support_attribution = false;
                }
                let event = match cue {
                    Cue::Started {
                        definition: Some(key),
                        action: id,
                        actor,
                    } if *actor == owner && (observe_chain_history || *key == action
                        && enemy_chain.as_ref().is_none_or(|chain| chain.target(&core))) => {
                        let selected = *key == action
                            && enemy_chain.as_ref().is_none_or(|chain| chain.target(&core));
                        if selected {
                            core.verify_random_input_admission(*actor, *id)?;
                            onset.get_or_insert(presentation);
                            onset_combat.get_or_insert(combat);
                            selected_action.get_or_insert(*id);
                        }
                        if observe_chain_history {
                            invocations.insert(*id, enemy_chain.as_ref().unwrap().invocation(&core)?);
                        }
                        let next = actions.len() + 1;
                        actions.entry(*id).or_insert(next);
                        Some(json!({"kind":"started","owner":owner.index(),"action":actions[id]}))
                    }
                    Cue::Released {
                        action: id,
                        parent: Some(parent),
                        actor,
                        ..
                    } if actions.contains_key(parent) => {
                        if observe_chain_history {
                            invocations.insert(*id, invocations[parent].clone());
                        }
                        let next = actions.len() + 1;
                        actions.entry(*id).or_insert(next);
                        Some(
                            json!({"kind":"released","owner":actor.index(),"action":actions[id],"parent":actions[parent]}),
                        )
                    }
                    Cue::ProjectileStarted {
                        projectile,
                        action: id,
                        ..
                    } if actions.contains_key(id) => {
                        let next = projectiles.len() + 1;
                        projectiles.entry(*projectile).or_insert(next);
                        projectile_actions.insert(*projectile, *id);
                        Some(
                            json!({"kind":"projectile_started","owner":owner.index(),"target":frame.projectiles.iter().find(|shot|shot.id==*projectile).map(|shot|shot.target.index()),"action":actions[id],"projectile":projectiles[projectile]}),
                        )
                    }
                    Cue::Hit {
                        source,
                        owner: actor,
                        actor: recipient,
                        result,
                        stunned,
                        ..
                    } if *actor == owner && linked(*source, &actions, &projectiles) => {
                        let root = match source {
                            ContactSource::Melee { action, .. } | ContactSource::Weapon { action, .. } => *action,
                            ContactSource::Projectile(id) => projectile_actions[id],
                        };
                        let selected_contact = !observe_chain_history
                            || invocations.get(&root) == selected_action.and_then(|id| invocations.get(&id));
                        if selected_contact {
                            damage += i64::from((-result.hp_change).max(0));
                        }
                        if drain_kind.is_some() {
                            drain_contact = (result.hp_change < 0).then_some((combat, presentation));
                        }
                        if observe_impacts && result.amount > 0 && result.amount == -result.hp_change {
                            impact_contacts.insert(*recipient, (combat, presentation, invocations.get(&root).cloned()));
                        }
                        if selected_contact && let Some(start) = onset_combat {
                            contact_stun.push(json!({
                                "clock": combat - start, "target": recipient.index(), "stunned": stunned,
                            }));
                        }
                        let mut event = json!({"kind":"hit","owner":actor.index(),"target":recipient.index(),"action":contact_action(*source, &actions, &projectile_actions),"projectile":match source { ContactSource::Projectile(id)=>Some(projectiles[id]), _=>None },"hp_change":result.hp_change,"critical":result.critical});
                        if result.protection == resonance_battle::HitProtection::Avoided
                            && core.actors()[recipient.index()].reaction.protection.mode
                                == resonance_battle::ProtectionMode::Recovery
                        {
                            event["protection"] = json!("recovery");
                        }
                        if recipient_input.is_some() {
                            event["guard"] = json!(match result.guard {
                                resonance_battle::GuardResult::None => None,
                                resonance_battle::GuardResult::Blocked { special: false, .. } => Some("normal"),
                                resonance_battle::GuardResult::Blocked { special: true, .. } => Some("special"),
                                resonance_battle::GuardResult::Broken => Some("broken"),
                            });
                        }
                        if let Some(invocation) = invocations.get(&root) {
                            event["invocation"] = invocation.clone();
                        }
                        Some(event)
                    }
                    Cue::Recovered {
                        actor,
                        kind,
                        applied,
                        ..
                    }
                        if onset.is_some()
                            && support_attribution
                            && *applied > 0
                            && recipient(actor.index(), &core.actors()[actor.index()])
                            && (drain_kind == Some(*kind) && drain_contact.is_some()
                                || *kind == resonance_battle::RecoveryKind::Hp && matches!(
                                    case.context,
                                    ContextKind::Support | ContextKind::Revival
                                )) =>
                    {
                        recovery += i64::from(*applied);
                        let mut event = json!({"kind":"recovered","owner":owner.index(),"target":actor.index(),"action":1,"amount":applied});
                        if drain_kind.is_some() {
                            let (contact_combat, contact_presentation) = drain_contact.take().unwrap();
                            event["kind"] = json!(if *kind == resonance_battle::RecoveryKind::Hp {
                                "hp_drained"
                            } else {
                                "tp_drained"
                            });
                            event["contact_clock"] = json!(contact_combat - onset_combat.unwrap());
                            event["contact_presentation_clock"] = json!(contact_presentation - onset.unwrap());
                        }
                        Some(event)
                    }
                    Cue::PoisonPulse { actor }
                        if onset.is_some() && support_attribution && *actor == target =>
                    {
                        let state = &core.actors()[actor.index()];
                        let severe = state.conditions.effective().contains(
                            resonance_battle::conditions::Condition::PoisonSevere,
                        );
                        let before = trace.frames.last().and_then(|frame| {
                            frame["actors"].as_array()?.iter()
                                .find(|value| value["actor"] == actor.index())?["hp"].as_i64()
                        }).context("poison pulse lacks its preceding HP observation")?;
                        let amount = (i64::from(state.equipment.max_hp) * if severe { 2 } else { 1 } / 100)
                            .max(1).min((before - 1).max(0));
                        (amount > 0).then(|| json!({"kind":"poison_pulse", "owner":owner.index(),
                            "target":actor.index(), "action":1, "amount":amount,
                            "condition":if severe { "poison_severe" } else { "poison_mild" }}))
                    }
                    Cue::IncidentalDamage { actor, amount }
                        if self_defeated
                            && onset.is_some()
                            && support_attribution
                            && *actor == owner =>
                    {
                        Some(
                            json!({"kind":"self_cost","owner":owner.index(),"target":owner.index(),"action":1,"amount":amount}),
                        )
                    }
                    Cue::IncidentalDamage { actor, amount }
                        if observe_impacts && frame.cues.iter().any(
                            |cue| matches!(cue, Cue::KnockdownImpact { actor: target } if target == actor),
                        ) =>
                    {
                        let (contact_combat, contact_presentation, invocation) = impact_contacts
                            .remove(actor)
                            .context("knockdown impact lacks an attributed uncapped contact")?;
                        let mut event = json!({"kind":"knockdown_impact","owner":owner.index(),
                            "target":actor.index(),"action":1,"amount":amount,
                            "contact_combat_tick":contact_combat,
                            "contact_presentation_tick":contact_presentation});
                        if let Some(invocation) = invocation {
                            event["invocation"] = invocation;
                        }
                        Some(event)
                    }
                    Cue::Interrupted { action: id }
                        if actions.get(id) == Some(&1)
                            && ((self_defeated && core.activity(owner) == Activity::Defeated)
                                || (self_stun && core.activity(owner) == Activity::Stunned)) =>
                    {
                        self_interrupted.insert(*id);
                        None
                    }
                    Cue::Completed { action: id } if actions.contains_key(id) => {
                        completed.insert(*id);
                        if case.context == ContextKind::Departure && actions[id] == 1 {
                            let actor = &core.actors()[owner.index()];
                            ensure!(
                                actor.hp > 0
                                    && actor.availability == resonance_battle::ActorAvailability::Absent,
                                "departure completed without a living absent actor"
                            );
                            Some(json!({"kind":"departed","owner":owner.index(),"action":1}))
                        } else {
                            Some(json!({"kind":"completed","action":actions[id]}))
                        }
                    }
                    Cue::ProjectileExpired { projectile }
                        if projectiles.contains_key(projectile) =>
                    {
                        expired.insert(*projectile);
                        Some(
                            json!({"kind":"projectile_expired","projectile":projectiles[projectile]}),
                        )
                    }
                    Cue::Rejected { actor, reason } if *actor == owner => Some(
                        json!({"kind":"rejected","owner":actor.index(),"reason":format!("{reason:?}")}),
                    ),
                    _ => None,
                };
                if let Some(mut event) = event {
                    event["presentation_tick"] = json!(presentation);
                    event["combat_tick"] = json!(combat);
                    if observe_chain_history {
                        let root = match cue {
                            Cue::Started { action, .. } => Some(*action),
                            Cue::ProjectileStarted { action, .. } => Some(*action),
                            Cue::ProjectileExpired { projectile } => {
                                projectile_actions.get(projectile).copied()
                            }
                            _ => None,
                        };
                        if let Some(invocation) = root.and_then(|id| invocations.get(&id)) {
                            event["invocation"] = invocation.clone();
                        }
                    }
                    if matches!(
                        event["kind"].as_str(),
                        Some("released" | "completed" | "rejected")
                    ) {
                        raw_events.push(event);
                    } else {
                        if event.get("action").is_some() {
                            event["action"] = json!(1);
                        }
                        if onset.is_none() && observe_chain_history {
                            trace.setup["chain_history"]["events"]
                                .as_array_mut()
                                .unwrap()
                                .push(event);
                        } else {
                            trace.events.push(event);
                        }
                    }
                }
            }
            if case.context == ContextKind::Revival
                && onset.is_some()
                && support_attribution
                && previous_target_availability == resonance_battle::ActorAvailability::Dead
                && core.actors()[target.index()].availability
                    == resonance_battle::ActorAvailability::Active
            {
                trace
                    .events
                    .push(json!({"kind":"revived", "owner":owner.index(),
                    "target":target.index(), "action":1,
                    "presentation_tick":presentation, "combat_tick":combat}));
            }
            for (index, actor) in core.actors().iter().enumerate() {
                let current = support::State::observe(actor);
                if observe_time_stop
                    && onset.is_some()
                    && support_attribution
                    && actor.side != owner_side
                    && actor.time_stop > 0
                    && trace
                        .frames
                        .last()
                        .is_none_or(|frame| frame["actors"][index]["time_stop"] == 0)
                {
                    trace
                        .events
                        .push(json!({"kind":"time_stop_started", "owner":owner.index(),
                        "target":index, "action":1, "ticks":actor.time_stop,
                        "presentation_tick":presentation, "combat_tick":combat}));
                }
                if onset.is_some() && support_attribution && recipient(index, actor) {
                    for mut event in current.changes(previous_support[index]) {
                        if support.is_none() && event["kind"] != "condition_changed" {
                            continue;
                        }
                        event["owner"] = json!(owner.index());
                        event["target"] = json!(index);
                        event["action"] = json!(1);
                        event["presentation_tick"] = json!(presentation);
                        event["combat_tick"] = json!(combat);
                        trace.events.push(event);
                    }
                }
                previous_support[index] = current;
            }
            if onset.is_some() {
                if self_defeated && !defeated_observed && core.activity(owner) == Activity::Defeated
                {
                    defeated_observed = true;
                    trace.events.push(
                        json!({"kind":"self_defeated","owner":owner.index(),"action":1,
                        "presentation_tick":presentation,"combat_tick":combat}),
                    );
                }
                let tp = core.actors()[owner.index()].tp;
                if tp < previous_tp {
                    trace.events.push(
                        json!({"kind":"payment","owner":owner.index(),"amount":previous_tp-tp,
                        "presentation_tick":presentation,"combat_tick":combat}),
                    );
                }
                // Summon recovery begins while the cinematic suspends ordinary actor timers.
                if !recovering
                    && (core.activity(owner) == Activity::Recovering
                        || frame
                            .summon
                            .as_ref()
                            .is_some_and(|scene| scene.owner == Some(owner)))
                {
                    recovering = true;
                    trace.events.push(
                        json!({"kind":"recovery_started","owner":owner.index(),"action":1,
                        "presentation_tick":presentation,"combat_tick":combat}),
                    );
                }
                if !idle_observed
                    && core.activity(owner) == Activity::Idle
                    && core.actors()[owner.index()].available()
                {
                    idle_observed = true;
                    trace
                        .events
                        .push(json!({"kind":"returned","owner":owner.index(),
                        "presentation_tick":presentation,"combat_tick":combat}));
                }
            }
            for request in effects.started_programs() {
                let event = json!({"kind":"program_started","owner":request.owner.index(),
                    "bank":banks.get(&request.appearance.resource),"program":request.appearance.member,
                    "presentation_tick":presentation,"combat_tick":combat});
                if onset.is_some() {
                    trace.events.push(event);
                } else if observe_chain_history && Some(request.appearance) == critical_feedback {
                    trace.setup["chain_history"]["events"]
                        .as_array_mut()
                        .unwrap()
                        .push(event);
                }
            }
            if observe_chain_history
                && onset.is_none()
                && request_at.is_some()
                && !ready_boundary
                && combat > trace.setup["native_ready"]["combat_tick"].as_u64().unwrap() as u32
            {
                trace.setup["chain_history"]["frames"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({
                        "presentation_tick":presentation, "combat_tick":combat,
                        "actors":core.actors().iter().enumerate().map(|(actor, state)|
                            json!({"actor":actor,"hp":state.hp,"tp":state.tp})).collect::<Vec<_>>()
                    }));
            }
            let shown = effects.frames();
            if onset.is_some()
                && trace.frames.is_empty()
                && !initial_effect_entry
                && !observe_chain_history
            {
                model_ordinals.clear();
                model_counts.clear();
            }
            observe_model_births(&shown, &mut model_ordinals, &mut model_counts);
            let mut births = BTreeSet::new();
            for particle in &shown {
                let born = !particles.contains_key(&particle.id);
                if born {
                    let next = particles.len() + 1;
                    particles.insert(particle.id, next);
                }
                if onset.is_some()
                    && born
                    && births.insert((particle.owner, particle.resource, particle.member))
                {
                    trace.events.push(
                        json!({"kind":"declaration_started","owner":particle.owner.index(),
                        "bank":banks.get(&particle.resource),"member":particle.member,
                        "presentation_tick":presentation,"combat_tick":combat}),
                    );
                }
            }
            if onset.is_some() {
                let mut observed_actors = actors(
                    frame.actors.iter().map(|actor| &actor.state),
                    support.is_some(),
                );
                for (index, actor) in frame.actors.iter().enumerate() {
                    if observe_time_stop {
                        observed_actors[index]["time_stop"] = json!(actor.state.time_stop);
                    }
                    if setup["actors"][index].get("overlimit_remaining").is_some() {
                        observed_actors[index]["overlimit_remaining"] =
                            json!(actor.state.overlimit.remaining());
                    }
                }
                if scripted_defeat {
                    observed_actors[owner.index()]["defeated"] =
                        json!(core.activity(owner) == Activity::Defeated);
                }
                if case.context == ContextKind::Departure {
                    let actor = &core.actors()[owner.index()];
                    observed_actors[owner.index()]["departed"] = json!(
                        actor.hp > 0
                            && actor.availability == resonance_battle::ActorAvailability::Absent
                    );
                }
                if case.context == ContextKind::Capture {
                    for (index, actor) in observed_actors
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .enumerate()
                    {
                        actor["capture"] = json!(
                            core.fixture_capture(ActorId::from_index(index)?)
                                .map(|(captor, hidden)| (captor.index(), hidden))
                        );
                    }
                }
                if let Some(recipient) = &recipient_input {
                    let guard = &frame.actors[recipient.actor].state.guard;
                    observed_actors[recipient.actor]["guard"] = json!(guard.active.then_some(
                        if guard.kind == resonance_battle::GuardKind::Special {
                            "special"
                        } else {
                            "normal"
                        },
                    ));
                }
                let partial_models = if case.context == ContextKind::Presentation {
                    Some(core.models().context("missing presentation models")?.partial_motion_observations(owner)
                        .map(|(first_bone, clip, frame)| json!({"actor":owner.index(),"first_bone":first_bone,"clip":clip,"frame":frame}))
                        .collect::<Vec<_>>())
                } else {
                    None
                };
                trace.frames.push(json!({"presentation_tick":presentation,"combat_tick":combat,
                    "action_age":if observe_chain_history { selected_action.and_then(|id| core.action_age(id)) } else { actions.keys().find_map(|id| core.action_age(*id)) },
                    "actors":observed_actors,
                    "camera":frame.camera.map(|camera| json!({
                        "eye":camera.eye,"target":camera.focus,
                        "pitch":camera.pitch,"yaw":camera.yaw,"radius":camera.radius,
                        "fov_degrees":frame.camera_fov_degrees.unwrap_or(resonance_battle::VERTICAL_FOV_DEGREES),
                    })),
                    "secondary_models":secondary_body::observe(&secondary_bodies, &frame)?,
                    "models":frame.models.iter().map(|model|
                        json!({"actor":model.actor.index(),"clip":model.clip,"frame":model.frame,"world":affine(resonance_content::animation::multiply(model.world, model.bones[0]))})).collect::<Vec<_>>(),
                    "projectiles":frame.projectiles.iter().filter(|shot|projectiles.contains_key(&shot.id)).map(|shot|
                        json!({"id":projectiles[&shot.id],"owner":shot.owner.index(),"target":shot.target.index(),"position":shot.position,"velocity":shot.velocity,"age":shot.age,"contact_active":shot.contact_active,"disarmed":shot.disarmed})).collect::<Vec<_>>(),
                    "effect_models":shown.iter().filter_map(|particle|particle.model.as_ref().map(|model|
                        json!({"owner":particle.owner.index(),"bank":banks.get(&particle.resource),"member":particle.member,
                            "ordinal":effects.fixture_model_ordinal(particle.id).unwrap_or(model_ordinals[&particle.id]),"attachment":attachment(effects.particle_attachment(particle.id),particle.owner_bone,particle.owner,&projectiles),
                            "model":{"clip":model.clip,"frame":model.clip.map(|_|model.frame),"world":affine(model.world)}}))).collect::<Vec<_>>(),
                    "particles":shown.iter().map(|particle|
                        json!({"id":particles[&particle.id],"owner":particle.owner.index(),"bank":banks.get(&particle.resource),"member":particle.member,"initialized":true,"origin":particle.origin,"offset":particle.state.offset,"angles":particle.state.angles,"model_slot":effects.particle_model_slot(particle.id),"age":effects.particle_age(particle.id),"owner_bone":particle.owner_bone,
                            "model":particle.model.as_ref().map(|model|json!({"clip":model.clip,"frame":model.clip.map(|_|model.frame),"world":affine(model.world)}))})).collect::<Vec<_>>() }));
                if let Some(parts) = partial_models {
                    trace.frames.last_mut().unwrap()["partial_models"] = json!(parts);
                }
            }
            let returned = onset.is_some()
                && if case.context == ContextKind::Departure {
                    let actor = &core.actors()[owner.index()];
                    actor.hp > 0
                        && actor.availability == resonance_battle::ActorAvailability::Absent
                } else if self_defeated {
                    defeated_observed && core.activity(owner) == Activity::Defeated
                } else {
                    core.activity(owner) == Activity::Idle
                };
            if trace.frames.len() == source.frames.len() {
                finished = returned
                    && (!observe_time_stop || core.hourglass_remaining() == 0)
                    && !actions.is_empty()
                    && actions.keys().all(|id| {
                        completed.contains(id)
                            || (self_interrupted.contains(id) && core.action_age(*id).is_none())
                    })
                    && projectiles.keys().all(|id| expired.contains(id));
                break;
            }
            if frame.outcome.is_some() {
                break;
            }
        }
        trace.setup["contact_stun"] = json!(contact_stun);
        if case.context == ContextKind::Departure {
            trace.setup["departure_prerequisite_observation"] = json!(departure_fatals);
        }
        ensure!(
            random_inputs.is_none(),
            "random input request was not submitted"
        );
        core.finish_random_inputs()?;
        if let Some(chain) = &enemy_chain {
            chain.finish(&core)?;
        }
        effect_inputs.finish(&effects)?;
        let benefit_observed = trace.events.iter().try_fold(false, |found, event| {
            Ok::<_, anyhow::Error>(found | support::beneficial(event)?)
        })?;
        if let (Some(presentation), Some(combat)) = (onset, onset_combat) {
            let prefix = trace
                .setup
                .get_mut("chain_history")
                .and_then(|history| history["events"].as_array_mut());
            for event in trace.events.iter_mut().chain(prefix.into_iter().flatten()) {
                let object = event.as_object_mut().unwrap();
                let p = object
                    .remove("presentation_tick")
                    .unwrap()
                    .as_u64()
                    .unwrap();
                let c = object.remove("combat_tick").unwrap().as_u64().unwrap();
                object.insert(
                    "presentation_clock".into(),
                    json!(p as i64 - i64::from(presentation)),
                );
                object.insert("clock".into(), json!(c as i64 - i64::from(combat)));
                if let Some(c) = object.remove("contact_combat_tick") {
                    let p = object.remove("contact_presentation_tick").unwrap();
                    object.insert(
                        "contact_clock".into(),
                        json!(c.as_i64().unwrap() - i64::from(combat)),
                    );
                    object.insert(
                        "contact_presentation_clock".into(),
                        json!(p.as_i64().unwrap() - i64::from(presentation)),
                    );
                }
            }
        }
        trace.assessment = json!({
            "outcome":case.context,
            "trace_schema":3,"declaration_clock":"first_initialized_particle",
            "diagnostics":diagnostics.entries(),"preparation_diagnostics":inputs.files.diagnostics().entries(),
            "raw_events":raw_events,"onset":onset,"requested":requested,"damage":damage,"recovery":recovery,
            "action_count":actions.len(),"completed_actions":completed.len(),
            "projectile_count":projectiles.len(),"expired_projectiles":expired.len(),"particle_count":particles.len()});
        if matches!(case.context, ContextKind::Support | ContextKind::Revival)
            && !support_attribution
        {
            trace
                .incomplete_observations
                .push("support_attribution_has_competing_action_or_passive_timer".into());
        }
        if onset.is_none() {
            trace
                .incomplete_observations
                .push("selected_action_never_admitted".into());
        } else if !finished {
            trace
                .incomplete_observations
                .push("execution_or_cleanup_not_completed_within_budget".into());
        }
        let harmful_observed = matches!(case.context, ContextKind::Attack | ContextKind::Aerial)
            && trace.events.iter().try_fold(false, |found, event| {
                Ok::<_, anyhow::Error>(
                    found | compare::owned_harmful_condition(event, &trace.setup, &json!(1))?,
                )
            })?;
        if matches!(
            case.context,
            ContextKind::Attack | ContextKind::Aerial | ContextKind::Capture
        ) && damage == 0
            && !harmful_observed
            && !(setup["self_defeat"]["terminal_only"] == true && defeated_observed)
        {
            trace
                .incomplete_observations
                .push("selected_action_did_not_damage_an_opponent".into());
        }
        if matches!(case.context, ContextKind::Support | ContextKind::Revival) && !benefit_observed
        {
            trace
                .incomplete_observations
                .push("support_benefit_not_observed".into());
        }
        if case.context == ContextKind::Presentation && !compare::presented_during_action(&trace)? {
            trace
                .incomplete_observations
                .push("authored_presentation_not_observed".into());
        }
        if case.context == ContextKind::Movement && !compare::moved_during_action(&trace)? {
            trace
                .incomplete_observations
                .push("selected_action_did_not_move_its_owner".into());
        }
        if let Some(kind) = drain_kind
            && !trace.events.iter().any(|event| {
                event["kind"]
                    == if kind == resonance_battle::RecoveryKind::Hp {
                        "hp_drained"
                    } else {
                        "tp_drained"
                    }
            })
        {
            trace
                .incomplete_observations
                .push("seal_drain_not_observed".into());
        }
        if case.context == ContextKind::Counter {
            trace
                .incomplete_observations
                .push("special_outcome_observer_not_prepared".into());
        }
        Ok(trace)
    }
}
fn attachment(
    follow: Option<EffectFollow>,
    bone: Option<u8>,
    owner: ActorId,
    projectiles: &BTreeMap<ProjectileId, usize>,
) -> Value {
    if let Some(bone) = bone {
        return json!({"kind":"bone","actor":owner.index(),"bone":bone});
    }
    match follow {
        None => Value::Null,
        Some(EffectFollow::Actor(actor)) => json!({"kind":"actor","actor":actor.index()}),
        Some(EffectFollow::Center(actor)) => json!({"kind":"center","actor":actor.index()}),
        Some(EffectFollow::Bone { actor, bone }) => {
            json!({"kind":"bone","actor":actor.index(),"bone":bone})
        }
        Some(EffectFollow::Weapon {
            actor,
            resource,
            bone,
        }) => {
            json!({"kind":"weapon","actor":actor.index(),"resource":resource,"bone":bone})
        }
        Some(EffectFollow::Projectile(id)) => {
            json!({"kind":"projectile","projectile":projectiles.get(&id)})
        }
        Some(EffectFollow::Spell(_)) => json!({"kind":"spell","action":1}),
    }
}
fn linked(
    source: ContactSource,
    actions: &BTreeMap<ActionId, usize>,
    projectiles: &BTreeMap<ProjectileId, usize>,
) -> bool {
    match source {
        ContactSource::Melee { action, .. } | ContactSource::Weapon { action, .. } => {
            actions.contains_key(&action)
        }
        ContactSource::Projectile(id) => projectiles.contains_key(&id),
    }
}
fn contact_action(
    source: ContactSource,
    actions: &BTreeMap<ActionId, usize>,
    projectiles: &BTreeMap<ProjectileId, ActionId>,
) -> usize {
    let id = match source {
        ContactSource::Melee { action, .. } | ContactSource::Weapon { action, .. } => action,
        ContactSource::Projectile(id) => projectiles[&id],
    };
    actions[&id]
}
fn actors<'a>(
    actors: impl IntoIterator<Item = &'a resonance_battle::Actor>,
    support: bool,
) -> Value {
    json!(
        actors
            .into_iter()
            .enumerate()
            .map(|(index, actor)| {
                let mut value = json!({"actor":index,
        "side":if actor.side==Side::Party {0}else{1},"hp":actor.hp,"tp":actor.tp,
        "position":actor.position,"heading":actor.heading});
                if support {
                    value["support"] = json!(support::State::observe(actor));
                }
                value
            })
            .collect::<Vec<_>>()
    )
}
fn setup_actors(source: &[resonance_battle::Actor], observed: &Value) -> Value {
    let mut result = actors(source, false);
    for (index, (value, actor)) in result
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(source)
        .enumerate()
    {
        value["max_hp"] = json!(actor.equipment.max_hp);
        value["max_tp"] = json!(actor.equipment.max_tp);
        value["conditions"] = json!(actor.conditions.base());
        if observed[index].get("support").is_some() {
            value["support"] = json!(support::State::observe(actor));
        }
        if observed[index].get("time_stop").is_some() {
            value["time_stop"] = json!(actor.time_stop);
        }
        if observed[index].get("overlimit_charge").is_some() {
            value["overlimit_charge"] = json!(actor.overlimit.charge());
        }
        if observed[index].get("overlimit_remaining").is_some() {
            value["overlimit_remaining"] = json!(actor.overlimit.remaining());
        }
        if observed[index].get("reaction_direction").is_some() {
            value["reaction_direction"] = json!(actor.reaction.direction);
        }
        for (name, direction) in [
            ("facing_direction", actor.facing_direction),
            ("movement_direction", actor.movement.direction),
            ("target_direction", actor.movement.target_direction),
        ] {
            if observed[index].get(name).is_some() {
                value[name] = json!(direction);
            }
        }
        value["stats"] = json!({"slash":actor.equipment.stats.slash,"thrust":actor.equipment.stats.thrust,
            "defense":actor.equipment.stats.defense,"intelligence":actor.equipment.stats.intelligence,
            "accuracy":actor.equipment.stats.accuracy,"evasion":actor.equipment.stats.evasion,"level":actor.equipment.stats.level,
            "technique_balance":(actor.side==Side::Party).then_some(actor.equipment.contact.technique_balance)});
    }
    result
}
fn affine(matrix: resonance_content::animation::Matrix) -> [f32; 12] {
    std::array::from_fn(|index| matrix[index % 4][index / 4])
}
fn field<T: serde::de::DeserializeOwned>(value: &Value, key: &str) -> Result<T> {
    serde_json::from_value(
        value
            .get(key)
            .with_context(|| format!("source setup lacks {key}"))?
            .clone(),
    )
    .with_context(|| format!("invalid source setup field {key}"))
}
struct Display;
impl resonance_game::battle::results::Presentation for Display {
    fn request(
        &mut self,
        _: resonance_game::battle::lifecycle::Request,
        _: Option<&resonance_game::battle::results::Results>,
        _: &resonance_battle::Battle,
    ) -> Result<()> {
        Ok(())
    }
}
