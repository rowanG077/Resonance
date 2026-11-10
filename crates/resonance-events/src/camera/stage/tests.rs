//! Snapshots from original DOL Native93 (80042FE4), REL initialization/control
//! (5CEA4/5C7F4), and angular consumer/helper (50924..5096C/4D750), executed
//! under qemu-ppc with only section relocations and a consumer return trampoline.
//! The ignored original-binary identity test pins the input revision and bodies.
use super::*;

#[test]
fn angular_commands_follow_original_native_snapshots() {
    let mut camera = Camera::new(35.).unwrap();
    let cases = [
        (
            "pose_full_before_update",
            Some([2, 76, 150, -102, -7, 360, 999]),
            0,
            [
                0xc0e00000, 0x43b40000, 0xc0e00000, 0x43b40000, 0x3f800000, 0x3f800000, 0x420c0000,
            ],
        ),
        (
            "pose_full_after_update",
            None,
            1,
            [
                0xc0e00000, 0x43b40000, 0xc0e00000, 0x43b40000, 0x00000000, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "pitch_timed_first",
            Some([5, -13, -2147483588, 7, 8, 9, 10]),
            1,
            [
                0xc0e33333, 0x43b40000, 0xc1500000, 0x43b40000, 0x3dcccccd, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "pitch_timed_30",
            None,
            29,
            [
                0xc1200007, 0x43b40000, 0xc1500000, 0x43b40000, 0x3dcccccd, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "pitch_timed_60",
            None,
            30,
            [
                0xc1500000, 0x43b40000, 0xc1500000, 0x43b40000, 0x00000000, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "pitch_timed_61",
            None,
            1,
            [
                0xc1500000, 0x43b40000, 0xc1500000, 0x43b40000, 0x00000000, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "pitch_by_first",
            Some([6, 20, -2147483608, 0, 0, 0, 0]),
            1,
            [
                0xc1480000, 0x43b40000, 0x40e00000, 0x43b40000, 0x3f000000, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "pitch_by_40",
            None,
            39,
            [
                0x40e00000, 0x43b40000, 0x40e00000, 0x43b40000, 0x00000000, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "pitch_by_41",
            None,
            1,
            [
                0x40e00000, 0x43b40000, 0x40e00000, 0x43b40000, 0x00000000, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "heading_to_30",
            Some([7, -270, -2147483588, 0, 0, 0, 0]),
            30,
            [
                0x40e00000, 0x42340000, 0x40e00000, 0xc3870000, 0x00000000, 0x41280000, 0x420c0000,
            ],
        ),
        (
            "heading_to_60",
            None,
            30,
            [
                0x40e00000, 0xc3870000, 0x40e00000, 0xc3870000, 0x00000000, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "heading_by_10",
            Some([8, 90, 300, 0, 0, 0, 0]),
            10,
            [
                0x40e00000, 0xc3700000, 0x40e00000, 0xc3340000, 0x00000000, 0x40400000, 0x420c0000,
            ],
        ),
        (
            "heading_by_interrupt_before",
            Some([8, -30, -2147483647, 0, 0, 0, 0]),
            0,
            [
                0x40e00000, 0xc3700000, 0x40e00000, 0xc3870000, 0x00000000, 0x41f00000, 0x420c0000,
            ],
        ),
        (
            "heading_by_interrupt_after",
            None,
            1,
            [
                0x40e00000, 0xc3870000, 0x40e00000, 0xc3870000, 0x00000000, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "pitch_zero_speed",
            Some([5, 90, 0, 0, 0, 0, 0]),
            3,
            [
                0x40e00000, 0xc3870000, 0x42b40000, 0xc3870000, 0x00000000, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "heading_zero_count_before",
            Some([7, 42, -2147483648, 0, 0, 0, 0]),
            0,
            [
                0x40e00000, 0xc3870000, 0x42b40000, 0x42280000, 0x00000000, 0x7f800000, 0x420c0000,
            ],
        ),
        (
            "heading_zero_count_after",
            None,
            1,
            [
                0x40e00000, 0x42280000, 0x42b40000, 0x42280000, 0x00000000, 0x00000000, 0x420c0000,
            ],
        ),
        (
            "pose_interrupt",
            Some([2, 1, 2, 3, 4, 5, -1]),
            0,
            [
                0x40800000, 0x40a00000, 0x40800000, 0x40a00000, 0x3f800000, 0x3f800000, 0x420c0000,
            ],
        ),
        (
            "lens_71",
            Some([10, 71, 123, 9, 8, 7, 6]),
            0,
            [
                0x40800000, 0x40a00000, 0x40800000, 0x40a00000, 0x3f800000, 0x3f800000, 0x40e33333,
            ],
        ),
        (
            "large_heading",
            Some([7, 2147483647, -2147483647, 0, 0, 0, 0]),
            1,
            [
                0x40800000, 0x4f000000, 0x40800000, 0x4f000000, 0x00000000, 0x00000000, 0x40e33333,
            ],
        ),
        (
            "relative_rounding",
            Some([8, 1, -2147483647, 0, 0, 0, 0]),
            1,
            [
                0x40800000, 0x4f000000, 0x40800000, 0x4f000000, 0x00000000, 0x00000000, 0x40e33333,
            ],
        ),
    ];
    for (name, command, updates, expected) in cases {
        if let Some(command) = command {
            camera.command(command).unwrap();
        }
        for _ in 0..updates {
            camera.step();
        }
        assert_eq!(
            [
                camera.vertical.value.to_bits(),
                camera.heading.value.to_bits(),
                camera.vertical.target.to_bits(),
                camera.heading.target.to_bits(),
                camera.vertical.speed.to_bits(),
                camera.heading.speed.to_bits(),
                camera.frame().vertical_fov_degrees.to_bits(),
            ],
            expected,
            "{name}",
        );
    }
    assert_eq!(camera.frame().eye, [1., 2., 3.]);
}

#[test]
fn set_pose_replaces_an_active_position_move_and_both_angles() {
    let mut camera = Camera::new(35.).unwrap();
    camera.command([2, 76, 150, -102, -7, 360, 999]).unwrap();
    camera
        .command([3, -4, 150, -102, i32::MIN | 120, 0, 0])
        .unwrap();
    camera.command([5, -13, i32::MIN | 60, 0, 0, 0, 0]).unwrap();
    camera.step();
    assert_ne!(camera.frame().eye, [76., 150., -102.]);
    camera.command([2, 1, 2, 3, 4, 5, -1]).unwrap();
    // Presentation publishes the new pose on its next visit.
    assert_ne!(camera.frame().eye, [1., 2., 3.]);
    camera.step();
    assert_eq!(camera.frame().eye, [1., 2., 3.]);
    assert_eq!(camera.vertical.value, 4.);
    assert_eq!(camera.heading.value, 5.);
    assert!(camera.eye.settled());
}
