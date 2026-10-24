use super::{
    Trace, attached_effects, ballistic_particles, effect_pitch, projectile_random, reaction,
    recoil, support,
};
use anyhow::{Context, Result, ensure};
use resonance_battle::conditions::{Condition, ConditionSet, MAGICAL_AILMENTS, PHYSICAL_AILMENTS};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
struct Baseline {
    owner: u64,
    bank: String,
    program: u64,
    member: u64,
    lifetime: u64,
    interval: u64,
    observed_starts: Vec<u64>,
    scenery: Option<SceneryBaseline>,
    scheduler: Option<PassiveScheduler>,
}

#[derive(Deserialize)]
struct SceneryBaseline {
    program_end: u64,
    spawn_offsets: Vec<u64>,
}

#[derive(Deserialize)]
struct PassiveScheduler {
    program_end: u64,
    observed_start_age: u64,
    spawn_offsets: Vec<u64>,
    #[serde(default)]
    pending_ages: Vec<u64>,
}

fn registered_baseline(entry: &Baseline, setup: &Value) -> bool {
    let Some(ready_tick) = setup["initial_battle"]["presentation_tick"].as_u64() else {
        return false;
    };
    let matches_clock = |ready: &Value| {
        ready["interval"].as_u64() == Some(entry.interval)
            && entry.interval > 0
            && ready["counter"].as_u64().is_some_and(|counter| {
                counter <= entry.interval
                    && entry.observed_starts.iter().all(|&time| {
                        (i128::from(time) - i128::from(ready_tick) + i128::from(counter) - 1)
                            .rem_euclid(i128::from(entry.interval))
                            == 0
                    })
            })
    };
    if entry.scenery.is_some() {
        let ready = &setup["ready_ambient_effect"];
        entry.owner == 0
            && ready["bank"] == entry.bank
            && ready["program"] == entry.program
            && matches_clock(ready)
    } else {
        [
            ("ready_passive_effects", "program"),
            ("ready_attachment_effects", "slot"),
        ]
        .iter()
        .any(|(field, program)| {
            setup[*field].as_array().is_some_and(|entries| {
                entries.iter().any(|ready| {
                    ready["actor"] == entry.owner
                        && ready[*program] == entry.program
                        && matches_clock(ready)
                })
            })
        })
    }
}

fn baseline_active_at_start(entry: &Baseline, trace: &Trace) -> bool {
    let (program_end, start_age) = match (&entry.scheduler, &entry.scenery) {
        (Some(scheduler), None) => (scheduler.program_end, scheduler.observed_start_age),
        (None, Some(scenery)) => (scenery.program_end, 1),
        _ => return false,
    };
    let Some(frame) = trace.frames.first() else {
        return false;
    };
    let Some(tick) = frame["presentation_tick"].as_u64() else {
        return false;
    };
    let Some(start) = entry
        .observed_starts
        .iter()
        .copied()
        .filter(|&at| at <= tick)
        .max()
    else {
        return false;
    };
    let Some(age) = (tick - start).checked_add(start_age) else {
        return false;
    };
    registered_baseline(entry, &trace.setup)
        && age <= program_end
        && frame["programs"].as_array().is_some_and(|programs| {
            programs.iter().any(|program| {
                program["owner"].as_u64() == Some(entry.owner)
                    && program["bank"].as_str() == Some(entry.bank.as_str())
                    && program["program"].as_u64() == Some(entry.program)
                    && program["age"].as_u64() == Some(age)
            })
        })
}

fn baseline_particles(frame: &Value, baseline: &[Baseline], final_frame: bool) -> Result<Vec<u64>> {
    let mut counts = vec![0; baseline.len()];
    let mut pending = vec![0_usize; baseline.len()];
    for particle in required(frame, "particles")?
        .as_array()
        .context("unknown visible particles")?
    {
        let entry = baseline.iter().position(|entry| {
            particle["owner"].as_u64() == Some(entry.owner)
                && particle["bank"].as_str() == Some(entry.bank.as_str())
                && particle["member"].as_u64() == Some(entry.member)
        });
        if let Some(index) = entry {
            let entry = &baseline[index];
            let observed_model = particle["model"]["world"]
                .as_array()
                .is_some_and(|world| world.len() == 12 && world.iter().all(Value::is_number))
                && frame["effect_models"].as_array().is_some_and(|models| {
                    models.iter().any(|model| {
                        model["owner"] == particle["owner"]
                            && model["bank"] == particle["bank"]
                            && model["member"] == particle["member"]
                            && model["model"] == particle["model"]
                    })
                });
            ensure!(
                particle["model"].is_null() || observed_model,
                "baseline model lacks a matching complete effect observation"
            );
            if particle["initialized"] == true {
                counts[index] += 1;
            } else {
                let allowed = |age| {
                    if let Some(scenery) = &entry.scenery {
                        usize::from(age == 1 && scenery.spawn_offsets.contains(&0))
                    } else if let Some(scheduler) = &entry.scheduler {
                        if scheduler.pending_ages.is_empty() {
                            usize::from(
                                age == 0
                                    && scheduler.program_end == 0
                                    && scheduler.observed_start_age == 0
                                    && scheduler.spawn_offsets == [0],
                            )
                        } else if age > 0 && age <= scheduler.program_end.saturating_add(1) {
                            scheduler
                                .pending_ages
                                .iter()
                                .filter(|&&at| at == age)
                                .count()
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                };
                let allowance = frame["programs"].as_array().map_or(0, |programs| {
                    programs
                        .iter()
                        .filter(|program| {
                            program["owner"].as_u64() == Some(entry.owner)
                                && program["bank"].as_str() == Some(entry.bank.as_str())
                                && program["program"].as_u64() == Some(entry.program)
                        })
                        .filter_map(|program| program["age"].as_u64())
                        .map(allowed)
                        .sum::<usize>()
                });
                pending[index] += 1;
                ensure!(
                    pending[index] <= allowance
                        && particle["initialized"] == false
                        && particle["age"] == 0,
                    "uninitialized baseline particle is outside its proven birth phase"
                );
            }
        } else {
            ensure!(
                !final_frame,
                "visible particle cleanup not observed outside the proven baseline"
            );
        }
    }
    Ok(counts)
}

/// Compare observed mechanics and effect identities, independently of random draws.
/// Cosmetic random distributions, rendered pixels and audio are outside this scope.
/// Body bone geometry is also diagnostic: the source's last completed display
/// matrix has no publication timestamp tying it to the observed gameplay update.
pub(super) fn equivalent(source: &Trace, native: &Trace) -> Result<()> {
    ensure!(source.case == native.case, "different arte identities");
    for (label, trace) in [("Dolphin", source), ("native", native)] {
        ensure!(
            trace.incomplete_observations.is_empty(),
            "{label} observations incomplete: {}",
            trace.incomplete_observations.join(", ")
        );
        if let Some(failures) = trace.assessment.get("execution_failures") {
            let failures: Vec<String> = serde_json::from_value(failures.clone())?;
            ensure!(
                failures.is_empty(),
                "{label} execution differs: {}",
                failures.join(", ")
            );
        }
        validate_execution(trace).with_context(|| label)?;
    }
    let randomized_projectiles = projectile_random::validate(source, native)?;
    let qualified_recoil = recoil::validate(source, native, &randomized_projectiles);
    let ballistic_particles = ballistic_particles::validate(source, native)?;
    let stationary_area_targets = projectile_random::stationary_area_targets(source, native)?;
    let random_rotations = effect_pitch::validate(source, native)?;
    let reactions = reaction::validate(source, native)?;
    let hidden_fixture_actors = hidden_fixture_actors(source, native);
    let attached_ordinals = [
        attached_effect_ordinals(source)?,
        attached_effect_ordinals(native)?,
    ];
    let follow_models = source.setup.get("projectile_follow_models");
    let baseline: Vec<Baseline> = source
        .setup
        .get("effect_baseline")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    for entry in &baseline {
        ensure!(
            entry.lifetime > 0
                && entry.interval > 0
                && (entry.observed_starts.len() >= 3
                    || (entry.observed_starts.len() >= 2
                        && registered_baseline(entry, &source.setup)))
                && entry
                    .observed_starts
                    .windows(2)
                    .all(|pair| pair[1].checked_sub(pair[0]) == Some(entry.interval))
                && entry.bank.len() == 64
                && entry.bank.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "periodic baseline lacks independent finite recurrence observations"
        );
        ensure!(
            source
                .events
                .iter()
                .any(|event| event["kind"] == "program_started"
                    && event["owner"].as_u64() == Some(entry.owner)
                    && event["bank"].as_str() == Some(entry.bank.as_str())
                    && event["program"].as_u64() == Some(entry.program))
                || baseline_active_at_start(entry, source),
            "periodic baseline lacks a compared program emission"
        );
        if let Some(scenery) = &entry.scenery {
            let ready = required(&source.setup, "ready_ambient_effect")?;
            ensure!(
                entry.owner == 0
                    && ready["bank"] == entry.bank
                    && ready["program"] == entry.program
                    && ready["interval"] == entry.interval
                    && ready["program_end"] == scenery.program_end
                    && scenery.program_end > 0
                    && !scenery.spawn_offsets.is_empty()
                    && scenery
                        .spawn_offsets
                        .iter()
                        .all(|&at| at <= scenery.program_end)
                    && scenery
                        .spawn_offsets
                        .windows(2)
                        .all(|pair| pair[0] <= pair[1]),
                "scenery baseline differs from its finite ready declaration"
            );
        }
    }
    for field in ["formation", "arena", "owner", "target", "actors"] {
        equal(
            required(&source.setup, field)?,
            required(&native.setup, field)?,
            &format!("setup.{field}"),
        )?;
    }
    // Register once at admission. No per-event retiming or skipped mismatched frames.
    let source_start = start(source)?;
    let native_start = start(native)?;
    let owner = required(&source.setup, "owner")?;
    ensure!(
        source.frames.len() <= native.frames.len(),
        "native observation is shorter than the source: {} / {} frames",
        source.frames.len(),
        native.frames.len()
    );
    if native.frames.len() > source.frames.len() {
        let cleanup = &native.assessment["cleanup"];
        ensure!(
            cleanup["source_frames"] == source.frames.len()
                && cleanup["tail_frames"] == native.frames.len() - source.frames.len()
                && cleanup["complete"] == true
                && cleanup["budget_exhausted"] == false
                && [
                    "remaining_projectiles",
                    "remaining_programs",
                    "remaining_particles"
                ]
                .iter()
                .all(|key| cleanup[*key] == 0),
            "native tail lacks observed complete cleanup"
        );
    }
    let (mut source_events, mut native_events) =
        contact_events_with_projectiles(source, native, &randomized_projectiles)?;
    randomized_projectiles.retain_events(0, &mut source_events)?;
    randomized_projectiles.retain_events(1, &mut native_events)?;
    source_events.retain(|event| reactions.retain_event(0, event));
    native_events.retain(|event| reactions.retain_event(1, event));
    for event in source_events.iter_mut().chain(&mut native_events) {
        projectile_random::canonicalize_stored_target(event, &stationary_area_targets);
    }
    for events in [&mut source_events, &mut native_events] {
        events.retain(|event| {
            !(event["kind"] == "projectile_expired"
                && event["projectile"]
                    .as_u64()
                    .is_some_and(|id| randomized_projectiles.boundary_retirements.contains(&id)))
        });
    }
    let traces = [source, native];
    let frame_count = source.frames.len();
    for (index, (source, native)) in source.frames.iter().zip(&native.frames).enumerate() {
        for (field, offset) in [("presentation_tick", 0), ("combat_tick", 1)] {
            let age = |frame: &Value, start: [u64; 2]| -> Result<u64> {
                required(frame, field)?
                    .as_u64()
                    .context("invalid clock")?
                    .checked_sub(start[offset])
                    .context("clock moved backwards")
            };
            ensure!(
                age(source, source_start)? == age(native, native_start)?,
                "frame {index}: {field} differs"
            );
        }
        for field in [
            "models",
            "secondary_models",
            "partial_models",
            "projectiles",
            "effect_models",
        ] {
            let observed =
                |frame: &Value, side: usize| -> Result<Value> {
                    let mut values = if matches!(field, "secondary_models" | "partial_models") {
                        frame
                            .get(field)
                            .cloned()
                            .unwrap_or_else(|| Value::Array(Vec::new()))
                    } else {
                        required(frame, field)?.clone()
                    };
                    if field == "projectiles" {
                        // Complete identity, visibility, flight and first boundary exit
                        // were checked per run. Random paths need not exit together.
                        values
                            .as_array_mut()
                            .context("invalid projectiles")?
                            .retain(|shot| {
                                !shot["id"].as_u64().is_some_and(|id| {
                                    randomized_projectiles.boundary_retirements.contains(&id)
                                })
                            });
                    }
                    if field == "effect_models" {
                        values
                            .as_array_mut()
                            .context("invalid effect models")?
                            .retain(|model| {
                                !model["attachment"]["projectile"]
                                    .as_u64()
                                    .is_some_and(|id| {
                                        randomized_projectiles.model_lifetimes.contains(&id)
                                    })
                            });
                    }
                    for value in values.as_array_mut().context("invalid observation list")? {
                        if field == "effect_models"
                            && let Some(world) = projectile_random::canonical_world(
                                follow_models,
                                frame,
                                value,
                                &randomized_projectiles,
                            )?
                        {
                            value["model"]["world"] = world;
                        }
                        if field == "effect_models" {
                            randomized_projectiles.canonicalize_trail_model(side, frame, value)?;
                            attached_effects::canonicalize(traces[0], index, side == 0, value)?;
                            let ordinal = attached_ordinals[side].get(&effect_key(value)?);
                            effect_pitch::canonicalize(value, &random_rotations[side], index)?;
                            ballistic_particles::canonicalize(
                                value,
                                &ballistic_particles[side],
                                index,
                            )?;
                            if let Some(&ordinal) = ordinal {
                                value["ordinal"] = ordinal.into();
                            }
                        }
                        let other_actor = field == "models" && &value["actor"] != owner;
                        if field == "projectiles" {
                            projectile_random::canonicalize_stored_target(
                                value,
                                &stationary_area_targets,
                            );
                        }
                        let hidden_fixture = field == "models"
                            && value["actor"]
                                .as_u64()
                                .is_some_and(|actor| hidden_fixture_actors.contains(&actor));
                        let conditional_reaction = other_actor
                            && value["actor"]
                                .as_u64()
                                .is_some_and(|actor| reactions.permits(index, actor));
                        let object = value.as_object_mut().context("invalid observation")?;
                        if field == "models" {
                            object.remove("world");
                            // Unrelated idle animation phase is not part of the arte fixture.
                            // Qualified random contact reactions were checked independently above.
                            if other_actor {
                                object.remove("frame");
                            }
                            // A proved hidden fixture corpse has no presented pose.
                            // Keep model presence and all actor movement/state checks.
                            if hidden_fixture {
                                object.remove("clip");
                            }
                            // The contact, actual stun pose and entire effect lifetime
                            // were checked per run. Actor movement remains exact.
                            if conditional_reaction {
                                object.remove("clip");
                            }
                        } else if field == "projectiles" {
                            // These counters describe different update phases. Actual trajectory,
                            // attributed contacts and retirement use the shared gameplay clock.
                            object.remove("age");
                            object.remove("contact_active");
                            if object
                                .get("id")
                                .and_then(Value::as_u64)
                                .is_some_and(|id| randomized_projectiles.ids.contains(&id))
                            {
                                object.remove("position");
                                object.remove("velocity");
                                if object.get("id").and_then(Value::as_u64).is_some_and(|id| {
                                    randomized_projectiles.ricochets.contains(&id)
                                }) {
                                    object.remove("disarmed");
                                }
                            }
                        }
                    }
                    if field == "effect_models" {
                        // Observers enumerate different containers; instance identity is stable.
                        values.as_array_mut().unwrap().sort_by_cached_key(|model| {
                            (
                                model["owner"].as_u64(),
                                model["bank"].as_str().map(str::to_owned),
                                model["member"].as_u64(),
                                model["attachment"].to_string(),
                                model["ordinal"].as_u64(),
                            )
                        });
                    }
                    Ok(values)
                };
            equal(
                &observed(source, 0)?,
                &observed(native, 1)?,
                &format!("frame {index}.{field}"),
            )?;
        }
        // Contact damage is checked against the source's rule/stat envelope below.
        // Comparing accumulated HP here would require identical random streams.
        let actors =
            |frame: &Value, trace: &Trace, start: [u64; 2], side: usize| -> Result<Value> {
                let elapsed = [
                    required(frame, "presentation_tick")?
                        .as_u64()
                        .context("invalid frame clock")?
                        .checked_sub(start[0])
                        .context("frame precedes admission")?,
                    required(frame, "combat_tick")?
                        .as_u64()
                        .context("invalid frame clock")?
                        .checked_sub(start[1])
                        .context("frame precedes admission")?,
                ];
                let mut actors = required(frame, "actors")?.clone();
                for actor in actors.as_array_mut().context("invalid actors")? {
                    qualified_recoil.canonicalize(side, index, actor);
                    actor.as_object_mut().context("invalid actor")?.remove("hp");
                    // Only Y values whose complete contact-to-floor recurrence was
                    // proved independently may differ. X/Z and heading stay exact.
                    if actor["actor"]
                        .as_u64()
                        .is_some_and(|actor| reactions.permits_height(index, actor))
                    {
                        actor["position"][1] = 0.into();
                    }
                    // Retain every other TP change after removing only the exact,
                    // independently validated drain received by this actor so far.
                    let drained = trace
                        .events
                        .iter()
                        .filter(|event| {
                            event["kind"] == "tp_drained"
                                && event["target"] == actor["actor"]
                                && event["clock"]
                                    .as_u64()
                                    .is_some_and(|clock| elapsed[1] >= clock)
                                && event["presentation_clock"]
                                    .as_u64()
                                    .is_some_and(|clock| elapsed[0] >= clock)
                        })
                        .try_fold(0_i64, |total, event| {
                            total
                                .checked_add(event["amount"].as_i64().context("invalid TP drain")?)
                                .context("TP drain overflow")
                        })?;
                    if drained != 0 {
                        let tp = required(actor, "tp")?
                            .as_i64()
                            .context("invalid actor TP")?;
                        actor["tp"] = (tp - drained).into();
                    }
                }
                Ok(actors)
            };
        equal(
            &actors(source, traces[0], source_start, 0)?,
            &actors(native, traces[1], native_start, 1)?,
            &format!("frame {index}.actors"),
        )?;
        let final_frame = index + 1 == frame_count;
        let source_counts = baseline_particles(source, &baseline, final_frame)?;
        let native_counts =
            baseline_particles(native, &baseline, index + 1 == traces[1].frames.len())?;
        ensure!(
            source_counts == native_counts,
            "frame {index}.baseline_particles: {source_counts:?} != {native_counts:?}"
        );
    }
    for (index, frame) in native.frames.iter().enumerate().skip(frame_count) {
        baseline_particles(frame, &baseline, index + 1 == native.frames.len())?;
    }
    equal(
        required(&source.assessment, "outcome")?,
        required(&native.assessment, "outcome")?,
        "outcome",
    )?;
    ensure!(
        source_events.len() == native_events.len(),
        "semantic event count differs: {} / {}",
        source_events.len(),
        native_events.len()
    );
    for (index, (source, native)) in source_events.iter().zip(&native_events).enumerate() {
        equal(source, native, &format!("semantic.events[{index}]"))?;
    }
    Ok(())
}

fn hidden_fixture_actors(source: &Trace, native: &Trace) -> BTreeSet<u64> {
    source.setup["ready_models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let actor = model["actor"].as_u64()?;
            if source.setup["owner"] == actor || model["defeated_hidden"] != true {
                return None;
            }
            let proof = source.setup["ready_model_observations"]
                .as_array()?
                .iter()
                .find(|proof| proof["actor"] == actor)?;
            let initial = source.setup["initial_battle"]["actors"]
                .as_array()?
                .get(actor as usize)?;
            if proof["status"].as_u64()? & 4 == 0
                || proof["source_slot"] != initial["slot"]
                || initial["hp"] != 0
                || initial["side"] != 1
                || initial["action"][0] != 18
                || initial["action"][4] != 16
                || !native.setup["ready_models"]
                    .as_array()?
                    .iter()
                    .any(|model| model["actor"] == actor && model["defeated_hidden"] == true)
            {
                return None;
            }
            // Revival clears the native fixture visibility override. Do not infer
            // later visibility from an old admission observation after a revival.
            [source, native]
                .iter()
                .all(|trace| {
                    trace.frames.iter().all(|frame| {
                        frame["actors"].as_array().is_some_and(|actors| {
                            let mut matches = actors.iter().filter(|value| value["actor"] == actor);
                            matches.next().is_some_and(|value| value["hp"] == 0)
                                && matches.next().is_none()
                        })
                    })
                })
                .then_some(actor)
        })
        .collect()
}

type EffectKey = (u64, String, u64, u64);

fn effect_key(model: &Value) -> Result<EffectKey> {
    Ok((
        model["owner"].as_u64().context("missing effect owner")?,
        model["bank"]
            .as_str()
            .context("missing effect bank")?
            .into(),
        model["member"].as_u64().context("missing effect member")?,
        model["ordinal"]
            .as_u64()
            .context("missing effect ordinal")?,
    ))
}

/// Actor-attached effects are numbered within their recipient, independently of
/// how different actors' emissions were interleaved. Bind once for the entire
/// lifetime; changing attachments or reusing an identity cannot repair a mismatch.
fn attached_effect_ordinals(trace: &Trace) -> Result<BTreeMap<EffectKey, u64>> {
    let mut instances: BTreeMap<EffectKey, (Value, usize)> = BTreeMap::new();
    for (frame, observation) in trace.frames.iter().enumerate() {
        for model in observation["effect_models"]
            .as_array()
            .context("missing effect models")?
        {
            let key = effect_key(model)?;
            if let Some((attachment, previous_frame)) = instances.get_mut(&key) {
                ensure!(
                    *attachment == model["attachment"] && *previous_frame + 1 == frame,
                    "attached effect identity changed, duplicated or reappeared"
                );
                *previous_frame = frame;
            } else if matches!(
                model["attachment"]["kind"].as_str(),
                Some("actor" | "center")
            ) {
                instances.insert(key, (model["attachment"].clone(), frame));
            }
        }
    }
    let mut ordinals = BTreeMap::new();
    Ok(instances
        .into_iter()
        .map(|(key, (attachment, _))| {
            let ordinal = ordinals
                .entry((key.0, key.1.clone(), key.2, attachment.to_string()))
                .or_insert(0);
            *ordinal += 1;
            (key, *ordinal)
        })
        .collect())
}

fn event_clock(event: &Value) -> Result<(u64, u64)> {
    Ok((
        required(event, "clock")?
            .as_u64()
            .context("invalid event clock")?,
        required(event, "presentation_clock")?
            .as_u64()
            .context("invalid event presentation clock")?,
    ))
}

fn canonical_events(events: &[Value]) -> Result<Vec<Value>> {
    let mut ordered = events
        .iter()
        .cloned()
        .map(|event| {
            let identity = event_identity(&event)?;
            let key = (event_clock(&event)?, serde_json::to_string(&identity)?);
            Ok((key, event))
        })
        .collect::<Result<Vec<_>>>()?;
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(ordered.into_iter().map(|(_, event)| event).collect())
}

// Include earlier contacts when their reactions or projectiles survive admission.
// Only vitals are needed here; selected-action geometry keeps its original scope.
fn contact_history(trace: &Trace) -> Result<Option<Trace>> {
    let Some(history) = trace.setup.get("chain_history") else {
        return Ok(None);
    };
    let mut frames = required(history, "frames")?
        .as_array()
        .context("missing chain vitals")?
        .clone();
    ensure!(!frames.is_empty(), "empty chain vital history");
    let start = start(trace)?;
    let first = [
        required(&frames[0], "presentation_tick")?
            .as_u64()
            .context("invalid history clock")?,
        required(&frames[0], "combat_tick")?
            .as_u64()
            .context("invalid history clock")?,
    ];
    ensure!(
        first[0] < start[0] && first[1] < start[1],
        "chain history does not precede admission"
    );
    frames.extend(trace.frames.iter().map(|frame| {
        serde_json::json!({
            "presentation_tick":frame["presentation_tick"], "combat_tick":frame["combat_tick"],
            "actors":frame["actors"],
        })
    }));
    let mut events = required(history, "events")?
        .as_array()
        .context("missing chain contacts")?
        .clone();
    events.extend_from_slice(&trace.events);
    for event in &mut events {
        for (clock, offset) in [
            ("clock", start[1] - first[1]),
            ("presentation_clock", start[0] - first[0]),
            ("contact_clock", start[1] - first[1]),
            ("contact_presentation_clock", start[0] - first[0]),
        ] {
            if let Some(value) = event.get_mut(clock) {
                let value_at = value
                    .as_i64()
                    .context("invalid historical event clock")?
                    .checked_add(i64::try_from(offset)?)
                    .context("historical clock overflow")?;
                ensure!(value_at >= 0, "contact precedes retained history");
                *value = value_at.into();
            }
        }
    }
    Ok(Some(Trace {
        case: trace.case.clone(),
        setup: trace.setup.clone(),
        frames,
        events,
        assessment: Value::Null,
        incomplete_observations: Vec::new(),
    }))
}

// Validate each random damage branch before comparing deterministic events. Only
// the critical feedback justified by those actual outcomes may differ.
fn contact_events(source: &Trace, native: &Trace) -> Result<(Vec<Value>, Vec<Value>)> {
    contact_events_with_projectiles(source, native, &projectile_random::Validated::default())
}

fn contact_events_with_projectiles(
    source: &Trace,
    native: &Trace,
    projectiles: &projectile_random::Validated,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let histories = [contact_history(source)?, contact_history(native)?];
    ensure!(
        histories[0].is_some() == histories[1].is_some(),
        "chain history scope differs"
    );
    if histories[0].is_some() {
        let vitals = |trace: &Trace| -> Result<Value> {
            let onset = start(trace)?;
            Ok(Value::Array(trace.setup["chain_history"]["frames"].as_array().unwrap().iter().map(|frame| {
                let mut actors = frame["actors"].clone();
                for actor in actors.as_array_mut().unwrap() {
                    actor.as_object_mut().unwrap().remove("hp");
                }
                serde_json::json!({
                    "clock":frame["combat_tick"].as_i64().unwrap() - onset[1] as i64,
                    "presentation_clock":frame["presentation_tick"].as_i64().unwrap() - onset[0] as i64,
                    "actors":actors,
                })
            }).collect()))
        };
        equal(&vitals(source)?, &vitals(native)?, "chain prelude vitals")?;
    }
    let source = histories[0].as_ref().unwrap_or(source);
    let native = histories[1].as_ref().unwrap_or(native);
    let native_damage_observed = native.setup["contact_damage_observations"] == true;
    ensure!(
        !native_damage_observed || native.setup["contact_damage_normalized"] == true,
        "native contact inputs lack independent envelope normalization"
    );
    let recipient_input = source.setup.get("recipient_input");
    ensure!(
        recipient_input == native.setup.get("recipient_input"),
        "recipient input differs"
    );
    let mut events = [
        canonical_events(&source.events)?,
        canonical_events(&native.events)?,
    ];
    super::acid_rain::retain_gameplay(source, native, &mut events)?;
    validate_contact_outcomes(source, &events[0])?;
    validate_contact_outcomes(native, &events[1])?;
    validate_poison_pulses(source, &events[0])?;
    validate_poison_pulses(native, &events[1])?;
    ensure!(
        source.setup["catalogue"] == native.setup["catalogue"],
        "condition catalogue differs"
    );
    optional_enemy_conditions(source, &mut events[0])?;
    optional_enemy_conditions(native, &mut events[1])?;
    optional_seal_conditions(source, native, &mut events)?;
    if let Some(programs) = source.setup.get("immediate_end_programs") {
        for program in programs
            .as_array()
            .context("invalid immediate-END programs")?
        {
            let bank = required(program, "bank")?;
            let member = required(program, "program")?;
            ensure!(
                bank.as_str()
                    .is_some_and(|bank| bank.len() == 64
                        && bank.bytes().all(|byte| byte.is_ascii_hexdigit()))
                    && member.is_u64()
                    && required(program, "first_command")?
                        == &serde_json::json!([0, 0, 254, 0, 0, 0]),
                "program lacks an exact immediate-END instruction"
            );
            let allocation = |event: &Value| {
                event["kind"] == "program_started"
                    && &event["bank"] == bank
                    && &event["program"] == member
            };
            ensure!(
                source.events.iter().any(allocation),
                "immediate-END program has no observed start"
            );
            // Source bytes prove this allocation has no operation. Keep all
            // emissions, actor state, timing and complete-retirement checks.
            for events in &mut events {
                events.retain(|event| !allocation(event));
            }
        }
    }
    let qualified = |event: &Value| {
        histories[0].is_none()
            && event["kind"] == "hit"
            && event["projectile"]
                .as_u64()
                .is_some_and(|id| projectiles.contact_timings.contains(&id))
    };
    let hits = events.each_ref().map(|events| {
        let mut hits = events
            .iter()
            .filter(|event| event["kind"] == "hit")
            .cloned()
            .collect::<Vec<_>>();
        hits.sort_by_cached_key(|hit| {
            if qualified(hit) {
                (
                    1,
                    hit["owner"].as_u64(),
                    hit["projectile"].as_u64(),
                    hit["target"].as_u64(),
                    hit["clock"].as_u64(),
                    hit["presentation_clock"].as_u64(),
                )
            } else {
                (
                    0,
                    hit["clock"].as_u64(),
                    hit["presentation_clock"].as_u64(),
                    None,
                    None,
                    None,
                )
            }
        });
        hits
    });
    ensure!(
        hits[0].len() == hits[1].len(),
        "attributed contact count differs"
    );
    let key = |event: &Value| -> Result<(u64, u64, u64)> {
        let owner = required(event, "owner")?
            .as_u64()
            .context("invalid contact owner")?;
        let (combat, presentation) = event_clock(event)?;
        Ok((owner, combat, presentation))
    };
    // Count simultaneous hits together; each critical hit emits its own feedback.
    let mut feedback = std::collections::BTreeMap::new();
    let mut native_feedback_keys = std::collections::BTreeMap::new();
    let actors = required(&source.setup, "actors")?
        .as_array()
        .context("invalid initial actors")?;
    for (source, native) in hits[0].iter().zip(&hits[1]) {
        if let Some(input) = recipient_input {
            let mode = required(input, "mode")?
                .as_str()
                .context("unknown recipient input mode")?;
            let guard = required(source, "guard")?;
            let permitted = guard.is_null()
                || match mode {
                    "normal" => matches!(guard.as_str(), Some("normal" | "broken")),
                    "special" => matches!(guard.as_str(), Some("normal" | "special" | "broken")),
                    _ => false,
                };
            ensure!(
                matches!(mode, "none" | "normal" | "special" | "attack" | "stick")
                    && permitted
                    && required(source, "target")? == required(input, "actor")?,
                "contact did not observe the requested recipient guard"
            );
        } else {
            ensure!(
                source.get("guard").is_none(),
                "guard contact lacks its input fixture"
            );
        }
        let guarded = matches!(source["guard"].as_str(), Some("normal" | "special"));
        let range = bounds(required(source, "damage_range")?)?;
        let branches = required(source, "damage_branches")?;
        let ordinary = bounds(required(branches, "ordinary")?)?;
        let critical = required(branches, "critical")?;
        let critical = (!critical.is_null())
            .then(|| bounds(critical))
            .transpose()?;
        let protected = source.get("protection").is_some();
        if protected {
            ensure!(
                matches!(
                    source["protection"].as_str(),
                    Some("recovery" | "down_window_closed")
                ) && native["protection"] == source["protection"]
                    && actors
                        .iter()
                        .any(|actor| actor["actor"] == source["target"] && actor["side"] == 0)
                    && !guarded
                    && range == (0., 0.)
                    && ordinary == (0., 0.)
                    && critical.is_none()
                    && source["critical"] == false
                    && native["critical"] == false,
                "protected contact lacks matched observed protection and exact zero damage bounds"
            );
        }
        let damage = |event: &Value, range: (f64, f64)| -> Result<f64> {
            let amount = -required(event, "hp_change")?
                .as_f64()
                .context("invalid contact damage")?;
            ensure!(
                (amount > 0.0 || protected && amount == 0.0)
                    && (range.0..=range.1).contains(&amount),
                "contact damage {amount} outside {}..={}",
                range.0,
                range.1
            );
            Ok(amount)
        };
        let source_amount = damage(source, range)?;
        let ordinary_matches = (ordinary.0..=ordinary.1).contains(&source_amount);
        let critical_matches = critical.is_some_and(|(lo, hi)| (lo..=hi).contains(&source_amount));
        ensure!(
            ordinary_matches || critical_matches,
            "source damage matches neither branch"
        );
        let inferred = (ordinary_matches != critical_matches).then_some(critical_matches);
        let observed = required(source, "critical")?;
        ensure!(
            observed.is_null() || observed.is_boolean(),
            "invalid source critical observation"
        );
        ensure!(
            observed.as_bool() == if guarded { Some(false) } else { inferred },
            "source critical feedback disagrees with its contact outcome"
        );
        let native_critical = required(native, "critical")?
            .as_bool()
            .context("unknown native critical outcome")?;
        // New native traces carry independently normalized bounds for their own
        // contact-time stun, protection, guard, strength and HP. Older retained
        // traces keep their conservative source-bound comparison.
        let (native_bounds, native_ordinary, native_critical_bounds) = if native_damage_observed {
            required(native, "damage_inputs")?;
            let branches = required(native, "damage_branches")?;
            let critical = required(branches, "critical")?;
            (
                bounds(required(native, "damage_range")?)?,
                bounds(required(branches, "ordinary")?)?,
                (!critical.is_null())
                    .then(|| bounds(critical))
                    .transpose()?,
            )
        } else {
            (range, ordinary, critical)
        };
        if protected {
            ensure!(
                native_bounds == (0., 0.)
                    && native_ordinary == (0., 0.)
                    && native_critical_bounds.is_none(),
                "native protected contact lacks exact zero damage bounds"
            );
        }
        let native_range = if native_critical {
            native_critical_bounds.context("native critical hit has no legal critical branch")?
        } else {
            native_ordinary
        };
        let native_amount = damage(native, native_bounds)?;
        if guarded {
            ensure!(
                !native_critical
                    && ((native_ordinary.0..=native_ordinary.1).contains(&native_amount)
                        || native_critical_bounds
                            .is_some_and(|(lo, hi)| (lo..=hi).contains(&native_amount))),
                "guarded damage {native_amount} matches neither independent damage branch"
            );
        } else {
            ensure!(
                (native_range.0..=native_range.1).contains(&native_amount),
                "native damage {native_amount} disagrees with its critical outcome"
            );
        }
        let mut identities = [event_identity(source)?, event_identity(native)?];
        if qualified(source) && qualified(native) {
            for identity in &mut identities {
                let object = identity.as_object_mut().unwrap();
                object.remove("clock");
                object.remove("presentation_clock");
            }
        }
        equal(&identities[0], &identities[1], "attributed contact")?;
        let source_key = key(source)?;
        if let Some(previous) = native_feedback_keys.insert(key(native)?, source_key) {
            ensure!(
                previous == source_key,
                "ambiguous randomized contact feedback pairing"
            );
        }
        let group = feedback.entry(key(source)?).or_insert(([0usize; 2], true));
        group.0[0] += usize::from(!guarded && inferred == Some(true));
        group.0[1] += usize::from(native_critical);
        group.1 &= guarded || inferred.is_some();
    }
    if !feedback.is_empty() {
        let common = required(&source.setup, "common_effect_bank")?;
        ensure!(
            common.as_str().is_some_and(|bank| !bank.is_empty()),
            "unknown common effect bank"
        );
        equal(
            common,
            required(&native.setup, "common_effect_bank")?,
            "common effect bank",
        )?;
        for (side, events) in events.iter_mut().enumerate() {
            let feedback_key = |event: &Value| -> Result<(u64, u64, u64)> {
                let actual = key(event)?;
                if side == 0 {
                    Ok(actual)
                } else {
                    native_feedback_keys
                        .get(&actual)
                        .copied()
                        .context("native critical feedback has no contact at its own clock")
                }
            };
            let is_critical = |event: &Value| {
                event["kind"] == "program_started"
                    && &event["bank"] == common
                    && event["program"] == 16
            };
            for event in events.iter().filter(|event| is_critical(event)) {
                ensure!(
                    feedback.contains_key(&feedback_key(event)?),
                    "critical feedback has no matching attacker/contact clock"
                );
            }
            for (contact, (counts, known)) in &feedback {
                if side == 1 || *known {
                    let count = events
                        .iter()
                        .filter(|event| {
                            is_critical(event) && feedback_key(event).ok().as_ref() == Some(contact)
                        })
                        .count();
                    ensure!(
                        count == counts[side],
                        "critical feedback count disagrees with observed contact outcome at {contact:?}"
                    );
                }
            }
            events.retain(|event| {
                !(is_critical(event)
                    && feedback
                        .get(&feedback_key(event).unwrap())
                        .is_some_and(|(_, known)| *known))
            });
        }
    }
    let [source, native] = events.map(|events| {
        events
            .iter()
            // Matched recipient multiplicities and every other contact identity
            // were retained above; each own clock was validated before pairing.
            .filter(|event| !qualified(event))
            .map(event_identity)
            .collect::<Result<Vec<_>>>()
    });
    Ok((source?, native?))
}

fn event_identity(event: &Value) -> Result<Value> {
    let mut identity = event.clone();
    let object = identity.as_object_mut().context("invalid semantic event")?;
    if event["kind"] == "hit" {
        for field in [
            "hp_change",
            "damage_range",
            "critical",
            "damage_branches",
            "damage_inputs",
        ] {
            object.remove(field);
        }
    } else if matches!(
        event["kind"].as_str(),
        Some("knockdown_impact" | "hp_drained" | "tp_drained" | "poison_pulse")
    ) {
        object.remove("amount");
    }
    Ok(identity)
}

fn validate_contact_outcomes(trace: &Trace, events: &[Value]) -> Result<()> {
    let time = |event: &Value, combat: &str, presentation: &str| -> Result<(u64, u64)> {
        Ok((
            required(event, combat)?
                .as_u64()
                .context("invalid outcome combat clock")?,
            required(event, presentation)?
                .as_u64()
                .context("invalid outcome presentation clock")?,
        ))
    };
    for (index, outcome) in events.iter().enumerate().filter(|(_, event)| {
        matches!(
            event["kind"].as_str(),
            Some("knockdown_impact" | "hp_drained" | "tp_drained")
        )
    }) {
        let tp_drain = outcome["kind"] == "tp_drained";
        let drain = outcome["kind"] == "hp_drained" || tp_drain;
        ensure!(
            if drain {
                trace.case
                    == if tp_drain {
                        "party:5:136"
                    } else {
                        "party:5:135"
                    }
                    && outcome["target"] == outcome["owner"]
                    && outcome["owner"] == trace.setup["owner"]
            } else {
                trace.setup["knockdown_impacts"] == true
            },
            "contact outcome lacks its supported observation"
        );
        let contact = events[..index]
            .iter()
            .rev()
            .find(|event| {
                if drain {
                    event["owner"] == outcome["owner"]
                        && matches!(
                            event["kind"].as_str(),
                            Some("hit" | "hp_drained" | "tp_drained")
                        )
                } else {
                    event["target"] == outcome["target"]
                        && matches!(event["kind"].as_str(), Some("hit" | "knockdown_impact"))
                }
            })
            .context("outcome has no preceding contact")?;
        let cause = time(outcome, "contact_clock", "contact_presentation_clock")?;
        ensure!(
            contact["kind"] == "hit"
                && contact["owner"] == outcome["owner"]
                && contact["action"] == outcome["action"]
                && contact.get("invocation") == outcome.get("invocation")
                && time(contact, "clock", "presentation_clock")? == cause
                && cause < time(outcome, "clock", "presentation_clock")?,
            "outcome does not follow its latest attributed contact"
        );
        ensure!(
            events
                .iter()
                .filter(|event| event["kind"] == "hit"
                    && (!drain || event["owner"] == contact["owner"])
                    && event["target"] == contact["target"]
                    && time(event, "clock", "presentation_clock").ok() == Some(cause))
                .count()
                == 1,
            "outcome contact is ambiguous"
        );
        let damage = required(contact, "hp_change")?
            .as_i64()
            .and_then(i64::checked_neg)
            .context("invalid outcome contact damage")?;
        let amount = required(outcome, "amount")?
            .as_i64()
            .context("invalid outcome damage")?;
        let onset = time(&trace.frames[0], "combat_tick", "presentation_tick")?;
        let clock = time(outcome, "clock", "presentation_clock")?;
        let frame = trace
            .frames
            .iter()
            .position(|frame| {
                time(frame, "combat_tick", "presentation_tick").ok()
                    == Some((onset.0 + clock.0, onset.1 + clock.1))
            })
            .filter(|index| *index > 0)
            .context("outcome HP transition was not observed")?;
        let vital = if tp_drain { "tp" } else { "hp" };
        let value = |frame: &Value| -> Result<i64> {
            frame["actors"]
                .as_array()
                .context("missing outcome actors")?
                .iter()
                .find(|actor| actor["actor"] == outcome["target"])
                .and_then(|actor| actor[vital].as_i64())
                .context("missing outcome recipient vital")
        };
        let before = value(&trace.frames[frame - 1])?;
        let expected = if drain {
            let maximum = trace.setup["actors"]
                .as_array()
                .context("missing drain actors")?
                .iter()
                .find(|actor| actor["actor"] == outcome["target"])
                .and_then(|actor| actor[if tp_drain { "max_tp" } else { "max_hp" }].as_i64())
                .context("missing drain capacity")?;
            (damage * if tp_drain { 3 } else { 10 } / 100)
                .max(1)
                .min(maximum - before)
        } else {
            damage / 4
        };
        ensure!(
            damage > 0 && amount > 0 && amount == expected,
            "contact outcome amount differs from its own damage"
        );
        let after = if drain {
            before + amount
        } else {
            // Landing precedes collision processing; another contact can consume
            // HP during the same observed update.
            events[index + 1..]
                .iter()
                .filter(|event| {
                    event["kind"] == "hit"
                        && event["target"] == outcome["target"]
                        && time(event, "clock", "presentation_clock").ok() == Some(clock)
                })
                .try_fold((before - amount).max(1), |hp, event| {
                    let change = required(event, "hp_change")?
                        .as_i64()
                        .filter(|change| *change < 0)
                        .context("invalid concurrent contact damage")?;
                    hp.checked_add(change)
                        .context("concurrent contact damage overflow")
                })?
        };
        ensure!(
            before >= i64::from(!tp_drain) && value(&trace.frames[frame])? == after,
            "contact outcome differs from its observed vital transition"
        );
    }
    Ok(())
}

fn validate_poison_pulses(trace: &Trace, events: &[Value]) -> Result<()> {
    for pulse in events
        .iter()
        .filter(|event| event["kind"] == "poison_pulse")
    {
        let at = event_clock(pulse)?;
        let condition = required(pulse, "condition")?
            .as_str()
            .context("missing poison kind")?;
        let percent = match condition {
            "poison_mild" => 1,
            "poison_severe" => 2,
            _ => anyhow::bail!("invalid poison kind"),
        };
        let applied = events
            .iter()
            .filter(|event| {
                event["kind"] == "condition_changed"
                    && event["target"] == pulse["target"]
                    && event_clock(event).is_ok_and(|value| value < at)
                    && event["before"]
                        .as_array()
                        .zip(event["after"].as_array())
                        .is_none_or(|(before, after)| {
                            before.iter().any(|value| value == condition)
                                != after.iter().any(|value| value == condition)
                        })
            })
            .max_by_key(|event| event_clock(event).ok())
            .context("poison pulse lacks its application")?;
        ensure!(
            applied["owner"] == pulse["owner"]
                && applied["action"] == pulse["action"]
                && applied["after"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|value| value == condition))
                && applied["before"]
                    .as_array()
                    .is_some_and(|values| values.iter().all(|value| value != condition)),
            "poison pulse is not bound to its condition application"
        );
        ensure!(
            events
                .iter()
                .filter(|event| event["kind"] == "program_started"
                    && event["owner"] == pulse["target"]
                    && event["bank"] == trace.setup["common_effect_bank"]
                    && event["program"] == 44
                    && event_clock(event).ok() == Some(at))
                .count()
                == 1,
            "poison pulse lacks its independent feedback emission"
        );
        let onset = &trace.frames[0];
        let frame = trace
            .frames
            .iter()
            .position(|frame| {
                frame["combat_tick"].as_u64()
                    == onset["combat_tick"]
                        .as_u64()
                        .and_then(|start| start.checked_add(at.0))
                    && frame["presentation_tick"].as_u64()
                        == onset["presentation_tick"]
                            .as_u64()
                            .and_then(|start| start.checked_add(at.1))
            })
            .filter(|&index| index > 0)
            .context("poison pulse lacks adjacent HP observations")?;
        let hp = |frame: &Value| -> Result<i64> {
            frame["actors"]
                .as_array()
                .context("missing poison actors")?
                .iter()
                .find(|actor| actor["actor"] == pulse["target"])
                .and_then(|actor| actor["hp"].as_i64())
                .context("missing poison recipient HP")
        };
        let maximum = trace.setup["actors"]
            .as_array()
            .context("missing poison setup actors")?
            .iter()
            .find(|actor| actor["actor"] == pulse["target"])
            .and_then(|actor| actor["max_hp"].as_i64())
            .context("missing poison maximum HP")?;
        let before = hp(&trace.frames[frame - 1])?;
        let amount = required(pulse, "amount")?
            .as_i64()
            .context("invalid poison amount")?;
        ensure!(
            maximum > 0
                && maximum <= i64::from(i32::MAX)
                && before > 1
                && amount > 0
                && amount == (maximum * percent / 100).max(1).min(before - 1),
            "poison amount differs from its percentage and nonlethal HP cap"
        );
        let after = events
            .iter()
            .filter(|event| {
                event["kind"] == "hit"
                    && event["target"] == pulse["target"]
                    && event_clock(event).ok() == Some(at)
            })
            .try_fold(before - amount, |hp, hit| -> Result<i64> {
                hp.checked_add(
                    required(hit, "hp_change")?
                        .as_i64()
                        .filter(|value| *value < 0)
                        .context("invalid concurrent poison contact")?,
                )
                .context("poison HP overflow")
            })?;
        ensure!(
            hp(&trace.frames[frame])? == after,
            "poison pulse differs from its observed HP transition"
        );
    }
    Ok(())
}

// The source observer registers only optional seal callbacks in chance_effects.
// These recipes are identified by the original techniques bank, not case IDs.
const SEAL_BANK: &str = "2a0b8378be586933a7a3e4d59eda0e1a073c59b9cbc6f2169f300edebe09c059";

fn optional_seal_conditions(
    source: &Trace,
    native: &Trace,
    events: &mut [Vec<Value>; 2],
) -> Result<()> {
    let Some(bindings) = source.setup["chance_effects"]
        .as_array()
        .filter(|bindings| !bindings.is_empty())
    else {
        return Ok(());
    };
    if source.setup.get("random_inputs").is_some()
        || native.setup.get("random_inputs").is_some()
        || !source.case.starts_with("party:")
    {
        return Ok(());
    }
    ensure!(
        source.setup["chance_effects"] == native.setup["chance_effects"],
        "seal callback witnesses differ"
    );
    // Strike-route seals are guaranteed. A registration cannot turn them into
    // optional outcomes, even if a malformed fixture supplies one.
    let catalogue = super::content_evidence::action_catalogue(source)?;
    let index: usize = source.case.rsplit(':').next().unwrap().parse()?;
    let definition = &catalogue["definitions"][index];
    if definition["learning"]["route"] == "strike" || definition["capabilities"]["spell"] != false {
        return Ok(());
    }
    ensure!(
        bindings.len() == 1,
        "overlapping seal recipes need separate attribution"
    );
    let binding = &bindings[0];
    ensure!(
        binding["owner"] == source.setup["owner"] && binding["bank"] == SEAL_BANK,
        "seal callback has the wrong owner or authored bank"
    );
    let (condition, recipe): (Condition, &[(u64, u64, u64)]) = match binding["program"].as_u64() {
        Some(134) => (
            Condition::DefenseDown,
            &[(0, 141, 25), (4, 141, 25), (8, 141, 25)],
        ),
        Some(135) => (
            Condition::AccuracyDown,
            &[(0, 141, 25), (4, 141, 25), (8, 141, 25)],
        ),
        Some(59) => (
            Condition::EvasionDown,
            &[
                (0, 103, 41),
                (0, 104, 51),
                (3, 104, 51),
                (5, 103, 41),
                (6, 104, 51),
                (8, 104, 51),
            ],
        ),
        _ => return Ok(()),
    };
    let owner = source.setup["owner"]
        .as_u64()
        .context("missing seal owner")? as usize;
    let character = &source.setup["party"][owner];
    let luck = source.setup["party_members"]
        .as_array()
        .and_then(|members| {
            members
                .iter()
                .find(|member| member["character"] == *character)
        })
        .and_then(|member| member["luck"].as_u64())
        .context("missing seal chance input")?;
    if 25 + luck / 10 >= 100 {
        return Ok(());
    }
    for (side, trace) in [source, native].into_iter().enumerate() {
        validate_seal_callbacks(
            trace,
            binding,
            condition,
            recipe,
            side == 0,
            &mut events[side],
        )?;
    }
    Ok(())
}

fn validate_seal_callbacks(
    trace: &Trace,
    binding: &Value,
    condition: Condition,
    recipe: &[(u64, u64, u64)],
    source: bool,
    events: &mut Vec<Value>,
) -> Result<()> {
    let attempts = binding["attempts"]
        .as_array()
        .filter(|attempts| !attempts.is_empty())
        .context("missing seal callback attempts")?;
    let is_program = |event: &Value| {
        event["kind"] == "program_started"
            && ["owner", "bank", "program"]
                .iter()
                .all(|key| event[key] == binding[key])
    };
    let actor = |id: &Value| {
        trace.setup["actors"]
            .as_array()
            .and_then(|actors| actors.iter().find(|actor| actor["actor"] == *id))
    };
    let owner = actor(&binding["owner"]).context("missing seal actor")?;
    let mut removed = Vec::new();
    let mut births = BTreeMap::<(u64, u64, u64), (usize, u64)>::new();
    let mut seen = BTreeSet::new();
    for attempt in attempts {
        let clock = event_clock(attempt)?;
        ensure!(seen.insert(clock), "duplicate seal callback attempt");
        let target = actor(&attempt["target"]).context("missing seal victim")?;
        ensure!(
            target["side"] != owner["side"],
            "seal callback targets an ally"
        );
        let hits: Vec<_> = events
            .iter()
            .filter(|event| {
                event["kind"] == "hit"
                    && event["owner"] == binding["owner"]
                    && event["target"] == attempt["target"]
                    && event["clock"] == attempt["contact_clock"]
                    && event["hp_change"].as_i64().is_some_and(|amount| amount < 0)
            })
            .collect();
        ensure!(
            hits.len() == 1,
            "seal callback lacks its unique damaging contact"
        );
        let hit = hits[0];
        let contact = event_clock(hit)?;
        let melee = hit["projectile"].is_null();
        ensure!(
            contact.0 < clock.0
                && contact.1 < clock.1
                && (melee || clock == (contact.0 + 1, contact.1 + 1)),
            "seal callback has the wrong contact phase"
        );
        ensure!(
            events.iter().any(|event| event["kind"] == "started"
                && event["owner"] == binding["owner"]
                && event["action"] == hit["action"]
                && event["clock"] == 0),
            "seal callback lacks its action admission"
        );
        let programs: Vec<_> = events
            .iter()
            .filter(|event| is_program(event) && event_clock(event).ok() == Some(clock))
            .collect();
        ensure!(programs.len() <= 1, "duplicate seal success program");
        let emitted = !programs.is_empty();
        ensure!(
            !source || attempt["emitted"].as_bool() == Some(emitted),
            "seal source success differs from its callback witness"
        );
        let before = events
            .iter()
            .filter(|event| {
                event["kind"] == "condition_changed"
                    && event["target"] == attempt["target"]
                    && event_clock(event).is_ok_and(|at| at < clock)
            })
            .max_by_key(|event| event_clock(event).ok())
            .map_or(&target["conditions"], |event| &event["after"]);
        let before: ConditionSet = serde_json::from_value(before.clone())?;
        let after = if emitted {
            before.union(ConditionSet::of(&[condition]))
        } else {
            before
        };
        let changes: Vec<_> = events
            .iter()
            .filter(|event| {
                event["kind"] == "condition_changed"
                    && event["target"] == attempt["target"]
                    && event_clock(event).ok() == Some(clock)
            })
            .collect();
        ensure!(
            changes.len() == usize::from(before != after),
            "seal condition and success program disagree"
        );
        if let Some(change) = changes.first() {
            ensure!(
                change["owner"] == binding["owner"]
                    && change["action"] == hit["action"]
                    && serde_json::from_value::<ConditionSet>(change["before"].clone())? == before
                    && serde_json::from_value::<ConditionSet>(change["after"].clone())? == after,
                "seal changed the wrong condition or attribution"
            );
            removed.push((*change).clone());
        }
        if emitted {
            removed.push(programs[0].clone());
            // InitializedEffect executes the melee callback's first scheduler
            // visit immediately; projectile callbacks queue an ordinary Effect.
            for &(at, member, lifetime) in recipe {
                let offset = at.saturating_sub(u64::from(melee));
                let count = births
                    .entry((clock.0 + offset, clock.1 + offset, member))
                    .or_insert((0, lifetime));
                count.0 += 1;
            }
        }
    }
    ensure!(
        events.iter().filter(|event| is_program(event)).count()
            == removed.iter().filter(|event| is_program(event)).count(),
        "seal program has no witnessed callback"
    );
    validate_seal_feedback(trace, binding, recipe, &births)?;
    for (&(clock, presentation, member), _) in &births {
        let declarations: Vec<_> = events
            .iter()
            .filter(|event| {
                event["kind"] == "declaration_started"
                    && event["owner"] == binding["owner"]
                    && event["bank"] == binding["bank"]
                    && event["member"] == member
                    && event_clock(event).ok() == Some((clock, presentation))
            })
            .collect();
        ensure!(
            declarations.len() == 1,
            "seal feedback declaration is missing or duplicated"
        );
        removed.push(declarations[0].clone());
    }
    ensure!(
        events
            .iter()
            .filter(|event| event["kind"] == "declaration_started"
                && event["owner"] == binding["owner"]
                && event["bank"] == binding["bank"]
                && recipe
                    .iter()
                    .any(|&(_, member, _)| event["member"] == member))
            .count()
            == births.len(),
        "seal feedback declaration has no success-program birth"
    );
    events.retain(|event| !removed.contains(event));
    Ok(())
}

fn validate_seal_feedback(
    trace: &Trace,
    binding: &Value,
    recipe: &[(u64, u64, u64)],
    expected: &BTreeMap<(u64, u64, u64), (usize, u64)>,
) -> Result<()> {
    let onset = start(trace)?;
    let mut lives = BTreeMap::<u64, Vec<((u64, u64), &Value)>>::new();
    for frame in &trace.frames {
        let clock = (
            frame["combat_tick"]
                .as_u64()
                .context("missing seal frame clock")?
                - onset[1],
            frame["presentation_tick"]
                .as_u64()
                .context("missing seal frame display clock")?
                - onset[0],
        );
        for particle in frame["particles"]
            .as_array()
            .context("missing seal particles")?
        {
            if particle["owner"] == binding["owner"]
                && particle["bank"] == binding["bank"]
                && recipe
                    .iter()
                    .any(|&(_, member, _)| particle["member"] == member)
            {
                lives
                    .entry(
                        particle["id"]
                            .as_u64()
                            .context("missing seal particle identity")?,
                    )
                    .or_default()
                    .push((clock, particle));
            }
        }
    }
    let mut actual = BTreeMap::<(u64, u64, u64), usize>::new();
    for life in lives.values() {
        let initialized = life
            .iter()
            .position(|(_, particle)| particle["initialized"] == true)
            .context("seal feedback never initializes")?;
        ensure!(
            initialized <= 1,
            "seal feedback has repeated uninitialized visits"
        );
        let (clock, first) = life[initialized];
        let member = first["member"].as_u64().context("missing seal member")?;
        let key = (clock.0, clock.1, member);
        let &(_, lifetime) = expected
            .get(&key)
            .context("seal feedback has no success-program birth")?;
        if initialized == 1 {
            ensure!(
                life[0].0 == (clock.0 - 1, clock.1 - 1) && life[0].1["age"] == 0,
                "seal feedback has an unbound allocation phase"
            );
        }
        let live = &life[initialized..];
        ensure!(
            live.len() as u64 == lifetime,
            "seal feedback has the wrong finite lifetime"
        );
        for (index, (at, particle)) in live.iter().enumerate() {
            let age = index as u64 + 1;
            ensure!(
                *at == (clock.0 + age - 1, clock.1 + age - 1)
                    && particle["age"] == age
                    && particle["initialized"] == true
                    && particle["member"] == member
                    && particle["model"].is_null(),
                "seal feedback identity, age or model changed"
            );
            equal(
                &first["origin"],
                &particle["origin"],
                "detached seal feedback origin",
            )?;
        }
        *actual.entry(key).or_default() += 1;
    }
    ensure!(
        actual
            == expected
                .iter()
                .map(|(&key, &(count, _))| (key, count))
                .collect(),
        "seal feedback births differ from its success programs"
    );
    Ok(())
}

#[test]
fn optional_seals_keep_contacts_and_validate_each_feedback_lifetime() {
    use serde_json::json;
    let binding = json!({"owner":0,"bank":SEAL_BANK,"program":134,"attempts":[
        {"clock":2,"presentation_clock":2,"target":1,"contact_clock":1,"emitted":true}]});
    let recipe = [(0, 141, 25), (4, 141, 25), (8, 141, 25)];
    let trace = Trace {
        case: "test".into(), setup: json!({"owner":0,"actors":[
            {"actor":0,"side":0,"conditions":[]},{"actor":1,"side":1,"conditions":[]}]}),
        frames: (0..=34).map(|clock| json!({"combat_tick":clock,"presentation_tick":clock,
            "particles":([2,5,9].into_iter().enumerate().filter(|(_,birth)| clock >= *birth && clock < *birth + 25)
                .map(|(id,birth)| json!({"id":id,"owner":0,"bank":SEAL_BANK,"member":141,
                    "initialized":true,"age":clock-birth+1,"origin":[1.,2.,3.],"model":null})).collect::<Vec<_>>())})).collect(),
        events: vec![
            json!({"kind":"started","owner":0,"action":1,"clock":0,"presentation_clock":0}),
            json!({"kind":"hit","owner":0,"target":1,"action":1,"projectile":null,"hp_change":-10,"clock":1,"presentation_clock":1}),
            json!({"kind":"condition_changed","owner":0,"target":1,"action":1,"before":[],"after":["defense_down"],"clock":2,"presentation_clock":2}),
            json!({"kind":"program_started","owner":0,"bank":SEAL_BANK,"program":134,"clock":2,"presentation_clock":2}),
            json!({"kind":"declaration_started","owner":0,"bank":SEAL_BANK,"member":141,"clock":2,"presentation_clock":2}),
            json!({"kind":"declaration_started","owner":0,"bank":SEAL_BANK,"member":141,"clock":5,"presentation_clock":5}),
            json!({"kind":"declaration_started","owner":0,"bank":SEAL_BANK,"member":141,"clock":9,"presentation_clock":9}),
            json!({"kind":"returned","owner":0,"clock":34,"presentation_clock":34}),
        ], assessment: json!({}), incomplete_observations: vec![],
    };
    let validate = |trace: &Trace, events: &mut Vec<Value>| {
        validate_seal_callbacks(
            trace,
            &binding,
            Condition::DefenseDown,
            &recipe,
            true,
            events,
        )
    };
    let mut accepted = trace.events.clone();
    validate(&trace, &mut accepted).unwrap();
    assert_eq!(
        accepted,
        vec![
            trace.events[0].clone(),
            trace.events[1].clone(),
            trace.events[7].clone()
        ]
    );
    for (index, field, value) in [
        (1, "hp_change", json!(0)),
        (2, "target", json!(0)),
        (2, "after", json!(["attack_down"])),
        (3, "clock", json!(3)),
        (5, "clock", json!(6)),
    ] {
        let mut events = trace.events.clone();
        events[index][field] = value;
        assert!(validate(&trace, &mut events).is_err());
    }
    let mut duplicate = trace.events.clone();
    duplicate.push(trace.events[3].clone());
    assert!(validate(&trace, &mut duplicate).is_err());
    let mut wrong: Trace = trace.clone();
    wrong.frames[10]["particles"][0]["age"] = json!(99);
    assert!(validate(&wrong, &mut trace.events.clone()).is_err());
    wrong = trace.clone();
    wrong.frames[33]["particles"]
        .as_array_mut()
        .unwrap()
        .clear();
    assert!(validate(&wrong, &mut trace.events.clone()).is_err());
    // Without an optional callback registration, a guaranteed condition remains
    // an exact event comparison. Recorded random inputs remain exact as well.
    let mut pair = [trace.events.clone(), trace.events.clone()];
    optional_seal_conditions(&trace, &trace, &mut pair).unwrap();
    assert_eq!(pair[0], trace.events);
    wrong.setup["chance_effects"] = json!([binding]);
    wrong.case = "party:5:126".into();
    wrong.setup["random_inputs"] = json!({});
    optional_seal_conditions(&wrong, &wrong, &mut pair).unwrap();
    assert_eq!(pair[0], trace.events);
}

/// Compare a chance-bearing contact's deterministic mechanics, while checking
/// each independently rolled condition and its poison consequences in its own run.
fn optional_enemy_conditions(trace: &Trace, events: &mut Vec<Value>) -> Result<()> {
    if !trace.case.starts_with("enemy:")
        || trace.setup["catalogue"].is_null()
        || trace.setup.get("random_inputs").is_some()
    {
        return Ok(());
    }
    let row: usize = trace.case.rsplit(':').next().unwrap().parse()?;
    let catalogue = super::content_evidence::action_catalogue(trace)?;
    let Some(authored) =
        catalogue["actions"]["rows"][row]["execution"]["attack"]["events"].as_array()
    else {
        return Ok(());
    };
    optional_contact_conditions(trace, authored, events)
}

fn optional_contact_conditions(
    trace: &Trace,
    authored: &[Value],
    events: &mut Vec<Value>,
) -> Result<()> {
    let owner = &trace.setup["owner"];
    let side = |actor: &Value| {
        trace.setup["actors"].as_array().and_then(|actors| {
            actors
                .iter()
                .find(|entry| entry["actor"] == *actor)
                .map(|entry| &entry["side"])
        })
    };
    let mut applications = Vec::new();
    for change in events.iter().filter(|event| {
        event["kind"] == "condition_changed"
            && &event["owner"] == owner
            && events.iter().any(|start| {
                start["kind"] == "started"
                    && start["owner"] == *owner
                    && start["action"] == event["action"]
                    && start["clock"] == 0
            })
    }) {
        let before: ConditionSet = serde_json::from_value(change["before"].clone())?;
        let after: ConditionSet = serde_json::from_value(change["after"].clone())?;
        if !before.without(after).is_empty() {
            continue;
        }
        let added = serde_json::to_value(after.without(before))?;
        let Some(added) = added.as_array().filter(|conditions| conditions.len() == 1) else {
            continue;
        };
        let permitted = events
            .iter()
            .filter(|hit| {
                hit["kind"] == "hit"
                    && ["owner", "target", "action", "clock", "presentation_clock"]
                        .iter()
                        .all(|key| hit[key] == change[key])
                    && hit["hp_change"].as_i64().is_some_and(|amount| amount < 0)
                    && side(&hit["owner"])
                        .zip(side(&hit["target"]))
                        .is_some_and(|(owner, target)| owner != target)
            })
            .any(|hit| {
                authored.iter().any(|entry| {
                    let Some(at) = entry[0].as_u64() else {
                        return false;
                    };
                    let projectile = !hit["projectile"].is_null();
                    let definition = &entry[1][if projectile { "projectile" } else { "contact" }];
                    let condition = &definition["hit"]["condition"];
                    condition["condition"] == added[0]
                        && condition["chance"]
                            .as_u64()
                            .is_some_and(|chance| (1..100).contains(&chance))
                        && if projectile {
                            events.iter().any(|birth| {
                                birth["kind"] == "projectile_started"
                                    && ["owner", "action", "projectile"]
                                        .iter()
                                        .all(|key| birth[key] == hit[key])
                                    && birth["clock"] == at
                            })
                        } else {
                            hit["clock"]
                                .as_u64()
                                .zip(definition["duration"].as_u64())
                                .is_some_and(|(clock, duration)| {
                                    clock >= at && clock < at + duration
                                })
                        }
                })
            });
        if permitted {
            ensure!(
                !applications.contains(change),
                "optional condition transition is duplicated"
            );
            applications.push(change.clone());
        }
    }
    applications.extend(authored_condition_groups(trace, authored, events)?);
    // Pauses, cures and poison rearming need their own observed clock contract.
    // Keep these intervals strict rather than extrapolating the first cycle.
    applications.retain(|change| {
        let poison = change["after"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "poison_mild" || value == "poison_severe");
        !poison
            || trace
                .frames
                .first()
                .zip(trace.frames.last())
                .is_some_and(|(first, last)| {
                    last["combat_tick"]
                        .as_u64()
                        .zip(first["combat_tick"].as_u64())
                        .zip(change["clock"].as_u64())
                        .is_some_and(|((end, onset), applied)| {
                            end.checked_sub(onset + applied)
                                .is_some_and(|elapsed| elapsed <= 301)
                        })
                })
                && trace.frames.windows(2).all(|pair| {
                    pair[0]["combat_tick"]
                        .as_u64()
                        .and_then(|at| at.checked_add(1))
                        == pair[1]["combat_tick"].as_u64()
                })
                && trace.frames.iter().all(|frame| {
                    frame["actors"].as_array().is_some_and(|actors| {
                        actors
                            .iter()
                            .all(|actor| actor["time_stop"].as_u64().unwrap_or(0) == 0)
                    })
                })
                && !events.iter().any(|later| {
                    later["kind"] == "condition_changed"
                        && later["target"] == change["target"]
                        && later["clock"].as_u64() > change["clock"].as_u64()
                        && change["after"].as_array().unwrap().iter().any(|condition| {
                            (condition == "poison_mild" || condition == "poison_severe")
                                && later["after"]
                                    .as_array()
                                    .is_some_and(|values| !values.contains(condition))
                        })
                })
    });
    let pulses: Vec<_> = events
        .iter()
        .filter(|pulse| {
            pulse["kind"] == "poison_pulse"
                && applications.iter().any(|change| {
                    ["owner", "target", "action"]
                        .iter()
                        .all(|key| pulse[key] == change[key])
                        && change["before"]
                            .as_array()
                            .is_some_and(|values| !values.contains(&pulse["condition"]))
                        && change["after"]
                            .as_array()
                            .is_some_and(|values| values.contains(&pulse["condition"]))
                })
        })
        .cloned()
        .collect();
    validate_optional_poison_cadence(trace, events, &applications, &pulses)?;
    for pulse in &pulses {
        ensure!(
            events
                .iter()
                .filter(|event| event["kind"] == "declaration_started"
                    && event["owner"] == pulse["target"]
                    && event["bank"] == trace.setup["common_effect_bank"]
                    && event["member"] == 73
                    && ["clock", "presentation_clock"]
                        .iter()
                        .all(|key| event[key] == pulse[key]))
                .count()
                == 1,
            "poison pulse lacks its single feedback declaration"
        );
    }
    events.retain(|event| {
        !applications.contains(event)
            && !pulses.contains(event)
            && !pulses.iter().any(|pulse| {
                event["owner"] == pulse["target"]
                    && event["bank"] == trace.setup["common_effect_bank"]
                    && ["clock", "presentation_clock"]
                        .iter()
                        .all(|key| event[key] == pulse[key])
                    && (event["kind"] == "program_started" && event["program"] == 44
                        || event["kind"] == "declaration_started" && event["member"] == 73)
            })
    });
    Ok(())
}

// A grouped affliction is mandatory: each clean, living opponent receives one
// choice from every authored group. Only the choices are independently random.
fn authored_condition_groups(
    trace: &Trace,
    authored: &[Value],
    events: &[Value],
) -> Result<Vec<Value>> {
    let mut applications = Vec::new();
    for entry in authored
        .iter()
        .filter(|entry| entry[1].get("afflict_opponents").is_some())
    {
        let groups = entry[1]["afflict_opponents"]["groups"]
            .as_array()
            .context("missing authored condition groups")?;
        let actors = trace.setup["actors"]
            .as_array()
            .context("missing affliction actors")?;
        let owner = &trace.setup["owner"];
        let side = &actors
            .iter()
            .find(|actor| actor["actor"] == *owner)
            .context("missing affliction owner")?["side"];
        let opponents: Vec<_> = actors
            .iter()
            .filter(|actor| &actor["side"] != side)
            .collect();
        // Existing ailments, immunity and pauses need their own state contract;
        // don't infer a missing group choice from an unobserved rejection/cure.
        if opponents.iter().any(|actor| {
            !actor["conditions"].as_array().is_some_and(Vec::is_empty)
                || !actor["actor"].as_u64().is_some_and(|id| {
                    trace.setup["damage_context"][id as usize]["blocked"]
                        .as_array()
                        .is_some_and(Vec::is_empty)
                })
        }) || trace.frames.windows(2).any(|pair| {
            ["combat_tick", "presentation_tick"].iter().any(|key| {
                pair[0][key].as_u64().and_then(|at| at.checked_add(1)) != pair[1][key].as_u64()
            })
        }) {
            continue;
        }
        let at = entry[0].as_u64().context("missing affliction clock")?;
        let Some(first) = trace.frames.first() else {
            continue;
        };
        let onset = first["combat_tick"]
            .as_u64()
            .context("missing affliction onset")?;
        let Some(frame) = trace
            .frames
            .iter()
            .position(|frame| frame["combat_tick"] == onset + at)
        else {
            continue;
        };
        ensure!(frame > 0, "affliction lacks preceding recipient state");
        let start = events
            .iter()
            .find(|event| {
                event["kind"] == "started" && event["owner"] == *owner && event["clock"] == 0
            })
            .context("affliction lacks action admission")?;
        for target in opponents {
            let hp = |frame: &Value| {
                frame["actors"]
                    .as_array()
                    .and_then(|actors| {
                        actors
                            .iter()
                            .find(|actor| actor["actor"] == target["actor"])
                    })
                    .and_then(|actor| actor["hp"].as_i64())
            };
            let before = hp(&trace.frames[frame - 1]).context("missing affliction recipient HP")?;
            let after = hp(&trace.frames[frame]).context("missing affliction recipient HP")?;
            if before == 0 && after == 0 {
                continue;
            }
            ensure!(
                before > 0 && before == after,
                "affliction recipient lacks unchanged living HP"
            );
            let changes: Vec<_> = events
                .iter()
                .filter(|event| {
                    event["kind"] == "condition_changed"
                        && event["owner"] == *owner
                        && event["target"] == target["actor"]
                        && event["action"] == start["action"]
                        && event["clock"] == at
                        && event["presentation_clock"].as_u64()
                            == trace.frames[frame]["presentation_tick"]
                                .as_u64()
                                .zip(first["presentation_tick"].as_u64())
                                .map(|(now, start)| now - start)
                })
                .collect();
            ensure!(
                changes.len() == 1,
                "required grouped affliction is missing or duplicated"
            );
            let change = changes[0];
            let choices = change["after"]
                .as_array()
                .context("missing affliction choices")?;
            ensure!(
                change["before"].as_array().is_some_and(Vec::is_empty)
                    && choices.len() == groups.len()
                    && groups
                        .iter()
                        .all(|group| group.as_array().is_some_and(|group| {
                            choices
                                .iter()
                                .filter(|choice| group.contains(choice))
                                .count()
                                == 1
                        }))
                    && choices.iter().all(|choice| groups
                        .iter()
                        .filter(|group| group
                            .as_array()
                            .is_some_and(|group| group.contains(choice)))
                        .count()
                        == 1),
                "affliction does not select exactly one condition from each authored group"
            );
            applications.push(change.clone());
        }
    }
    Ok(applications)
}

fn validate_optional_poison_cadence(
    trace: &Trace,
    events: &[Value],
    applications: &[Value],
    pulses: &[Value],
) -> Result<()> {
    let Some(first) = trace.frames.first() else {
        return Ok(());
    };
    let onset = first["combat_tick"]
        .as_u64()
        .context("missing poison clock")?;
    let end = trace.frames.last().unwrap()["combat_tick"]
        .as_u64()
        .context("missing poison end")?
        - onset;
    let active = |target: &Value, condition: &str, clock: u64| {
        events
            .iter()
            .filter(|event| {
                event["kind"] == "condition_changed"
                    && event["target"] == *target
                    && event["clock"].as_u64().is_some_and(|at| at <= clock)
            })
            .max_by_key(|event| event["clock"].as_u64())
            .and_then(|event| event["after"].as_array())
            .is_some_and(|values| {
                values.iter().any(|value| value == condition)
                    && (condition == "poison_severe"
                        || !values.iter().any(|value| value == "poison_severe"))
            })
    };
    for change in applications {
        for condition in ["poison_mild", "poison_severe"] {
            if !change["after"]
                .as_array()
                .is_some_and(|values| values.iter().any(|value| value == condition))
                || change["before"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|value| value == condition))
            {
                continue;
            }
            let applied = change["clock"]
                .as_u64()
                .context("missing poison application clock")?;
            for pulse in pulses.iter().filter(|pulse| {
                pulse["target"] == change["target"] && pulse["condition"] == condition
            }) {
                let elapsed = pulse["clock"]
                    .as_u64()
                    .context("missing poison pulse clock")?
                    .checked_sub(applied)
                    .context("poison precedes application")?;
                ensure!(
                    elapsed >= 31
                        && (elapsed - 31) % 30 == 0
                        && active(&pulse["target"], condition, applied + elapsed),
                    "poison pulse cadence or severity differs from its application"
                );
            }
            for clock in (applied + 31..=end).step_by(30) {
                let before = trace
                    .frames
                    .iter()
                    .find(|frame| frame["combat_tick"].as_u64() == Some(onset + clock - 1))
                    .and_then(|frame| frame["actors"].as_array())
                    .and_then(|actors| {
                        actors
                            .iter()
                            .find(|actor| actor["actor"] == change["target"])
                    });
                if active(&change["target"], condition, clock) {
                    let hp = before
                        .and_then(|actor| actor["hp"].as_i64())
                        .context("scheduled poison lacks its recipient HP observation")?;
                    if hp <= 1 {
                        continue;
                    }
                    ensure!(
                        pulses
                            .iter()
                            .filter(|pulse| pulse["target"] == change["target"]
                                && pulse["condition"] == condition
                                && pulse["clock"] == clock)
                            .count()
                            == 1,
                        "active poison is missing its scheduled pulse"
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn optional_contact_conditions_keep_contacts_and_reject_unbound_changes() {
    use serde_json::json;
    let trace = Trace {
        case: "test".into(),
        setup: json!({"owner":0,"actors":[{"actor":0,"side":0},{"actor":1,"side":1}]}),
        frames: vec![],
        events: vec![],
        assessment: json!({}),
        incomplete_observations: vec![],
    };
    let authored =
        vec![json!([18,{"projectile":{"hit":{"condition":{"condition":"heavy","chance":50}}}}])];
    let events = vec![
        json!({"kind":"started","owner":0,"action":1,"clock":0}),
        json!({"kind":"projectile_started","owner":0,"action":1,"projectile":1,"clock":18}),
        json!({"kind":"hit","owner":0,"target":1,"action":1,"projectile":1,"clock":19,"presentation_clock":19,"hp_change":-10}),
        json!({"kind":"condition_changed","owner":0,"target":1,"action":1,"clock":19,"presentation_clock":19,"before":[],"after":["heavy"]}),
    ];
    let mut accepted = events.clone();
    optional_contact_conditions(&trace, &authored, &mut accepted).unwrap();
    assert_eq!(accepted, events[..3]);
    for (index, field, value) in [
        (1, "clock", json!(17)),
        (2, "hp_change", json!(0)),
        (3, "target", json!(0)),
        (3, "clock", json!(20)),
        (3, "after", json!(["weak"])),
        (3, "after", json!(["heavy", "weak"])),
    ] {
        let mut wrong = events.clone();
        wrong[index][field] = value;
        let before = wrong.clone();
        optional_contact_conditions(&trace, &authored, &mut wrong).unwrap();
        assert_eq!(wrong, before);
    }
    for chance in [0, 100] {
        let mut definition = authored.clone();
        definition[0][1]["projectile"]["hit"]["condition"]["chance"] = json!(chance);
        let mut strict = events.clone();
        optional_contact_conditions(&trace, &definition, &mut strict).unwrap();
        assert_eq!(strict, events);
    }
    let mut duplicate = events.clone();
    duplicate.push(events[3].clone());
    assert!(optional_contact_conditions(&trace, &authored, &mut duplicate).is_err());
}

#[test]
fn authored_condition_groups_require_each_recipient_and_choice() {
    use serde_json::json;
    let trace = Trace {
        case: "test".into(),
        setup: json!({"owner":0,"actors":[
            {"actor":0,"side":0,"conditions":[]},
            {"actor":1,"side":1,"conditions":[]}],
            "damage_context":[{"blocked":[]},{"blocked":[]}]}),
        frames: (0..=2)
            .map(|clock| {
                json!({"combat_tick":clock,"presentation_tick":clock,
            "actors":[{"actor":1,"hp":100}]})
            })
            .collect(),
        events: vec![],
        assessment: json!({}),
        incomplete_observations: vec![],
    };
    let authored = vec![json!([1,{"afflict_opponents":{"groups":[
        ["weak","heavy"],["attack_down","defense_down"]],"dispel":true}}])];
    let events = vec![
        json!({"kind":"started","owner":0,"action":1,"clock":0}),
        json!({"kind":"condition_changed","owner":0,"target":1,"action":1,
            "clock":1,"presentation_clock":1,"before":[],"after":["weak","attack_down"]}),
        json!({"kind":"program_started","owner":0,"program":7,"clock":1}),
    ];
    let mut accepted = events.clone();
    optional_contact_conditions(&trace, &authored, &mut accepted).unwrap();
    assert_eq!(accepted, vec![events[0].clone(), events[2].clone()]);
    for (field, value) in [
        ("target", json!(0)),
        ("owner", json!(1)),
        ("clock", json!(2)),
        ("after", json!(["weak"])),
        ("after", json!(["weak", "heavy"])),
        ("after", json!(["weak", "unknown"])),
    ] {
        let mut wrong = events.clone();
        wrong[1][field] = value;
        assert!(optional_contact_conditions(&trace, &authored, &mut wrong).is_err());
    }
    let mut missing = vec![events[0].clone(), events[2].clone()];
    assert!(optional_contact_conditions(&trace, &authored, &mut missing).is_err());
    let mut duplicate = events.clone();
    duplicate.push(events[1].clone());
    assert!(optional_contact_conditions(&trace, &authored, &mut duplicate).is_err());
}

#[test]
fn optional_poison_requires_every_scheduled_pulse() {
    use serde_json::json;
    let trace = Trace {
        case: "test".into(),
        setup: json!({}),
        frames: (0..=80)
            .map(|clock| json!({"combat_tick":clock,"actors":[{"actor":1,"hp":100}]}))
            .collect(),
        events: vec![],
        assessment: json!({}),
        incomplete_observations: vec![],
    };
    let applications = vec![
        json!({"kind":"condition_changed","owner":0,"target":1,"action":1,"clock":19,"before":[],"after":["poison_mild"]}),
    ];
    let pulses = vec![
        json!({"target":1,"condition":"poison_mild","clock":50}),
        json!({"target":1,"condition":"poison_mild","clock":80}),
    ];
    assert!(
        validate_optional_poison_cadence(&trace, &applications, &applications, &pulses).is_ok()
    );
    assert!(
        validate_optional_poison_cadence(&trace, &applications, &applications, &pulses[..1])
            .is_err()
    );
    let mut delayed = pulses.clone();
    delayed[0]["clock"] = json!(51);
    assert!(
        validate_optional_poison_cadence(&trace, &applications, &applications, &delayed).is_err()
    );
    let mut severe = applications.clone();
    severe[0]["after"] = json!(["poison_mild", "poison_severe"]);
    assert!(validate_optional_poison_cadence(&trace, &severe, &applications, &pulses).is_err());
}

fn bounds(value: &Value) -> Result<(f64, f64)> {
    let values = value.as_array().context("missing numeric envelope")?;
    ensure!(
        values.len() == 2,
        "envelope must contain minimum and maximum"
    );
    let minimum = values[0].as_f64().context("invalid minimum")?;
    let maximum = values[1].as_f64().context("invalid maximum")?;
    ensure!(
        minimum.is_finite() && maximum.is_finite() && minimum <= maximum,
        "invalid numeric envelope"
    );
    Ok((minimum, maximum))
}

fn validate_execution(trace: &Trace) -> Result<()> {
    ensure!(
        !trace.events.iter().any(|event| matches!(
            event["kind"].as_str(),
            Some("unattributed_hp_loss" | "unattributed_hp_gain")
        )),
        "unattributed HP change cannot establish a matched fixture"
    );
    for (case, kind) in [("party:5:135", "hp_drained"), ("party:5:136", "tp_drained")] {
        ensure!(
            trace.case != case || trace.events.iter().any(|event| event["kind"] == kind),
            "seal drain was not observed"
        );
    }
    let owner = required(&trace.setup, "owner")?
        .as_u64()
        .context("invalid owner")?;
    let target = required(&trace.setup, "target")?
        .as_u64()
        .context("invalid target")?;
    let capture_observed = if trace.assessment["outcome"] == "capture" {
        let captured = |frame: &Value| -> Result<bool> {
            let actor = frame["actors"]
                .as_array()
                .context("missing captured actors")?
                .iter()
                .find(|actor| actor["actor"].as_u64() == Some(target))
                .context("missing captured recipient")?;
            let capture = required(actor, "capture")?;
            ensure!(
                capture.is_null()
                    || capture.as_array().is_some_and(|link| link.len() == 2
                        && link[0].as_u64() == Some(owner)
                        && link[1].is_boolean()),
                "invalid capture ownership"
            );
            Ok(!capture.is_null())
        };
        ensure!(
            trace
                .frames
                .iter()
                .try_fold(false, |found, frame| Ok::<_, anyhow::Error>(
                    captured(frame)? || found
                ))?,
            "selected action did not capture its recipient"
        );
        ensure!(
            !captured(trace.frames.last().context("missing release frame")?)?,
            "captured recipient was not released"
        );
        true
    } else {
        false
    };
    let actors = required(&trace.setup, "actors")?
        .as_array()
        .context("invalid initial actors")?;
    for id in [owner, target] {
        ensure!(
            actors
                .iter()
                .any(|actor| actor["actor"].as_u64() == Some(id)),
            "missing actor {id}"
        );
    }
    for actor in actors {
        ensure!(
            required(actor, "conditions")?.is_array(),
            "unknown initial conditions"
        );
    }
    for frame in &trace.frames {
        let actors = required(frame, "actors")?
            .as_array()
            .context("missing actor observations")?;
        let models = required(frame, "models")?
            .as_array()
            .context("missing model observations")?;
        ensure!(
            !actors.is_empty() && !models.is_empty(),
            "empty actor/model observations"
        );
        for id in [owner, target] {
            ensure!(
                actors
                    .iter()
                    .any(|actor| actor["actor"].as_u64() == Some(id)),
                "frame lost actor {id}"
            );
        }
        for actor in actors {
            for field in ["actor", "side", "hp", "tp", "position", "heading"] {
                ensure!(!required(actor, field)?.is_null(), "unknown actor {field}");
            }
        }
        for model in models {
            for field in ["actor", "clip"] {
                ensure!(required(model, field)?.is_number(), "unknown model {field}");
            }
            if model["actor"].as_u64() == Some(owner) {
                ensure!(
                    required(model, "frame")?.is_number(),
                    "unknown owned model frame"
                );
            }
        }
        let secondary = frame.get("secondary_models").and_then(Value::as_array);
        let bindings = trace
            .setup
            .get("secondary_bodies")
            .and_then(Value::as_array);
        ensure!(
            secondary.map_or(0, Vec::len) == bindings.map_or(0, Vec::len),
            "secondary controller count differs from its ready registration"
        );
        for (model, binding) in secondary
            .into_iter()
            .flatten()
            .zip(bindings.into_iter().flatten())
        {
            ensure!(
                model["actor"] == binding["actor"],
                "secondary controller identity differs"
            );
            for field in ["clip", "frame"] {
                ensure!(
                    required(model, field)?.is_number(),
                    "unknown secondary {field}"
                );
            }
            ensure!(
                required(model, "visible")?.is_boolean(),
                "unknown secondary visibility"
            );
        }
        for field in ["projectiles", "effect_models"] {
            ensure!(
                required(frame, field)?.is_array(),
                "unknown {field} observations"
            );
        }
        for effect in frame["effect_models"].as_array().unwrap() {
            ensure!(
                required(effect, "owner")?.is_u64()
                    && required(effect, "member")?.is_u64()
                    && required(effect, "ordinal")?.is_u64(),
                "unknown effect model identity"
            );
            ensure!(
                required(effect, "bank")?.is_string(),
                "unknown effect model bank"
            );
            ensure!(required(required(effect, "model")?, "world")?.as_array().is_some_and(|matrix|
                matrix.len() == 12 && matrix.iter().all(Value::is_number)), "missing effect model transform");
            required(effect, "attachment")?;
        }
    }
    let outcome = required(&trace.assessment, "outcome")?
        .as_str()
        .context("unknown intended outcome")?;
    let events = &trace.events;
    for event in events {
        for clock in ["clock", "presentation_clock"] {
            ensure!(
                required(event, clock)?.is_u64(),
                "unknown semantic event clock"
            );
        }
    }
    let started = events
        .iter()
        .find(|event| event["kind"] == "started" && event["owner"].as_u64() == Some(owner))
        .context("no attributed action admission")?;
    let action = required(started, "action")?;
    ensure!(!action.is_null(), "unknown admitted action");
    let support_observation: Option<support::Observation> = trace
        .setup
        .get("support")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?;
    if support_observation.is_some_and(|observation| observation.time_stop) {
        ensure!(
            actors.iter().all(|actor| actor["time_stop"] == 0)
                && trace
                    .frames
                    .iter()
                    .all(
                        |frame| frame["actors"]
                            .as_array()
                            .is_some_and(|actors| actors.iter().all(|actor| actor["time_stop"]
                                .as_u64()
                                .is_some_and(|ticks| ticks <= u64::from(u16::MAX))))
                    )
                && trace.frames.last().is_some_and(|frame| frame["actors"]
                    .as_array()
                    .is_some_and(|actors| actors.iter().all(|actor| actor["time_stop"] == 0))),
            "time stop lacks observed initial state, countdown or complete thaw"
        );
    }
    let scripted_defeat = trace.setup.get("self_defeat").is_some();
    let self_defeated = scripted_defeat
        || support_observation
            .is_some_and(|observation| observation.terminal == support::Terminal::SelfDefeated);
    if self_defeated {
        ensure!(
            outcome == "support" || scripted_defeat && outcome == "attack",
            "self-defeat terminal requires an authored attack or support action"
        );
        ensure!(
            events.iter().any(|event| event["kind"] == "self_defeated"
                && event["owner"].as_u64() == Some(owner)
                && &event["action"] == action),
            "action did not reach its observed self-defeated terminal"
        );
        ensure!(
            scripted_defeat
                || events.iter().any(|event| event["kind"] == "self_cost"
                    && event["owner"].as_u64() == Some(owner)
                    && event["target"].as_u64() == Some(owner)
                    && &event["action"] == action
                    && event["amount"].as_u64().is_some_and(|amount| amount > 0)),
            "self-defeated support action lacks an attributed caster cost"
        );
        ensure!(
            trace
                .frames
                .last()
                .and_then(|frame| frame["actors"].as_array())
                .is_some_and(|actors| actors.iter().any(|actor| actor["actor"].as_u64()
                    == Some(owner)
                    && if scripted_defeat {
                        actor["defeated"] == true
                    } else {
                        actor["hp"].as_i64() == Some(0)
                    })),
            "self-defeated actor did not remain defeated through cleanup"
        );
    } else if outcome != "departure" {
        ensure!(
            events
                .iter()
                .any(|event| event["kind"] == "recovery_started" && &event["action"] == action),
            "admitted action never recovered"
        );
        ensure!(
            events
                .iter()
                .any(|event| event["kind"] == "returned" && event["owner"].as_u64() == Some(owner)),
            "actor never returned after recovery"
        );
    }
    for event in events {
        if event["kind"] == "program_started" {
            ensure!(
                required(event, "bank")?
                    .as_str()
                    .is_some_and(|bank| !bank.is_empty()),
                "unknown effect bank"
            );
            ensure!(
                required(event, "program")?.is_u64(),
                "unknown effect program"
            );
        } else if event["kind"] == "declaration_started" {
            ensure!(
                required(event, "bank")?
                    .as_str()
                    .is_some_and(|bank| !bank.is_empty()),
                "unknown declaration bank"
            );
            ensure!(
                required(event, "member")?.is_u64(),
                "unknown effect declaration"
            );
        } else if event["kind"] == "projectile_started" {
            let id = required(event, "projectile")?;
            ensure!(
                events
                    .iter()
                    .any(|end| end["kind"] == "projectile_expired" && &end["projectile"] == id),
                "gameplay projectile never retired"
            );
        }
    }
    let owner_side = actors
        .iter()
        .find(|actor| actor["actor"].as_u64() == Some(owner))
        .context("missing support owner")?["side"]
        .clone();
    let mut observed_outcome = capture_observed
        || (scripted_defeat && trace.setup["self_defeat"]["terminal_only"] == true)
        || (outcome == "movement" && moved_during_action(trace)?)
        || (outcome == "presentation" && presented_during_action(trace)?);
    for event in events {
        let support_recipient = support_observation.is_some()
            && actors
                .iter()
                .any(|actor| actor["actor"] == event["target"] && actor["side"] == owner_side);
        let stopped_opponent = support_observation.is_some_and(|observation| observation.time_stop)
            && event["kind"] == "time_stop_started"
            && actors
                .iter()
                .any(|actor| actor["actor"] == event["target"] && actor["side"] != owner_side);
        let attributed = event["owner"].as_u64() == Some(owner)
            && (outcome == "departure"
                || support_recipient
                || stopped_opponent
                || event["target"].as_u64() == Some(target))
            && &event["action"] == action
            && event.get("invocation") == started.get("invocation");
        observed_outcome |= attributed
            && match outcome {
                "attack" | "aerial" => {
                    event["kind"] == "hit" && event["hp_change"].as_i64().is_some_and(|hp| hp < 0)
                        || owned_harmful_condition(event, &trace.setup, action)?
                }
                "support" => support::beneficial(event)?,
                "revival" => event["kind"] == "revived",
                "counter" => event["kind"] == "countered" || event["kind"] == "guarded",
                "departure" => event["kind"] == "departed",
                _ => false,
            };
    }
    ensure!(
        observed_outcome,
        "intended {outcome} outcome was not observed"
    );
    Ok(())
}

pub(super) fn presented_during_action(trace: &Trace) -> Result<bool> {
    let expected = required(&trace.setup, "presentation")?;
    let poses = required(expected, "poses")?
        .as_array()
        .context("invalid presentation poses")?;
    let effects = required(expected, "effects")?
        .as_array()
        .context("invalid presentation effects")?;
    ensure!(
        !poses.is_empty() || !effects.is_empty() || expected["retained_pose"] == true,
        "empty authored presentation"
    );
    let owner = required(&trace.setup, "owner")?;
    if expected["retained_pose"] == true {
        let ready = required(&trace.setup, "ready_models")?
            .as_array()
            .context("missing ready models")?
            .iter()
            .find(|model| &model["actor"] == owner)
            .context("missing retained ready motion")?;
        let mut first = None;
        let mut advanced = false;
        for frame in trace
            .frames
            .iter()
            .filter(|frame| frame["action_age"].is_u64())
        {
            let model = required(frame, "models")?
                .as_array()
                .context("missing body models")?
                .iter()
                .find(|model| &model["actor"] == owner)
                .context("missing retained body motion")?;
            if model["clip"] != ready["clip"] {
                return Ok(false);
            }
            let value = required(model, "frame")?
                .as_f64()
                .context("invalid retained frame")?;
            advanced |= first.is_some_and(|prior| prior != value);
            first.get_or_insert(value);
        }
        if !advanced {
            return Ok(false);
        }
    }
    for pose in poses {
        let field = if pose["first_bone"].is_null() {
            "models"
        } else {
            "partial_models"
        };
        let mut first = None;
        let mut advanced = false;
        for frame in &trace.frames {
            if !frame["action_age"].is_u64() {
                continue;
            }
            for model in required(frame, field)?
                .as_array()
                .context("invalid presentation models")?
            {
                if &model["actor"] == owner
                    && model["clip"] == pose["clip"]
                    && model["first_bone"] == pose["first_bone"]
                {
                    let value = required(model, "frame")?
                        .as_f64()
                        .context("invalid presentation frame")?;
                    advanced |= first.is_some_and(|prior| prior != value);
                    first.get_or_insert(value);
                }
            }
        }
        if !advanced {
            return Ok(false);
        }
    }
    Ok(effects.iter().all(|effect| {
        trace.events.iter().any(|event| {
            event["kind"] == "program_started"
                && &event["owner"] == owner
                && &event["program"] == effect
        })
    }))
}

pub(super) fn moved_during_action(trace: &Trace) -> Result<bool> {
    let owner: usize = serde_json::from_value(trace.setup["owner"].clone())?;
    let initial: [f32; 3] =
        serde_json::from_value(trace.setup["actors"][owner]["position"].clone())?;
    for frame in &trace.frames {
        if frame["action_age"].as_u64().is_some() {
            let position: [f32; 3] =
                serde_json::from_value(frame["actors"][owner]["position"].clone())?;
            if position != initial {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn owned_harmful_condition(
    event: &Value,
    setup: &Value,
    action: &Value,
) -> Result<bool> {
    let owner = setup["owner"].as_u64();
    let target = setup["target"].as_u64();
    if event["kind"] != "condition_changed"
        || owner.is_none()
        || target.is_none()
        || event["owner"].as_u64() != owner
        || event["target"].as_u64() != target
        || !action.is_u64()
        || &event["action"] != action
    {
        return Ok(false);
    }
    let actors = required(setup, "actors")?
        .as_array()
        .context("invalid initial actors")?;
    let side = |id| {
        actors
            .iter()
            .find(|actor| actor["actor"].as_u64() == id)
            .and_then(|actor| actor["side"].as_u64())
    };
    if !side(owner)
        .zip(side(target))
        .is_some_and(|(owner, target)| owner != target)
    {
        return Ok(false);
    }
    let before: ConditionSet = serde_json::from_value(event["before"].clone())?;
    let after: ConditionSet = serde_json::from_value(event["after"].clone())?;
    Ok(after.without(before).intersects(
        PHYSICAL_AILMENTS
            .union(MAGICAL_AILMENTS)
            .union(ConditionSet::of(&[Condition::RecoveryDown])),
    ))
}

fn start(trace: &Trace) -> Result<[u64; 2]> {
    let frame = trace.frames.first().context("missing admission frame")?;
    Ok([
        required(frame, "presentation_tick")?
            .as_u64()
            .context("invalid presentation clock")?,
        required(frame, "combat_tick")?
            .as_u64()
            .context("invalid combat clock")?,
    ])
}

fn required<'a>(value: &'a Value, field: &str) -> Result<&'a Value> {
    value
        .get(field)
        .with_context(|| format!("missing required observation {field}"))
}

pub(super) fn equal(source: &Value, native: &Value, path: &str) -> Result<()> {
    let heading = path == "heading" || path.ends_with(".heading");
    let actor_position = path.contains(".actors[")
        && [".position[0]", ".position[1]", ".position[2]"]
            .iter()
            .any(|suffix| path.ends_with(suffix));
    match (source, native) {
        (Value::Object(source), Value::Object(native)) => {
            ensure!(
                source.keys().eq(native.keys()),
                "{path}: observation fields differ"
            );
            for (key, value) in source {
                equal(value, &native[key], &format!("{path}.{key}"))?;
            }
        }
        (Value::Array(source), Value::Array(native)) => {
            ensure!(
                source.len() == native.len(),
                "{path}: count differs {} / {}",
                source.len(),
                native.len()
            );
            for (index, (source, native)) in source.iter().zip(native).enumerate() {
                equal(source, native, &format!("{path}[{index}]"))?;
            }
        }
        (Value::Number(source), Value::Number(native))
            if source.is_f64() || native.is_f64() || heading =>
        {
            // Portable direction arithmetic can accumulate sub-unit recoil drift.
            // Keep animation, heading, contact clocks and other quantities strict.
            let mut difference = (source.as_f64().context("invalid source number")?
                - native.as_f64().context("invalid native number")?)
            .abs();
            if heading {
                // Heading is a direction in degrees, independent of complete turns.
                difference = difference.rem_euclid(360.);
                difference = difference.min(360. - difference);
            }
            let tolerance = if actor_position { 0.25 } else { 0.01 };
            ensure!(difference <= tolerance, "{path}: {source} != {native}");
        }
        _ => ensure!(source == native, "{path}: {source} != {native}"),
    }
    Ok(())
}

#[test]
fn incomplete_empty_and_wrong_effect_observations_cannot_pass() {
    use serde_json::json;
    let make = || Trace {
        case: "party:2:35".into(),
        setup: json!({"formation":2,"arena":13,"owner":0,"target":1,"common_effect_bank":"common","actors":[{"actor":0,"side":0,"conditions":[]},{"actor":1,"side":1,"conditions":[]}]}),
        frames: vec![
            json!({"presentation_tick":10,"combat_tick":4,"action_age":0,
            "actors":[{"actor":0,"side":0,"hp":50,"tp":10,"position":[0.,0.,0.],"heading":90.},
                {"actor":1,"side":1,"hp":40,"tp":10,"position":[100.,0.,0.],"heading":270.}],
            "models":[{"actor":0,"clip":1,"frame":0.,"world":[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.]}],
            "effect_models":[{"owner":0,"bank":"techniques","member":47,"ordinal":1,
                "model":{"clip":null,"frame":null,"world":[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.]},"attachment":null}],
            "projectiles":[],"particles":[]}),
        ],
        events: vec![
            json!({"kind":"started","owner":0,"action":1,"clock":0,"presentation_clock":0}),
            json!({"kind":"program_started","owner":0,"bank":"techniques","program":30,"clock":0,"presentation_clock":0}),
            json!({"kind":"declaration_started","owner":0,"bank":"techniques","member":47,"clock":0,"presentation_clock":0}),
            json!({"kind":"hit","owner":0,"target":1,"projectile":null,"action":1,"hp_change":-10,"damage_range":[8,18],"damage_branches":{"ordinary":[8,12],"critical":[15,18]},"critical":false,"clock":0,"presentation_clock":0}),
            json!({"kind":"recovery_started","owner":0,"action":1,"clock":0,"presentation_clock":0}),
            json!({"kind":"returned","owner":0,"clock":0,"presentation_clock":0}),
        ],
        assessment: json!({"outcome":"attack"}),
        incomplete_observations: vec![],
    };
    assert!(equivalent(&make(), &make()).is_ok());
    let mut hidden = make();
    hidden.setup["ready_models"] = json!([{"actor":2,"defeated_hidden":true}]);
    hidden.setup["ready_model_observations"] = json!([{"actor":2,"source_slot":5,"status":6}]);
    hidden.setup["initial_battle"] = json!({"actors":[{}, {},
        {"slot":5,"side":1,"hp":0,"action":[18,0,0,0,16,0,0,0]}]});
    hidden.frames[0]["actors"]
        .as_array_mut()
        .unwrap()
        .push(json!({"actor":2,"side":1,"hp":0,"tp":0,"position":[200.,0.,0.],"heading":0.}));
    hidden.frames[0]["models"].as_array_mut().unwrap().push(
        json!({"actor":2,"clip":0,"frame":2.5,"world":[1.,0.,0.,200.,0.,1.,0.,0.,0.,0.,1.,0.]}),
    );
    let mut hidden_native: Trace = hidden.clone();
    hidden_native.frames[0]["models"][1]["clip"] = json!(3);
    equivalent(&hidden, &hidden_native).expect("proved invisible defeated fixture pose");
    hidden.setup["ready_model_observations"][0]["status"] = json!(2);
    assert!(equivalent(&hidden, &hidden_native).is_err());
    hidden.setup["ready_model_observations"][0]["status"] = json!(6);
    hidden_native.frames[0]["actors"][2]["hp"] = json!(1);
    assert!(equivalent(&hidden, &hidden_native).is_err());
    hidden_native.frames[0]["actors"][2]["hp"] = json!(0);
    hidden_native.frames[0]["actors"][2]["position"][0] = json!(210.);
    assert!(equivalent(&hidden, &hidden_native).is_err());
    hidden_native.frames[0]["actors"][2]["position"][0] = json!(200.);
    hidden_native.frames[0]["models"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(equivalent(&hidden, &hidden_native).is_err());
    for failure in [
        "execution_or_cleanup_not_completed_within_source_window",
        "selected_action_did_not_damage_an_opponent",
        "departure_prerequisite_not_reproduced",
        "observed_cast_replacement_not_reproduced",
    ] {
        let mut failed = make();
        failed.assessment["execution_failures"] = json!([failure]);
        assert!(failed.incomplete_observations.is_empty());
        let error = equivalent(&make(), &failed).unwrap_err().to_string();
        assert!(error.contains("native execution differs") && error.contains(failure));
        assert!(equivalent(&failed, &failed).is_err());
    }
    let mut retained = make();
    retained.setup["presentation"] = json!({"poses":[],"effects":[],"retained_pose":true});
    retained.setup["ready_models"] = json!([{"actor":0,"clip":1}]);
    assert!(!presented_during_action(&retained).unwrap());
    let mut advanced = retained.frames[0].clone();
    advanced["models"][0]["frame"] = json!(1.);
    retained.frames.push(advanced);
    assert!(presented_during_action(&retained).unwrap());
    retained.frames[1]["models"][0]["clip"] = json!(2);
    assert!(!presented_during_action(&retained).unwrap());
    let mut stopped = make();
    stopped.assessment["outcome"] = json!("support");
    stopped.setup["support"] = json!({"terminal":"returned","time_stop":true});
    for actor in stopped.setup["actors"].as_array_mut().unwrap() {
        actor["time_stop"] = json!(0);
    }
    for actor in stopped.frames[0]["actors"].as_array_mut().unwrap() {
        actor["time_stop"] = json!(0);
    }
    let mut thawed = stopped.frames[0].clone();
    thawed["presentation_tick"] = json!(11);
    thawed["combat_tick"] = json!(5);
    stopped.frames[0]["actors"][1]["time_stop"] = json!(1);
    stopped.frames.push(thawed);
    stopped.events.retain(|event| event["kind"] != "hit");
    stopped
        .events
        .push(json!({"kind":"time_stop_started","owner":0,"target":1,
        "action":1,"ticks":1,"clock":0,"presentation_clock":0}));
    assert!(equivalent(&stopped, &stopped).is_ok());
    let thawed = stopped.frames.pop().unwrap();
    assert!(equivalent(&stopped, &stopped).is_err());
    stopped.frames.push(thawed);
    let mut wrong_countdown: Trace = stopped.clone();
    wrong_countdown.frames[0]["actors"][1]["time_stop"] = json!(2);
    assert!(equivalent(&stopped, &wrong_countdown).is_err());
    let mut defeated = make();
    defeated.setup["self_defeat"] = json!({"retain_body":false});
    defeated.frames[0]["actors"][0]["defeated"] = json!(true);
    defeated
        .events
        .retain(|event| event["kind"] != "returned" && event["kind"] != "recovery_started");
    defeated.events.push(
        json!({"kind":"self_defeated","owner":0,"action":1,"clock":0,"presentation_clock":0}),
    );
    assert!(equivalent(&defeated, &defeated).is_ok());
    defeated.events.retain(|event| event["kind"] != "hit");
    assert!(equivalent(&defeated, &defeated).is_err());
    defeated.setup["self_defeat"]["terminal_only"] = json!(true);
    assert!(equivalent(&defeated, &defeated).is_ok());
    defeated.frames[0]["actors"][0]["defeated"] = json!(false);
    assert!(equivalent(&defeated, &defeated).is_err());
    defeated.frames[0]["actors"][0]["defeated"] = json!(true);
    defeated
        .events
        .retain(|event| event["kind"] != "self_defeated");
    assert!(equivalent(&defeated, &defeated).is_err());
    let mut attached = make();
    let first = attached.frames[0]["effect_models"][0].clone();
    attached.frames[0]["effect_models"] = json!([first, first]);
    for (index, model) in attached.frames[0]["effect_models"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        model["ordinal"] = json!(index + 1);
        model["attachment"] = json!({"kind":"center", "actor":index});
        model["model"]["world"][3] = json!(index * 100);
    }
    let mut later = attached.frames[0].clone();
    later["presentation_tick"] = json!(11);
    later["combat_tick"] = json!(5);
    attached.frames.push(later);
    let mut reordered = attached.clone();
    for frame in &mut reordered.frames {
        for model in frame["effect_models"].as_array_mut().unwrap() {
            model["ordinal"] = json!(3 - model["ordinal"].as_u64().unwrap());
        }
        frame["effect_models"].as_array_mut().unwrap().reverse();
    }
    assert!(equivalent(&attached, &reordered).is_ok());
    reordered.frames[1]["effect_models"][0]["attachment"]["actor"] = json!(0);
    assert!(equivalent(&attached, &reordered).is_err());
    reordered.frames[1]["effect_models"][0]["attachment"]["actor"] = json!(1);
    reordered.frames[1]["effect_models"][0]["model"]["world"][3] = json!(101.);
    assert!(equivalent(&attached, &reordered).is_err());
    reordered.frames[1]["effect_models"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    assert!(equivalent(&attached, &reordered).is_err());

    let mut recurring = make();
    let bank = "0".repeat(64);
    recurring.events[1]["bank"] = json!(bank);
    recurring.setup["initial_battle"] = json!({"presentation_tick":0});
    recurring.setup["ready_ambient_effect"] =
        json!({"bank":bank,"program":30,"interval":9,"counter":0,"program_end":8});
    recurring.setup["effect_baseline"] = json!([{
        "owner":0,"bank":bank,"program":30,"member":47,"lifetime":17,"interval":9,
        "observed_starts":[1,10],"scenery":{"program_end":8,"spawn_offsets":[0]}
    }]);
    assert!(equivalent(&recurring, &recurring).is_ok());
    let baseline: Vec<Baseline> =
        serde_json::from_value(recurring.setup["effect_baseline"].clone()).unwrap();
    let mut active = make();
    active.setup["initial_battle"] = recurring.setup["initial_battle"].clone();
    active.setup["ready_passive_effects"] =
        json!([{"actor":0,"program":30,"interval":9,"counter":0}]);
    let mut active_baseline = recurring.setup["effect_baseline"][0].clone();
    active_baseline["scenery"] = Value::Null;
    active_baseline["scheduler"] =
        json!({"program_end":8,"observed_start_age":2,"spawn_offsets":[0]});
    let active_baseline: Baseline = serde_json::from_value(active_baseline).unwrap();
    active.frames[0]["presentation_tick"] = json!(14);
    active.frames[0]["programs"] = json!([{"owner":0,"bank":bank,"program":30,"age":6}]);
    assert!(baseline_active_at_start(&active_baseline, &active));
    active.frames[0]["programs"][0]["age"] = json!(5);
    assert!(!baseline_active_at_start(&active_baseline, &active));
    active.frames[0]["programs"][0]["age"] = json!(6);
    active.setup["ready_passive_effects"][0]["counter"] = json!(1);
    assert!(!baseline_active_at_start(&active_baseline, &active));
    active.setup["ready_passive_effects"][0]["counter"] = json!(0);
    active.setup["ready_ambient_effect"] = recurring.setup["ready_ambient_effect"].clone();
    active.frames[0]["programs"][0]["age"] = json!(5);
    assert!(baseline_active_at_start(&baseline[0], &active));
    active.frames[0]["programs"][0]["age"] = json!(6);
    assert!(!baseline_active_at_start(&baseline[0], &active));
    active.frames[0]["programs"] = json!([]);
    assert!(!baseline_active_at_start(&active_baseline, &active));
    let mut pending = json!({"presentation_tick":15,
        "programs":[{"owner":0,"bank":bank,"program":30,"age":1}],
        "particles":[{"owner":0,"bank":bank,"member":47,"age":0,"initialized":false}]});
    assert!(baseline_particles(&pending, &baseline, true).is_ok());
    let mut modeled = pending.clone();
    modeled["particles"][0]["initialized"] = json!(true);
    modeled["particles"][0]["model"] = recurring.frames[0]["effect_models"][0]["model"].clone();
    modeled["effect_models"] = json!([{
        "owner":0,"bank":bank,"member":47,"model":modeled["particles"][0]["model"]
    }]);
    assert!(baseline_particles(&modeled, &baseline, true).is_ok());
    modeled["effect_models"][0]["model"]["world"][3] = json!(101.);
    assert!(baseline_particles(&modeled, &baseline, true).is_err());
    modeled["effect_models"] = json!([]);
    assert!(baseline_particles(&modeled, &baseline, true).is_err());
    pending["programs"][0]["age"] = json!(2);
    assert!(baseline_particles(&pending, &baseline, true).is_err());
    pending["programs"] = json!([]);
    assert!(baseline_particles(&pending, &baseline, true).is_err());
    let mut passive = recurring.setup["effect_baseline"].clone();
    passive[0]["scenery"] = Value::Null;
    passive[0]["scheduler"] = json!({"program_end":0,"observed_start_age":0,"spawn_offsets":[0]});
    let passive: Vec<Baseline> = serde_json::from_value(passive).unwrap();
    pending["programs"] = json!([{"owner":0,"bank":bank,"program":30,"age":0}]);
    assert!(baseline_particles(&pending, &passive, true).is_ok());
    pending["programs"][0]["age"] = json!(1);
    assert!(baseline_particles(&pending, &passive, true).is_err());
    pending["programs"][0]["age"] = json!(0);
    let duplicate = pending["particles"][0].clone();
    pending["particles"].as_array_mut().unwrap().push(duplicate);
    assert!(baseline_particles(&pending, &passive, true).is_err());
    let mut finite = recurring.setup["effect_baseline"].clone();
    finite[0]["scenery"] = Value::Null;
    finite[0]["scheduler"] = json!({"program_end":2,"observed_start_age":2,
        "spawn_offsets":[0,1],"pending_ages":[1,3]});
    let finite: Vec<Baseline> = serde_json::from_value(finite).unwrap();
    pending["particles"].as_array_mut().unwrap().truncate(1);
    for age in [1, 3] {
        pending["programs"][0]["age"] = json!(age);
        assert!(baseline_particles(&pending, &finite, true).is_ok());
    }
    pending["programs"][0]["age"] = json!(2);
    assert!(baseline_particles(&pending, &finite, true).is_err());
    pending["programs"][0]["age"] = json!(1);
    let duplicate = pending["particles"][0].clone();
    pending["particles"].as_array_mut().unwrap().push(duplicate);
    assert!(baseline_particles(&pending, &finite, true).is_err());
    for (field, value) in [("counter", json!(1)), ("interval", json!(8))] {
        let before = recurring.setup["ready_ambient_effect"][field].clone();
        recurring.setup["ready_ambient_effect"][field] = value;
        assert!(equivalent(&recurring, &recurring).is_err());
        recurring.setup["ready_ambient_effect"][field] = before;
    }
    recurring.setup["effect_baseline"][0]["observed_starts"] = json!([10]);
    assert!(equivalent(&recurring, &recurring).is_err());
    recurring.setup["effect_baseline"][0]["observed_starts"] = json!([1, 10]);
    recurring.setup["ready_ambient_effect"]["program_end"] = json!(18);
    recurring.setup["effect_baseline"][0]["scenery"] =
        json!({"program_end":18,"spawn_offsets":[0,0,10]});
    assert!(equivalent(&recurring, &recurring).is_ok());
    recurring.setup["effect_baseline"][0]["scenery"]["spawn_offsets"] = json!([0, 19]);
    assert!(equivalent(&recurring, &recurring).is_err());
    recurring.setup["effect_baseline"][0]["scenery"] = Value::Null;
    assert!(equivalent(&recurring, &recurring).is_err());
    for (source_heading, native_heading) in [
        (json!(-183.0248566), json!(176.9751587)),
        (json!(-180), json!(180)),
        (json!(90.), json!(810.)),
        (json!(359.996), json!(0.004)),
    ] {
        let mut source = make();
        let mut native = make();
        source.frames[0]["actors"][0]["heading"] = source_heading;
        native.frames[0]["actors"][0]["heading"] = native_heading.clone();
        assert!(equivalent(&source, &native).is_ok());
        assert!(equivalent(&native, &source).is_ok());
        native.frames[0]["actors"][0]["heading"] = json!(native_heading.as_f64().unwrap() + 0.02);
        assert!(equivalent(&source, &native).is_err());
    }
    let mut wrong_position = make();
    wrong_position.frames[0]["actors"][0]["position"][0] = json!(0.2);
    assert!(equivalent(&make(), &wrong_position).is_ok());
    wrong_position.frames[0]["actors"][0]["position"][0] = json!(1.);
    assert!(equivalent(&make(), &wrong_position).is_err());
    wrong_position.frames[0]["actors"][0]["position"][0] = json!(360.);
    assert!(equivalent(&make(), &wrong_position).is_err());
    let mut wrong_world = make();
    wrong_world.frames[0]["effect_models"][0]["model"]["world"][0] = json!(361.);
    assert!(equivalent(&make(), &wrong_world).is_err());
    let mut empty_program = make();
    let empty_bank = "a".repeat(64);
    empty_program
        .events
        .push(json!({"kind":"program_started","owner":0,
        "bank":empty_bank,"program":1,"clock":0,"presentation_clock":0}));
    assert!(equivalent(&empty_program, &make()).is_err());
    empty_program.setup["immediate_end_programs"] = json!([{
        "bank":empty_bank,"program":1,"first_command":[0,0,254,0,0,0]
    }]);
    assert!(equivalent(&empty_program, &make()).is_ok());
    assert!(equivalent(&empty_program, &empty_program).is_ok());
    for (field, value) in [
        ("bank", json!("unknown")),
        ("program", json!(2)),
        ("program", json!("1")),
        ("first_command", json!([0, 1, 254, 0, 0, 0])),
        ("first_command", json!([0, 0, 0, 0, 0, 0])),
        ("first_command", Value::Null),
    ] {
        let mut malformed = empty_program.clone();
        malformed.setup["immediate_end_programs"][0][field] = value;
        assert!(equivalent(&malformed, &make()).is_err());
    }
    let mut emission = make();
    emission
        .events
        .push(json!({"kind":"declaration_started","owner":0,
        "bank":empty_bank,"program":1,"member":1,"clock":0,"presentation_clock":0}));
    assert!(equivalent(&empty_program, &emission).is_err());
    let mut observed_inputs = make();
    observed_inputs.setup["contact_stun"] = json!([{"clock":8,"target":1,"stunned":true}]);
    observed_inputs.events.extend([
        json!({"kind":"projectile_started","owner":0,"target":1,"action":1,"projectile":1,"presentation_clock":7}),
        json!({"kind":"hit","owner":0,"target":1,"action":1,"projectile":1,"presentation_clock":8}),
    ]);
    let mut proof = json!({
        "input_types":{"scatter_return":"signed_i16_remainder","stun_return":"unsigned_u16_modulo_100",
            "scatter":{"position_range":4,"position_step":35.,"lateral_range":40,"lateral_step":0.1}},
        "launches":[{"clock":7,"owner":0,"target":1,"action":1,"projectile":1,"values":[55693,23658,13173]}],
        "contacts":[{"clock":8,"owner":0,"target":1,"action":1,"projectile":1,"stun_value":3403}],
        "contact_stun":observed_inputs.setup["contact_stun"]
    });
    proof["launches"][0]["matching_source_triples"] = json!(1);
    proof["launches"][0]["signed_remainders"] = json!([-3, 2, 13]);
    proof["contacts"][0]["effective_chance"] = json!(10);
    proof["contacts"][0]["stun_remainder"] = json!(3);
    assert!(super::random_inputs::qualify(&observed_inputs, &proof).is_ok());
    let mut critical_proof = proof.clone();
    critical_proof["input_types"]["critical_chance_return"] = json!("unsigned_u16_modulo_4");
    critical_proof["input_types"]["critical_return"] = json!("unsigned_u16_modulo_100");
    critical_proof["critical_draws"] = json!([{
        "clock":8,"owner":0,"target":1,"action":1,"projectile":1,
        "chance_value":58405,"value":3204
    }]);
    assert!(super::random_inputs::qualify(&observed_inputs, &critical_proof).is_ok());
    for (field, value) in [
        ("clock", json!(7)),
        ("target", json!(0)),
        ("chance_value", json!(65536)),
        ("value", Value::Null),
    ] {
        let mut wrong = critical_proof.clone();
        wrong["critical_draws"][0][field] = value;
        assert!(super::random_inputs::qualify(&observed_inputs, &wrong).is_err());
    }
    let mut ailment_proof = proof.clone();
    ailment_proof["input_types"]["ailment_return"] = json!("unsigned_u16_modulo_100");
    ailment_proof["ailment_draws"] = json!([{
        "clock":8,"owner":0,"target":1,"action":1,"projectile":1,
        "condition":{"condition":"curse","chance":50,"value":0},
        "resistance":false,"value":4343,"remainder":43
    }]);
    assert!(super::random_inputs::qualify(&observed_inputs, &ailment_proof).is_ok());
    for (field, value) in [
        ("clock", json!(7)),
        ("remainder", json!(42)),
        ("resistance", json!("false")),
        ("value", json!(65536)),
    ] {
        let mut wrong = ailment_proof.clone();
        wrong["ailment_draws"][0][field] = value;
        assert!(super::random_inputs::qualify(&observed_inputs, &wrong).is_err());
    }
    let mut seal_proof = proof.clone();
    seal_proof["input_types"]["seal_return"] = json!("unsigned_u16_reverse_percentile");
    seal_proof["seal_draws"] = json!([{
        "clock":9,"contact_clock":8,"owner":0,"target":1,"action":1,"projectile":1,
        "condition":"defense_down","chance_percent":27,"value":6488,"remainder":88,"percentile":11
    }]);
    assert!(super::random_inputs::qualify(&observed_inputs, &seal_proof).is_ok());
    for (field, value) in [
        ("clock", json!(10)),
        ("contact_clock", json!(7)),
        ("percentile", json!(12)),
        ("target", json!(0)),
    ] {
        let mut wrong = seal_proof.clone();
        wrong["seal_draws"][0][field] = value;
        assert!(super::random_inputs::qualify(&observed_inputs, &wrong).is_err());
    }
    let mut ring_proof = proof.clone();
    ring_proof["launches"] = json!([]);
    ring_proof["input_types"]["hammer_ring_return"] = json!("signed_i16_remainder");
    ring_proof["hammer_ring_launches"] = json!([{
        "clock":7,"owner":0,"target":1,"action":1,"projectile":1,"ring_index":2,
        "values":[44012,6446,53151],"signed_remainders":[-24,46,-35],"matching_source_triples":1,
        "distribution":{"count":10,"angle_step":0.6,"speed":4.,"speed_jitter":0.05,
            "lift":7.,"lift_jitter":0.1,"jitter_range":50}
    }]);
    assert!(super::random_inputs::qualify(&observed_inputs, &ring_proof).is_ok());
    for (field, value) in [
        ("ring_index", json!(10)),
        ("values", json!([44012, 6446, 65536])),
        ("signed_remainders", json!([-24, 46, 35])),
        ("clock", json!(8)),
    ] {
        let mut wrong = ring_proof.clone();
        wrong["hammer_ring_launches"][0][field] = value;
        assert!(super::random_inputs::qualify(&observed_inputs, &wrong).is_err());
    }
    let mut velocity_proof = proof.clone();
    velocity_proof["launches"] = json!([]);
    velocity_proof["input_types"]["velocity_jitter_return"] = json!("signed_i16_remainder");
    velocity_proof["velocity_launches"] = json!([{
        "clock":7,"owner":0,"target":1,"action":1,"projectile":1,
        "axis":1,"amplitude":2.,"value":33995,"steps":20,"scale":0.1,
        "signed_remainder":-1,"matching_source_values":1
    }]);
    assert!(super::random_inputs::qualify(&observed_inputs, &velocity_proof).is_ok());
    for (field, value) in [
        ("axis", json!(3)),
        ("value", json!(65536)),
        ("amplitude", json!(0.5)),
        ("steps", json!(19)),
        ("clock", json!(8)),
    ] {
        let mut wrong = velocity_proof.clone();
        wrong["velocity_launches"][0][field] = value;
        assert!(super::random_inputs::qualify(&observed_inputs, &wrong).is_err());
    }
    for (kind, field) in [
        ("launches", "values"),
        ("contacts", "stun_value"),
        ("contacts", "clock"),
        ("contacts", "target"),
    ] {
        let mut wrong = proof.clone();
        wrong[kind][0][field] = json!(99);
        assert!(super::random_inputs::qualify(&observed_inputs, &wrong).is_err());
    }
    let mut wrong_proof = proof.clone();
    wrong_proof["contacts"][0]["stun_value"] = json!(65536);
    assert!(super::random_inputs::qualify(&observed_inputs, &wrong_proof).is_err());
    wrong_proof = proof.clone();
    wrong_proof["input_types"]["scatter"]["position_range"] = json!(5);
    assert!(super::random_inputs::qualify(&observed_inputs, &wrong_proof).is_err());
    wrong_proof = proof.clone();
    wrong_proof["contacts"].as_array_mut().unwrap().clear();
    assert!(super::random_inputs::qualify(&observed_inputs, &wrong_proof).is_err());
    let mut protected = observed_inputs.clone();
    protected.setup["contact_stun"]
        .as_array_mut()
        .unwrap()
        .push(json!({"clock":0,"target":1,"stunned":false}));
    let mut protected_proof = proof.clone();
    protected_proof["contact_stun"] = protected.setup["contact_stun"].clone();
    protected_proof["protected_contacts"] = json!([
        {"clock":0,"owner":0,"target":1,"action":1,"projectile":null}
    ]);
    assert!(super::random_inputs::qualify(&protected, &protected_proof).is_ok());
    protected_proof["protected_contacts"][0]["clock"] = json!(7);
    assert!(super::random_inputs::qualify(&protected, &protected_proof).is_err());
    let mut attached = make();
    attached.setup["contact_stun"] = json!([{"clock":0,"target":1,"stunned":true}]);
    let mut attached_proof = proof.clone();
    attached_proof["input_types"] = json!({"stun_return":"unsigned_u16_modulo_100"});
    attached_proof["launches"] = json!([]);
    attached_proof["contacts"][0]["clock"] = json!(0);
    attached_proof["contacts"][0]["projectile"] = Value::Null;
    attached_proof["contacts"][0]["contact_source"] = json!("attached");
    attached_proof["contact_stun"] = attached.setup["contact_stun"].clone();
    assert!(super::random_inputs::qualify(&attached, &attached_proof).is_ok());
    for identity in [Value::Null, json!("weapon"), json!("projectile")] {
        let mut wrong = attached_proof.clone();
        wrong["contacts"][0]["contact_source"] = identity;
        assert!(super::random_inputs::qualify(&attached, &wrong).is_err());
    }
    attached.events.push(attached.events[3].clone());
    assert!(super::random_inputs::qualify(&attached, &attached_proof).is_err());
    let mut debuffed = make();
    debuffed.events[3] = json!({"kind":"condition_changed","owner":0,"target":1,"action":1,
        "clock":0,"presentation_clock":0,"before":[],"after":["recovery_down"]});
    assert!(equivalent(&debuffed, &debuffed).is_ok());
    assert!(owned_harmful_condition(&debuffed.events[3], &debuffed.setup, &json!(1)).unwrap());
    for field in ["owner", "target", "action"] {
        let mut wrong = debuffed.clone();
        wrong.events[3][field] = json!(99);
        assert!(equivalent(&wrong, &wrong).is_err());
        assert!(
            !owned_harmful_condition(&wrong.events[3], &wrong.setup, &json!(1)).unwrap_or(false)
        );
    }
    for (before, after) in [
        (json!(["recovery_down"]), json!(["recovery_down"])),
        (json!(["recovery_down"]), json!([])),
        (json!([]), json!(["attack_up"])),
        (json!([]), json!(["unknown"])),
        (Value::Null, json!(["recovery_down"])),
    ] {
        let mut wrong = debuffed.clone();
        wrong.events[3]["before"] = before;
        wrong.events[3]["after"] = after;
        assert!(equivalent(&wrong, &wrong).is_err());
        assert!(
            !owned_harmful_condition(&wrong.events[3], &wrong.setup, &json!(1)).unwrap_or(false)
        );
    }
    let mut ally = debuffed.clone();
    ally.setup["actors"][1]["side"] = json!(0);
    assert!(equivalent(&ally, &ally).is_err());
    assert!(!owned_harmful_condition(&ally.events[3], &ally.setup, &json!(1)).unwrap());
    let condition = json!({"kind":"condition_changed","owner":0,"target":1,"action":1,
        "clock":0,"presentation_clock":0,"before":[],"after":["curse"]});
    let attempts = [
        json!({"owner":0,"target":1,"condition":"curse","chance_percent":50,
        "clock":0,"presentation_clock":0,"applied":true}),
    ];
    let mut afflicted = make();
    afflicted.events.push(condition);
    assert_eq!(
        super::tests::condition_branch(&afflicted, &attempts),
        [true]
    );
    assert_eq!(super::tests::condition_branch(&make(), &attempts), [false]);
    assert!(equivalent(&afflicted, &make()).is_err());
    for field in ["owner", "target", "clock", "presentation_clock"] {
        let mut wrong = afflicted.clone();
        wrong.events.last_mut().unwrap()[field] = json!(99);
        assert_eq!(super::tests::condition_branch(&wrong, &attempts), [false]);
    }
    let mut drift = afflicted.clone();
    drift.frames[0]["models"][0]["clip"] = json!(99);
    assert_eq!(super::tests::condition_branch(&drift, &attempts), [true]);
    assert!(equivalent(&afflicted, &drift).is_err());
    // A draw changes the initial yaw only. Simultaneous births retain the same
    // one-to-one identity after their distinct deterministic origins are matched.
    let mut yaw_source = make();
    let bank = "a".repeat(64);
    yaw_source.setup["effect_random_yaw"] =
        json!((1..=2).map(|ordinal| json!({
        "owner":0,"bank":bank,"member":47,"ordinal":ordinal,"birth_clock":0,
        "program_clock":0,"first_step":5.,"template":{"path":"source.bank","sha256":bank,
        "member":47,"program":30,"modifier_words":[11,92,1800,0,65535]}
    })).collect::<Vec<_>>());
    for event in &mut yaw_source.events[1..=2] {
        event["bank"] = json!(bank);
    }
    let mut yaw_native = yaw_source.clone();
    let yaw_model = |ordinal: u64, angle: f64, scale: f64, x: f64| {
        let (sine, cosine) = angle.to_radians().sin_cos();
        json!({"owner":0,"bank":bank,"member":47,"ordinal":ordinal,"attachment":null,
            "model":{"clip":null,"frame":null,"world":[cosine*scale,-sine*scale,0.,x,
                0.,0.,scale,35.,-sine*scale,-cosine*scale,0.,-15.]}})
    };
    for index in 0..2 {
        let mut source_frame = make().frames.remove(0);
        source_frame["combat_tick"] = json!(4 + index);
        source_frame["presentation_tick"] = json!(10 + index);
        let step = f64::from(index) * 5.;
        let scale = 1.25 + f64::from(index) * 0.25;
        let mut native_frame = source_frame.clone();
        source_frame["effect_models"] = json!([
            yaw_model(1, 35. + step, scale, 0.),
            yaw_model(2, -45. + step, scale, 12.)
        ]);
        native_frame["effect_models"] = json!([
            yaw_model(1, 175. + step, scale, 12.),
            yaw_model(2, -75. + step, scale, 0.)
        ]);
        if index == 0 {
            yaw_source.frames.clear();
            yaw_native.frames.clear();
        }
        yaw_source.frames.push(source_frame);
        yaw_native.frames.push(native_frame);
    }
    assert!(equivalent(&yaw_source, &yaw_native).is_ok());
    let mut anisotropic_source: Trace = yaw_source.clone();
    let mut anisotropic_native: Trace = yaw_native.clone();
    for trace in [&mut anisotropic_source, &mut anisotropic_native] {
        for frame in &mut trace.frames {
            for model in frame["effect_models"].as_array_mut().unwrap() {
                for (column, factor) in [1.5, 0.75, 0.5].into_iter().enumerate() {
                    for row in 0..3 {
                        let value = model["model"]["world"][row * 4 + column].as_f64().unwrap();
                        model["model"]["world"][row * 4 + column] = json!(value * factor);
                    }
                }
            }
        }
    }
    equivalent(&anisotropic_source, &anisotropic_native)
        .expect("random yaw retains independent authored scale axes");
    for row in 0..3 {
        let value = anisotropic_native.frames[1]["effect_models"][0]["model"]["world"][row * 4 + 2]
            .as_f64()
            .unwrap();
        anisotropic_native.frames[1]["effect_models"][0]["model"]["world"][row * 4 + 2] =
            json!(value * 1.25);
    }
    assert!(equivalent(&anisotropic_source, &anisotropic_native).is_err());
    let accepted = yaw_native.frames.clone();
    yaw_native.frames[1]["effect_models"][0]["model"]["world"][3] = json!(13.);
    assert!(equivalent(&yaw_source, &yaw_native).is_err());
    yaw_native.frames = accepted.clone();
    yaw_native.frames[1]["effect_models"][0]["model"]["world"] =
        yaw_model(1, 179., 1.5, 12.)["model"]["world"].clone();
    assert!(equivalent(&yaw_source, &yaw_native).is_err());
    yaw_native.frames = accepted.clone();
    yaw_native.frames[0]["effect_models"][1]["model"]["world"] =
        yaw_model(2, -75., 1.25, 12.)["model"]["world"].clone();
    assert!(equivalent(&yaw_source, &yaw_native).is_err());
    yaw_native.frames = accepted.clone();
    yaw_native.frames[0]["effect_models"][1]["model"]["world"] =
        yaw_model(2, -75.05, 1.25, 0.)["model"]["world"].clone();
    assert!(equivalent(&yaw_source, &yaw_native).is_err());
    yaw_native.frames = accepted.clone();
    yaw_native.frames[1]["effect_models"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(equivalent(&yaw_source, &yaw_native).is_err());
    yaw_native.frames = accepted;
    let mut absent = yaw_native.frames[1].clone();
    absent["effect_models"] = json!([]);
    yaw_native.frames.insert(1, absent);
    assert!(effect_pitch::validate(&yaw_native, &yaw_native).is_err());
    yaw_source.frames[0]["effect_models"][1]["model"]["world"][3] = json!(0.);
    assert!(effect_pitch::validate(&yaw_source, &yaw_source).is_err());
    // Independent birth choices retain one particle identity and all later motion.
    let mut choice_source = make();
    let angle_range = json!({"min":-179.9,"max":179.9,"step":0.1});
    let offset_range = json!({"min":-89.9,"max":89.9,"step":0.1});
    choice_source.setup["effect_random_births"] = json!([{
        "owner":0,"bank":bank,"member":25,"ordinal":1,"birth_clock":0,"program_clock":0,
        "heading":90.,"origin":[99.,0.,0.],"first_step":[4.,5.,6.],"first_offset":[0.,0.,0.],
        "offsets":[offset_range,null,offset_range],
        "angles":[angle_range,null,angle_range],"model":{"min":3.,"max":4.,"step":1.},"lifetime":2,
        "template":{"path":"source.bank","sha256":bank,"member":25,"program":21,
            "modifier_words":[11,52,900,0,11,60,900,0,11,88,1800,0,11,96,1800,0,2,32764,2,0,12,212,32764,0,65535]}
    }]);
    choice_source.events[1]["bank"] = json!(bank);
    choice_source.events[1]["program"] = json!(21);
    choice_source.events[2]["bank"] = json!(bank);
    choice_source.events[2]["member"] = json!(25);
    let mut choice_native = choice_source.clone();
    for (trace, pitch, roll, offset, slot, particle_id) in [
        (&mut choice_source, -10_f32, 20_f32, [7., -9.], 3, 71),
        (&mut choice_native, 30_f32, -40_f32, [-13., 2.], 4, 29),
    ] {
        trace.frames.clear();
        for index in 0..3 {
            let mut frame = make().frames.remove(0);
            frame["combat_tick"] = json!(4 + index);
            frame["presentation_tick"] = json!(10 + index);
            frame["effect_models"] = json!([]);
            if index < 2 {
                let age = (index + 1) as f32;
                let angles = [pitch + 4. * age, 90. + 5. * age, roll + 6. * age];
                let [x, y, z] = angles.map(f32::to_radians);
                let position = [99. + offset[0], index as f32 * 20., offset[1]];
                let world = bevy::math::Mat4::from_scale_rotation_translation(
                    bevy::math::Vec3::new(0.4, 0.5, 0.7),
                    bevy::math::Quat::from_euler(bevy::math::EulerRot::ZYX, z, y, x),
                    bevy::math::Vec3::from_array(position),
                );
                let model = json!({"clip":null,"frame":null,"world":super::affine(world.to_cols_array_2d())});
                frame["effect_models"] = json!([{"owner":0,"bank":bank,"member":25,"ordinal":1,"attachment":null,"model":model}]);
                frame["particles"] = json!([{"id":particle_id,"owner":0,"bank":bank,"member":25,"initialized":true,
                    "origin":[99.,0.,0.],"offset":[offset[0],index as f32 * 20.,offset[1]],"angles":angles,"model_slot":slot,"model":model}]);
            }
            trace.frames.push(frame);
        }
    }
    assert!(equivalent(&choice_source, &choice_native).is_ok());
    // Independently sampled XYZ angles with authored multi-axis spin cannot be
    // normalized by applying only one constant inverse rotation to later frames.
    let mut spinning_source: Trace = choice_source.clone();
    let mut spinning_native: Trace = choice_native.clone();
    let set_spin_angles = |frame: &mut Value, angles: [f32; 3]| {
        let [x, y, z] = angles.map(f32::to_radians);
        let position = [3, 7, 11].map(|index| {
            frame["effect_models"][0]["model"]["world"][index]
                .as_f64()
                .unwrap() as f32
        });
        let world = bevy::math::Mat4::from_scale_rotation_translation(
            bevy::math::Vec3::new(0.4, 0.5, 0.7),
            bevy::math::Quat::from_euler(bevy::math::EulerRot::ZYX, z, y, x),
            bevy::math::Vec3::from_array(position),
        );
        frame["effect_models"][0]["model"]["world"] =
            json!(super::affine(world.to_cols_array_2d()));
        frame["particles"][0]["model"] = frame["effect_models"][0]["model"].clone();
        frame["particles"][0]["angles"] = json!(angles);
    };
    for (trace, yaw) in [(&mut spinning_source, -70.), (&mut spinning_native, 40.)] {
        trace.setup["effect_random_births"][0]["angles"][1] = angle_range.clone();
        trace.setup["effect_random_births"][0]["angular_velocity"] = json!([4., 5., 6.]);
        trace.setup["effect_random_births"][0]["template"]["modifier_words"] = json!([
            11, 52, 900, 0, 11, 60, 900, 0, 11, 88, 1800, 0, 11, 92, 1800, 0, 11, 96, 1800, 0, 2,
            32764, 2, 0, 12, 212, 32764, 0, 65535
        ]);
        for (index, frame) in trace.frames[..2].iter_mut().enumerate() {
            let age = index + 1;
            let mut angles: [f32; 3] =
                serde_json::from_value(frame["particles"][0]["angles"].clone()).unwrap();
            angles[1] = yaw + 5. * age as f32;
            set_spin_angles(frame, angles);
            frame["particles"][0]["age"] = json!(age);
        }
    }
    equivalent(&spinning_source, &spinning_native)
        .expect("independent XYZ draws retain authored spin");
    for failure in ["spin", "age", "slot", "scale", "translation"] {
        let mut wrong: Trace = spinning_native.clone();
        match failure {
            "spin" => {
                let mut angles: [f32; 3] =
                    serde_json::from_value(wrong.frames[1]["particles"][0]["angles"].clone())
                        .unwrap();
                angles[1] += 1.;
                set_spin_angles(&mut wrong.frames[1], angles);
            }
            "age" => wrong.frames[1]["particles"][0]["age"] = json!(3),
            "slot" => wrong.frames[1]["particles"][0]["model_slot"] = json!(3),
            "scale" | "translation" => {
                let index = if failure == "scale" { 0 } else { 7 };
                let value = wrong.frames[1]["effect_models"][0]["model"]["world"][index]
                    .as_f64()
                    .unwrap();
                wrong.frames[1]["effect_models"][0]["model"]["world"][index] = json!(value + 1.);
                wrong.frames[1]["particles"][0]["model"] =
                    wrong.frames[1]["effect_models"][0]["model"].clone();
            }
            _ => unreachable!(),
        }
        assert!(
            equivalent(&spinning_source, &wrong).is_err(),
            "XYZ {failure} must remain checked"
        );
    }
    for field in ["angles", "model_slot"] {
        let mut missing: Trace = spinning_source.clone();
        missing.frames[0]["particles"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            equivalent(&missing, &spinning_native).is_err(),
            "missing actual XYZ {field}"
        );
    }
    for factor in [0., -1., 1.5] {
        let mut scaled: Trace = choice_native.clone();
        for row in 0..3 {
            let value = scaled.frames[1]["effect_models"][0]["model"]["world"][row * 4 + 1]
                .as_f64()
                .unwrap();
            scaled.frames[1]["effect_models"][0]["model"]["world"][row * 4 + 1] =
                json!(value * factor);
        }
        scaled.frames[1]["particles"][0]["model"] =
            scaled.frames[1]["effect_models"][0]["model"].clone();
        assert!(equivalent(&choice_source, &scaled).is_err());
        if factor <= 0. {
            assert!(effect_pitch::validate(&scaled, &scaled).is_err());
        }
    }
    let mut zero_choice: Trace = choice_source.clone();
    let words = zero_choice.setup["effect_random_births"][0]["template"]["modifier_words"]
        .as_array_mut()
        .unwrap();
    let start = words.len() - 1;
    words.splice(
        start..start,
        [
            11, 32760, 1, 0, 8, 176, 0, 0, 32760, 0, 8, 180, 0, 0, 32760, 0, 8, 184, 0, 0, 32760, 0,
        ]
        .map(Value::from),
    );
    assert!(equivalent(&zero_choice, &choice_native).is_ok());
    for (offset, value) in [(2, 2), (4, 10), (5, 212), (6, 1), (8, 32764)] {
        let words = &mut zero_choice.setup["effect_random_births"][0]["template"]["modifier_words"];
        let original = words[start + offset].clone();
        words[start + offset] = json!(value);
        assert!(equivalent(&zero_choice, &choice_native).is_err());
        zero_choice.setup["effect_random_births"][0]["template"]["modifier_words"]
            [start + offset] = original;
    }
    zero_choice.frames[1]["effect_models"][0]["model"]["world"][0] = json!(1.);
    zero_choice.frames[1]["particles"][0]["model"] =
        zero_choice.frames[1]["effect_models"][0]["model"].clone();
    assert!(equivalent(&zero_choice, &choice_native).is_err());
    let mut added_choice: Trace = choice_source.clone();
    let mut fixed_native: Trace = choice_native.clone();
    added_choice.setup["effect_random_births"][0]["model"] = json!({"min":3.,"max":3.});
    added_choice.setup["effect_random_births"][0]["template"]["modifier_words"] = json!([
        11, 52, 900, 0, 11, 60, 900, 0, 11, 32760, 1800, 0, 8, 88, 0, 0, 32760, 0, 11, 96, 1800, 0,
        0, 32764, 3, 0, 10, 212, 32764, 0, 65535
    ]);
    for frame in &mut fixed_native.frames[..2] {
        frame["particles"][0]["model_slot"] = json!(3);
    }
    assert!(equivalent(&added_choice, &fixed_native).is_ok());
    added_choice.setup["effect_random_births"][0]["template"]["modifier_words"][16] = json!(32764);
    assert!(equivalent(&added_choice, &fixed_native).is_err());
    added_choice.setup["effect_random_births"][0]["template"]["modifier_words"][16] = json!(32760);
    fixed_native.frames[1]["particles"][0]["model_slot"] = json!(4);
    assert!(equivalent(&added_choice, &fixed_native).is_err());
    let mut assigned_choice: Trace = choice_source.clone();
    let mut assigned_native: Trace = choice_native.clone();
    assigned_choice.setup["effect_random_births"][0]["model"] =
        json!({"min":0.,"max":1.,"step":1.});
    assigned_choice.setup["effect_random_births"][0]["template"]["modifier_words"][20] = json!(10);
    for frame in &mut assigned_choice.frames[..2] {
        frame["particles"][0]["model_slot"] = json!(0);
    }
    for frame in &mut assigned_native.frames[..2] {
        frame["particles"][0]["model_slot"] = json!(1);
    }
    assert!(equivalent(&assigned_choice, &assigned_native).is_ok());
    assigned_choice.setup["effect_random_births"][0]["model"] =
        json!({"min":1.,"max":2.,"step":1.});
    assert!(equivalent(&assigned_choice, &assigned_native).is_err());
    // Simultaneous births may have identical contracts and different container
    // order. Match complete histories; identical per-frame multisets alone do
    // not permit switching identity midway through a lifetime.
    let mut simultaneous = choice_source.clone();
    let mut descriptor = choice_source.setup["effect_random_births"][0].clone();
    descriptor["lifetime"] = json!(3);
    let mut second = descriptor.clone();
    second["ordinal"] = json!(2);
    simultaneous.setup["effect_random_births"] = json!([descriptor, second]);
    simultaneous.frames.clear();
    for index in 0..4 {
        let mut frame = make().frames.remove(0);
        frame["combat_tick"] = json!(4 + index);
        frame["presentation_tick"] = json!(10 + index);
        frame["effect_models"] = json!([]);
        frame["particles"] = json!([]);
        if index < 3 {
            for (instance, prototype) in [&choice_source, &choice_native].into_iter().enumerate() {
                let mut model = prototype.frames[index.min(1)]["effect_models"][0].clone();
                let mut particle = prototype.frames[index.min(1)]["particles"][0].clone();
                let height = if index == 0 {
                    0
                } else {
                    index * 30 + instance * 20
                };
                model["ordinal"] = json!(instance + 1);
                model["model"]["world"][7] = json!(height);
                particle["id"] = json!(instance + 1);
                particle["offset"][1] = json!(height);
                particle["model"] = model["model"].clone();
                frame["effect_models"].as_array_mut().unwrap().push(model);
                frame["particles"].as_array_mut().unwrap().push(particle);
            }
        }
        simultaneous.frames.push(frame);
    }
    let mut permuted: Trace = simultaneous.clone();
    for frame in &mut permuted.frames[..3] {
        for model in frame["effect_models"].as_array_mut().unwrap() {
            model["ordinal"] = json!(3 - model["ordinal"].as_u64().unwrap());
        }
        for particle in frame["particles"].as_array_mut().unwrap() {
            particle["id"] = json!(10 + particle["id"].as_u64().unwrap());
        }
        frame["effect_models"].as_array_mut().unwrap().reverse();
        frame["particles"].as_array_mut().unwrap().reverse();
    }
    equivalent(&simultaneous, &permuted).expect("simultaneous whole-lifetime permutation");
    let mut exact_source: Trace = simultaneous.clone();
    let mut exact_native: Trace = permuted.clone();
    exact_source
        .setup
        .as_object_mut()
        .unwrap()
        .remove("effect_random_births");
    equivalent(&exact_source, &exact_native)
        .expect("exact detached lifetime permutation without random metadata");
    // Preserve the per-frame multiset while exchanging identity midway: this
    // must still fail when no random birth normalization has been admitted.
    for (instance, height) in [60, 80].into_iter().enumerate() {
        exact_native.frames[2]["effect_models"][instance]["model"]["world"][7] = json!(height);
    }
    assert!(equivalent(&exact_source, &exact_native).is_err());
    exact_native.frames = permuted.frames.clone();
    exact_native.frames[1]["effect_models"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(equivalent(&exact_source, &exact_native).is_err());
    exact_native.frames = permuted.frames.clone();
    exact_native.frames[2]["effect_models"][0]["model"]["world"][0] = json!(0.7);
    assert!(equivalent(&exact_source, &exact_native).is_err());
    let mut identical: Trace = simultaneous.clone();
    let mut identical_permuted: Trace = permuted.clone();
    for trace in [&mut identical, &mut identical_permuted] {
        for (index, frame) in trace.frames[..3].iter_mut().enumerate() {
            for instance in 0..2 {
                frame["effect_models"][instance]["model"]["world"][7] = json!(index * 30);
                frame["particles"][instance]["offset"][1] = json!(index * 30);
                frame["particles"][instance]["model"] =
                    frame["effect_models"][instance]["model"].clone();
            }
        }
    }
    equivalent(&identical, &identical_permuted).expect("interchangeable complete histories");
    let accepted_permutation = permuted.frames.clone();
    for (instance, height) in [60, 80].into_iter().enumerate() {
        permuted.frames[2]["effect_models"][instance]["model"]["world"][7] = json!(height);
        permuted.frames[2]["particles"][instance]["offset"][1] = json!(height);
        permuted.frames[2]["particles"][instance]["model"] =
            permuted.frames[2]["effect_models"][instance]["model"].clone();
    }
    assert!(equivalent(&simultaneous, &permuted).is_err());
    permuted.frames = accepted_permutation;
    permuted.frames[1]["effect_models"]
        .as_array_mut()
        .unwrap()
        .pop();
    permuted.frames[1]["particles"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(equivalent(&simultaneous, &permuted).is_err());
    let motion_trace = |velocity: f32| {
        let mut trace = choice_source.clone();
        trace.setup["effect_random_births"] = json!((0..2).map(|index| {
            let yaw = index as f32 * 120.;
            json!({"owner":0,"bank":bank,"member":25,"ordinal":index+1,
                "birth_clock":0,"program_clock":0,"heading":0.,"origin":[0.,0.,0.],
                "first_step":[-90.,yaw,0.],"first_offset":[0.,0.,0.],
                "offsets":[null,null,null],"angles":[null,null,null],
                "angle_additions":if index == 1 { Some([0.,120.,0.]) } else { None },
                "vertical_velocity":if index == 1 { Some(json!({"min":-0.1,"max":0.1,"step":0.1})) } else { None },
                "model":{"min":0.,"max":0.},"lifetime":3,
                "template":{"path":"source.bank","sha256":bank,"member":25,"program":21,
                    "modifier_words":if index == 1 { vec![8,92,17136,0,0,0,11,32760,2,0,8,68,0,0,32760,0,65535] } else { vec![65535] }}})
        }).collect::<Vec<_>>());
        trace.frames.clear();
        for (index, age) in [1, 1, 2].into_iter().enumerate() {
            let mut frame = make().frames.remove(0);
            frame["combat_tick"] = json!(4 + index);
            frame["presentation_tick"] = json!(10 + index);
            let mut models = Vec::new();
            let mut particles = Vec::new();
            for ordinal in 1..=2 {
                let angles = [-90_f32, (ordinal - 1) as f32 * 120., 0.];
                let offset = [
                    0.,
                    if ordinal == 2 {
                        velocity * age as f32
                    } else {
                        0.
                    },
                    0.,
                ];
                let [x, y, z] = angles.map(f32::to_radians);
                let world = bevy::math::Mat4::from_rotation_translation(
                    bevy::math::Quat::from_euler(bevy::math::EulerRot::ZYX, z, y, x),
                    bevy::math::Vec3::from_array(offset),
                );
                let model = json!({"clip":null,"frame":null,"world":super::affine(world.to_cols_array_2d())});
                models.push(json!({"owner":0,"bank":bank,"member":25,"ordinal":ordinal,"attachment":null,"model":model}));
                particles.push(json!({"id":ordinal,"owner":0,"bank":bank,"member":25,"initialized":true,
                    "origin":[0.,0.,0.],"offset":offset,"angles":angles,"age":age,"model_slot":0,"model":model}));
            }
            frame["effect_models"] = json!(models);
            frame["particles"] = json!(particles);
            trace.frames.push(frame);
        }
        let mut retired = make().frames.remove(0);
        retired["combat_tick"] = json!(7);
        retired["presentation_tick"] = json!(13);
        retired["effect_models"] = json!([]);
        retired["particles"] = json!([]);
        trace.frames.push(retired);
        trace
    };
    let moving_source = motion_trace(0.1);
    let mut moving_native = motion_trace(-0.1);
    equivalent(&moving_source, &moving_native).expect("qualified vertical birth motion");
    let mut retained_source = motion_trace(0.1);
    for (index, choice) in retained_source.setup["effect_random_births"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        choice["static_binding"] = json!({"angles":[-90.,index as f32 * 120.,0.],"model":0});
    }
    for frame in &mut retained_source.frames {
        for particle in frame["particles"].as_array_mut().unwrap() {
            particle.as_object_mut().unwrap().remove("angles");
            particle.as_object_mut().unwrap().remove("model_slot");
        }
    }
    let mut observed_native = motion_trace(-0.1);
    observed_native.setup = retained_source.setup.clone();
    equivalent(&retained_source, &observed_native).expect("deterministic source binding");
    observed_native.frames[0]["particles"][0]
        .as_object_mut()
        .unwrap()
        .remove("model_slot");
    assert!(equivalent(&retained_source, &observed_native).is_err());
    retained_source.frames[1]["effect_models"][0]["model"]["world"][0] = json!(2.);
    retained_source.frames[1]["particles"][0]["model"] =
        retained_source.frames[1]["effect_models"][0]["model"].clone();
    assert!(equivalent(&retained_source, &motion_trace(-0.1)).is_err());
    moving_native.frames[1]["particles"][1]["age"] = json!(2);
    assert!(equivalent(&moving_source, &moving_native).is_err());
    moving_native = motion_trace(-0.1);
    moving_native.frames[2]["particles"][1]["offset"][1] = json!(-0.22);
    moving_native.frames[2]["effect_models"][1]["model"]["world"][7] = json!(-0.22);
    moving_native.frames[2]["particles"][1]["model"] =
        moving_native.frames[2]["effect_models"][1]["model"].clone();
    assert!(equivalent(&moving_source, &moving_native).is_err());
    assert!(equivalent(&moving_source, &motion_trace(0.05)).is_err());
    let accepted_choices = choice_native.frames.clone();
    choice_native.frames[1]["particles"][0]["model_slot"] = json!(3);
    assert!(equivalent(&choice_source, &choice_native).is_err());
    choice_native.frames = accepted_choices.clone();
    choice_native.frames[1]["particles"][0]["id"] = json!(30);
    assert!(equivalent(&choice_source, &choice_native).is_err());
    choice_native.frames = accepted_choices.clone();
    choice_native.frames[0]["particles"][0]["angles"][0] = json!(34.05);
    assert!(equivalent(&choice_source, &choice_native).is_err());
    choice_native.frames = accepted_choices.clone();
    choice_native.frames[1]["effect_models"][0]["model"]["world"][7] = json!(21.);
    choice_native.frames[1]["particles"][0]["model"] =
        choice_native.frames[1]["effect_models"][0]["model"].clone();
    assert!(equivalent(&choice_source, &choice_native).is_err());
    choice_source.setup["effect_random_births"][0]["angles"][0]["draw"] = json!(0);
    choice_native.frames = accepted_choices;
    assert!(equivalent(&choice_source, &choice_native).is_err());
    // Independent modifier blocks may be authored in another order. Retained
    // raw angles distinguish full-turn draws without relaxing the matrix checks.
    choice_source.setup["effect_random_births"][0]["angles"][0] =
        json!({"min":-359.9,"max":359.9,"step":0.1});
    choice_source.setup["effect_random_births"][0]["template"]["modifier_words"] = json!([
        2, 32764, 2, 0, 12, 212, 32764, 0, 11, 96, 1800, 0, 11, 88, 3600, 0, 11, 60, 900, 0, 11,
        52, 900, 0, 65535
    ]);
    for frame in &mut choice_source.frames[..2] {
        let angle = frame["particles"][0]["angles"][0].as_f64().unwrap();
        frame["particles"][0]["angles"][0] = json!(angle + 360.);
    }
    assert!(equivalent(&choice_source, &choice_native).is_ok());
    choice_source.frames[0]["particles"][0]["angles"][0] = json!(364.);
    assert!(equivalent(&choice_source, &choice_native).is_err());
    choice_source.frames[0]["particles"][0]["angles"][0] = json!(354.);
    let reordered =
        choice_source.setup["effect_random_births"][0]["template"]["modifier_words"].clone();
    choice_source.setup["effect_random_births"][0]["template"]["modifier_words"][9] = json!(88);
    assert!(equivalent(&choice_source, &choice_native).is_err());
    choice_source.setup["effect_random_births"][0]["template"]["modifier_words"] = reordered;
    choice_native.frames[1]["effect_models"][0]["model"]["world"][0] = json!(0.7);
    choice_native.frames[1]["particles"][0]["model"] =
        choice_native.frames[1]["effect_models"][0]["model"].clone();
    assert!(equivalent(&choice_source, &choice_native).is_err());
    // Each flight owns its random launch and ground pitch; neither may excuse
    // an incorrect later position, disarm state, or model transform.
    let flight = |shifted: bool| {
        let mut trace = make();
        trace.setup["projectile_launches"] = json!([{
            "projectile":1,"template":{"kind":"projectiles","content":projectile_random::test_evidence("battle/projectiles.json", &bank),"index":0},
            "origin":[0.,3.,0.],"heading":0.,"velocity":[0.,-2.,1.],"acceleration":[0.,0.,0.],
            "motion":{"face_velocity":true,"velocity_jitter":[1.,1.,1.],
                "response":{"bounce":false,"ricochet":true,"ground_ricochet":true,"restitution":0.75}},
            "clamp_ground":false,"lifetime":2,"survives_contact":true,
            "model":{"bank":bank,"member":2,"template":{"kind":"effects","content":projectile_random::test_evidence("effects.json", &bank),"index":2}}
        }]);
        trace.events[1]["bank"] = json!(bank);
        trace.events[2]["bank"] = json!(bank);
        trace.events[2]["member"] = json!(2);
        trace.events.push(json!({"kind":"projectile_started","projectile":1,"owner":0,"target":1,"action":1,"clock":0,"presentation_clock":0}));
        trace.events.push(
            json!({"kind":"projectile_expired","projectile":1,"clock":3,"presentation_clock":3}),
        );
        let samples = if shifted {
            [
                ([0.1, 1., 1.], [0.1, -2., 1.]),
                ([0.2, -1., 2.], [0.199007438, 9., 1.99007438]),
                (
                    [0.399007438, 8., 3.99007438],
                    [0.199007438, 8.7, 1.99007438],
                ),
            ]
        } else {
            [
                ([0., 1., 1.], [0., -2., 1.]),
                ([0., -1., 2.], [0., 9., 2.]),
                ([0., 8., 4.], [0., 8.7, 2.]),
            ]
        };
        trace.frames.clear();
        for tick in 0..4 {
            let mut frame = make().frames.remove(0);
            frame["combat_tick"] = json!(4 + tick);
            frame["presentation_tick"] = json!(10 + tick);
            frame["effect_models"] = json!([]);
            if let Some(sample) = samples.get(tick) {
                let (position, velocity): ([f64; 3], [f64; 3]) = *sample;
                let yaw = if shifted { 0.1_f64.atan() } else { 0. };
                let pitch = if tick == 0 {
                    (-90.
                        - velocity[1]
                            .atan2(velocity[0].hypot(velocity[2]))
                            .to_degrees())
                    .to_radians()
                } else {
                    ((if shifted { 270. } else { 120. }) - 6. * tick as f64).to_radians()
                };
                let world = bevy::math::Mat4::from_rotation_translation(
                    bevy::math::Quat::from_euler(
                        bevy::math::EulerRot::YXZ,
                        yaw as f32,
                        pitch as f32,
                        0.,
                    ),
                    bevy::math::Vec3::from_array(position.map(|value| value as f32)),
                );
                frame["projectiles"] = json!([{"id":1,"owner":0,"target":1,"age":tick+1,
                    "position":position,"velocity":velocity,"disarmed":tick>0,"contact_active":tick==0}]);
                frame["effect_models"] = json!([{"owner":0,"bank":bank,"member":2,"ordinal":1,
                    "attachment":{"kind":"projectile","projectile":1},
                    "model":{"clip":null,"frame":null,"world":super::affine(world.to_cols_array_2d())}}]);
            }
            trace.frames.push(frame);
        }
        trace
    };
    assert!(equivalent(&flight(false), &flight(true)).is_ok());
    for (field, value) in [
        ("disarmed", json!(false)),
        ("position", json!([0.2, 0., 2.])),
    ] {
        let mut wrong = flight(true);
        wrong.frames[1]["projectiles"][0][field] = value;
        assert!(projectile_random::validate(&wrong, &wrong).is_err());
    }
    let mut wrong = flight(true);
    wrong.frames[2]["effect_models"][0]["model"]["world"][3] = json!(3.);
    assert!(projectile_random::validate(&wrong, &wrong).is_err());
    wrong = flight(true);
    let pitch = 264.05_f64.to_radians();
    wrong.frames[1]["effect_models"][0]["model"]["world"][5] = json!(pitch.cos());
    wrong.frames[1]["effect_models"][0]["model"]["world"][6] = json!(-pitch.sin());
    assert!(projectile_random::validate(&wrong, &wrong).is_err());
    wrong = flight(true);
    wrong
        .events
        .push(json!({"kind":"hit","projectile":1,"owner":0,"target":1,"hp_change":-1,"clock":1}));
    assert!(projectile_random::validate(&wrong, &wrong).is_err());
    // A missed shot may retire at the first arena exit, but neither before
    // crossing that boundary nor after publishing an additional live sample.
    let mut boundary = flight(false);
    boundary.setup["projectile_launches"][0]["origin"] = json!([0., -30., 0.]);
    boundary.setup["projectile_launches"][0]["velocity"] = json!([0., -10., 1.]);
    boundary.setup["projectile_launches"][0]["motion"] = json!({"velocity_jitter":[1.,1.,1.]});
    boundary.setup["projectile_launches"][0]["lifetime"] = json!(10);
    boundary.setup["projectile_launches"][0]["model"] = Value::Null;
    let retirement = boundary
        .events
        .iter()
        .position(|event| event["kind"] == "projectile_expired")
        .unwrap();
    boundary.events[retirement]["clock"] = json!(2);
    for (tick, frame) in boundary.frames.iter_mut().enumerate() {
        frame["effect_models"] = json!([]);
        frame["projectiles"] = if tick < 2 {
            json!([{"id":1,"owner":0,"target":1,"age":tick+1,
                "position":[0.,-40.-10.*tick as f64,1.+tick as f64],
                "velocity":[0.,-10.,1.],"disarmed":false}])
        } else {
            json!([])
        };
    }
    assert!(projectile_random::validate(&boundary, &boundary).is_ok());
    let mut premature: Trace = boundary.clone();
    premature.events[retirement]["clock"] = json!(1);
    premature.frames[1]["projectiles"] = json!([]);
    assert!(projectile_random::validate(&premature, &premature).is_err());
    let mut late = boundary;
    late.events[retirement]["clock"] = json!(3);
    late.frames[2]["projectiles"] = json!([{"id":1,"owner":0,"target":1,"age":3,
        "position":[0.,-60.,3.],"velocity":[0.,-10.,1.],"disarmed":false}]);
    assert!(projectile_random::validate(&late, &late).is_err());
    let mut incomplete = make();
    incomplete
        .incomplete_observations
        .push("effect identity unavailable".into());
    assert!(equivalent(&incomplete, &make()).is_err());
    let mut empty = make();
    empty.frames.clear();
    assert!(equivalent(&empty, &make()).is_err());
    let mut unobserved = make();
    unobserved.events.truncate(1);
    assert!(equivalent(&unobserved, &unobserved).is_err());
    unobserved = make();
    unobserved.frames[0]["actors"] = json!([]);
    assert!(equivalent(&unobserved, &unobserved).is_err());
    let mut wrong = make();
    wrong.events[2]["member"] = json!(46);
    assert!(equivalent(&make(), &wrong).is_err());
    wrong = make();
    wrong.events.remove(2);
    assert!(equivalent(&make(), &wrong).is_err());
    wrong = make();
    wrong.frames[0]["actors"][0]["position"][0] = json!(6.0);
    assert!(equivalent(&make(), &wrong).is_err());
    wrong = make();
    wrong.frames[0]["models"][0]["clip"] = json!(2);
    assert!(equivalent(&make(), &wrong).is_err());
    let mut secondary = make();
    secondary.setup["secondary_bodies"] = json!([{"actor":1}]);
    secondary.frames[0]["secondary_models"] =
        json!([{"actor":1,"clip":36,"frame":13.,"visible":true}]);
    assert!(equivalent(&secondary, &secondary).is_ok());
    for (field, value) in [
        ("clip", json!(37)),
        ("frame", json!(14.)),
        ("visible", json!(false)),
    ] {
        let mut wrong = secondary.clone();
        wrong.frames[0]["secondary_models"][0][field] = value;
        assert!(equivalent(&secondary, &wrong).is_err());
    }
    secondary.frames[0]["secondary_models"] = json!([]);
    assert!(equivalent(&secondary, &secondary).is_err());
    let mut lingering = make();
    lingering.frames[0]["particles"] = json!([{"member":30}]);
    assert!(equivalent(&make(), &lingering).is_err());
    assert!(equivalent(&lingering, &lingering).is_err());
    let mut random = make();
    random.frames[0]["particles"] = json!([]);
    random.frames[0]["actors"][1]["hp"] = json!(39);
    random.events[3]["hp_change"] = json!(-11);
    random.events[3]
        .as_object_mut()
        .unwrap()
        .remove("damage_range");
    assert!(equivalent(&make(), &random).is_ok());
    random.events[3]["hp_change"] = json!(-13);
    assert!(equivalent(&make(), &random).is_err());
    let mut unstunned = make();
    unstunned.events[3]["hp_change"] = json!(-43);
    unstunned.events[3]["damage_range"] = json!([42, 68]);
    unstunned.events[3]["damage_branches"] = json!({"ordinary":[42,45],"critical":[63,68]});
    let mut stunned = make();
    stunned.setup["contact_damage_observations"] = json!(true);
    stunned.setup["contact_damage_normalized"] = json!(true);
    stunned.events[3]["damage_inputs"] = json!({"context":{"target_accuracy_override":true}});
    stunned.events[3]["hp_change"] = json!(-52);
    stunned.events[3]["damage_range"] = json!([48, 79]);
    stunned.events[3]["damage_branches"] = json!({"ordinary":[48,52],"critical":[73,79]});
    assert!(contact_events(&unstunned, &stunned).is_ok());
    for damage in [-47, -53, -72] {
        let mut wrong = stunned.clone();
        wrong.events[3]["hp_change"] = json!(damage);
        assert!(contact_events(&unstunned, &wrong).is_err());
    }
    stunned.events[3]["critical"] = json!(true);
    assert!(contact_events(&unstunned, &stunned).is_err());
    stunned.events[3]["critical"] = json!(false);
    stunned.setup["contact_damage_normalized"] = json!(false);
    assert!(contact_events(&unstunned, &stunned).is_err());
    let mut protected = make();
    protected.setup["actors"][1]["side"] = json!(0);
    protected
        .events
        .push(json!({"kind":"hit","owner":0,"target":1,"projectile":null,
        "action":1,"hp_change":0,"protection":"recovery","damage_range":[0,0],
        "damage_branches":{"ordinary":[0,0],"critical":null},"critical":false,
        "clock":0,"presentation_clock":0}));
    assert!(contact_events(&protected, &protected).is_ok());
    for (field, value) in [
        ("protection", Value::Null),
        ("protection", json!("down")),
        ("hp_change", json!(-1)),
        ("damage_range", json!([0, 1])),
        ("damage_branches", json!({"ordinary":[0,1],"critical":null})),
        (
            "damage_branches",
            json!({"ordinary":[0,0],"critical":[0,0]}),
        ),
        ("critical", json!(true)),
    ] {
        let mut wrong = protected.clone();
        wrong.events.last_mut().unwrap()[field] = value;
        assert!(contact_events(&wrong, &protected).is_err());
        if matches!(field, "protection" | "hp_change" | "critical") {
            assert!(contact_events(&protected, &wrong).is_err());
        }
    }
    protected.setup["actors"][1]["side"] = json!(1);
    assert!(contact_events(&protected, &protected).is_err());
    protected.setup["actors"][1]["side"] = json!(0);
    protected
        .events
        .last_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("protection");
    assert!(contact_events(&protected, &protected).is_err());
    let mut impact = make();
    impact.setup["knockdown_impacts"] = json!(true);
    for tick in 1..=2 {
        let mut frame = impact.frames[0].clone();
        frame["combat_tick"] = json!(4 + tick);
        frame["presentation_tick"] = json!(10 + tick);
        frame["actors"][1]["hp"] = json!(if tick == 2 { 38 } else { 40 });
        impact.frames.push(frame);
    }
    impact.events.push(json!({"kind":"knockdown_impact","owner":0,"target":1,"action":1,
        "amount":2,"contact_clock":0,"contact_presentation_clock":0,"clock":2,"presentation_clock":2}));
    let mut own_damage = impact.clone();
    own_damage.events[3]["hp_change"] = json!(-12);
    own_damage.events.last_mut().unwrap()["amount"] = json!(3);
    for (index, frame) in own_damage.frames.iter_mut().enumerate() {
        frame["actors"][1]["hp"] = json!(if index == 2 { 35 } else { 38 });
    }
    assert!(equivalent(&impact, &own_damage).is_ok());
    own_damage.events.last_mut().unwrap()["amount"] = json!(4);
    assert!(equivalent(&impact, &own_damage).is_err());
    own_damage.events.last_mut().unwrap()["amount"] = json!(3);
    own_damage.frames[2]["actors"][1]["hp"] = json!(34);
    assert!(equivalent(&impact, &own_damage).is_err());
    let mut simultaneous = impact.events.clone();
    let mut hit = simultaneous[3].clone();
    hit["clock"] = json!(2);
    hit["presentation_clock"] = json!(2);
    simultaneous.push(hit);
    impact.frames[2]["actors"][1]["hp"] = json!(28);
    assert!(validate_contact_outcomes(&impact, &simultaneous).is_ok());
    impact.frames[2]["actors"][1]["hp"] = json!(27);
    assert!(validate_contact_outcomes(&impact, &simultaneous).is_err());
    impact.frames[2]["actors"][1]["hp"] = json!(28);
    simultaneous.last_mut().unwrap()["clock"] = json!(3);
    assert!(validate_contact_outcomes(&impact, &simultaneous).is_err());
    let mut poison = make();
    poison.setup["actors"][1]["max_hp"] = json!(100);
    poison.setup["common_effect_bank"] = json!("poison-bank");
    let mut after = poison.frames[0].clone();
    after["combat_tick"] = json!(5);
    after["presentation_tick"] = json!(11);
    after["actors"][1]["hp"] = json!(39);
    poison.frames.push(after);
    poison.frames[0]["actors"][1]["hp"] = json!(40);
    poison.events.extend([
        json!({"kind":"condition_changed", "owner":0, "target":1, "action":1,
            "before":[], "after":["poison_mild"], "clock":0, "presentation_clock":0}),
        json!({"kind":"program_started", "owner":1, "bank":"poison-bank", "program":44,
            "clock":1, "presentation_clock":1}),
        json!({"kind":"poison_pulse", "owner":0, "target":1, "action":1,
            "condition":"poison_mild", "amount":1, "clock":1, "presentation_clock":1}),
    ]);
    assert!(validate_poison_pulses(&poison, &poison.events).is_ok());
    let application = poison.events.len() - 3;
    let unrelated = json!({"kind":"condition_changed", "owner":0, "target":1, "action":1,
        "before":["poison_mild"], "after":["poison_mild","paralysis"],
        "clock":0, "presentation_clock":1});
    poison.events.insert(application + 1, unrelated.clone());
    assert!(validate_poison_pulses(&poison, &poison.events).is_ok());
    poison.events[application + 1]["after"] = json!(["paralysis"]);
    assert!(validate_poison_pulses(&poison, &poison.events).is_err());
    poison.events[application + 1]["after"] = Value::Null;
    assert!(validate_poison_pulses(&poison, &poison.events).is_err());
    poison.events[application + 1] = unrelated;
    poison.events[application]["clock"] = json!(2);
    poison.events[application]["presentation_clock"] = json!(2);
    assert!(validate_poison_pulses(&poison, &poison.events).is_err());
    poison.events[application]["clock"] = json!(0);
    poison.events[application]["presentation_clock"] = json!(0);
    poison.events.last_mut().unwrap()["amount"] = json!(2);
    assert!(validate_poison_pulses(&poison, &poison.events).is_err());
    poison.events.last_mut().unwrap()["amount"] = json!(1);
    poison.frames[1]["actors"][1]["hp"] = json!(38);
    assert!(validate_poison_pulses(&poison, &poison.events).is_err());
    poison.frames[1]["actors"][1]["hp"] = json!(39);
    let feedback = poison.events.len() - 2;
    poison.events.remove(feedback);
    assert!(validate_poison_pulses(&poison, &poison.events).is_err());

    let mut drain = make();
    drain.case = "party:5:135".into();
    drain.setup["actors"][0]["max_hp"] = json!(60);
    drain.events[3]["damage_range"] = json!([1, 28]);
    drain.events[3]["damage_branches"] = json!({"ordinary":[1,28],"critical":null});
    let mut after = drain.frames[0].clone();
    after["combat_tick"] = json!(5);
    after["presentation_tick"] = json!(11);
    after["actors"][0]["hp"] = json!(51);
    drain.frames.push(after);
    drain.events.push(json!({"kind":"hp_drained","owner":0,"target":0,"action":1,
        "amount":1,"contact_clock":0,"contact_presentation_clock":0,"clock":1,"presentation_clock":1}));
    let mut own_drain = drain.clone();
    own_drain.events[3]["hp_change"] = json!(-20);
    own_drain.events.last_mut().unwrap()["amount"] = json!(2);
    for frame in &mut own_drain.frames {
        frame["actors"][1]["hp"] = json!(30);
    }
    own_drain.frames[1]["actors"][0]["hp"] = json!(52);
    assert!(equivalent(&drain, &own_drain).is_ok());
    own_drain.events.last_mut().unwrap()["amount"] = json!(1);
    assert!(equivalent(&drain, &own_drain).is_err());
    drain.setup["actors"][0]["max_hp"] = json!(51);
    own_drain.setup = drain.setup.clone();
    own_drain.frames[1]["actors"][0]["hp"] = json!(51);
    assert!(equivalent(&drain, &own_drain).is_ok());
    own_drain.events[3]["hp_change"] = json!(-1);
    for frame in &mut own_drain.frames {
        frame["actors"][1]["hp"] = json!(49);
    }
    assert!(equivalent(&drain, &own_drain).is_ok());
    own_drain.events.last_mut().unwrap()["contact_clock"] = json!(1);
    assert!(equivalent(&drain, &own_drain).is_err());
    own_drain.events.last_mut().unwrap()["contact_clock"] = json!(0);
    own_drain.events.last_mut().unwrap()["kind"] = json!("unattributed_hp_gain");
    assert!(equivalent(&drain, &own_drain).is_err());
    let mut tp_drain = make();
    tp_drain.case = "party:5:136".into();
    tp_drain.setup["actors"][0]["max_tp"] = json!(999);
    tp_drain.events[3]["hp_change"] = json!(-50);
    tp_drain.events[3]["damage_range"] = json!([47, 75]);
    tp_drain.events[3]["damage_branches"] = json!({"ordinary":[47,51],"critical":[70,75]});
    tp_drain.frames[0]["actors"][0]["tp"] = json!(984);
    tp_drain.frames[0]["actors"][1]["hp"] = json!(29950);
    for tick in 1..=2 {
        let mut frame = tp_drain.frames[0].clone();
        frame["combat_tick"] = json!(4 + tick);
        frame["presentation_tick"] = json!(10 + tick);
        frame["actors"][0]["tp"] = json!(985);
        tp_drain.frames.push(frame);
    }
    tp_drain.events.push(json!({"kind":"tp_drained","owner":0,"target":0,"action":1,
        "amount":1,"contact_clock":0,"contact_presentation_clock":0,"clock":1,"presentation_clock":1}));
    let mut own_tp = tp_drain.clone();
    own_tp.events[3]["hp_change"] = json!(-70);
    own_tp.events[3]["critical"] = json!(true);
    own_tp.events.last_mut().unwrap()["amount"] = json!(2);
    own_tp
        .events
        .push(json!({"kind":"program_started","owner":0,"bank":"common",
        "program":16,"clock":0,"presentation_clock":0}));
    for (index, frame) in own_tp.frames.iter_mut().enumerate() {
        frame["actors"][1]["hp"] = json!(29930);
        if index > 0 {
            frame["actors"][0]["tp"] = json!(986);
        }
    }
    assert!(equivalent(&tp_drain, &own_tp).is_ok());
    own_tp.frames[1]["actors"][0]["tp"] = json!(985);
    assert!(equivalent(&tp_drain, &own_tp).is_err());
    own_tp.frames[1]["actors"][0]["tp"] = json!(986);
    own_tp.frames[2]["actors"][0]["tp"] = json!(987);
    assert!(equivalent(&tp_drain, &own_tp).is_err());
    own_tp.frames[2]["actors"][0]["tp"] = json!(986);
    own_tp.frames[2]["actors"][1]["tp"] = json!(11);
    assert!(equivalent(&tp_drain, &own_tp).is_err());
    own_tp.frames[2]["actors"][1]["tp"] = json!(10);
    for (field, value) in [
        ("owner", json!(1)),
        ("target", json!(1)),
        ("contact_clock", json!(1)),
        ("amount", json!(7)),
    ] {
        let before = own_tp.events[6][field].clone();
        own_tp.events[6][field] = value;
        assert!(equivalent(&tp_drain, &own_tp).is_err());
        own_tp.events[6][field] = before;
    }
    own_tp.events[6]["amount"] = json!(7);
    for frame in &mut own_tp.frames[1..] {
        frame["actors"][0]["tp"] = json!(991);
    }
    assert!(equivalent(&tp_drain, &own_tp).is_err());
    own_tp.events[6]["amount"] = json!(2);
    for frame in &mut own_tp.frames[1..] {
        frame["actors"][0]["tp"] = json!(986);
    }
    own_tp.events.push(own_tp.events[6].clone());
    assert!(equivalent(&tp_drain, &own_tp).is_err());
    own_tp.events.pop();
    assert!(equivalent(&tp_drain, &own_tp).is_ok());
    let mut missing_range = make();
    missing_range.events[3]
        .as_object_mut()
        .unwrap()
        .remove("damage_range");
    assert!(equivalent(&missing_range, &missing_range).is_err());
    let mut critical = make();
    critical.events[3]["hp_change"] = json!(-16);
    critical.events[3]["critical"] = json!(true);
    let flash = json!({"kind":"program_started","owner":0,"bank":"common","program":16,"clock":0,"presentation_clock":0});
    critical.events.push(flash.clone());
    assert!(equivalent(&critical, &make()).is_ok());
    assert!(equivalent(&make(), &critical).is_ok());
    let mut missing = critical.clone();
    missing.events.pop();
    assert!(equivalent(&missing, &make()).is_err());
    let mut spurious = make();
    spurious.events.push(flash);
    assert!(equivalent(&make(), &spurious).is_err());
    for field in ["owner", "clock", "presentation_clock", "program", "bank"] {
        let mut misplaced = critical.clone();
        misplaced.events.last_mut().unwrap()[field] = json!(99);
        assert!(equivalent(&misplaced, &make()).is_err());
    }
    let mut ambiguous = make();
    ambiguous.events[3]["damage_branches"]["critical"] = json!([8, 12]);
    ambiguous.events[3]["critical"] = Value::Null;
    let mut native_critical = critical;
    native_critical.events[3]["hp_change"] = json!(-10);
    assert!(equivalent(&ambiguous, &make()).is_ok());
    assert!(equivalent(&ambiguous, &native_critical).is_err());
    for mode in ["normal", "special"] {
        let mut guarded = make();
        guarded.setup["recipient_input"] = json!({"actor":1,"mode":mode});
        guarded.events[3]["guard"] = json!(mode);
        let mut guarded_critical = guarded.clone();
        guarded_critical.events[3]["hp_change"] = json!(-16);
        assert!(equivalent(&guarded, &guarded_critical).is_ok());
        assert!(equivalent(&guarded_critical, &guarded).is_ok());
        guarded_critical.events[3]["hp_change"] = json!(-13);
        assert!(equivalent(&guarded, &guarded_critical).is_err());
        guarded_critical.events[3]["hp_change"] = json!(-16);
        guarded_critical.events[3]["critical"] = json!(true);
        assert!(equivalent(&guarded, &guarded_critical).is_err());
        guarded_critical.events[3]["critical"] = json!(false);
        guarded_critical.events[3]["guard"] = Value::Null;
        assert!(equivalent(&guarded, &guarded_critical).is_err());
    }
    for outcome in [json!("broken"), Value::Null] {
        let mut source = make();
        source.setup["recipient_input"] = json!({"actor":1,"mode":"normal"});
        source.events[3]["guard"] = outcome.clone();
        let mut native = source.clone();
        assert!(equivalent(&source, &native).is_ok());
        native.events[3]["guard"] = json!("normal");
        assert!(equivalent(&source, &native).is_err());
        for mode in ["attack", "stick"] {
            source.setup["recipient_input"]["mode"] = json!(mode);
            native = source.clone();
            assert_eq!(equivalent(&source, &native).is_ok(), outcome.is_null());
        }
    }
}
