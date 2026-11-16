use crate::{Error, FPS, Options, Playback, Renderer, exposure, timeline::Visit};

#[test]
fn exposure_clock() {
    for time in [-1., f64::NAN, f64::INFINITY] {
        assert_eq!(exposure(time), 0.);
    }
    assert_eq!(exposure(5. + 0.999 / FPS as f64), 5.);
    assert_eq!(exposure(121. / FPS as f64), 121. / FPS as f64);
    // Subtracting a visit's start must not round an exact exposure backward.
    for start in [1, 37, 239] {
        for frame in start..start + 2 * FPS {
            let local = exposure(exposure(frame as f64 / FPS as f64) - start as f64 / FPS as f64);
            assert_eq!(local, (frame - start) as f64 / FPS as f64);
        }
    }
}

#[test]
fn growth_is_independent_of_time() {
    let mut renderer = Renderer::new().unwrap();
    let options = Options::default();
    let mut modes = vec![Playback::Growth, Playback::Journey];
    modes.extend(
        renderer
            .events()
            .into_iter()
            .map(|event| Playback::Event(event.id)),
    );
    for mode in modes {
        for (seconds, progress, expected) in [
            (2., 0.25, 0.25),
            (2., 0.75, 0.75),
            (7.5, 0.75, 0.75),
            (7.5, 2., 1.),
        ] {
            let frame = renderer
                .render(&mode, seconds, Some(progress), &options)
                .unwrap();
            assert_eq!(frame.seconds, seconds);
            assert_eq!(frame.progress, expected);
        }
    }
    assert!(matches!(
        renderer.render(&Playback::Event("missing".into()), 0., None, &options),
        Err(Error::UnknownEvent(_))
    ));
}

#[test]
fn keyframes_interpolate_motion_and_release_props_on_their_boundary() {
    let mut visit: Visit = serde_json::from_str(
        r#"{
        "event": {"id":"test", "title":"", "start":0, "duration":2, "growth":0.25},
        "actors": [[
            {"id":"actor", "t":0, "x":10, "y":20, "face":-1, "frame":1},
            {"id":"actor", "t":2, "x":30, "y":40, "face":-1, "frame":2}
        ]],
        "props": [[
            {"id":"prop", "type":"can", "t":0, "x":2, "y":3, "attach":"actor", "visible":true},
            {"id":"prop", "type":"can", "t":2, "x":80, "y":90, "visible":true}
        ]]
    }"#,
    )
    .unwrap();
    // Sample out of order, including outside the clip, to exercise seeking.
    for (time, actor_position, prop_position, frame, attached) in [
        (3., (30., 40.), (80., 90.), 2, false),
        (1., (20., 30.), (18., 33.), 1, true),
        (-1., (10., 20.), (8., 23.), 1, true),
        (2., (30., 40.), (80., 90.), 2, false),
    ] {
        let state = visit.sample(time);
        let actor = &state.actors[0];
        let prop = &state.props[0];
        assert_eq!((actor.x, actor.y), actor_position);
        assert_eq!(actor.frame, frame, "drawing changes only on its keyframe");
        assert_eq!((prop.x, prop.y), prop_position);
        assert_eq!(prop.actor.is_some(), attached);
        assert!(!state.finished);
    }
    visit.finale = true;
    assert!(!visit.sample(1.).finished);
    assert!(visit.sample(2.).finished);
    assert!(visit.sample(3.).finished);
}
