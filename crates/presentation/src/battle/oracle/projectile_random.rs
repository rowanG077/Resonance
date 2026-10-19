//! Validate ballistic launch variation without requiring identical random draws.
use super::Trace;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_projectile::{ProjectileMotion, ProjectileOrigin};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Deserialize)]
struct Template {
    path: String,
    sha256: String,
    index: u32,
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
    model: Option<RicochetModel>,
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
}

/// Only these IDs may omit trajectory fields from the paired comparison. Their
/// identity, birth, retirement and attributed contacts still compare normally.
pub(super) fn validate(source: &Trace, native: &Trace) -> Result<Validated> {
    let launches: Vec<Launch> = source
        .setup
        .get("projectile_launches")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    let mut admitted = Validated::default();
    for launch in launches {
        ensure!(
            admitted.ids.insert(launch.projectile),
            "duplicate projectile launch descriptor"
        );
        let motion = &launch.motion;
        let ricochet = motion.response.is_some();
        ensure!(
            launch.lifetime > 0
                && !launch.template.path.is_empty()
                && launch.template.sha256.len() == 64
                && launch
                    .template
                    .sha256
                    .bytes()
                    .all(|value| value.is_ascii_hexdigit()),
            "missing authored projectile provenance"
        );
        ensure!(
            (motion.origin == ProjectileOrigin::Actor
                || ricochet && matches!(motion.origin, ProjectileOrigin::Bone(_)))
                && motion.bone_direction.is_none()
                && motion.departure.is_none()
                && motion.velocity_reset.is_none()
                && motion.face_velocity == ricochet
                && !motion.aim_at_target
                && !motion.stop_on_ground
                && !motion.aqua_edge
                && motion.contact_path.is_empty()
                && motion.speed.is_none()
                && motion.steering.is_none()
                && !launch.clamp_ground,
            "unsupported randomized projectile motion"
        );
        if let Some(response) = motion.response {
            ensure!(
                !response.bounce
                    && response.ricochet
                    && response.ground_ricochet
                    && !response.ricochet_on_deflect
                    && response.restitution == 0.75
                    && launch.acceleration == [0.; 3]
                    && launch.survives_contact
                    && launch.model.is_some(),
                "unsupported randomized projectile response"
            );
            admitted.ricochets.insert(launch.projectile);
        }
        ensure!(
            motion.velocity_jitter.iter().any(|value| value.abs() >= 1.)
                && motion.velocity_jitter.iter().all(|value| value.is_finite())
                && launch
                    .origin
                    .iter()
                    .chain(&launch.velocity)
                    .chain(&launch.acceleration)
                    .chain(std::iter::once(&launch.heading))
                    .all(|value| value.is_finite()),
            "invalid randomized projectile launch"
        );
        for (label, trace) in [("Dolphin", source), ("native", native)] {
            validate_flight(trace, &launch, label == "Dolphin").with_context(|| {
                format!(
                    "{label} projectile {} ({} record {})",
                    launch.projectile, launch.template.path, launch.template.index
                )
            })?;
        }
        if ricochet {
            continue;
        }
        let heading = if motion.world_direction {
            0.
        } else {
            launch.heading
        };
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
    Ok(admitted)
}

fn validate_flight(trace: &Trace, launch: &Launch, source: bool) -> Result<()> {
    let event_clock = |kind: &str| -> Result<u64> {
        let events: Vec<_> = trace
            .events
            .iter()
            .filter(|event| {
                event["kind"] == kind && event["projectile"].as_u64() == Some(launch.projectile)
            })
            .collect();
        ensure!(events.len() == 1, "missing or duplicate {kind}");
        integer(events[0], "clock")
    };
    let birth = event_clock("projectile_started")?;
    let retirement = event_clock("projectile_expired")?;
    let duration = retirement
        .checked_sub(birth)
        .context("retirement precedes launch")?;
    ensure!(
        duration > 0 && duration <= u64::from(launch.lifetime) + 1,
        "projectile exceeds authored lifetime"
    );
    let ricochet = launch.motion.response.is_some();
    let birth_event = trace
        .events
        .iter()
        .find(|event| {
            event["kind"] == "projectile_started"
                && event["projectile"].as_u64() == Some(launch.projectile)
        })
        .unwrap();
    let hits: Vec<_> = trace
        .events
        .iter()
        .filter(|event| {
            event["kind"] == "hit" && event["projectile"].as_u64() == Some(launch.projectile)
        })
        .collect();
    if ricochet {
        ensure!(
            duration == u64::from(launch.lifetime) + 1
                && hits.len() <= 1
                && hits.iter().all(|hit| {
                    hit["owner"] == birth_event["owner"]
                        && hit["target"] == birth_event["target"]
                        && hit["hp_change"].as_i64().is_some_and(|change| change < 0)
                        && hit["clock"]
                            .as_u64()
                            .is_some_and(|clock| (birth..retirement - 1).contains(&clock))
                })
                && !trace
                    .events
                    .iter()
                    .any(|event| event["kind"] == "projectile_clashed"
                        && event["projectile"].as_u64() == Some(launch.projectile)),
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
        if ricochet {
            ensure!(
                previous_clock.is_none_or(|previous| clock == previous + 1),
                "ricochet observation clock skipped or repeated"
            );
            previous_clock = Some(clock);
        }
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
        for axis in 0..3 {
            if ricochet {
                expected_position[axis] += expected_velocity[axis];
                expected_velocity[axis] += acceleration[axis];
            } else {
                let steps = (clock - birth + 1) as f64;
                expected_velocity[axis] = initial.unwrap()[axis] + steps * acceleration[axis];
                expected_position[axis] = launch.origin[axis]
                    + steps * initial.unwrap()[axis]
                    + steps * (steps - 1.) / 2. * acceleration[axis];
            }
        }
        let contact = ricochet
            && hits
                .first()
                .is_some_and(|hit| hit["clock"].as_u64() == clock.checked_sub(1));
        let ground = ricochet && expected_position[1] < 0.1;
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
                shot["owner"] == birth_event["owner"] && shot["target"] == birth_event["target"],
                "ricochet owner or target changed"
            );
            ensure!(
                !disarmed || !hits.iter().any(|hit| hit["clock"].as_u64() == Some(clock)),
                "hit after projectile disarm"
            );
            let world = ricochet_world(models[0], launch, &birth_event["owner"])?;
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
                angles[0] -= 6.;
            } else {
                let horizontal = expected_velocity[0].hypot(expected_velocity[2]);
                ensure!(horizontal >= 0.1, "unresolved model follow direction");
                angles = [
                    -90. - expected_velocity[1].atan2(horizontal).to_degrees(),
                    expected_velocity[0]
                        .atan2(expected_velocity[2])
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
                || hits.iter().any(|event| event["clock"]
                    .as_u64()
                    .is_some_and(|clock| (birth..retirement).contains(&clock))),
            "projectile retired early without an attributed contact or boundary exit"
        );
    }
    Ok(())
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
            && !binding.template.path.is_empty()
            && binding.template.sha256.len() == 64
            && binding
                .template
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            && model["model"]["clip"].is_null()
            && model["model"]["frame"].is_null(),
        "unproven ricochet model binding"
    );
    Ok(serde_json::from_value(model["model"]["world"].clone())?)
}

fn rotate([x, y, z]: [f64; 3], heading: f64) -> [f64; 3] {
    let (sin, cos) = heading.to_radians().sin_cos();
    [cos * x + sin * z, y, cos * z - sin * x]
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

/// Express a proven motion-following model in its own projectile's frame. Keep
/// all authored scale, rotation and offset components available for comparison.
pub(super) fn canonical_world(
    descriptors: Option<&Value>,
    frame: &Value,
    effect: &Value,
    randomized: &Validated,
) -> Result<Option<Value>> {
    #[derive(Deserialize)]
    struct FollowModel {
        bank: String,
        member: u64,
        template: Template,
        follow_origin: bool,
        follow_orientation: bool,
    }
    let attachment = &effect["attachment"];
    let Some(id) = attachment["projectile"]
        .as_u64()
        .filter(|id| randomized.ids.contains(id))
    else {
        return Ok(None);
    };
    if randomized.ricochets.contains(&id) {
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
        binding.follow_origin
            && !binding.template.path.is_empty()
            && binding.template.sha256.len() == 64
            && binding
                .template
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
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
        let velocity = vector(shot, "velocity")?;
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
