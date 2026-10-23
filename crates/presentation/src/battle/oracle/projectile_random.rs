//! Validate ballistic launch variation without requiring identical random draws.
use super::Trace;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_projectile::{ProjectileMotion, ProjectileOrigin};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
struct Template {
    path: String,
    sha256: String,
    index: u32,
}

impl Template {
    fn has_provenance(&self) -> bool {
        !self.path.is_empty()
            && self.sha256.len() == 64
            && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    }
}

#[derive(Deserialize)]
struct Launch {
    projectile: u64,
    template: Template,
    origin: [f64; 3],
    heading: f64,
    velocity: [f64; 3],
    acceleration: [f64; 3],
    motion: ProjectileMotion,
    clamp_ground: bool,
    lifetime: u32,
    #[serde(default)]
    survives_contact: bool,
    contact: Option<Contact>,
    authored_action: Option<AuthoredAction>,
    model: Option<RicochetModel>,
    trail: Option<Trail>,
    ground: Option<Trail>,
}

#[derive(Deserialize)]
struct Trail {
    bank: String,
    template: Template,
    #[serde(default)]
    interval: u64,
}

#[derive(Deserialize)]
struct FollowModel {
    bank: String,
    member: u64,
    template: Template,
    follow_origin: bool,
    follow_orientation: bool,
}

#[derive(Deserialize, PartialEq)]
struct Contact {
    cooldown: u64,
    repeat_limit: u64,
    active: Option<[u64; 2]>,
}

#[derive(Deserialize)]
struct AuthoredAction {
    path: String,
    sha256: String,
    index: usize,
    clock: u64,
}

#[derive(Deserialize)]
struct RicochetModel {
    bank: String,
    member: u64,
    template: Template,
}

#[derive(Default)]
pub(super) struct Validated {
    pub ids: BTreeSet<u64>,
    pub ricochets: BTreeSet<u64>,
    /// Only velocity-facing ricochets have their complete spin checked per run.
    spinning_ricochets: BTreeSet<u64>,
    /// Models face the velocity used for this movement, before acceleration.
    model_accelerations: BTreeMap<u64, [f64; 3]>,
    /// A floor clamp changes movement without changing the stored velocity.
    clamped_model_steps: BTreeMap<String, [f64; 3]>,
    /// Each side retires at its own independently checked boundary crossing.
    pub boundary_retirements: BTreeSet<u64>,
    /// Same recipient multiplicities, with each contact independently qualified.
    /// This never permits a hit in one run to replace a miss in the other.
    pub contact_timings: BTreeSet<u64>,
    pub model_lifetimes: BTreeSet<u64>,
    trail_events: [BTreeMap<String, usize>; 2],
    trail_model_origins: [BTreeMap<String, [f64; 3]>; 2],
}

/// A stationary area volume's stored aim actor is bookkeeping, not its hit recipient.
/// Admit only a uniquely registered scene whose entire authored projectile table
/// is target-independent. The ordinary comparison retains every trajectory,
/// birth, retirement and actual contact target.
pub(super) fn stationary_area_targets(source: &Trace, native: &Trace) -> Result<BTreeSet<u64>> {
    let ids = BTreeSet::new();
    let Some(registrations) = source.setup["stored_scenes"]["registrations"].as_array() else {
        return Ok(ids);
    };
    let [registration] = registrations.as_slice() else {
        return Ok(ids);
    };
    if registration["kind"] != "spell" || registration["physical_release"] != false {
        return Ok(ids);
    }
    let owner = &source.setup["owner"];
    let Some(slot) = source.setup["source_actor_map"]
        .as_array()
        .and_then(|actors| {
            actors
                .iter()
                .find(|actor| &actor["actor"] == owner)
                .map(|actor| &actor["source_slot"])
        })
    else {
        return Ok(ids);
    };
    if !source.setup["initial_battle"]["actors"]
        .as_array()
        .is_some_and(|actors| {
            actors
                .iter()
                .any(|actor| &actor["slot"] == slot && actor["address"] == registration["owner"])
        })
    {
        return Ok(ids);
    }
    ensure!(
        source.setup["stored_scenes"] == native.setup["stored_scenes"],
        "stationary scene registration differs"
    );
    let pin: super::random_inputs::Pin = serde_json::from_value(registration["scene"].clone())?;
    let scene: resonance_content::battle_scene::Scene = serde_json::from_slice(&pin.read_bytes()?)?;
    ensure!(
        registration["package_sha256"] == scene.source_sha256,
        "stationary scene package differs"
    );
    if scene.projectiles.is_empty()
        || !scene.projectiles.iter().all(|entry| {
            let definition = &entry.projectile;
            definition.unsupported_reason.is_none()
                && definition.lifetime.is_some_and(|lifetime| lifetime > 0)
                && stationary_motion(
                    definition.velocity,
                    definition.acceleration,
                    &definition.motion,
                )
                && definition.birth_effect.member == 0
                && definition.ground_effect.member == 0
                && definition.trail_effect.member == 0
        })
    {
        return Ok(ids);
    }
    let actions: Vec<_> = source
        .events
        .iter()
        .filter(|event| event["kind"] == "started" && &event["owner"] == owner)
        .collect();
    let [action] = actions.as_slice() else {
        return Ok(ids);
    };
    source
        .events
        .iter()
        .filter(|event| {
            event["kind"] == "projectile_started"
                && &event["owner"] == owner
                && event["action"] == action["action"]
        })
        .map(|event| integer(event, "projectile"))
        .collect()
}

fn stationary_motion(
    velocity: [f32; 3],
    acceleration: [f32; 3],
    motion: &ProjectileMotion,
) -> bool {
    velocity == [0.; 3] && acceleration == [0.; 3] && *motion == ProjectileMotion::default()
}

/// Called only on projectile frame records or semantic events. Actual hit targets
/// are deliberately retained: they are the recipients resolved by area contact.
pub(super) fn canonicalize_stored_target(value: &mut Value, ids: &BTreeSet<u64>) {
    let id = match value.get("kind").and_then(Value::as_str) {
        None => value["id"].as_u64(),
        Some("projectile_started") => value["projectile"].as_u64(),
        Some(_) => None,
    };
    if id.is_some_and(|id| ids.contains(&id))
        && let Some(object) = value.as_object_mut()
    {
        object.remove("target");
    }
}

#[cfg(test)]
mod stationary_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stationary_target_bookkeeping_keeps_actual_hits_motion_and_retirement_strict() {
        let ids = BTreeSet::from([1]);
        for mut value in [
            json!({"id":1,"target":2,"position":[0.,0.,0.]}),
            json!({"kind":"projectile_started","projectile":1,"target":2,"clock":0}),
        ] {
            let mut other = value.clone();
            other["target"] = json!(3);
            canonicalize_stored_target(&mut value, &ids);
            canonicalize_stored_target(&mut other, &ids);
            assert_eq!(value, other);
        }
        for (mut original, mut wrong) in [
            (
                json!({"kind":"hit","projectile":1,"target":2}),
                json!({"kind":"hit","projectile":1,"target":3}),
            ),
            (
                json!({"id":1,"position":[0.,0.,0.]}),
                json!({"id":1,"position":[1.,0.,0.]}),
            ),
            (
                json!({"kind":"projectile_expired","projectile":1,"clock":5}),
                json!({"kind":"projectile_expired","projectile":1,"clock":6}),
            ),
        ] {
            canonicalize_stored_target(&mut original, &ids);
            canonicalize_stored_target(&mut wrong, &ids);
            assert!(super::super::compare::equal(&original, &wrong, "projectile").is_err());
        }
        let mut directed = json!({"id":2,"target":3});
        canonicalize_stored_target(&mut directed, &ids);
        assert_eq!(directed["target"], 3);
    }

    #[test]
    fn targeted_or_moving_motion_cannot_discard_stored_aim() {
        let mut motion = ProjectileMotion::default();
        assert!(stationary_motion([0.; 3], [0.; 3], &motion));
        assert!(!stationary_motion([1., 0., 0.], [0.; 3], &motion));
        assert!(!stationary_motion([0.; 3], [0., 1., 0.], &motion));
        motion.aim_at_target = true;
        assert!(!stationary_motion([0.; 3], [0.; 3], &motion));
        motion.aim_at_target = false;
        motion.origin = ProjectileOrigin::TargetFloor;
        assert!(!stationary_motion([0.; 3], [0.; 3], &motion));
    }
}

/// Only these IDs may omit trajectory fields from the paired comparison. Their
/// identity, birth, retirement and attributed contacts still compare normally.
pub(super) fn validate(source: &Trace, native: &Trace) -> Result<Validated> {
    let launches: Vec<Launch> =
        super::optional_field(&source.setup, "projectile_launches")?.unwrap_or_default();
    let mut admitted = Validated::default();
    for launch in &launches {
        ensure!(
            admitted.ids.insert(launch.projectile),
            "duplicate projectile launch descriptor"
        );
        let motion = &launch.motion;
        let ricochet = motion.response.is_some_and(|response| response.ricochet);
        ensure!(
            launch.lifetime > 0 && launch.template.has_provenance(),
            "missing authored projectile provenance"
        );
        if motion.steering.is_some() {
            validate_steering(source, native, launch)?;
            continue;
        }
        ensure!(
            matches!(
                motion.origin,
                ProjectileOrigin::Actor | ProjectileOrigin::Bone(_)
            ) && motion.bone_direction.is_none()
                && motion.departure.is_none()
                && motion.velocity_reset.is_none()
                && !motion.aim_at_target
                && !motion.stop_on_ground
                && !motion.aqua_edge
                && motion.contact_path.is_empty()
                && motion.speed.is_none()
                && motion.steering.is_none(),
            "unsupported randomized projectile motion"
        );
        if let Some(response) = motion.response {
            ensure!(
                !response.bounce
                    && response.restitution == 0.75
                    && if ricochet {
                        !response.ricochet_on_deflect
                            && launch.survives_contact
                            && launch.model.is_some()
                    } else {
                        response.ricochet_on_deflect && !response.ground_ricochet
                    },
                "unsupported randomized projectile response"
            );
            if ricochet {
                admitted.ricochets.insert(launch.projectile);
                if motion.face_velocity {
                    admitted.spinning_ricochets.insert(launch.projectile);
                }
            }
        }
        if !motion.velocity_jitter.iter().any(|value| value.abs() >= 1.) {
            ensure!(
                motion.velocity_jitter == [0.; 3]
                    && launch.authored_action.is_none()
                    && launch.contact.is_none(),
                "deterministic flight requires an unmodified registered scene template"
            );
            let pin = serde_json::json!({
                "path": launch.template.path, "sha256": launch.template.sha256
            });
            for trace in [source, native] {
                ensure!(
                    trace.setup["stored_scenes"]["registrations"]
                        .as_array()
                        .is_some_and(|rows| rows.iter().any(|row| row["scene"] == pin)),
                    "deterministic flight scene is not registered"
                );
            }
            let definition = launch_definition(launch)?;
            ensure!(
                definition["unsupported_reason"].is_null()
                    && vector(&definition, "velocity")? == launch.velocity
                    && vector(&definition, "acceleration")? == launch.acceleration
                    && serde_json::from_value::<ProjectileMotion>(definition["motion"].clone())?
                        == *motion
                    && definition["lifetime"].as_u64() == Some(u64::from(launch.lifetime))
                    && definition["clamp_ground"] == launch.clamp_ground
                    && definition["contact"]["survives_contact"] == launch.survives_contact,
                "deterministic flight differs from its registered template"
            );
        }
        ensure!(
            motion.velocity_jitter.iter().all(|value| value.is_finite())
                && launch
                    .origin
                    .iter()
                    .chain(&launch.velocity)
                    .chain(&launch.acceleration)
                    .chain(std::iter::once(&launch.heading))
                    .all(|value| value.is_finite()),
            "invalid randomized projectile launch"
        );
        validate_authored_action(&launch)?;
        let mut boundary = false;
        for (label, trace) in [("Dolphin", source), ("native", native)] {
            boundary |= validate_flight(trace, &launch, label == "Dolphin").with_context(|| {
                format!(
                    "{label} projectile {} ({} record {})",
                    launch.projectile, launch.template.path, launch.template.index
                )
            })?;
        }
        if boundary {
            admitted.boundary_retirements.insert(launch.projectile);
        }
        if launch.contact.is_some()
            && launch.authored_action.is_some()
            && native.setup["contact_damage_observations"] == true
            && native.setup["contact_damage_normalized"] == true
        {
            let source_contacts = validate_contacts(source, &launch, false)?;
            let native_contacts = validate_contacts(native, &launch, true)?;
            if source_contacts == native_contacts && !source_contacts.is_empty() {
                admitted.contact_timings.insert(launch.projectile);
            }
        }
        if ricochet {
            continue;
        }
        let heading = if motion.world_direction {
            0.
        } else {
            launch.heading
        };
        admitted
            .model_accelerations
            .insert(launch.projectile, rotate(launch.acceleration, heading));
        if launch.clamp_ground {
            for trace in [source, native] {
                let mut previous = launch.origin;
                for frame in &trace.frames {
                    let Some(shot) = frame["projectiles"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|shot| shot["id"].as_u64() == Some(launch.projectile))
                    else {
                        continue;
                    };
                    let position = vector(shot, "position")?;
                    let step = std::array::from_fn(|axis| position[axis] - previous[axis]);
                    if let Some(other) = admitted
                        .clamped_model_steps
                        .insert(model_step_key(frame, shot), step)
                    {
                        for (a, b) in step.into_iter().zip(other) {
                            near(a, b, "ambiguous clamped model movement")?;
                        }
                    }
                    previous = position;
                }
            }
        }
        for (source, native) in source.frames.iter().zip(&native.frames) {
            let pair = [source, native].map(|frame| {
                frame["projectiles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|shot| shot["id"].as_u64() == Some(launch.projectile))
            });
            let [Some(source), Some(native)] = pair else {
                continue;
            };
            for field in ["position", "velocity"] {
                let source = rotate(vector(source, field)?, -heading);
                let native = rotate(vector(native, field)?, -heading);
                for (axis, amplitude) in motion.velocity_jitter.iter().enumerate() {
                    if amplitude.abs() < 1. {
                        near(source[axis], native[axis], "deterministic trajectory axis")?;
                    }
                }
            }
        }
    }
    for launch in &launches {
        if admitted.boundary_retirements.contains(&launch.projectile)
            && validate_static_model_lifetime(source, native, launch, &admitted)?
        {
            admitted.model_lifetimes.insert(launch.projectile);
        }
        if let Some(trail) = &launch.trail {
            validate_trail(source, native, launch, trail, false, &mut admitted)?;
        }
        if let Some(ground) = &launch.ground {
            validate_trail(source, native, launch, ground, true, &mut admitted)?;
        }
    }
    Ok(admitted)
}

// A blend above one amplifies portable launch rounding. Check each complete
// recurrence locally; contact identity, timing and lifetime remain paired.
fn validate_steering(source: &Trace, native: &Trace, launch: &Launch) -> Result<()> {
    let motion = &launch.motion;
    let steering = motion.steering.context("missing steering recipe")?;
    let speed = motion.speed.context("missing fixed speed")?;
    ensure!(
        steering.planar
            && steering.blend > 1.
            && steering.blend.is_finite()
            && steering.start == 0
            && steering.end.is_none()
            && speed > 0.
            && speed.is_finite()
            && launch.velocity == [0.; 3]
            && launch.acceleration == [0.; 3]
            && !launch.clamp_ground
            && !launch.survives_contact
            && launch.authored_action.is_some()
            && launch.model.is_none()
            && *motion
                == ProjectileMotion {
                    origin: motion.origin,
                    speed: Some(speed),
                    steering: Some(steering),
                    ..Default::default()
                },
        "unsupported extrapolating flight"
    );
    validate_authored_action(launch)?;
    validate_steering_history(source, native, launch)
}

fn validate_steering_history(source: &Trace, native: &Trace, launch: &Launch) -> Result<()> {
    let steering = launch.motion.steering.context("missing steering recipe")?;
    let speed = launch.motion.speed.context("missing fixed speed")?;
    let mut initial: Option<([f64; 3], [f64; 3])> = None;
    for trace in [source, native] {
        let events = |kind| projectile_events(trace, launch.projectile, kind).collect::<Vec<_>>();
        let births = events("projectile_started");
        let ends = events("projectile_expired");
        ensure!(
            births.len() == 1 && ends.len() == 1,
            "steering lifetime is ambiguous"
        );
        let birth = integer(births[0], "clock")?;
        let end = integer(ends[0], "clock")?;
        ensure!(
            launch.authored_action.as_ref().unwrap().clock == birth,
            "steering birth differs from authored emission"
        );
        let hits = events("hit");
        ensure!(
            hits.len() <= 1,
            "one-shot steering projectile hit repeatedly"
        );
        let expected_end = if let Some(hit) = hits.first() {
            integer(hit, "clock")? + 2
        } else {
            birth + u64::from(launch.lifetime) + 1
        };
        ensure!(
            end == expected_end,
            "steering retirement differs from its contact or lifetime"
        );
        let start = integer(&trace.frames[0], "combat_tick")?;
        let mut previous: Option<(u64, [f64; 3], [f64; 3])> = None;
        let mut target = [0.; 3];
        for frame in &trace.frames {
            let clock = integer(frame, "combat_tick")? - start;
            let shots = frame["projectiles"]
                .as_array()
                .context("missing projectiles")?;
            let matching: Vec<_> = shots
                .iter()
                .filter(|p| p["id"].as_u64() == Some(launch.projectile))
                .collect();
            ensure!(
                matching.len() == usize::from((birth..end).contains(&clock)),
                "steering projectile visibility differs from its lifetime"
            );
            let Some(shot) = matching.first() else {
                continue;
            };
            ensure!(
                shot["owner"] == births[0]["owner"] && shot["target"] == births[0]["target"],
                "steering projectile identity changed"
            );
            let position = vector(shot, "position")?;
            let velocity = vector(shot, "velocity")?;
            let prior_velocity =
                if let Some((prior_clock, prior_position, prior_velocity)) = previous {
                    ensure!(
                        clock == prior_clock + 1,
                        "steering samples are not consecutive updates"
                    );
                    for axis in 0..3 {
                        near(
                            position[axis],
                            prior_position[axis] + f64::from(speed) * prior_velocity[axis],
                            "steering position recurrence",
                        )?;
                    }
                    prior_velocity
                } else {
                    ensure!(clock == birth, "missing steering birth");
                    let actors = frame["actors"]
                        .as_array()
                        .context("missing target actors")?;
                    let actor = actors
                        .iter()
                        .find(|a| a["actor"] == births[0]["target"])
                        .context("missing steering target")?;
                    target = vector(actor, "position")?;
                    if let Some((first_position, first_velocity)) = initial {
                        for axis in 0..3 {
                            near(
                                position[axis],
                                first_position[axis],
                                "paired steering birth position",
                            )?;
                            near(
                                velocity[axis],
                                first_velocity[axis],
                                "paired steering birth direction",
                            )?;
                        }
                    } else {
                        initial = Some((position, velocity));
                    }
                    [0.; 3]
                };
            let offset = [target[0] - position[0], 0., target[2] - position[2]];
            let distance = offset[0].hypot(offset[2]);
            ensure!(
                distance >= 0.1,
                "steering target proximity needs a separate contract"
            );
            let blend = f64::from(steering.blend);
            let direction = std::array::from_fn::<_, 3, _>(|axis| {
                prior_velocity[axis] * (1. - blend) + offset[axis] / distance * blend
            });
            let length = direction[0].hypot(direction[1]).hypot(direction[2]);
            ensure!(
                length >= 0.1,
                "steering cancellation needs a separate contract"
            );
            for axis in 0..3 {
                near(
                    velocity[axis],
                    direction[axis] / length,
                    "steering direction recurrence",
                )?;
            }
            previous = Some((clock, position, velocity));
        }
        ensure!(
            previous.is_some_and(|(clock, _, _)| clock + 1 == end),
            "steering trace omits its final update"
        );
    }
    Ok(())
}

fn validate_authored_action(launch: &Launch) -> Result<()> {
    let Some(action) = &launch.authored_action else {
        ensure!(
            launch.contact.is_none(),
            "contact recipe lacks its authored action"
        );
        return Ok(());
    };
    let pin: super::random_inputs::Pin = serde_json::from_value(serde_json::json!({
        "path": action.path, "sha256": action.sha256,
    }))?;
    let definition: Value = serde_json::from_slice(&pin.read_bytes()?)?;
    let events = definition["actions"]["rows"][action.index]["execution"]["attack"]["events"]
        .as_array()
        .context("launch has no authored attack events")?;
    let emissions: Vec<_> = events
        .iter()
        .filter(|event| {
            event[0].as_u64() == Some(action.clock) && event[1].get("projectile").is_some()
        })
        .collect();
    let [emission] = emissions.as_slice() else {
        anyhow::bail!("launch does not identify a unique authored emission");
    };
    let recipe = &emission[1]["projectile"];
    let definition = &recipe["definition"];
    let motion: ProjectileMotion = serde_json::from_value(definition["motion"].clone())?;
    let contact: Contact = serde_json::from_value(serde_json::json!({
        "cooldown": recipe["hit"]["cooldown"],
        "repeat_limit": definition["contact"]["repeat_limit"],
        "active": definition["active"],
    }))?;
    ensure!(
        definition["unsupported_reason"].is_null()
            && vector(definition, "velocity")? == launch.velocity
            && vector(definition, "acceleration")? == launch.acceleration
            && motion == launch.motion
            && definition["clamp_ground"] == launch.clamp_ground
            && definition["lifetime"].as_u64() == Some(u64::from(launch.lifetime))
            && definition["contact"]["survives_contact"] == launch.survives_contact
            && launch.contact.as_ref() == Some(&contact),
        "launch parameters differ from the pinned authored action"
    );
    Ok(())
}

/// Contact timing may differ only after flight and each observed contact pass
/// these checks. The caller separately retains recipient multiplicities across
/// runs: root positions cannot prove an unobserved animated reception overlap.
fn validate_contacts(trace: &Trace, launch: &Launch, native: bool) -> Result<BTreeMap<u64, u64>> {
    let contact = launch.contact.as_ref().context("missing contact recipe")?;
    let birth = projectile_events(trace, launch.projectile, "projectile_started")
        .next()
        .context("missing projectile birth")?;
    let born = integer(birth, "clock")?;
    let owner = integer(birth, "owner")?;
    let actors = trace.setup["actors"]
        .as_array()
        .context("missing contact actors")?;
    let owner_side = &actors
        .iter()
        .find(|actor| actor["actor"].as_u64() == Some(owner))
        .context("unknown projectile owner")?["side"];
    let start = integer(
        trace.frames.first().context("missing frames")?,
        "combat_tick",
    )?;
    let mut counts = BTreeMap::new();
    let mut previous = BTreeMap::new();
    for hit in projectile_events(trace, launch.projectile, "hit") {
        let target = integer(hit, "target")?;
        let clock = integer(hit, "clock")?;
        let age = clock.checked_sub(born).context("contact precedes birth")?;
        ensure!(
            hit["owner"] == birth["owner"]
                && hit["action"] == birth["action"]
                && actors
                    .iter()
                    .any(|actor| actor["actor"].as_u64() == Some(target)
                        && &actor["side"] != owner_side)
                && age > 0
                && contact
                    .active
                    .is_none_or(|[first, last]| (first..=last).contains(&age)),
            "projectile contact has invalid ownership, recipient or active age"
        );
        let frame = trace
            .frames
            .iter()
            .find(|frame| frame["combat_tick"].as_u64() == Some(start + clock))
            .context("contact lacks its observation frame")?;
        let shot = frame["projectiles"]
            .as_array()
            .context("missing projectile observations")?
            .iter()
            .find(|shot| shot["id"].as_u64() == Some(launch.projectile))
            .context("contact has no live projectile")?;
        ensure!(shot["disarmed"] == false, "contact after projectile disarm");
        if native {
            ensure!(
                shot["contact_active"] == true && hit["damage_inputs"].is_object(),
                "native contact lacks active state or independent damage inputs"
            );
        }
        if let Some(last) = previous.insert(target, clock) {
            ensure!(
                clock
                    .checked_sub(last)
                    .is_some_and(|gap| gap >= contact.cooldown.max(1)),
                "projectile recipient cooldown violated"
            );
        }
        let count = counts.entry(target).or_insert(0);
        *count += 1;
        ensure!(
            contact.repeat_limit == 0 || *count <= contact.repeat_limit,
            "projectile recipient repeat limit exceeded"
        );
        validate_contact_damage(hit)?;
    }
    for target in counts.keys() {
        validate_contact_hp(trace, *target)?;
    }
    Ok(counts)
}

fn validate_contact_damage(hit: &Value) -> Result<()> {
    let bounds = |value: &Value| -> Result<[i64; 2]> {
        let range: [i64; 2] = serde_json::from_value(value.clone())?;
        ensure!(
            range[0] >= 0 && range[0] <= range[1],
            "invalid contact damage bounds"
        );
        Ok(range)
    };
    let amount = hit["hp_change"]
        .as_i64()
        .and_then(i64::checked_neg)
        .context("invalid contact HP change")?;
    let range = bounds(&hit["damage_range"])?;
    let ordinary = bounds(&hit["damage_branches"]["ordinary"])?;
    let critical = (!hit["damage_branches"]["critical"].is_null())
        .then(|| bounds(&hit["damage_branches"]["critical"]))
        .transpose()?;
    let contains = |[low, high]: [i64; 2]| (low..=high).contains(&amount);
    let ordinary_matches = contains(ordinary);
    let critical_matches = critical.is_some_and(contains);
    let observed = hit["critical"].as_bool();
    let protected = matches!(
        hit["protection"].as_str(),
        Some("recovery" | "down_window_closed")
    );
    ensure!(
        contains(range)
            && (amount > 0 || protected && amount == 0)
            && (ordinary_matches || critical_matches)
            && match observed {
                Some(true) => critical_matches,
                Some(false) => ordinary_matches,
                None => hit["critical"].is_null() && ordinary_matches && critical_matches,
            }
            && (!protected
                || range == [0, 0]
                    && ordinary == [0, 0]
                    && critical.is_none()
                    && observed == Some(false)),
        "projectile damage disagrees with its independent branch"
    );
    Ok(())
}

fn validate_contact_hp(trace: &Trace, target: u64) -> Result<()> {
    let start = integer(
        trace.frames.first().context("missing frames")?,
        "combat_tick",
    )?;
    let mut previous = None;
    for frame in &trace.frames {
        let clock = integer(frame, "combat_tick")? - start;
        let actor = frame["actors"]
            .as_array()
            .context("missing actors")?
            .iter()
            .find(|actor| actor["actor"].as_u64() == Some(target))
            .context("contact recipient disappeared")?;
        let hp = actor["hp"].as_i64().context("missing recipient HP")?;
        if let Some((before_clock, before_hp)) = previous {
            if clock == before_clock {
                ensure!(
                    hp == before_hp,
                    "recipient HP changed without a combat update"
                );
            } else {
                ensure!(
                    clock == before_clock + 1,
                    "contact HP observation skipped an update"
                );
                // Each supplemental outcome is independently checked by the
                // ordinary poison/contact-outcome validators. Include its
                // observed HP transition here without admitting unexplained loss.
                let expected = trace
                    .events
                    .iter()
                    .filter(|event| {
                        event["target"].as_u64() == Some(target)
                            && event["clock"].as_u64() == Some(clock)
                    })
                    .try_fold(before_hp, |hp, event| -> Result<i64> {
                        let amount = || {
                            event["amount"]
                                .as_i64()
                                .filter(|amount| *amount > 0)
                                .context("invalid supplemental HP amount")
                        };
                        match event["kind"].as_str() {
                            Some("hit") => hp
                                .checked_add(
                                    event["hp_change"]
                                        .as_i64()
                                        .context("missing hit HP change")?,
                                )
                                .context("contact HP overflow"),
                            Some("poison_pulse") => {
                                hp.checked_sub(amount()?).context("poison HP overflow")
                            }
                            Some("knockdown_impact") => Ok(hp
                                .checked_sub(amount()?)
                                .context("impact HP overflow")?
                                .max(1)),
                            Some("hp_drained") => {
                                hp.checked_add(amount()?).context("drain HP overflow")
                            }
                            _ => Ok(hp),
                        }
                    })?;
                ensure!(
                    hp == expected,
                    "recipient HP change lacks matching observed contacts"
                );
            }
        }
        previous = Some((clock, hp));
    }
    Ok(())
}

#[cfg(test)]
mod contact_tests {
    use super::*;
    use serde_json::json;

    fn sample() -> (Trace, Launch) {
        let hit = |clock| {
            json!({"kind":"hit","projectile":1,"owner":0,"target":1,
            "action":1,"clock":clock,"hp_change":-10,"critical":false,"damage_inputs":{},
            "damage_range":[8,18],"damage_branches":{"ordinary":[8,12],"critical":[15,18]}})
        };
        let trace = Trace {
            case: "contact-validation".into(),
            setup: json!({"actors":[{"actor":0,"side":0},{"actor":1,"side":1}]}),
            frames: [100, 90, 90, 80]
                .into_iter()
                .enumerate()
                .map(|(clock, hp)| {
                    json!({"combat_tick":clock,"actors":[{"actor":1,"hp":hp}],
                    "projectiles":[{"id":1,"disarmed":false,"contact_active":true}]})
                })
                .collect(),
            events: vec![
                json!({"kind":"projectile_started","projectile":1,"owner":0,
                "target":1,"action":1,"clock":0}),
                hit(1),
                hit(3),
            ],
            assessment: json!({}),
            incomplete_observations: vec![],
        };
        let launch = serde_json::from_value(json!({"projectile":1,
            "template":{"path":"unused","sha256":"unused","index":0},
            "origin":[0.,0.,0.],"heading":0.,"velocity":[0.,0.,1.],
            "acceleration":[0.,0.,0.],"motion":ProjectileMotion::default(),"clamp_ground":false,"lifetime":5,
            "contact":{"cooldown":2,"repeat_limit":2,"active":[1,3]}}))
        .unwrap();
        (trace, launch)
    }

    #[test]
    fn contact_qualification_retains_ownership_window_damage_and_hp_checks() {
        let (trace, launch) = sample();
        assert_eq!(
            validate_contacts(&trace, &launch, true).unwrap(),
            BTreeMap::from([(1, 2)])
        );
        for (field, value) in [
            ("owner", json!(2)),
            ("target", json!(0)),
            ("action", json!(2)),
            ("clock", json!(0)),
            ("hp_change", json!(-99)),
            ("critical", json!(true)),
            ("damage_inputs", Value::Null),
        ] {
            let (mut trace, launch) = sample();
            trace.events[1][field] = value;
            assert!(validate_contacts(&trace, &launch, true).is_err(), "{field}");
        }
        for (field, value) in [("disarmed", json!(true)), ("contact_active", json!(false))] {
            let (mut trace, launch) = sample();
            trace.frames[1]["projectiles"][0][field] = value;
            assert!(validate_contacts(&trace, &launch, true).is_err(), "{field}");
        }
        let (mut trace, launch) = sample();
        trace.frames[1]["actors"][0]["hp"] = json!(89);
        assert!(validate_contacts(&trace, &launch, false).is_err());
    }

    #[test]
    fn contact_qualification_keeps_cooldowns_repeat_limits_and_missing_hits() {
        let (mut trace, mut launch) = sample();
        trace.events[2]["clock"] = json!(2);
        assert!(validate_contacts(&trace, &launch, false).is_err());
        trace.events[2]["clock"] = json!(3);
        launch.contact.as_mut().unwrap().repeat_limit = 1;
        assert!(validate_contacts(&trace, &launch, false).is_err());
        launch.contact.as_mut().unwrap().repeat_limit = 2;
        launch.contact.as_mut().unwrap().active = Some([2, 3]);
        assert!(validate_contacts(&trace, &launch, false).is_err());
        let (mut missing, launch) = sample();
        missing.events.pop();
        missing.frames[3]["actors"][0]["hp"] = json!(90);
        assert_ne!(
            validate_contacts(&missing, &launch, false).unwrap(),
            validate_contacts(&sample().0, &launch, false).unwrap()
        );
    }
}

fn validate_flight(trace: &Trace, launch: &Launch, source: bool) -> Result<bool> {
    let event = |kind: &'static str| -> Result<&Value> {
        super::exactly_one(projectile_events(trace, launch.projectile, kind))
            .with_context(|| format!("missing or duplicate {kind}"))
    };
    let birth_event = event("projectile_started")?;
    let birth = integer(birth_event, "clock")?;
    ensure!(
        launch
            .authored_action
            .as_ref()
            .is_none_or(|action| action.clock == birth),
        "projectile birth differs from its authored emission"
    );
    let retirement = integer(event("projectile_expired")?, "clock")?;
    let duration = retirement
        .checked_sub(birth)
        .context("retirement precedes launch")?;
    ensure!(
        duration > 0 && duration <= u64::from(launch.lifetime) + 1,
        "projectile exceeds authored lifetime"
    );
    let ricochet = launch
        .motion
        .response
        .is_some_and(|response| response.ricochet);
    let hits: Vec<_> = projectile_events(trace, launch.projectile, "hit").collect();
    if launch.motion.response.is_some() {
        ensure!(
            projectile_events(trace, launch.projectile, "projectile_clashed")
                .next()
                .is_none(),
            "projectile clash requires a separately observed response"
        );
    }
    if ricochet {
        ensure!(
            hits.len() <= 1
                && hits.iter().all(|hit| {
                    hit["owner"] == birth_event["owner"]
                        && hit["target"] == birth_event["target"]
                        && hit["hp_change"].as_i64().is_some_and(|change| change < 0)
                        && hit["clock"]
                            .as_u64()
                            .is_some_and(|clock| (birth..retirement - 1).contains(&clock))
                }),
            "ricochet lacks complete lifetime or owned contact observations"
        );
    }
    let start = integer(
        trace.frames.first().context("missing admission frame")?,
        "combat_tick",
    )?;
    let heading = if launch.motion.world_direction {
        0.
    } else {
        launch.heading
    };
    let mut acceleration = rotate(launch.acceleration, heading);
    let mut initial = None;
    let mut expected_position = launch.origin;
    let mut expected_velocity = [0.; 3];
    let mut disarmed = false;
    let mut contact_rebounded = false;
    let mut angles = [0.; 2];
    let mut spin = -6.;
    let mut previous_clock = None;
    let mut boundary_retirement = None;
    for frame in &trace.frames {
        let clock = integer(frame, "combat_tick")?
            .checked_sub(start)
            .context("clock moved backwards")?;
        let shots: Vec<_> = frame["projectiles"]
            .as_array()
            .context("missing projectile observations")?
            .iter()
            .filter(|shot| shot["id"].as_u64() == Some(launch.projectile))
            .collect();
        ensure!(
            shots.len() == usize::from((birth..retirement).contains(&clock)),
            "projectile visibility disagrees with birth/retirement"
        );
        let models: Vec<_> = frame["effect_models"]
            .as_array()
            .context("missing model observations")?
            .iter()
            .filter(|model| model["attachment"]["projectile"].as_u64() == Some(launch.projectile))
            .collect();
        if ricochet {
            ensure!(
                models.len() == shots.len(),
                "ricochet must have one owned model throughout its lifetime"
            );
        }
        let Some(shot) = shots.first() else { continue };
        ensure!(
            shot["owner"] == birth_event["owner"] && shot["target"] == birth_event["target"],
            "projectile owner or target changed"
        );
        ensure!(
            previous_clock.is_none_or(|previous| clock == previous + 1),
            "projectile observation clock skipped or repeated"
        );
        previous_clock = Some(clock);
        let position = vector(shot, "position")?;
        let velocity = vector(shot, "velocity")?;
        if initial.is_none() {
            ensure!(clock == birth, "launch observation missing");
            if source {
                ensure!(
                    integer(shot, "age")? == 1,
                    "source launch must be observed after its first update"
                );
            }
            // Both observers publish after the first position and velocity update.
            let first = std::array::from_fn(|axis| velocity[axis] - acceleration[axis]);
            let local = rotate(first, -heading);
            for (axis, observed) in local.into_iter().enumerate() {
                let delta = observed - launch.velocity[axis];
                let amplitude = launch.motion.velocity_jitter[axis];
                if amplitude.abs() < 1. {
                    near(delta, 0., "deterministic launch axis")?;
                } else {
                    let divisor = (amplitude * 10.).trunc().abs() as f64;
                    ensure!(
                        divisor.is_finite() && (1. ..=f64::from(i32::MAX)).contains(&divisor),
                        "unsupported jitter divisor"
                    );
                    let step = (delta * 10.).round();
                    // Float rotation may introduce rounding; the accepted grid
                    // resolution remains the existing 0.01 game-unit precision.
                    near(
                        delta,
                        step / 10.,
                        "launch delta is not on the tenth-unit grid",
                    )?;
                    ensure!(
                        step >= -(divisor - 1.).min(32768.) && step <= (divisor - 1.).min(32767.),
                        "launch delta outside signed remainder range"
                    );
                }
            }
            initial = Some(first);
            expected_velocity = first;
        }
        let orientation_velocity = expected_velocity;
        let bounced = advance_ballistic(
            &mut expected_position,
            &mut expected_velocity,
            acceleration,
            launch.clamp_ground,
            contact_rebounded.then_some(0.75),
        );
        if bounced {
            spin *= 0.5;
        }
        let contact = ricochet
            && hits
                .first()
                .is_some_and(|hit| hit["clock"].as_u64() == clock.checked_sub(1));
        let ground = launch
            .motion
            .response
            .is_some_and(|response| response.ground_ricochet)
            && expected_position[1] < 0.1;
        ensure!(
            !(ground && (contact || contact_rebounded)),
            "unsupported combined ground/contact response"
        );
        if contact || ground {
            ensure!(
                clock > birth,
                "ricochet has no observed orientation before response"
            );
            ensure!(!contact || !disarmed, "contact after projectile disarm");
            let horizontal = expected_velocity[0].hypot(expected_velocity[2]);
            let scale =
                if contact { -2. } else { 2. } / if horizontal >= 0.1 { horizontal } else { 1. };
            expected_velocity = [
                expected_velocity[0] * scale,
                9.,
                expected_velocity[2] * scale,
            ];
            acceleration = [0., -0.3, 0.];
            disarmed = true;
            contact_rebounded |= contact;
        }
        ensure!(
            shot["disarmed"] == disarmed,
            "projectile disarm disagrees with its response"
        );
        if ricochet {
            ensure!(
                !disarmed || !hits.iter().any(|hit| hit["clock"].as_u64() == Some(clock)),
                "hit after projectile disarm"
            );
            let world = ricochet_world(models[0], launch, &birth_event["owner"])?;
            if launch.motion.face_velocity {
                if ground {
                    let pitch = ((-world[6]).atan2(world[5]).to_degrees() + 6.).rem_euclid(360.);
                    let choice = pitch.round().rem_euclid(360.);
                    near(
                        (pitch - choice + 180.).rem_euclid(360.) - 180.,
                        0.,
                        "ground pitch is not an integer choice",
                    )?;
                    angles[0] = choice - 6.;
                } else if disarmed {
                    angles[0] += spin;
                } else {
                    let horizontal = orientation_velocity[0].hypot(orientation_velocity[2]);
                    ensure!(horizontal >= 0.1, "unresolved model follow direction");
                    angles = [
                        -90. - orientation_velocity[1].atan2(horizontal).to_degrees(),
                        orientation_velocity[0]
                            .atan2(orientation_velocity[2])
                            .to_degrees(),
                    ];
                }
                let [pitch, yaw] = angles.map(f64::to_radians);
                let (sx, cx) = pitch.sin_cos();
                let (sy, cy) = yaw.sin_cos();
                let expected = [
                    cy,
                    sy * sx,
                    sy * cx,
                    expected_position[0],
                    0.,
                    cx,
                    -sx,
                    expected_position[1],
                    -sy,
                    cy * sx,
                    cy * cx,
                    expected_position[2],
                ];
                for (actual, expected) in world.into_iter().zip(expected) {
                    near(actual, expected, "ricochet model transform")?;
                }
            }
        }
        for axis in 0..3 {
            near(
                velocity[axis],
                expected_velocity[axis],
                "projectile velocity",
            )?;
            near(
                position[axis],
                expected_position[axis],
                "projectile position",
            )?;
        }
        // The final out-of-bounds pose is published before the next visit
        // removes it. Verify its exact retirement clock after validating flight.
        if boundary_retirement.is_none()
            && clock > birth
            && (position[1] < f64::from(-49.9_f32)
                || !launch.motion.unlimited_range && position[0].hypot(position[2]) > 1350.)
        {
            boundary_retirement = Some(clock + 1);
        }
    }
    ensure!(initial.is_some(), "projectile launch not observed");
    ensure!(
        boundary_retirement.is_none_or(|clock| clock == retirement),
        "projectile retirement differs from its first boundary exit"
    );
    if duration <= u64::from(launch.lifetime) {
        ensure!(
            boundary_retirement == Some(retirement)
                || !launch.survives_contact
                    && hits.iter().any(|event| event["clock"]
                        .as_u64()
                        .is_some_and(|clock| clock.checked_add(2) == Some(retirement))),
            "projectile retired early without an attributed contact or boundary exit"
        );
    }
    Ok(boundary_retirement.is_some())
}

/// Ballistic motion runs before the authored floor response; contact ricochets
/// enable subsequent floor bounces even when the launch did not initially bounce.
fn advance_ballistic(
    position: &mut [f64; 3],
    velocity: &mut [f64; 3],
    acceleration: [f64; 3],
    clamp_ground: bool,
    bounce: Option<f64>,
) -> bool {
    for axis in 0..3 {
        position[axis] += velocity[axis];
        velocity[axis] += acceleration[axis];
    }
    let bounced = bounce.is_some() && position[1] < 0.1;
    if let Some(restitution) = bounce.filter(|_| bounced) {
        position[1] = 0.;
        velocity[1] *= -restitution;
    }
    if clamp_ground && position[1] < 0.1 {
        position[1] = 0.;
    }
    bounced
}

fn ricochet_world(model: &Value, launch: &Launch, owner: &Value) -> Result<[f64; 12]> {
    let binding = launch
        .model
        .as_ref()
        .context("missing ricochet model binding")?;
    ensure!(
        model["owner"] == *owner
            && model["attachment"]["kind"] == "projectile"
            && model["bank"].as_str() == Some(binding.bank.as_str())
            && model["member"].as_u64() == Some(binding.member)
            && binding.member == u64::from(binding.template.index)
            && binding.template.has_provenance()
            && (!launch.motion.face_velocity
                || model["model"]["clip"].is_null() && model["model"]["frame"].is_null()),
        "unproven ricochet model binding"
    );
    Ok(serde_json::from_value(model["model"]["world"].clone())?)
}

fn rotate([x, y, z]: [f64; 3], heading: f64) -> [f64; 3] {
    let (sin, cos) = heading.to_radians().sin_cos();
    [cos * x + sin * z, y, cos * z - sin * x]
}

fn projectile_events<'a>(
    trace: &'a Trace,
    projectile: u64,
    kind: &'static str,
) -> impl Iterator<Item = &'a Value> {
    trace.events.iter().filter(move |event| {
        event["kind"] == kind && event["projectile"].as_u64() == Some(projectile)
    })
}

fn integer(value: &Value, field: &str) -> Result<u64> {
    value[field]
        .as_u64()
        .with_context(|| format!("missing numeric {field}"))
}

fn vector(value: &Value, field: &str) -> Result<[f64; 3]> {
    let result: [f64; 3] = serde_json::from_value(value[field].clone())?;
    ensure!(
        result.iter().all(|value| value.is_finite()),
        "invalid {field}"
    );
    Ok(result)
}

fn near(actual: f64, expected: f64, label: &str) -> Result<()> {
    ensure!(
        actual.is_finite() && expected.is_finite() && (actual - expected).abs() <= 0.01,
        "{label}: {actual} != {expected}"
    );
    Ok(())
}

fn model_step_key(frame: &Value, shot: &Value) -> String {
    serde_json::json!([
        frame["combat_tick"],
        shot["id"],
        shot["position"],
        shot["velocity"]
    ])
    .to_string()
}

/// Express a proven motion-following model in its own projectile's frame. Keep
/// all authored scale, rotation and offset components available for comparison.
pub(super) fn canonical_world(
    descriptors: Option<&Value>,
    frame: &Value,
    effect: &Value,
    randomized: &Validated,
) -> Result<Option<Value>> {
    let attachment = &effect["attachment"];
    let Some(id) = attachment["projectile"]
        .as_u64()
        .filter(|id| randomized.ids.contains(id))
    else {
        return Ok(None);
    };
    if randomized.spinning_ricochets.contains(&id) {
        // Every component of this owned model was already checked against its
        // independently validated flight and allowed pitch choices.
        return Ok(Some(serde_json::json!([
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.
        ])));
    }
    ensure!(
        attachment["kind"] == "projectile",
        "unproven model attachment"
    );
    let Some(descriptors) = descriptors else {
        return Ok(None);
    };
    let bindings: Vec<FollowModel> = serde_json::from_value(descriptors.clone())?;
    let bindings: Vec<_> = bindings
        .iter()
        .filter(|binding| {
            effect["bank"].as_str() == Some(binding.bank.as_str())
                && effect["member"].as_u64() == Some(binding.member)
        })
        .collect();
    if bindings.is_empty() {
        return Ok(None);
    }
    ensure!(bindings.len() == 1, "ambiguous model follow binding");
    let binding = bindings[0];
    ensure!(
        binding.follow_origin && binding.template.has_provenance(),
        "unproven authored model follow behavior"
    );
    let shot = frame["projectiles"]
        .as_array()
        .context("missing projectiles")?
        .iter()
        .find(|shot| shot["id"].as_u64() == Some(id))
        .context("model has no live projectile")?;
    let position = vector(shot, "position")?;
    let basis = if binding.follow_orientation {
        let mut velocity = vector(shot, "velocity")?;
        if let Some(acceleration) = randomized.model_accelerations.get(&id) {
            for axis in 0..3 {
                velocity[axis] -= acceleration[axis];
            }
        }
        if let Some(step) = randomized
            .clamped_model_steps
            .get(&model_step_key(frame, shot))
        {
            velocity = *step;
        }
        let horizontal = velocity[0].hypot(velocity[2]);
        let speed = horizontal.hypot(velocity[1]);
        ensure!(
            horizontal >= 0.1 && speed >= 0.1,
            "unresolved model follow direction"
        );
        let right = [velocity[2] / horizontal, 0., -velocity[0] / horizontal];
        let up = velocity.map(|value| -value / speed);
        let forward = [
            right[1] * up[2] - right[2] * up[1],
            right[2] * up[0] - right[0] * up[2],
            right[0] * up[1] - right[1] * up[0],
        ];
        [right, up, forward]
    } else {
        [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
    };
    let world: [f64; 12] = serde_json::from_value(effect["model"]["world"].clone())?;
    let local = std::array::from_fn::<_, 12, _>(|index| {
        let (row, column) = (index / 4, index % 4);
        (0..3)
            .map(|axis| {
                basis[row][axis]
                    * (world[axis * 4 + column] - if column == 3 { position[axis] } else { 0. })
            })
            .sum::<f64>()
    });
    ensure!(
        local.iter().all(|value| value.is_finite()),
        "invalid model transform"
    );
    Ok(Some(serde_json::to_value(local)?))
}

#[cfg(test)]
mod response_contract_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn floor_clamping_keeps_falling_velocity_and_contact_bounce_uses_restitution() {
        let initial = [0., 0.5, 0.];
        let falling = [1., -1., 2.];
        for (clamp, bounce, expected_position, expected_velocity) in [
            (false, None, [1., -0.5, 2.], [1., -1.2, 2.]),
            (true, None, [1., 0., 2.], [1., -1.2, 2.]),
            (false, Some(0.75), [1., 0., 2.], [1., 0.9, 2.]),
        ] {
            let (mut position, mut velocity) = (initial, falling);
            let bounced =
                advance_ballistic(&mut position, &mut velocity, [0., -0.2, 0.], clamp, bounce);
            assert_eq!(bounced, bounce.is_some());
            assert_eq!(position, expected_position);
            for (actual, expected) in velocity.into_iter().zip(expected_velocity) {
                assert!((actual - expected).abs() < 1e-12);
            }
        }
    }

    fn model_binding(orientation: bool) -> Value {
        json!([{"bank":"bank","member":0,"template":{"path":"model","sha256":"a".repeat(64),"index":0},
            "follow_origin":true,"follow_orientation":orientation}])
    }

    #[test]
    fn non_facing_ricochet_keeps_rotation_scale_offset_and_animation_observations() {
        let validated = Validated {
            ids: BTreeSet::from([1]),
            ricochets: BTreeSet::from([1]),
            ..Default::default()
        };
        let frame = json!({"projectiles":[{"id":1,"position":[5.,6.,7.]}]});
        let effect = json!({"bank":"bank","member":0,"attachment":{"kind":"projectile","projectile":1},
            "model":{"clip":0,"frame":4.,"world":[0.,2.,0.,6., -3.,0.,0.,8., 0.,0.,4.,10.]}});
        let expected = json!([0., 2., 0., 1., -3., 0., 0., 2., 0., 0., 4., 3.]);
        assert_eq!(
            canonical_world(Some(&model_binding(false)), &frame, &effect, &validated).unwrap(),
            Some(expected.clone())
        );
        for index in [0, 1, 3, 4, 5, 7, 8, 9, 10, 11] {
            let mut wrong = effect.clone();
            wrong["model"]["world"][index] =
                json!(wrong["model"]["world"][index].as_f64().unwrap() + 0.1);
            assert_ne!(
                canonical_world(Some(&model_binding(false)), &frame, &wrong, &validated).unwrap(),
                Some(expected.clone())
            );
        }
        assert_eq!(effect["model"]["clip"], 0);
        assert_eq!(effect["model"]["frame"], 4.);
    }

    #[test]
    fn accelerating_model_uses_the_same_pre_acceleration_direction_as_motion() {
        let validated = Validated {
            ids: BTreeSet::from([1]),
            model_accelerations: BTreeMap::from([(1, [1., 2., 0.])]),
            ..Default::default()
        };
        let frame = json!({"projectiles":[{"id":1,"position":[5.,6.,7.],"velocity":[1.,2.,1.]}]});
        let effect = json!({"bank":"bank","member":0,"attachment":{"kind":"projectile","projectile":1},
            "model":{"world":[1.,0.,0.,5., 0.,0.,1.,6., 0.,-1.,0.,7.]}});
        assert_eq!(
            canonical_world(Some(&model_binding(true)), &frame, &effect, &validated).unwrap(),
            Some(json!([1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.]))
        );
        let mut wrong = effect.clone();
        wrong["model"]["world"][0] = json!(0.5);
        assert_ne!(
            canonical_world(Some(&model_binding(true)), &frame, &wrong, &validated).unwrap(),
            canonical_world(Some(&model_binding(true)), &frame, &effect, &validated).unwrap()
        );
    }

    #[test]
    fn clamped_model_faces_observed_movement_and_keeps_its_relative_transform() {
        let frame = json!({"combat_tick":6,"projectiles":[{"id":1,"position":[5.,0.,0.],"velocity":[1.,-2.,0.]}]});
        let mut validated = Validated {
            ids: BTreeSet::from([1]),
            ..Default::default()
        };
        validated.clamped_model_steps.insert(
            model_step_key(&frame, &frame["projectiles"][0]),
            [1., 0., 0.],
        );
        let effect = json!({"bank":"bank","member":0,"attachment":{"kind":"projectile","projectile":1},
            "model":{"world":[0.,-1.,0.,5., 0.,0.,1.,0., -1.,0.,0.,0.]}});
        let expected = json!([1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.]);
        assert_eq!(
            canonical_world(Some(&model_binding(true)), &frame, &effect, &validated).unwrap(),
            Some(expected.clone())
        );
        let mut wrong = effect.clone();
        wrong["model"]["world"][3] = json!(5.1);
        assert_ne!(
            canonical_world(Some(&model_binding(true)), &frame, &wrong, &validated).unwrap(),
            Some(expected.clone())
        );
        validated.clamped_model_steps.clear();
        assert_ne!(
            canonical_world(Some(&model_binding(true)), &frame, &effect, &validated).unwrap(),
            Some(expected)
        );
    }
}

fn pinned_json(template: &Template) -> Result<Value> {
    let pin: super::random_inputs::Pin = serde_json::from_value(serde_json::json!({
        "path":template.path,"sha256":template.sha256,
    }))?;
    Ok(serde_json::from_slice(&pin.read_bytes()?)?)
}

fn validate_static_model_lifetime(
    source: &Trace,
    native: &Trace,
    launch: &Launch,
    validated: &Validated,
) -> Result<bool> {
    if launch.motion.response.is_some() {
        return Ok(false);
    }
    let Some(descriptors) = source.setup.get("projectile_follow_models") else {
        return Ok(false);
    };
    let bindings: Vec<FollowModel> = serde_json::from_value(descriptors.clone())?;
    let Some(first) = source
        .frames
        .iter()
        .flat_map(|frame| frame["effect_models"].as_array().into_iter().flatten())
        .find(|model| model["attachment"]["projectile"].as_u64() == Some(launch.projectile))
    else {
        return Ok(false);
    };
    let matches: Vec<_> = bindings
        .iter()
        .filter(|binding| {
            first["bank"].as_str() == Some(binding.bank.as_str())
                && first["member"].as_u64() == Some(binding.member)
        })
        .collect();
    let [binding] = matches.as_slice() else {
        return Ok(false);
    };
    let bank = pinned_json(&binding.template)?;
    ensure!(
        bank["source_sha256"] == binding.bank
            && binding.member == u64::from(binding.template.index),
        "model lifetime has inconsistent authored bank"
    );
    let declaration = &bank["actors"][binding.template.index as usize];
    let template = &declaration["template"];
    let state = &template["state"];
    let zero = serde_json::json!([0., 0., 0.]);
    if declaration["kind"] != "model_particle"
        || !declaration["binding"]["animation"].is_null()
        || !template["lifetime"].is_null()
        || template["follow_origin"] != true
        || template["follow_orientation"] != true
        || !binding.follow_origin
        || !binding.follow_orientation
        || [
            "offset",
            "velocity",
            "acceleration",
            "angular_velocity",
            "orbit",
        ]
        .iter()
        .any(|field| state[*field] != zero)
        || template["jerk"] != zero
        || template["orbit_velocity"] != zero
        || state["geometry"]["Size"]["velocity"] != zero
        || state["geometry"]["Size"]["acceleration"] != zero
    {
        return Ok(false);
    }
    validate_static_model_samples(
        source,
        native,
        launch.projectile,
        validated,
        descriptors,
        binding,
    )
}

fn validate_static_model_samples(
    source: &Trace,
    native: &Trace,
    projectile: u64,
    validated: &Validated,
    descriptors: &Value,
    binding: &FollowModel,
) -> Result<bool> {
    let mut expected = None;
    for trace in [source, native] {
        for frame in &trace.frames {
            let shots: Vec<_> = frame["projectiles"]
                .as_array()
                .context("missing projectile frame")?
                .iter()
                .filter(|shot| shot["id"].as_u64() == Some(projectile))
                .collect();
            let models: Vec<_> = frame["effect_models"]
                .as_array()
                .context("missing effect models")?
                .iter()
                .filter(|model| model["attachment"]["projectile"].as_u64() == Some(projectile))
                .collect();
            ensure!(
                models.len() == shots.len(),
                "owned model lifetime differs from its projectile"
            );
            let Some(model) = models.first() else {
                continue;
            };
            ensure!(
                model["owner"] == shots[0]["owner"]
                    && model["bank"] == binding.bank
                    && model["member"].as_u64() == Some(binding.member)
                    && model["model"]["clip"].is_null()
                    && model["model"]["frame"].is_null(),
                "owned model identity or animation changed"
            );
            let matrix: [f64; 12] = serde_json::from_value(
                canonical_world(Some(descriptors), frame, model, validated)?
                    .context("owned model lacks its relative transform")?,
            )?;
            let reference = expected.get_or_insert(matrix);
            for (actual, expected) in matrix.into_iter().zip(*reference) {
                near(actual, expected, "owned model relative transform")?;
            }
        }
    }
    Ok(expected.is_some())
}

fn launch_definition(launch: &Launch) -> Result<Value> {
    if let Some(action) = &launch.authored_action {
        let bank = pinned_json(&Template {
            path: action.path.clone(),
            sha256: action.sha256.clone(),
            index: 0,
        })?;
        let events = bank["actions"]["rows"][action.index]["execution"]["attack"]["events"]
            .as_array()
            .context("missing authored launch events")?;
        let matching: Vec<_> = events
            .iter()
            .filter(|event| {
                event[0].as_u64() == Some(action.clock) && event[1]["projectile"].is_object()
            })
            .collect();
        ensure!(matching.len() == 1, "ambiguous authored launch");
        Ok(matching[0][1]["projectile"]["definition"].clone())
    } else {
        let bank = pinned_json(&launch.template)?;
        let definition = if let Some(records) = bank["records"].as_array() {
            records.get(launch.template.index as usize)
        } else {
            bank["projectiles"]
                .as_array()
                .and_then(|records| records.get(launch.template.index as usize))
                .and_then(|record| {
                    record
                        .get("definition")
                        .or_else(|| record.get("projectile"))
                })
        };
        definition
            .cloned()
            .context("missing authored projectile template")
    }
}

fn trail_event_key(event: &Value) -> Option<String> {
    let member = match event["kind"].as_str()? {
        "program_started" => &event["program"],
        "declaration_started" => &event["member"],
        _ => return None,
    };
    Some(
        serde_json::json!([
            event["kind"],
            event["owner"],
            event["bank"],
            member,
            event["clock"],
            event["presentation_clock"]
        ])
        .to_string(),
    )
}

fn trail_model_key(frame: &Value, model: &Value) -> String {
    serde_json::json!([
        frame["combat_tick"],
        model["owner"],
        model["bank"],
        model["member"],
        model["ordinal"]
    ])
    .to_string()
}

impl Validated {
    /// Remove only the exact birth events whose per-run trail schedule and
    /// complete particle lifetime were checked below. Contacts stay untouched.
    pub(super) fn retain_events(&self, side: usize, events: &mut Vec<Value>) -> Result<()> {
        let mut counts = self.trail_events[side].clone();
        events.retain(|event| {
            if let Some(count) = trail_event_key(event).and_then(|key| counts.get_mut(&key))
                && *count > 0
            {
                *count -= 1;
                return false;
            }
            true
        });
        ensure!(
            counts.values().all(|count| *count == 0),
            "qualified trail events disappeared before comparison"
        );
        Ok(())
    }

    /// A free trail retains its own recorded birth origin after its projectile
    /// moves. Normalize only that proven translation; compare every local matrix.
    pub(super) fn canonicalize_trail_model(
        &self,
        side: usize,
        frame: &Value,
        model: &mut Value,
    ) -> Result<()> {
        if let Some(origin) = self.trail_model_origins[side].get(&trail_model_key(frame, model)) {
            let mut matrix: [f64; 12] = serde_json::from_value(model["model"]["world"].clone())?;
            for axis in 0..3 {
                matrix[axis * 4 + 3] -= origin[axis];
            }
            model["model"]["world"] = serde_json::to_value(matrix)?;
        }
        Ok(())
    }
}

fn validate_trail(
    source: &Trace,
    native: &Trace,
    launch: &Launch,
    trail: &Trail,
    ground: bool,
    validated: &mut Validated,
) -> Result<()> {
    let definition = launch_definition(launch)?;
    ensure!(
        (ground
            || trail.interval > 0 && definition["trail_interval"].as_u64() == Some(trail.interval))
            && definition[if ground {
                "ground_effect"
            } else {
                "trail_effect"
            }]["member"]
                .as_u64()
                == Some(u64::from(trail.template.index)),
        "trail schedule differs from authored projectile"
    );
    let bank = pinned_json(&trail.template)?;
    ensure!(
        bank["source_sha256"] == trail.bank,
        "trail bank differs from its source bytes"
    );
    let program = bank["programs"][trail.template.index as usize]
        .as_array()
        .context("missing trail program")?;
    let [spawn] = program.as_slice() else {
        return Ok(());
    };
    if spawn["at"] != 0 || spawn["operation"]["kind"] != "spawn" {
        return Ok(());
    }
    let member = spawn["operation"]["particle"]
        .as_u64()
        .context("missing trail declaration")?;
    let declaration = &bank["actors"][member as usize];
    let template = &declaration["template"];
    let Some(lifetime) = template["lifetime"].as_u64().filter(|value| *value > 0) else {
        return Ok(());
    };
    if template["follow_origin"] != false
        || template["follow_orientation"] != false
        || !matches!(
            declaration["kind"].as_str(),
            Some("particle" | "model_particle")
        )
    {
        return Ok(());
    }
    for (side, trace) in [source, native].into_iter().enumerate() {
        let start = integer(
            trace.frames.first().context("missing trail frames")?,
            "combat_tick",
        )?;
        let presentation = integer(&trace.frames[0], "presentation_tick")?;
        let birth = projectile_events(trace, launch.projectile, "projectile_started")
            .next()
            .context("missing trail projectile birth")?;
        let born = integer(birth, "clock")?;
        let mut ground_emitted = false;
        for frame in &trace.frames {
            let clock = integer(frame, "combat_tick")? - start;
            let Some(shot) = frame["projectiles"]
                .as_array()
                .context("missing trail projectile samples")?
                .iter()
                .find(|shot| shot["id"].as_u64() == Some(launch.projectile))
            else {
                continue;
            };
            let mut origin = vector(shot, "position")?;
            if ground {
                if ground_emitted || origin[1] >= f64::from(0.1_f32) {
                    continue;
                }
                ground_emitted = true;
                origin[1] = 0.;
            } else if (clock - born) % trail.interval != 0 {
                continue;
            }
            let particles: Vec<_> = frame["particles"]
                .as_array()
                .context("missing trail particles")?
                .iter()
                .filter(|particle| {
                    particle["owner"] == birth["owner"]
                        && particle["bank"] == trail.bank
                        && particle["member"].as_u64() == Some(member)
                        && particle["age"] == 1
                        && vector(particle, "origin").is_ok_and(|value| {
                            value
                                .into_iter()
                                .zip(origin)
                                .all(|(a, b)| (a - b).abs() <= 0.01)
                        })
                })
                .collect();
            ensure!(
                particles.len() == 1,
                "trail emission lacks its unique source-relative particle"
            );
            let id = integer(particles[0], "id")?;
            let mut observed = 0;
            for sample in &trace.frames {
                let at = integer(sample, "combat_tick")? - start;
                let instances: Vec<_> = sample["particles"]
                    .as_array()
                    .context("missing trail lifetime samples")?
                    .iter()
                    .filter(|particle| particle["id"].as_u64() == Some(id))
                    .collect();
                ensure!(
                    instances.len() == usize::from((clock..clock + lifetime).contains(&at)),
                    "trail particle lifetime differs from its authored duration"
                );
                let Some(particle) = instances.first() else {
                    continue;
                };
                observed += 1;
                ensure!(
                    particle["owner"] == birth["owner"]
                        && particle["bank"] == trail.bank
                        && particle["member"].as_u64() == Some(member)
                        && particle["age"].as_u64() == Some(at - clock + 1)
                        && particle["initialized"] == true,
                    "trail particle identity or update age changed"
                );
                for (a, b) in vector(particle, "origin")?.into_iter().zip(origin) {
                    near(a, b, "trail particle retained origin")?;
                }
                if declaration["kind"] == "model_particle" {
                    let models: Vec<_> = sample["effect_models"]
                        .as_array()
                        .context("missing trail model samples")?
                        .iter()
                        .filter(|model| {
                            model["owner"] == birth["owner"]
                                && model["bank"] == trail.bank
                                && model["member"].as_u64() == Some(member)
                                && model["attachment"].is_null()
                                && model["model"] == particle["model"]
                        })
                        .collect();
                    ensure!(
                        models.len() == 1,
                        "trail model lacks its unique observed particle"
                    );
                    validated.trail_model_origins[side]
                        .insert(trail_model_key(sample, models[0]), origin);
                }
            }
            ensure!(
                observed as u64 == lifetime,
                "trail cleanup was not fully observed"
            );
            let display = integer(frame, "presentation_tick")? - presentation;
            for event in [
                serde_json::json!({"kind":"program_started","owner":birth["owner"],"bank":trail.bank,"program":trail.template.index,"clock":clock,"presentation_clock":display}),
                serde_json::json!({"kind":"declaration_started","owner":birth["owner"],"bank":trail.bank,"member":member,"clock":clock,"presentation_clock":display}),
            ] {
                let key = trail_event_key(&event).unwrap();
                let count = validated.trail_events[side].entry(key.clone()).or_default();
                // Both observers coalesce declaration births by owner, bank,
                // member and tick; program births retain instance multiplicity.
                if event["kind"] == "declaration_started" {
                    *count = 1;
                } else {
                    *count += 1;
                }
                ensure!(
                    trace
                        .events
                        .iter()
                        .filter(|event| trail_event_key(event).as_ref() == Some(&key))
                        .count()
                        >= *count,
                    "authored trail birth is missing or incorrectly timed"
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod lifecycle_contract_tests {
    use super::*;
    use serde_json::json;

    fn frame(position: f64) -> Value {
        json!({"combat_tick":0,"projectiles":[{"id":1,"owner":0,"position":[position,0.,0.],"velocity":[0.,0.,1.]}],
            "effect_models":[{"owner":0,"bank":"bank","member":0,"ordinal":1,"attachment":{"kind":"projectile","projectile":1},
                "model":{"clip":null,"frame":null,"world":[1.,0.,0.,position,0.,0.,1.,0.,0.,-1.,0.,0.]}}]})
    }

    #[test]
    fn model_lifetime_keeps_every_owned_model_and_relative_matrix() {
        let source = Trace {
            case: "lifetime".into(),
            setup: json!({}),
            frames: vec![
                frame(0.),
                frame(1.),
                json!({"projectiles":[],"effect_models":[]}),
            ],
            events: vec![],
            assessment: json!({}),
            incomplete_observations: vec![],
        };
        let mut native: Trace = source.clone();
        native.frames[0] = frame(2.);
        native.frames[1] = frame(4.);
        let descriptors = json!([{"bank":"bank","member":0,"template":{"path":"model","sha256":"a".repeat(64),"index":0},"follow_origin":true,"follow_orientation":true}]);
        let bindings: Vec<FollowModel> = serde_json::from_value(descriptors.clone()).unwrap();
        let validated = Validated {
            ids: BTreeSet::from([1]),
            ..Default::default()
        };
        let check = |native: &Trace| {
            validate_static_model_samples(
                &source,
                native,
                1,
                &validated,
                &descriptors,
                &bindings[0],
            )
        };
        assert!(check(&native).unwrap());
        for field in ["bank", "owner", "member"] {
            let mut wrong: Trace = native.clone();
            wrong.frames[1]["effect_models"][0][field] = json!(99);
            assert!(check(&wrong).is_err(), "{field}");
        }
        let mut wrong: Trace = native.clone();
        wrong.frames[1]["effect_models"] = json!([]);
        assert!(check(&wrong).is_err());
        let mut wrong: Trace = native.clone();
        wrong.frames[2]["effect_models"] = native.frames[1]["effect_models"].clone();
        assert!(check(&wrong).is_err());
        let mut wrong: Trace = native.clone();
        wrong.frames[1]["effect_models"][0]["model"]["world"][0] = json!(2.);
        assert!(check(&wrong).is_err());
        let mut wrong: Trace = native.clone();
        wrong.frames[1]["effect_models"][0]["model"]["clip"] = json!(1);
        assert!(check(&wrong).is_err());
    }

    #[test]
    fn qualified_trail_events_do_not_remove_contacts_or_unattributed_births() {
        let birth = json!({"kind":"program_started","owner":0,"bank":"bank","program":1,"clock":2,"presentation_clock":2});
        let hit = json!({"kind":"hit","owner":0,"target":1,"projectile":1,"clock":2});
        let mut other = birth.clone();
        other["clock"] = json!(3);
        let mut validated = Validated::default();
        validated.trail_events[0].insert(trail_event_key(&birth).unwrap(), 1);
        let mut events = vec![birth.clone(), hit.clone(), other.clone()];
        validated.retain_events(0, &mut events).unwrap();
        assert_eq!(events, vec![hit, other.clone()]);
        assert!(validated.retain_events(0, &mut vec![other]).is_err());
        let mut duplicate = vec![birth.clone(), birth.clone()];
        validated.retain_events(0, &mut duplicate).unwrap();
        assert_eq!(duplicate, vec![birth]);
    }

    #[test]
    fn trail_model_translation_preserves_local_offset_rotation_and_scale() {
        let frame = json!({"combat_tick":7});
        let mut model = json!({"owner":0,"bank":"bank","member":1,"ordinal":2,"model":{"world":[0.,2.,0.,15.,-3.,0.,0.,26.,0.,0.,4.,37.]}});
        let mut validated = Validated::default();
        validated.trail_model_origins[0].insert(trail_model_key(&frame, &model), [10., 20., 30.]);
        validated
            .canonicalize_trail_model(0, &frame, &mut model)
            .unwrap();
        assert_eq!(
            model["model"]["world"],
            json!([0., 2., 0., 5., -3., 0., 0., 6., 0., 0., 4., 7.])
        );
    }
}

#[cfg(test)]
mod supplemental_hp_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hp_conservation_includes_observed_poison_and_rejects_unexplained_loss() {
        let mut trace = Trace {
            case: "poison-hp".into(),
            setup: json!({}),
            frames: [100, 90, 85]
                .into_iter()
                .enumerate()
                .map(|(clock, hp)| json!({"combat_tick":clock,"actors":[{"actor":1,"hp":hp}]}))
                .collect(),
            events: vec![
                json!({"kind":"hit","target":1,"clock":1,"hp_change":-10}),
                json!({"kind":"poison_pulse","target":1,"clock":2,"amount":5}),
            ],
            assessment: json!({}),
            incomplete_observations: vec![],
        };
        assert!(validate_contact_hp(&trace, 1).is_ok());
        trace.events[1]["amount"] = json!(4);
        assert!(validate_contact_hp(&trace, 1).is_err());
        trace.events.pop();
        assert!(validate_contact_hp(&trace, 1).is_err());
    }
}

#[cfg(test)]
mod steering_contract_tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (Trace, Launch) {
        let frames = (0..6)
            .map(|clock| {
                json!({
                    "combat_tick":clock,
                    "actors":[{"actor":0,"position":[1000.,0.,0.]}],
                    "projectiles":if (1..5).contains(&clock) {
                        json!([{"id":1,"owner":1,"target":0,
                            "position":[f64::from(clock-1)*4.,0.,0.],"velocity":[1.,0.,0.]}])
                    } else {json!([])}
                })
            })
            .collect();
        let trace = Trace {
            case: "steering".into(),
            setup: json!({}),
            frames,
            events: vec![
                json!({"kind":"projectile_started","projectile":1,"owner":1,"target":0,"clock":1}),
                json!({"kind":"hit","projectile":1,"clock":3}),
                json!({"kind":"projectile_expired","projectile":1,"clock":5}),
            ],
            assessment: json!({}),
            incomplete_observations: vec![],
        };
        let launch = serde_json::from_value(json!({"projectile":1,
            "template":{"path":"unused","sha256":"a".repeat(64),"index":0},
            "origin":[0.,0.,0.],"heading":0.,"velocity":[0.,0.,0.],"acceleration":[0.,0.,0.],
            "motion":{"velocity_jitter":[0.,0.,0.],"speed":4.,"steering":{"blend":90.,"start":0,"end":null,"planar":true}},
            "clamp_ground":false,"lifetime":90,
            "authored_action":{"path":"unused","sha256":"a".repeat(64),"index":0,"clock":1}
        }))
        .unwrap();
        (trace, launch)
    }

    #[test]
    fn steering_keeps_recurrence_birth_and_retirement_checks() {
        let (source, launch) = fixture();
        let (mut native, _) = fixture();
        for frame in &mut native.frames[1..5] {
            frame["projectiles"][0]["position"][0] =
                json!(frame["projectiles"][0]["position"][0].as_f64().unwrap() + 0.0001);
        }
        validate_steering_history(&source, &native, &launch).unwrap();
        let (mut wrong, _) = fixture();
        wrong.frames[2]["projectiles"][0]["velocity"][2] = json!(0.1);
        assert!(validate_steering_history(&source, &wrong, &launch).is_err());
        let (mut wrong, _) = fixture();
        wrong.frames[2]["projectiles"][0]["position"][0] = json!(5.);
        assert!(validate_steering_history(&source, &wrong, &launch).is_err());
        let (mut wrong, _) = fixture();
        wrong.frames[1]["projectiles"][0]["position"][0] = json!(1.);
        assert!(validate_steering_history(&source, &wrong, &launch).is_err());
        let (mut wrong, _) = fixture();
        wrong.events[2]["clock"] = json!(4);
        assert!(validate_steering_history(&source, &wrong, &launch).is_err());
    }
}
