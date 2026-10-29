use super::*;
use anyhow::Result;
use serde_json::json;

#[test]
fn compact_particles_keep_required_geometry_and_explicit_overrides() -> Result<()> {
    let value = json!({"kind":"particle","template":{"lifetime":30,"state":{
        "geometry":{"Size":{"value":[1,2,3]}},"colors":[[128,128,128,128],[128,128,128,128]]
    }},"visual":{"geometry":{"kind":"quad"},"orientation":"world","texture":{"kind":"untextured"},"blend":"additive"}});
    let mut declaration: declaration::Declaration = serde_json::from_value(value.clone())?;
    let declaration::Declaration::Particle { template, visual } = &mut declaration else {
        unreachable!()
    };
    template.validate()?;
    assert_eq!(template.state.velocity, [0.; 3]);
    assert_eq!(template.jerk, [0.; 3]);
    assert_eq!(template.fade, [0; 4]);
    assert!(visual.depth_test && !visual.depth_write && !visual.anchored);
    assert!(!template.follow_origin && !template.follow_orientation);
    visual.depth_test = false;
    template.state.velocity = [0., 2., 0.];
    template.fade = [0, 0, 0, 8];
    let compact = serde_json::to_value(&declaration)?;
    assert_eq!(compact["visual"]["depth_test"], false);
    assert_eq!(
        compact["template"]["state"]["velocity"],
        json!([0., 2., 0.])
    );
    assert!(compact["template"].get("jerk").is_none());
    assert_eq!(
        serde_json::from_value::<declaration::Declaration>(compact)?,
        declaration
    );
    let mut incomplete = value;
    incomplete["template"]["state"]
        .as_object_mut()
        .unwrap()
        .remove("colors");
    assert!(serde_json::from_value::<declaration::Declaration>(incomplete).is_err());
    Ok(())
}

#[test]
fn fixed_birth_values_do_not_erase_zero_assignments_or_random_draws() -> Result<()> {
    let event: ScheduledEvent = serde_json::from_value(json!({"at":2,"operation":{
        "kind":"spawn","particle":3,"birth":{"velocity":[null,0,null],"lifetime":30,
        "size":[{"min":1,"max":5,"step":2,"draw":0,"reverse":true},null,null],"random_draws":1}
    }}))?;
    let EffectOperation::Spawn { birth, .. } = &event.operation else {
        unreachable!()
    };
    birth.validate()?;
    assert_eq!(birth.velocity[1], Some(ValueRange::fixed(0.)));
    assert_eq!(birth.lifetime, Some(ValueRange::fixed(30.)));
    assert_eq!(birth.size[0].unwrap().sample(0.4), 3.);
    assert_eq!(birth.random_draws, 1);
    let value = serde_json::to_value(&event)?;
    assert!(value["operation"].get("blend").is_none());
    assert_eq!(value["operation"]["birth"]["velocity"][1], 0.);
    assert_eq!(serde_json::from_value::<ScheduledEvent>(value)?, event);
    let motion: MotionChange = serde_json::from_value(json!({"velocity":[null,0,null]}))?;
    assert_eq!(
        serde_json::to_value(motion)?,
        json!({"velocity":[null,0.,null]})
    );
    let frozen: ModelAnimation = serde_json::from_value(json!({"model":0,"clip":1,"rate":0}))?;
    assert_eq!(frozen.rate, 0.);
    assert_eq!(serde_json::to_value(frozen)?["rate"], 0.);
    let ordinary: ModelAnimation = serde_json::from_value(json!({"model":0,"clip":1}))?;
    assert_eq!(ordinary.rate, 1.);
    Ok(())
}

#[test]
fn compact_ranges_keep_explicit_choice_metadata_and_reject_bad_syntax() -> Result<()> {
    let fixed = ValueRange::fixed(-4.5);
    assert_eq!(serde_json::to_value(fixed)?, json!(-4.5));
    assert_eq!(serde_json::from_value::<ValueRange>(json!(-4.5))?, fixed);
    for value in [
        ValueRange {
            draw: Some(0),
            ..fixed
        },
        ValueRange { step: 1., ..fixed },
        ValueRange {
            reverse: true,
            ..fixed
        },
        ValueRange {
            min: -0.,
            max: 0.,
            ..ValueRange::fixed(0.)
        },
    ] {
        let stored = serde_json::to_value(value)?;
        assert!(stored.is_object());
        assert_eq!(serde_json::from_value::<ValueRange>(stored)?, value);
    }
    for invalid in [
        json!(true),
        json!("4"),
        json!({"min":0}),
        json!({"min":0,"max":1,"mxa":2}),
    ] {
        assert!(serde_json::from_value::<ValueRange>(invalid).is_err());
    }
    assert!(
        serde_json::from_value::<ValueRange>(json!({"min":2,"max":1}))?
            .validate()
            .is_err()
    );
    Ok(())
}
