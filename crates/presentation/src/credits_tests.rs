use super::*;
use resonance_events::{EventRuntime, GameWorld, ResourceLibrary};
use resonance_game::clock::{UPDATE_RATE_DENOMINATOR, UPDATE_RATE_NUMERATOR};

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and extracted discs; publishes named session/credits and runs audio without a device"]
fn credits_finish_the_music_and_hold_before_resuming_and_cancel_cleanly() -> Result<()> {
    let work = crate::test_support::named_opening_fixture()?;
    let root = work.path();
    let library = resonance_content::library::Library::open(root, &[])?;
    let mut snapshot = library.snapshot();
    resonance_content::credits::document::capture(&mut snapshot, &|| false)?;
    let files = resonance_content::prepared::Files::from_snapshot(
        snapshot,
        &mut Default::default(),
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    let manifest = Manifest::load(&files)?;
    let font: BitmapFont = files.json("fonts/dialogue.json")?;
    let batches = layout(&manifest, &font)?;
    assert!(batches[1].positions.len() > 4);
    assert!(
        batches
            .iter()
            .flat_map(|b| &b.positions)
            .flatten()
            .all(|v| v.is_finite())
    );
    let audio = Audio {
        clip: Arc::new(crate::field_audio::Clip::prepare(
            files.read(&manifest.music.asset.path)?,
            &manifest.music,
        )?),
        rate: manifest.music.sample_rate,
        gain: 1.,
        mono: false,
    };
    let program = Arc::new(symphonia_script::Program::decode(
        &symphonia_script::scenario::assemble(
            ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\n\
         push.s8 18\ncalc 0\narg\npush.s8 0\ncalc 0\narg\nproc 0x6d\nend\n",
        )?,
    )?);
    let mut session = crate::new_game::Session::load(root)?;
    let resources = Arc::new(ResourceLibrary {
        session_data: session.events().resources().session_data.clone(),
        ..default()
    });
    let mut world = World::new();
    world.init_resource::<Assets<Audio>>();
    // Exercise cancellation first, then reuse the session and decoded music.
    for cancel in [true, false] {
        let mut state = GameWorld::default();
        state.party = session.field_mut().events.world.party.take();
        session.field_mut().events =
            EventRuntime::with_state(program.clone(), resources.clone(), state, default())?;
        let operation = session
            .events()
            .world
            .screen_request
            .as_ref()
            .unwrap()
            .operation
            .clone();
        world.insert_resource(session);
        begin(&mut world, audio.clone(), manifest.final_hold_ticks)?;
        let entity = world.resource::<Playback>().audio;
        if cancel {
            world
                .resource_mut::<crate::new_game::Session>()
                .events_mut()
                .cancel();
            retire_cancelled(&mut world);
            assert!(!world.contains_resource::<Playback>());
            assert!(world.get_entity(entity).is_err());
            assert_eq!(
                operation.progress().outcome,
                Some(resonance_events::Outcome::Cancelled)
            );
        } else {
            // No sink yet: preparation must not consume any of the credits clock.
            advance(&mut world);
            assert_eq!(operation.progress().position, 0);
            let (mixer, mut output) = resonance_playback::Offline::new();
            crate::audio_output::attach::<Audio>(&mut world, &mixer)?;
            let mut frame = 0;
            let mut audible = false;
            let music_ticks = (f64::from(manifest.music.frames) / f64::from(audio.rate) * UPDATE_HZ)
                .ceil() as u64;
            for tick in 1..=music_ticks + 2 {
                let until =
                    tick * u64::from(SOURCE_RATE) * UPDATE_RATE_DENOMINATOR / UPDATE_RATE_NUMERATOR;
                for _ in frame..until {
                    for _ in 0..2 {
                        let sample = output.next().unwrap();
                        assert!(sample.is_finite());
                        audible |= sample.abs() > 0.01;
                    }
                }
                frame = until;
                advance(&mut world);
                assert!(operation.is_pending());
                world
                    .resource_mut::<crate::new_game::Session>()
                    .field_mut()
                    .step(default())?;
                assert_eq!(
                    world.resource::<crate::new_game::Session>().events().tick(),
                    0
                );
                if world.resource::<Playback>().tail.is_some() {
                    break;
                }
            }
            assert!(audible);
            assert_eq!(world.resource::<Playback>().tail, Some(0));
            for _ in 1..manifest.final_hold_ticks {
                advance(&mut world);
                assert!(operation.is_pending());
            }
            assert!(world.resource::<Playback>().brightness() < 0.01);
            advance(&mut world);
            assert_eq!(
                operation.progress().outcome,
                Some(resonance_events::Outcome::Completed(Some(0)))
            );
            assert!(!world.contains_resource::<Playback>());
            assert!(world.get_entity(entity).is_err());
            let mut current = world.resource_mut::<crate::new_game::Session>();
            assert!(current.events().world.screen_request.is_none());
            current.events_mut().step()?;
            assert_eq!(current.events().active_instances(), 0);
        }
        session = world.remove_resource::<crate::new_game::Session>().unwrap();
    }
    Ok(())
}
