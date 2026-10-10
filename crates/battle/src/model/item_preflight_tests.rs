//! Model admission for synchronous item release must finish before any clock write.
use super::*;
use crate::model::{Model, ModelDefinition, MotionBinding};
use resonance_content::animation::{Bone, Transform, TransformChannels};

fn definition() -> ModelDefinition {
    let skeleton = Skeleton {
        bones: vec![Bone {
            name: "root".into(),
            parent: None,
            bind_channels: TransformChannels(8),
            bind: Transform::default(),
        }],
    };
    let motions = |entries: &[(u16, f32)]| {
        entries
            .iter()
            .map(|&(clip, duration_frames)| {
                (
                    clip,
                    Motion {
                        duration_frames,
                        tracks: vec![],
                    },
                )
            })
            .collect()
    };
    let child = |slot, offset, mapped| {
        let mut entries = vec![(offset, 2.), (offset + 1, 8.)];
        if mapped {
            entries.push((offset + 2, 5.));
        }
        Arc::new(WeaponDefinition {
            slot,
            attachment: 0,
            layers: vec![crate::WeaponLayerDefinition {
                resource: u32::from(offset),
                skeleton: skeleton.clone(),
                motions: motions(&entries),
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
    ModelDefinition {
        tint: [64, 64, 64, 255],
        fade_on_defeat: false,
        resource: 7,
        skeleton: skeleton.clone(),
        motions: motions(&[(1, 8.), (2, 10.)]),
        initial: Playback {
            clip: 1,
            frame: 1.,
            rate: 0.5,
            repeat: true,
        },
        secondary_motion: vec![],
        reactions: Default::default(),
        idle_motions: [None; 2],
        idle_expression: [0; 4],
        blink: None,
        weapons: vec![child(0, 60, true), child(1, 100, false)],

        shadow: None,
        suppress_root_translation: [false; 3],
    }
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
            .play(MotionBinding { model: 7, clip: 99 }, 4., 0.5, false, 2)
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
    let binding = MotionBinding { model: 7, clip: 2 };
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
