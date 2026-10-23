//! Model admission for synchronous item release must finish before any clock write.
use super::*;
use crate::ResourceId;
use crate::model::{Model, ModelDefinition, MotionBinding};

fn definition() -> ModelDefinition {
    let mut definition = crate::tests::model_definition(7, [(1, 8.), (2, 10.)]);
    definition.initial.frame = 1.;
    let child = |slot, offset, mapped| {
        let mut entries = vec![(offset, 2.), (offset + 1, 8.)];
        if mapped {
            entries.push((offset + 2, 5.));
        }
        Arc::new(WeaponDefinition {
            slot: Some(crate::WeaponSlot(slot)),
            attachment: Some(0),
            effect_interval: None,
            initially_visible: true,
            layers: vec![crate::WeaponLayerDefinition {
                texture_animation: None,
                resource: ResourceId(u32::from(offset)),
                skeleton: definition.skeleton.clone(),
                motions: crate::tests::model_definition(u32::from(offset), entries).motions,
                playback: WeaponPlayback::Owner {
                    offset,
                    fallback: offset,
                    initial: Playback {
                        clip: offset + 1,
                        frame: 1.,
                        rate: 0.25,
                        repeat: true,
                    },
                },
                secondary_motion: vec![],
            }],
            links: vec![],
        })
    };
    definition.weapons = vec![child(0, 60, true), child(1, 100, false)];
    definition
}

fn fixture() -> Result<(Model, crate::Actor)> {
    let definition = definition();
    let actor = crate::tests::actor(crate::Side::Party);
    let mut model = Model::new(Arc::new(definition), ActorId(0), &actor)?;
    for _ in 0..3 {
        model.step(&actor, true)?;
    }
    Ok((model, actor))
}

#[test]
fn rejected_play_preserves_visible_body_children_and_continuation() -> Result<()> {
    let (mut model, actor) = fixture()?;
    let mut control = model.clone();
    assert!(
        model
            .play(
                MotionBinding {
                    model: ResourceId(7),
                    clip: 99
                },
                4.,
                0.5,
                false,
                2
            )
            .is_err()
    );
    assert_eq!(model.shown, control.shown);
    assert_eq!(model.weapon_frames(), control.weapon_frames());
    for advance in [
        false, true, true, false, true, true, true, true, true, true, true, true,
    ] {
        model.step(&actor, advance)?;
        control.step(&actor, advance)?;
        assert_eq!(model.shown, control.shown);
        assert_eq!(model.weapon_frames(), control.weapon_frames());
    }
    Ok(())
}

#[test]
fn prepared_play_preserves_global_holds_and_independent_child_endpoints() -> Result<()> {
    let (mut model, actor) = fixture()?;
    let before = model.shown.clone();
    let binding = MotionBinding {
        model: ResourceId(7),
        clip: 2,
    };
    let prepared = model.prepare_play(binding, 4., 0.5, false, 2)?;
    model.apply_play(prepared);
    model.step(&actor, false)?;
    assert_eq!((model.shown.clip, model.shown.frame), (2, 4.));
    assert_eq!(model.shown.bones, before.bones);
    assert_eq!(model.weapons[0].layers[0].shown.frame, 2.);
    assert_eq!(model.weapons[1].layers[0].shown.frame, 0.);
    model.step(&actor, true)?;
    let held = model.shown.clone();
    let children = model.weapon_frames();
    model.step(&actor, false)?;
    assert_eq!(model.shown, held);
    assert_eq!(model.weapon_frames(), children);
    for _ in 0..20 {
        model.step(&actor, true)?;
    }
    assert_eq!(model.shown.frame, 10.);
    assert_eq!(model.weapons[0].layers[0].shown.frame, 5.);
    assert_eq!(model.weapons[1].layers[0].shown.frame, 2.);
    Ok(())
}
