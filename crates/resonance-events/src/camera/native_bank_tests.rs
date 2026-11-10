//! Scalar control snapshots from the original eight-camera bank. The reference
//! ran 80060100/8005FD2C and native A0/A1/A2 in QEMU, adapting only paired-single
//! stack saves/restores; the ordinary floating-point saves and business code
//! were retained. The ignored original-binary test pins the unadapted bodies.
use super::*;
use crate::{EventRuntime, GameWorld, ResourceLibrary};
use std::sync::Arc;
use symphonia_script::{NativeCall, Program, Width};

#[test]
fn all_eight_camera_templates_match_original_scalar_controls_and_stay_independent() {
    let mut calls = Vec::new();
    let properties = [0, 1, 2, 3, 4, 5, 6, 7, 20, 21, 22, 23];
    for slot in 0..8 {
        calls.push((NativeCall::SelectCamera, vec![slot]));
        for property in properties {
            calls.push((NativeCall::GetCameraProperty, vec![property]));
        }
    }
    for slot in 0..8 {
        calls.push((NativeCall::SelectCamera, vec![slot]));
        for (property, value) in [
            (2, 1800 + slot * 100),
            (8, -1000 - slot),
            (9, 1000 + slot),
            (14, -2000 - slot),
            (15, 2000 + slot),
        ] {
            calls.push((NativeCall::SetCameraProperty, vec![property, value]));
        }
    }
    for selector in [7, 6, 5, 4, 3, 2, 1, 0, 262, 256, 257, -250, -256, i32::MIN] {
        calls.push((NativeCall::SelectCamera, vec![selector]));
        calls.push((NativeCall::GetCameraProperty, vec![2]));
    }
    calls.push((NativeCall::SelectCamera, vec![-1]));
    for property in properties {
        calls.push((NativeCall::GetCameraProperty, vec![property]));
    }
    calls.extend([
        (NativeCall::SetCameraProperty, vec![2, 3100]),
        (NativeCall::GetCameraProperty, vec![2]),
        (NativeCall::SelectCamera, vec![6]),
        (NativeCall::GetCameraProperty, vec![2]),
    ]);
    // Independent native snapshots, in the exact call order above.
    let expected = [
        0, 1, 1, 2700, 8, 8, 1, 1, 1, 322, 0, 42, 1890, 0, 1, 1, 2700, 8, 8, 1, 1, 1, 322, 0, 42,
        1890, 1, 1, 1, 2700, 8, 8, 1, 1, 1, 322, 0, 42, 1890, 2, 1, 1, 2700, 8, 8, 1, 1, 1, 322, 0,
        42, 1890, 3, 1, 1, 2700, 8, 8, 1, 1, 1, 322, 0, 42, 1890, 4, 1, 1, 2700, 8, 8, 1, 1, 1,
        322, 0, 42, 1890, 5, 1, 1, 2700, 8, 8, 1, 1, 1, 322, 0, 42, 1890, 6, 1, 1, 2700, 8, 8, 1,
        1, 1, 322, 0, 42, 1890, 7, 2700, -10000, 10000, -10000, 10000, 0, 2700, -10000, 10000,
        -10000, 10000, 1, 2700, -10000, 10000, -10000, 10000, 2, 2700, -10000, 10000, -10000,
        10000, 3, 2700, -10000, 10000, -10000, 10000, 4, 2700, -10000, 10000, -10000, 10000, 5,
        2700, -10000, 10000, -10000, 10000, 6, 2700, -10000, 10000, -10000, 10000, 7, 2500, 7,
        2400, 6, 2300, 5, 2200, 4, 2100, 3, 2000, 2, 1900, 1, 1800, 0, 2400, 6, 1800, 0, 1900, 1,
        2400, 6, 1800, 0, 1800, 0, 1, 1, 2700, 8, 8, 1, 1, 1, 322, 0, 42, 1890, 2700, 3100, 0,
        3100,
    ];
    assert_eq!(calls.len(), expected.len());
    let mut code = Vec::new();
    for (index, (call, arguments)) in calls.iter().enumerate() {
        for &argument in arguments {
            code.extend([
                0x0200,
                argument as u16,
                (argument as u32 >> 16) as u16,
                0x3000,
                0x4000,
            ]);
        }
        code.push(0x2000 | *call as u16);
        code.extend([
            0x3000,
            0x1200,
            0x100 + index as u16 * 4,
            0x1200,
            0x20,
            0x3010,
            0x3000,
        ]);
    }
    code.extend([0x0200, 777, 0, 0x3000, 0x4000, 0x2068, 0x20ff]);
    let bytes = [4, 0, 0, 0]
        .into_iter()
        .chain(code)
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
    let world = GameWorld {
        controlled_actor: 1,
        actors: [(1, Actor::new(1, [0.; 3]))].into(),
        field_camera: Some(CameraRig::following(1)),
        ..Default::default()
    };
    let mut events = EventRuntime::with_state(
        Arc::new(Program::decode(&bytes).unwrap()),
        Arc::new(ResourceLibrary::default()),
        world,
        Default::default(),
    )
    .unwrap();
    for _ in 0..16 {
        if events.world.event_flags.contains(&777) {
            break;
        }
        events.step().unwrap();
    }
    assert!(events.world.event_flags.contains(&777));
    for (index, ((call, arguments), expected)) in calls.iter().zip(expected).enumerate() {
        assert_eq!(
            events
                .memory()
                .read(0x100 + index as u16 * 4, Width::S32)
                .unwrap(),
            expected,
            "call {index}: {call:?}{arguments:?}"
        );
    }
    let rig = events.world.field_camera.as_ref().unwrap();
    assert_eq!(rig.cameras.len(), 8);
    assert_eq!(rig.selected, 6);
    assert_eq!(rig.entry.as_ref().unwrap().camera.fov_degrees, 31.);
    assert_eq!(
        rig.entry.as_ref().unwrap().camera.position_bounds,
        [[-10000., 10000.]; 3]
    );
    // Raw final native-bank fields prove that editing the entry camera and
    // switching through byte-aliased selectors did not overwrite any template.
    let expected_fields = [
        [0x41900000, 0xc47a0000, 0x447a0000, 0xc4fa0000, 0x44fa0000],
        [0x41980000, 0xc47a4000, 0x447a4000, 0xc4fa2000, 0x44fa2000],
        [0x41a00000, 0xc47a8000, 0x447a8000, 0xc4fa4000, 0x44fa4000],
        [0x41a80000, 0xc47ac000, 0x447ac000, 0xc4fa6000, 0x44fa6000],
        [0x41b00000, 0xc47b0000, 0x447b0000, 0xc4fa8000, 0x44fa8000],
        [0x41b80000, 0xc47b4000, 0x447b4000, 0xc4faa000, 0x44faa000],
        [0x41c00000, 0xc47b8000, 0x447b8000, 0xc4fac000, 0x44fac000],
        [0x41c80000, 0xc47bc000, 0x447bc000, 0xc4fae000, 0x44fae000],
    ];
    for (slot, expected) in expected_fields.into_iter().enumerate() {
        let camera = &rig.cameras[slot];
        assert_eq!(
            [
                camera.fov_degrees.to_bits(),
                camera.position_bounds[0][0].to_bits(),
                camera.position_bounds[0][1].to_bits(),
                camera.target_bounds[0][0].to_bits(),
                camera.target_bounds[0][1].to_bits()
            ],
            expected,
            "template {slot}"
        );
    }
}
