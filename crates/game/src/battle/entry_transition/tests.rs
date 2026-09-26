use super::*;

fn source() -> ScreenBreak {
    // The first source triangle (point indices 10,14,15), repeated to retain
    // the source record count. Geometry does not affect the random draw order.
    let mut points = vec![[0.; 3]; 43];
    points[10] = [219.58041, 403.35663, 0.];
    points[14] = [297.57748, 411.94366, 0.];
    points[15] = [311.1111, 480., 0.];
    ScreenBreak {
        points,
        triangles: vec![[10, 14, 15]; 62],
        viewport: [640., 480.],
        viewport_center: [320., 240.],
        center_weight: 0.333,
        center_expansion: 0.025,
        velocity_scale: 2.5,
        angular_base: 1.,
        angular_variation: 0.15,
        radians_per_degree: 0.017453292,
        secondary_rotation_scale: 1.5,
        draw_depth: -0.5,
    }
}

const SOUND: SoundBinding = SoundBinding {
    resource: 0,
    index: 130,
};

#[test]
fn source_records_retain_each_draw_before_actor_initialization() -> Result<()> {
    let mut random = Random::from_state(55023);
    let transition = EntryTransition::new(&source(), [128; 3], &mut random, SOUND)?;
    // Original seed-watch VI1612, before 40C8 creates any actor.
    assert_eq!(random.state(), 1_415_389_221);
    assert_eq!(transition.pieces().len(), 62);
    let first = &transition.pieces()[0];
    assert_eq!(first.center.map(f32::to_bits), [0x43895abe, 0x43da0f27]);
    assert_eq!(first.velocity.map(f32::to_bits), [0xbeb52a10, 0x4002bede]);
    assert_eq!(
        first.points[0].map(f32::to_bits),
        [0xc260eec8, 0xc1dfd3b0, 0]
    );
    assert_eq!(first.angular_velocity.to_bits(), 0x40633334);
    assert_eq!(
        transition.pieces()[61].angular_velocity.to_bits(),
        0x40633334
    );
    assert_eq!(first.angle, 0.);
    assert_eq!(first.uv[2][1], 1.);
    Ok(())
}

#[test]
fn source_clock_holds_then_breaks_and_survives_camera_handoff() -> Result<()> {
    let mut random = Random::from_state(55023);
    let mut transition = EntryTransition::new(&source(), [128; 3], &mut random, SOUND)?;
    let first = transition.pieces()[0].clone();
    for _ in 0..30 {
        assert_eq!(transition.advance(false), None);
    }
    assert_eq!(transition.timer(), 30);
    assert_eq!(transition.pieces()[0], first);
    assert_eq!(transition.advance(true), None);
    assert_eq!(transition.timer(), 30);
    assert_eq!(transition.advance(false), Some(SOUND));
    assert_eq!(transition.pieces()[0].angle, first.angular_velocity);
    assert_eq!(
        transition.pieces()[0].center,
        [
            first.center[0] + first.velocity[0],
            first.center[1] + first.velocity[1]
        ]
    );
    for age in 32..=181 {
        assert_eq!(transition.advance(false), None);
        assert_eq!(transition.timer(), age);
        match age {
            57 => assert_eq!(transition.alpha(), 255),
            58 => assert_eq!(transition.alpha(), 251),
            119 => assert_eq!(transition.alpha(), 7),
            120 | 180 | 181 => assert_eq!(transition.alpha(), 0),
            _ => {}
        }
        assert_eq!(transition.active(), age <= 180);
    }
    assert_eq!(transition.advance(false), None);
    assert_eq!(transition.timer(), 181);
    Ok(())
}

#[test]
fn malformed_geometry_does_not_consume_entry_randomness() {
    let mut source = source();
    source.triangles[61][2] = 43;
    let mut random = Random::from_state(55023);
    assert!(EntryTransition::new(&source, [128; 3], &mut random, SOUND).is_err());
    assert_eq!(random.state(), 55023);
}

#[test]
fn camera_fade_has_its_own_clock_and_uses_prepared_rgb() -> Result<()> {
    let mut random = Random::from_state(55023);
    let mut transition = EntryTransition::new(&source(), [120, 100, 80], &mut random, SOUND)?;
    let retained_seed = random.state();
    for _ in 0..81 {
        transition.advance(false);
    }
    assert!(transition.fade().is_none());
    transition.begin_camera_fade();
    assert_eq!(transition.fade().unwrap().color, [120, 100, 80]);
    // 40C8/P0 advances BDA8 alone, with full opacity still retained.
    transition.advance(false);
    assert_eq!(transition.timer(), 82);
    assert_eq!(transition.fade().unwrap().alpha, 255);
    for presentation in 1..=16 {
        transition.advance_camera_fade();
        transition.advance(false);
        assert_eq!(
            transition.fade().map_or(0, |fade| fade.alpha),
            255_u16.saturating_sub(presentation * 16) as u8,
        );
    }
    assert!(transition.fade().is_none());
    assert_eq!(transition.timer(), 98);
    assert_eq!(transition.alpha(), 91);
    transition.advance_camera_fade();
    assert!(transition.fade().is_none());
    assert_eq!(random.state(), retained_seed);
    Ok(())
}
