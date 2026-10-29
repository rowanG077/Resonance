use super::*;
use crate::TextureAnimation;
use serde_json::{Value, json};

fn read(sequence: Value) -> Result<TextureAnimation> {
    Ok(serde_json::from_value(
        json!({"texture":8,"sequence":sequence}),
    )?)
}

#[test]
fn editable_motion_parameters_preserve_loop_intro_and_precise_sampling() -> Result<()> {
    let mut scroll = read(json!({"kind":"scroll","ticks":1000,"step":[0.001,0.001]}))?;
    scroll.validate()?;
    assert_eq!(scroll.offset(999), [999f32 * 0.001; 2]);
    assert_eq!(scroll.offset(1000), [0.; 2]);
    scroll.delay_ticks = 3;
    scroll.loop_start = 1;
    assert_eq!(scroll.offset(3), [0.; 2]);
    assert_eq!(scroll.offset(1003), [0.001; 2]);

    let cycle = read(json!({"kind":"cycle","ticks":500,"turns":[2,-5]}))?;
    for tick in 0..500 {
        assert_eq!(
            cycle.offset(tick),
            [
                ((tick * 4) % 1000) as f32 / 1000.,
                (-(tick as i32) * 10).rem_euclid(1000) as f32 / 1000.
            ]
        );
    }
    assert_eq!(cycle.offset(500), [0.; 2]);
    let atlas = read(json!({"kind":"atlas","grid":[1,11],"interval":4}))?;
    assert_eq!(atlas.offset(40), [0., 10f32 / 11.]);
    assert_eq!(atlas.offset(43), [0., 10f32 / 11.]);
    assert_eq!(atlas.offset(44), [0.; 2]);
    let grid = read(json!({"kind":"atlas","grid":[2,3],"interval":2,"frames":3,"first":2}))?;
    assert_eq!(
        grid.offsets,
        [
            [0., 1f32 / 3.],
            [0., 1f32 / 3.],
            [0.5, 1f32 / 3.],
            [0.5, 1f32 / 3.],
            [0., 2f32 / 3.],
            [0., 2f32 / 3.]
        ]
    );
    Ok(())
}

#[test]
fn irregular_timelines_keep_held_frames_and_field_cinematic_ownership_flat() -> Result<()> {
    let sequence = json!([
        {"kind":"hold","ticks":10},
        {"kind":"scroll","ticks":4,"step":[0.25,0.],"origin":[1.,0.]},
        [2.,0.]
    ]);
    let mut animation = read(sequence.clone())?;
    animation.loop_start = 14;
    assert_eq!(animation.offset(10), [1., 0.]);
    assert_eq!(animation.offset(13), [1.75, 0.]);
    assert_eq!(animation.offset(1000), [2., 0.]);
    let written: Value = serde_json::from_slice(&serde_json::to_vec(&animation)?)?;
    assert!(written.get("offsets").is_none());
    let restored: TextureAnimation = serde_json::from_value(written)?;
    assert!(
        animation
            .offsets
            .iter()
            .zip(restored.offsets)
            .all(|(&a, b)| equal(a, b))
    );

    let motion = crate::field::TextureMotion::Sampled {
        animation: TextureAnimation {
            texture: (),
            delay_ticks: 0,
            loop_start: 0,
            offsets: animation.offsets,
        },
    };
    let field = serde_json::to_value(&motion)?;
    assert_eq!(field["kind"], "timeline");
    for key in ["animation", "texture", "delay_ticks", "loop_from"] {
        assert!(field.get(key).is_none());
    }
    serde_json::from_value::<crate::field::TextureMotion>(field)?;
    let cinematic = crate::overworld::CinematicUv {
        clock: crate::overworld::CinematicClock::Scene,
        animation: read(sequence)?,
    };
    let value = serde_json::to_value(&cinematic)?;
    assert_eq!(value["texture"], 8);
    assert!(value.get("animation").is_none());
    serde_json::from_value::<crate::overworld::CinematicUv>(value)?;
    Ok(())
}

#[test]
fn malformed_motions_are_bounded_and_unknown_instructions_are_errors() {
    for sequence in [
        json!([]),
        json!({"kind":"cycle","ticks":0,"turns":[1,1]}),
        json!({"kind":"hold","ticks":36001}),
        json!({"kind":"atlas","grid":[0,8]}),
        json!({"kind":"atlas","grid":[u32::MAX,u32::MAX],"interval":u32::MAX}),
        json!({"kind":"hold","ticks":1,"typo":2}),
        json!([{ "kind":"hold","ticks":36000},[0,0]]),
        json!({"kind":"scroll","ticks":36000,"step":[3e38,3e38]}),
    ] {
        assert!(read(sequence.clone()).is_err(), "accepted {sequence}");
    }
    assert!(
        serde_json::from_value::<TextureAnimation>(
            json!({"texture":0,"offsets":[[0,0]],"loop_start":0,"delay_ticks":0})
        )
        .is_err()
    );
}
