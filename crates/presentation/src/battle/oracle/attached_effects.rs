//! Attachment matrices and their root placements publish in separate phases.
use super::Trace;
use anyhow::{Context, Result, ensure};
use serde_json::Value;

/// Compare attachments in the current root frame. Source roots use the fixed
/// preceding publication, never a searched-for match. Missing actor admission
/// roots stay strict; retained model caches have separately verified validity.
pub(super) fn canonicalize(
    source: &Trace,
    frame: usize,
    source_side: bool,
    model: &mut Value,
) -> Result<()> {
    if model["attachment"]["kind"] != "bone" {
        return retained_parent(source, frame, source_side, model);
    }
    if !source_side {
        return Ok(());
    }
    let current_observation = source
        .frames
        .get(frame)
        .context("missing attachment publication frame")?;
    let previous_observation = if frame == 0 {
        let Some(previous) = source.setup.get("pre_admission_actors") else {
            return Ok(());
        };
        ensure!(
            previous["presentation_tick"]
                .as_u64()
                .and_then(|tick| tick.checked_add(1))
                == Some(
                    current_observation["presentation_tick"]
                        .as_u64()
                        .context("missing attachment admission clock")?
                ),
            "attachment admission root is not the preceding observation"
        );
        previous
    } else {
        &source.frames[frame - 1]
    };
    let actor = model["attachment"]["actor"]
        .as_u64()
        .context("bone attachment has no actor")?;
    let root = |observation: &Value| -> Result<([f64; 3], f64)> {
        let parent = observation["actors"]
            .as_array()
            .context("missing attachment actor observations")?
            .iter()
            .find(|parent| parent["actor"].as_u64() == Some(actor))
            .context("bone attachment actor was not observed")?;
        let position: [f64; 3] = serde_json::from_value(parent["position"].clone())?;
        let heading = parent["heading"]
            .as_f64()
            .context("missing attachment actor heading")?;
        ensure!(
            position.iter().all(|value| value.is_finite()) && heading.is_finite(),
            "invalid attachment actor placement"
        );
        Ok((position, heading))
    };
    let (previous, previous_heading) = root(previous_observation)?;
    let (current, current_heading) = root(current_observation)?;
    let mut world: [f64; 12] = serde_json::from_value(model["model"]["world"].clone())?;
    ensure!(
        world.iter().all(|value| value.is_finite()),
        "invalid attached model matrix"
    );
    let (sine, cosine) = (current_heading - previous_heading).to_radians().sin_cos();
    world[3] -= previous[0];
    world[11] -= previous[2];
    for column in 0..4 {
        let x = world[column];
        let z = world[8 + column];
        world[column] = cosine * x + sine * z;
        world[8 + column] = cosine * z - sine * x;
    }
    world[3] += current[0];
    world[11] += current[2];
    world[7] += current[1] - previous[1];
    model["model"]["world"] = serde_json::to_value(world)?;
    Ok(())
}

/// A retained model joint has the current animation and preceding parent root.
/// Only translation is qualified here; changing parent bases remain strict.
fn retained_parent(
    source: &Trace,
    frame: usize,
    source_side: bool,
    model: &mut Value,
) -> Result<()> {
    let Some(relations) = source.setup.get("effect_model_parents") else {
        return Ok(());
    };
    let key_matches = |key: &Value, value: &Value| {
        ["owner", "bank", "member", "ordinal"]
            .into_iter()
            .all(|field| key[field] == value[field])
    };
    let mut relations = relations
        .as_array()
        .context("invalid retained model relations")?
        .iter()
        .filter(|relation| key_matches(&relation["child"], model));
    let Some(relation) = relations.next() else {
        return Ok(());
    };
    ensure!(
        relations.next().is_none(),
        "ambiguous retained model parent"
    );
    ensure!(relation["joint"].is_u64(), "missing retained model joint");
    let parent = |index: usize| -> Result<Option<&Value>> {
        let mut matches = source.frames[index]["effect_models"]
            .as_array()
            .context("missing retained model observations")?
            .iter()
            .filter(|value| key_matches(&relation["parent"], value));
        let first = matches.next();
        ensure!(
            matches.next().is_none(),
            "ambiguous retained model observation"
        );
        Ok(first)
    };
    let current = parent(frame)?.context("retained model parent was not observed")?;
    let previous = frame.checked_sub(1).map(parent).transpose()?.flatten();
    if previous.is_none() && unpublished_birth(source, frame, relation)? {
        // The first render has not populated this generation's joint cache.
        // Presence, animation identity and the parent's transform remain compared.
        model["model"]
            .as_object_mut()
            .context("invalid retained model")?
            .remove("world");
        return Ok(());
    }
    let Some(previous) = previous.filter(|_| source_side) else {
        return Ok(());
    };
    let current_model = &current["model"];
    let current: [f64; 12] = serde_json::from_value(current_model["world"].clone())?;
    let previous_model = &previous["model"];
    let previous: [f64; 12] = serde_json::from_value(previous_model["world"].clone())?;
    let mut world: [f64; 12] = serde_json::from_value(model["model"]["world"].clone())?;
    ensure!(
        current
            .iter()
            .chain(&previous)
            .chain(&world)
            .all(|value| value.is_finite()),
        "invalid retained model matrix"
    );
    if (0..12)
        .filter(|index| index % 4 != 3)
        .any(|index| current[index] != previous[index])
    {
        return Ok(());
    }
    for index in [3, 7, 11] {
        world[index] += current[index] - previous[index];
    }
    if let Some(updated) = transitioned_joint(
        source,
        frame,
        relation,
        current_model,
        previous_model,
        &world,
    )? {
        world = updated;
    }
    model["model"]["world"] = serde_json::to_value(world)?;
    Ok(())
}

/// A controller replacement can precede publication of its joint cache. Verify
/// the observed outgoing pose before comparing the newly selected authored pose.
fn transitioned_joint(
    source: &Trace,
    frame: usize,
    relation: &Value,
    current: &Value,
    previous: &Value,
    world: &[f64; 12],
) -> Result<Option<[f64; 12]>> {
    use super::random_inputs::Pin;
    use resonance_content::{
        animation::{Motion, multiply},
        battle_scene::Scene,
    };
    let Some(transitions) = source.frames[frame].get("model_parent_transitions") else {
        return Ok(None);
    };
    let mut entries = transitions
        .as_array()
        .context("invalid joint publication transitions")?
        .iter()
        .filter(|entry| entry["child"] == relation["child"]);
    let Some(entry) = entries.next() else {
        return Ok(None);
    };
    ensure!(
        entries.next().is_none(),
        "ambiguous joint publication transition"
    );
    ensure!(
        current["clip"] != previous["clip"] && entry["outgoing"]["clip"] == previous["clip"],
        "joint publication lacks its outgoing controller transition"
    );
    let plans = source.setup["observer"]["model_joints"]
        .as_array()
        .context("missing joint observation plan")?;
    let mut plans = plans
        .iter()
        .filter(|plan| plan["bank"] == relation["child"]["bank"]);
    let plan = plans.next().context("missing bound joint scene")?;
    ensure!(plans.next().is_none(), "ambiguous bound joint scene");
    let scene_pin: Pin = serde_json::from_value(plan["scene"].clone())?;
    let scene: Scene = serde_json::from_slice(&scene_pin.read_bytes()?)?;
    let model = entry["model"]
        .as_u64()
        .and_then(|value| u8::try_from(value).ok())
        .context("invalid joint model slot")?;
    let definition = scene
        .models
        .get(&model)
        .context("missing joint model slot")?;
    let mut joints = plan["joints"]
        .as_array()
        .context("missing bound joints")?
        .iter()
        .filter(|joint| joint["model"] == model && joint["joint"] == relation["joint"]);
    let bone = joints.next().context("missing bound joint")?["bone"]
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .context("invalid bound joint")?;
    ensure!(joints.next().is_none(), "ambiguous bound joint");
    let scene_path = std::path::Path::new(
        plan["scene"]["path"]
            .as_str()
            .context("missing scene path")?,
    );
    let assets = scene_path
        .ancestors()
        .nth(3)
        .context("invalid scene asset path")?;
    let root: [f32; 12] = serde_json::from_value(current["world"].clone())?;
    let root = std::array::from_fn(|column| {
        std::array::from_fn(|row| {
            if row < 3 {
                root[row * 4 + column]
            } else {
                if column == 3 { 1. } else { 0. }
            }
        })
    });
    let pose = |controller: &Value| -> Result<[f64; 12]> {
        let clip = controller["clip"]
            .as_u64()
            .context("missing joint controller clip")?;
        let mut clips = definition
            .layers
            .iter()
            .flat_map(|layer| &layer.scene.clips)
            .filter(|candidate| u64::from(candidate.resource_slot) == clip);
        let clip = clips.next().context("missing joint controller motion")?;
        ensure!(
            clips.all(|other| other.motion == clip.motion),
            "ambiguous joint controller motion"
        );
        let file = scene
            .files
            .get(&clip.motion)
            .context("unbound joint controller motion")?;
        let pin: Pin = serde_json::from_value(
            serde_json::json!({"path":assets.join(&clip.motion),"sha256":file.sha256}),
        )?;
        let motion = Motion::decode(&pin.read_bytes()?)?;
        let frame = controller["frame"]
            .as_f64()
            .context("missing joint controller frame")? as f32;
        let matrix = multiply(
            root,
            definition
                .rig
                .skeleton
                .sample_matrix(&motion, frame, bone)?,
        );
        Ok(super::affine(matrix).map(f64::from))
    };
    let outgoing = pose(&entry["outgoing"])?;
    super::compare::equal(
        &serde_json::to_value(world)?,
        &serde_json::to_value(outgoing)?,
        "outgoing retained joint pose",
    )?;
    // A blend starts at the outgoing pose. Both observations already contain
    // that pose, so replacing it with the incoming clip would introduce a jump.
    let effects_file = scene
        .files
        .get(&scene.effects)
        .context("unbound joint scene effects")?;
    let effects_pin: Pin = serde_json::from_value(serde_json::json!({
        "path": assets.join(&scene.effects), "sha256": effects_file.sha256
    }))?;
    let effects: resonance_content::battle_effect::SourceBank =
        serde_json::from_slice(&effects_pin.read_bytes()?)?;
    ensure!(
        relation["parent"]["bank"] == effects.source_sha256,
        "joint controller effect bank changed"
    );
    let parent_age = source.frames[frame]["particles"]
        .as_array()
        .context("missing joint parent particles")?
        .iter()
        .find(|particle| particle["id"] == relation["parent_particle"])
        .and_then(|particle| particle["age"].as_u64())
        .context("missing joint parent age")?;
    use resonance_content::battle_effect::EffectOperation;
    let mut blended = Vec::new();
    for program in &effects.programs {
        let births = program
            .iter()
            .filter_map(|event| match event.operation {
                EffectOperation::Spawn { particle, .. } => Some((event.at, particle)),
                _ => None,
            })
            .collect::<Vec<_>>();
        for event in program {
            let EffectOperation::SetMotion { spawn, motion } = &event.operation else {
                continue;
            };
            let Some(animation) = motion.animation else {
                continue;
            };
            if animation.model != model
                || current["clip"] != u64::from(animation.clip)
                || animation.blend_ticks == 0
            {
                continue;
            }
            let &(born, particle) = births
                .get(*spawn)
                .context("missing retained motion birth")?;
            if relation["parent"]["member"] == u64::from(particle)
                && event.at.checked_sub(born).map(|age| u64::from(age) + 1) == Some(parent_age)
            {
                blended.push(animation);
            }
        }
    }
    if let Some(animation) = blended.first() {
        ensure!(
            blended.len() == 1 && current["frame"] == f64::from(animation.frame),
            "ambiguous or advanced retained model blend"
        );
        return Ok(None);
    }
    Ok(Some(pose(current)?))
}

fn unpublished_birth(source: &Trace, frame: usize, relation: &Value) -> Result<bool> {
    if frame == 0 {
        return Ok(false);
    }
    let observation = &source.frames[frame];
    let Some(child_id) = relation["child_particle"].as_u64() else {
        return Ok(false);
    };
    let Some(parent_id) = relation["parent_particle"].as_u64() else {
        return Ok(false);
    };
    let particles = observation["particles"]
        .as_array()
        .context("missing retained birth particles")?;
    let mut child_world = None;
    for (id, key) in [
        (child_id, &relation["child"]),
        (parent_id, &relation["parent"]),
    ] {
        let mut matches = particles
            .iter()
            .filter(|particle| particle["id"].as_u64() == Some(id));
        let particle = matches
            .next()
            .context("retained birth particle was not observed")?;
        ensure!(
            matches.next().is_none(),
            "ambiguous retained birth particle"
        );
        ensure!(
            ["owner", "bank", "member"]
                .into_iter()
                .all(|field| particle[field] == key[field]),
            "retained birth particle identity changed"
        );
        if particle["age"] != 1 || particle["initialized"] != true {
            return Ok(false);
        }
        if source.frames[frame - 1]["particles"]
            .as_array()
            .context("missing preceding retained birth particles")?
            .iter()
            .any(|previous| previous["id"].as_u64() == Some(id) && previous["initialized"] == true)
        {
            return Ok(false);
        }
        if id == child_id {
            let child = observation["effect_models"]
                .as_array()
                .context("missing retained child")?
                .iter()
                .find(|child| {
                    ["owner", "bank", "member", "ordinal"]
                        .into_iter()
                        .all(|field| child[field] == key[field])
                })
                .context("retained child was not observed")?;
            ensure!(
                child["model"] == particle["model"],
                "retained birth cache observations disagree"
            );
            child_world = Some(&particle["model"]["world"]);
        }
    }
    let world: [f64; 12] =
        serde_json::from_value(child_world.context("missing retained birth cache")?.clone())?;
    Ok(world == [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn observations() -> Trace {
        Trace {
            case: String::new(),
            setup: Value::Null,
            frames: vec![
                json!({"actors":[{"actor":2,"position":[10.,2.,20.],"heading":0.}]}),
                json!({"actors":[{"actor":2,"position":[30.,5.,40.],"heading":90.}]}),
            ],
            events: vec![],
            assessment: Value::Null,
            incomplete_observations: vec![],
        }
    }

    fn attachment() -> Value {
        json!({"owner":2,"bank":"bank","member":4,"ordinal":7,
            "attachment":{"kind":"bone","actor":2,"bone":10},
            "model":{"clip":3,"frame":12.,"world":[
                2.,0.,0.,15., 0.,3.,0.,7., 0.,0.,4.,23.]}})
    }

    #[test]
    fn prior_root_changes_without_changing_bone_pose_scale_or_identity() -> Result<()> {
        let original = attachment();
        let mut actual = original.clone();
        canonicalize(&observations(), 1, true, &mut actual)?;
        let mut expected = original;
        expected["model"]["world"] = json!([0., 0., 4., 33., 0., 3., 0., 10., -2., 0., 0., 35.]);
        super::super::compare::equal(&actual, &expected, "attached model")?;

        // A wrong bone offset or scale must still disagree after root conversion.
        for (element, value) in [(3, 16.), (0, 2.5)] {
            let mut wrong = attachment();
            wrong["model"]["world"][element] = json!(value);
            canonicalize(&observations(), 1, true, &mut wrong)?;
            assert!(super::super::compare::equal(&wrong, &expected, "wrong bone").is_err());
        }
        Ok(())
    }

    #[test]
    fn admission_and_unattached_models_stay_strict_and_missing_parents_fail() -> Result<()> {
        let original = attachment();
        let mut admission = original.clone();
        canonicalize(&observations(), 0, true, &mut admission)?;
        assert_eq!(admission, original);
        let mut detached = original.clone();
        detached["attachment"] = Value::Null;
        let unchanged = detached.clone();
        canonicalize(&observations(), 1, true, &mut detached)?;
        assert_eq!(detached, unchanged);
        let mut missing = original;
        missing["attachment"]["actor"] = json!(9);
        assert!(canonicalize(&observations(), 1, true, &mut missing).is_err());
        Ok(())
    }

    #[test]
    fn admission_accepts_only_an_immediately_preceding_root_observation() -> Result<()> {
        let mut source = observations();
        let mut previous = source.frames.remove(0);
        previous["presentation_tick"] = json!(10);
        source.frames[0]["presentation_tick"] = json!(11);
        source.setup = json!({"pre_admission_actors":previous});
        let mut actual = attachment();
        canonicalize(&source, 0, true, &mut actual)?;
        let mut expected = attachment();
        canonicalize(&observations(), 1, true, &mut expected)?;
        assert_eq!(actual, expected);
        source.setup["pre_admission_actors"]["presentation_tick"] = json!(9);
        assert!(canonicalize(&source, 0, true, &mut attachment()).is_err());
        Ok(())
    }

    #[test]
    fn retained_joint_rebases_only_observed_parent_translation() -> Result<()> {
        let mut source = observations();
        let key = json!({"owner":2,"bank":"bank","member":8,"ordinal":1});
        let mut child = key.clone();
        child["attachment"] = Value::Null;
        child["model"] = json!({"clip":null,"frame":null,
            "world":[2.,0.,0.,15.,0.,3.,0.,7.,0.,0.,4.,23.]});
        let mut parent_key = key.clone();
        parent_key["member"] = json!(7);
        let mut parent = parent_key.clone();
        parent["model"] = json!({"world":[1.,0.,0.,10.,0.,1.,0.,2.,0.,0.,1.,20.]});
        source.setup =
            json!({"effect_model_parents":[{"child":key,"parent":parent_key,"joint":0}]});
        source.frames[0]["effect_models"] = json!([parent]);
        parent["model"]["world"][3] = json!(30.);
        parent["model"]["world"][7] = json!(5.);
        source.frames[1]["effect_models"] = json!([parent]);
        let mut actual = child.clone();
        canonicalize(&source, 1, true, &mut actual)?;
        let mut expected = child.clone();
        expected["model"]["world"][3] = json!(35.);
        expected["model"]["world"][7] = json!(10.);
        assert_eq!(actual, expected);
        child["model"]["world"][0] = json!(2.5);
        canonicalize(&source, 1, true, &mut child)?;
        assert_ne!(child, expected);
        source.frames[1]["effect_models"] = json!([]);
        assert!(canonicalize(&source, 1, true, &mut expected).is_err());
        Ok(())
    }

    #[test]
    fn only_verified_unpublished_birth_caches_are_excluded() -> Result<()> {
        let mut source = observations();
        let child_key = json!({"owner":2,"bank":"bank","member":8,"ordinal":1});
        let parent_key = json!({"owner":2,"bank":"bank","member":7,"ordinal":1});
        let identity = json!([1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.]);
        let mut child = child_key.clone();
        child["model"] = json!({"clip":null,"frame":null,"world":identity});
        let mut parent = parent_key.clone();
        parent["model"] =
            json!({"clip":0,"frame":0.,"world":[1.,0.,0.,10.,0.,1.,0.,20.,0.,0.,1.,30.]});
        let particle = |id: u64, value: &Value| {
            let mut value = value.clone();
            value["id"] = json!(id);
            value["age"] = json!(1);
            value["initialized"] = json!(true);
            value
        };
        source.setup = json!({"effect_model_parents":[{"child":child_key,"parent":parent_key,
            "joint":0,"child_particle":10,"parent_particle":11}]});
        source.frames[0]["effect_models"] = json!([]);
        source.frames[0]["particles"] = json!([]);
        source.frames[1]["effect_models"] = json!([child, parent]);
        source.frames[1]["particles"] = json!([particle(10, &child), particle(11, &parent)]);
        for side in [true, false] {
            let mut observed = child.clone();
            canonicalize(&source, 1, side, &mut observed)?;
            assert!(observed["model"].get("world").is_none());
            assert_eq!(observed["member"], child["member"]);
        }
        source.frames[1]["particles"][0]["age"] = json!(2);
        let mut observed = child.clone();
        canonicalize(&source, 1, true, &mut observed)?;
        assert_eq!(observed, child);
        source.frames[1]["particles"][0]["age"] = json!(1);
        source.frames[1]["particles"][0]["model"]["world"][3] = json!(1.);
        assert!(canonicalize(&source, 1, true, &mut child).is_err());
        Ok(())
    }
}
