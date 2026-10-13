use super::Trace;
use anyhow::{Context, Result, ensure};
use serde_json::Value;

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
        validate_execution(trace).with_context(|| label)?;
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
        source.frames.len() == native.frames.len(),
        "observation duration differs: {} / {} frames",
        source.frames.len(),
        native.frames.len()
    );
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
        for field in ["models", "projectiles", "effect_models"] {
            let observed = |frame: &Value| -> Result<Value> {
                let mut values = required(frame, field)?.clone();
                for value in values.as_array_mut().context("invalid observation list")? {
                    let other_actor = field == "models" && &value["actor"] != owner;
                    let object = value.as_object_mut().context("invalid observation")?;
                    if field == "models" {
                        object.remove("world");
                        // Unrelated idle animation phase is not part of the arte fixture.
                        // Every actor's selected reaction clip and world movement remain compared.
                        if other_actor {
                            object.remove("frame");
                        }
                    } else if field == "projectiles" {
                        // These counters describe different update phases. Actual trajectory,
                        // attributed contacts and retirement use the shared gameplay clock.
                        object.remove("age");
                        object.remove("contact_active");
                    }
                }
                Ok(values)
            };
            equal(
                &observed(source)?,
                &observed(native)?,
                &format!("frame {index}.{field}"),
            )?;
        }
        // Contact damage is checked against the source's rule/stat envelope below.
        // Comparing accumulated HP here would require identical random streams.
        let actors = |frame: &Value| -> Result<Value> {
            let mut actors = required(frame, "actors")?.clone();
            for actor in actors.as_array_mut().context("invalid actors")? {
                actor.as_object_mut().context("invalid actor")?.remove("hp");
            }
            Ok(actors)
        };
        equal(
            &actors(source)?,
            &actors(native)?,
            &format!("frame {index}.actors"),
        )?;
    }
    let final_particles = |trace: &Trace| -> Result<bool> {
        Ok(required(
            trace.frames.last().context("missing final frame")?,
            "particles",
        )?
        .as_array()
        .context("unknown final visible particles")?
        .is_empty())
    };
    let source_cleared = final_particles(source)?;
    let native_cleared = final_particles(native)?;
    ensure!(
        source_cleared && native_cleared,
        "visible particle cleanup not observed; persistent baselines require explicit observation"
    );
    equal(
        required(&source.assessment, "outcome")?,
        required(&native.assessment, "outcome")?,
        "outcome",
    )?;
    let (source_events, native_events) = contact_events(source, native)?;
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

fn canonical_events(events: &[Value]) -> Result<Vec<Value>> {
    let key = |event: &Value| -> Result<(u64, u64, String)> {
        let identity = event_identity(event)?;
        Ok((
            required(event, "clock")?
                .as_u64()
                .context("invalid event clock")?,
            required(event, "presentation_clock")?
                .as_u64()
                .context("invalid presentation clock")?,
            serde_json::to_string(&identity)?,
        ))
    };
    let mut ordered = events
        .iter()
        .cloned()
        .map(|event| Ok((key(&event)?, event)))
        .collect::<Result<Vec<_>>>()?;
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(ordered.into_iter().map(|(_, event)| event).collect())
}

// Validate each random damage branch before comparing deterministic events. Only
// the critical feedback justified by those actual outcomes may differ.
fn contact_events(source: &Trace, native: &Trace) -> Result<(Vec<Value>, Vec<Value>)> {
    let mut events = [
        canonical_events(&source.events)?,
        canonical_events(&native.events)?,
    ];
    let hits = events.each_ref().map(|events| {
        events
            .iter()
            .filter(|event| event["kind"] == "hit")
            .cloned()
            .collect::<Vec<_>>()
    });
    ensure!(
        hits[0].len() == hits[1].len(),
        "attributed contact count differs"
    );
    let key = |event: &Value| -> Result<(u64, u64, u64)> {
        Ok((
            required(event, "owner")?
                .as_u64()
                .context("invalid contact owner")?,
            required(event, "clock")?
                .as_u64()
                .context("invalid contact clock")?,
            required(event, "presentation_clock")?
                .as_u64()
                .context("invalid contact presentation clock")?,
        ))
    };
    // Count simultaneous hits together; each critical hit emits its own feedback.
    let mut feedback = std::collections::BTreeMap::new();
    for (source, native) in hits[0].iter().zip(&hits[1]) {
        let range = bounds(required(source, "damage_range")?)?;
        let branches = required(source, "damage_branches")?;
        let ordinary = bounds(required(branches, "ordinary")?)?;
        let critical = required(branches, "critical")?;
        let critical = (!critical.is_null())
            .then(|| bounds(critical))
            .transpose()?;
        let damage = |event: &Value| -> Result<f64> {
            let amount = -required(event, "hp_change")?
                .as_f64()
                .context("invalid contact damage")?;
            ensure!(
                amount > 0.0 && (range.0..=range.1).contains(&amount),
                "contact damage {amount} outside {}..={}",
                range.0,
                range.1
            );
            Ok(amount)
        };
        let source_amount = damage(source)?;
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
            observed.as_bool() == inferred,
            "source critical label disagrees with damage branches"
        );
        let native_critical = required(native, "critical")?
            .as_bool()
            .context("unknown native critical outcome")?;
        let native_range = if native_critical {
            critical.context("native critical hit has no legal critical branch")?
        } else {
            ordinary
        };
        let native_amount = damage(native)?;
        ensure!(
            (native_range.0..=native_range.1).contains(&native_amount),
            "native damage {native_amount} disagrees with its critical outcome"
        );
        equal(
            &event_identity(source)?,
            &event_identity(native)?,
            "attributed contact",
        )?;
        let group = feedback.entry(key(source)?).or_insert(([0usize; 2], true));
        group.0[0] += usize::from(inferred == Some(true));
        group.0[1] += usize::from(native_critical);
        group.1 &= inferred.is_some();
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
            let is_critical = |event: &Value| {
                event["kind"] == "program_started"
                    && &event["bank"] == common
                    && event["program"] == 16
            };
            for event in events.iter().filter(|event| is_critical(event)) {
                ensure!(
                    feedback.contains_key(&key(event)?),
                    "critical feedback has no matching attacker/contact clock"
                );
            }
            for (contact, (counts, known)) in &feedback {
                if side == 1 || *known {
                    let count = events
                        .iter()
                        .filter(|event| {
                            is_critical(event) && key(event).ok().as_ref() == Some(contact)
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
                        .get(&key(event).unwrap())
                        .is_some_and(|(_, known)| *known))
            });
        }
    }
    let [source, native] = events.map(|events| {
        events
            .iter()
            .map(event_identity)
            .collect::<Result<Vec<_>>>()
    });
    Ok((source?, native?))
}

fn event_identity(event: &Value) -> Result<Value> {
    let mut identity = event.clone();
    let object = identity.as_object_mut().context("invalid semantic event")?;
    if event["kind"] == "hit" {
        for field in ["hp_change", "damage_range", "critical", "damage_branches"] {
            object.remove(field);
        }
    }
    Ok(identity)
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
        !trace
            .events
            .iter()
            .any(|event| event["kind"] == "unattributed_hp_loss"),
        "unattributed HP loss cannot establish a matched fixture"
    );
    let owner = required(&trace.setup, "owner")?
        .as_u64()
        .context("invalid owner")?;
    let target = required(&trace.setup, "target")?
        .as_u64()
        .context("invalid target")?;
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
        for field in ["projectiles", "effect_models"] {
            ensure!(
                required(frame, field)?.is_array(),
                "unknown {field} observations"
            );
        }
        for effect in frame["effect_models"].as_array().unwrap() {
            ensure!(
                required(effect, "owner")?.is_u64() && required(effect, "member")?.is_u64(),
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
    if outcome != "departure" {
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
    ensure!(
        events.iter().any(|event| {
            let attributed = event["owner"].as_u64() == Some(owner)
                && (outcome == "departure" || event["target"].as_u64() == Some(target))
                && &event["action"] == action;
            attributed
                && match outcome {
                    "attack" | "aerial" => {
                        event["kind"] == "hit"
                            && event["hp_change"].as_i64().is_some_and(|hp| hp < 0)
                    }
                    "support" => {
                        (event["kind"] == "recovered"
                            && event["amount"].as_i64().is_some_and(|amount| amount > 0))
                            || (event["kind"] == "condition_changed"
                                && !event["before"].is_null()
                                && !event["after"].is_null()
                                && event["before"] != event["after"])
                    }
                    "revival" => event["kind"] == "revived",
                    "counter" => event["kind"] == "countered" || event["kind"] == "guarded",
                    "capture" => event["kind"] == "captured",
                    "departure" => event["kind"] == "departed",
                    _ => false,
                }
        }),
        "intended {outcome} outcome was not observed"
    );
    Ok(())
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

fn equal(source: &Value, native: &Value, path: &str) -> Result<()> {
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
        (Value::Number(source), Value::Number(native)) if source.is_f64() || native.is_f64() => {
            // The existing paired oracle resolves geometry to 0.01 game units.
            let difference = (source.as_f64().context("invalid source number")?
                - native.as_f64().context("invalid native number")?)
            .abs();
            ensure!(difference <= 0.01, "{path}: {source} != {native}");
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
        setup: json!({"formation":2,"arena":13,"owner":0,"target":1,"common_effect_bank":"common","actors":[{"actor":0,"conditions":[]},{"actor":1,"conditions":[]}]}),
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
    let mut missing = make();
    missing.events = critical.events.clone();
    missing.events.pop();
    assert!(equivalent(&missing, &make()).is_err());
    let mut spurious = make();
    spurious.events.push(flash);
    assert!(equivalent(&make(), &spurious).is_err());
    for field in ["owner", "clock", "presentation_clock", "program", "bank"] {
        let mut misplaced = make();
        misplaced.events = critical.events.clone();
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
}
