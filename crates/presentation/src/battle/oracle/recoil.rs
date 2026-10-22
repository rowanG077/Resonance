//! Compare a single grounded travel recoil in the independently observed shot basis.
//! Unsupported or unproved inputs retain ordinary world-position comparison.
use super::{Trace, projectile_random};
use anyhow::{Context, Result};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct QualifiedRecoil {
    actor: Option<u64>,
    first: usize,
    origins: [[f64; 3]; 2],
    bases: [[f64; 2]; 2],
}

impl QualifiedRecoil {
    /// Changes only the two already proved planar coordinates of this recipient.
    pub(super) fn canonicalize(&self, side: usize, frame: usize, actor: &mut Value) {
        if frame < self.first || actor["actor"].as_u64() != self.actor || self.actor.is_none() {
            return;
        }
        if let Some(position) = vector(&actor["position"]) {
            let [along, across] = project(position, self.origins[side], self.bases[side]);
            actor["position"] = json!([along, position[1], across]);
        }
    }
}

pub(super) fn validate(
    source: &Trace,
    native: &Trace,
    projectiles: &projectile_random::Validated,
) -> QualifiedRecoil {
    // An incomplete provenance or recurrence proof gives no coordinate exemption.
    qualify(source, native, projectiles)
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn qualify(
    source: &Trace,
    native: &Trace,
    projectiles: &projectile_random::Validated,
) -> Result<Option<QualifiedRecoil>> {
    let decline = || Ok(None);
    if source.frames.len() != native.frames.len() || source.frames.is_empty() {
        return decline();
    }
    let contacts = |trace: &Trace| {
        trace
            .events
            .iter()
            .filter(|event| event["kind"] == "hit")
            .cloned()
            .collect::<Vec<_>>()
    };
    let source_contacts = contacts(source);
    let [source_hit] = source_contacts.as_slice() else {
        return decline();
    };
    let source_hit = source_hit.clone();
    let native_contacts = contacts(native);
    let [native_hit] = native_contacts.as_slice() else {
        return decline();
    };
    let native_hit = native_hit.clone();
    if [
        "owner",
        "target",
        "projectile",
        "action",
        "clock",
        "presentation_clock",
    ]
    .iter()
    .any(|key| source_hit[*key] != native_hit[*key])
        || source_hit["critical"] != false
        || native_hit["critical"] != false
    {
        return decline();
    }
    let id = source_hit["projectile"]
        .as_u64()
        .context("missing recoil projectile")?;
    let target = source_hit["target"]
        .as_u64()
        .context("missing recoil recipient")?;
    if !projectiles.ids.contains(&id)
        || projectiles.ricochets.contains(&id)
        || source.setup["recipient_input"].is_object()
    {
        return decline();
    }
    let launches = source.setup["projectile_launches"]
        .as_array()
        .context("missing launches")?;
    let matching_launches = launches
        .iter()
        .filter(|launch| launch["projectile"] == id)
        .collect::<Vec<_>>();
    let [launch] = matching_launches.as_slice() else {
        return decline();
    };
    let launch = (*launch).clone();
    let authored = &launch["authored_action"];
    let action_pin = json!({"path": authored["path"], "sha256": authored["sha256"]});
    if source.setup["catalogue"] != action_pin || native.setup["catalogue"] != action_pin {
        return decline();
    }
    let catalogue = read_pin(&action_pin)?;
    let row = authored["index"].as_u64().context("missing action row")? as usize;
    let events = catalogue["actions"]["rows"][row]["execution"]["attack"]["events"]
        .as_array()
        .context("missing physical attack")?;
    let emissions = events
        .iter()
        .filter(|event| event[0] == authored["clock"] && event[1].get("projectile").is_some())
        .collect::<Vec<_>>();
    let [emission] = emissions.as_slice() else {
        return decline();
    };
    let recipe = &emission[1]["projectile"];
    let definition = &recipe["definition"];
    let hit = &recipe["hit"];
    if definition["contact"]["recoil_direction"] != "travel"
        || definition["contact"]["survives_contact"] != false
        || hit["recoil_delay"] != 0
        || [
            "suppress_reaction",
            "alternate_motion",
            "knock_down",
            "launch",
        ]
        .iter()
        .any(|key| hit[*key] != false)
    {
        return decline();
    }
    let pin = &source.setup["reaction_recoil"];
    let module_pin = &source.setup["reaction_recoil_module"];
    if pin.is_null()
        || module_pin.is_null()
        || native.setup["reaction_recoil"] != *pin
        || native.setup["reaction_recoil_module"] != *module_pin
    {
        return decline();
    }
    let recoil = read_pin(pin)?;
    let module: super::random_inputs::Pin = serde_json::from_value(module_pin.clone())?;
    module.read_bytes()?;
    if recoil["source_sha256"] != module_pin["sha256"] {
        return decline();
    }
    let impulse_row = hit["recoil"].as_u64().context("missing recoil rule")? as usize;
    let impulse = &recoil["impulses"][impulse_row];
    let amount = impulse[0]
        .as_f64()
        .filter(|value| value.is_finite() && *value > 0.)
        .context("invalid recoil impulse")?;
    if impulse[1] != 0.0 {
        return decline();
    }
    let stun = native.setup["contact_stun"]
        .as_array()
        .context("missing native reaction witness")?;
    if !stun.iter().any(|entry| {
        entry["clock"] == source_hit["clock"]
            && entry["target"] == target
            && entry["stunned"] == false
    }) || stun
        .iter()
        .any(|entry| entry["target"] == target && entry["stunned"] != false)
        || source.events.iter().chain(&native.events).any(|event| {
            event["kind"] == "declaration_started"
                && event["owner"] == target
                && event["member"] == 19
                && event["bank"]
                    == "2c72d54fcdf809b93f10d0c9213790710b4f0144a4f55253ebab415f4e69a48b"
        })
    {
        return decline();
    }
    let clock = source_hit["presentation_clock"]
        .as_u64()
        .context("missing contact clock")?;
    let mut result = QualifiedRecoil {
        actor: Some(target),
        ..Default::default()
    };
    let mut residuals = [[0.; 3]; 2];
    for (side, trace) in [source, native].into_iter().enumerate() {
        let start = trace.frames[0]["presentation_tick"]
            .as_u64()
            .context("missing presentation clock")?;
        let contact = trace
            .frames
            .iter()
            .position(|frame| frame["presentation_tick"].as_u64() == start.checked_add(clock))
            .context("unobserved contact")?;
        if contact + 1 >= trace.frames.len() || (side == 1 && result.first != contact + 1) {
            return decline();
        }
        result.first = contact + 1;
        let shot = trace.frames[contact]["projectiles"]
            .as_array()
            .and_then(|shots| shots.iter().find(|shot| shot["id"] == id))
            .context("unobserved contact velocity")?;
        let velocity = vector(&shot["velocity"]).context("invalid contact velocity")?;
        let length = velocity[0].hypot(velocity[2]);
        if length <= 0.01 {
            return decline();
        }
        result.bases[side] = [velocity[0] / length, velocity[2] / length];
        let before = position(&trace.frames[contact], target).context("missing recipient")?;
        let first =
            position(&trace.frames[result.first], target).context("missing recipient step")?;
        if before[1] != 0. || first[1] != 0. {
            return decline();
        }
        result.origins[side] = first;
        residuals[side] = [
            first[0] - before[0] - amount * result.bases[side][0],
            first[1] - before[1],
            first[2] - before[2] - amount * result.bases[side][1],
        ];
    }
    if !near(residuals[0], residuals[1]) {
        return decline();
    }
    for index in result.first..source.frames.len() {
        let a = position(&source.frames[index], target).context("missing source recipient tail")?;
        let b = position(&native.frames[index], target).context("missing native recipient tail")?;
        if a[1] != 0. || b[1] != 0. {
            return decline();
        }
        let a = project(a, result.origins[0], result.bases[0]);
        let b = project(b, result.origins[1], result.bases[1]);
        if !near([a[0], 0., a[1]], [b[0], 0., b[1]]) {
            return decline();
        }
    }
    Ok(Some(result))
}

fn read_pin(value: &Value) -> Result<Value> {
    let pin: super::random_inputs::Pin = serde_json::from_value(value.clone())?;
    Ok(serde_json::from_slice(&pin.read_bytes()?)?)
}

fn vector(value: &Value) -> Option<[f64; 3]> {
    let vector: [f64; 3] = serde_json::from_value(value.clone()).ok()?;
    vector
        .iter()
        .all(|value| value.is_finite())
        .then_some(vector)
}

fn position(frame: &Value, actor: u64) -> Option<[f64; 3]> {
    vector(
        &frame["actors"]
            .as_array()?
            .iter()
            .find(|entry| entry["actor"] == actor)?["position"],
    )
}

fn project(position: [f64; 3], origin: [f64; 3], basis: [f64; 2]) -> [f64; 2] {
    let (x, z) = (position[0] - origin[0], position[2] - origin[2]);
    [x * basis[0] + z * basis[1], -x * basis[1] + z * basis[0]]
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.into_iter()
        .zip(b)
        .all(|(a, b)| a.is_finite() && b.is_finite() && (a - b).abs() <= 0.01)
}
