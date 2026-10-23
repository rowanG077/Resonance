use super::*;
use resonance_content::overworld::*;

fn fixture() -> Cinematic {
    Cinematic {
        world: World::Sylvarant,
        camera: vec![
            CameraKey {
                time: 0.,
                position: [0.; 3],
                target: [0.; 3],
            },
            CameraKey {
                time: 2.,
                position: [4.; 3],
                target: [2.; 3],
            },
        ],
        actors: Default::default(),
        dialogue: vec![CinematicDialogue {
            tick: 2,
            duration: 8,
            speaker: 1,
            voice: 123,
            text: "fixture".into(),
        }],
        presentation: CinematicPresentation {
            provenance: "synthetic changed-parameter fixture".into(),
            fade: CinematicFade {
                white: true,
                end_rate: 128,
                intro_rate: None,
                flashes: vec![],
            },
            initial_audio: vec![CinematicAudio::Sound {
                id: 42,
                pan: 20,
                volume: 30,
                slot: Some(7),
                repeat: true,
            }],
            audio: vec![],
            end_audio: vec![CinematicAudio::Stop { slot: 7 }],
            actors: Default::default(),
        },
    }
}

fn playback(definition: Cinematic) -> Result<Playback> {
    // No scene-specific code should override the recipe for scene 525.
    Playback::new(
        525,
        Arc::new(definition),
        SceneDestination {
            map: 1,
            position: [0.; 3],
            heading: 0.,
        },
    )
}

#[test]
fn prepared_audio_preserves_phase_order_duplicates_and_owned_end_stop() -> Result<()> {
    let mut definition = fixture();
    let sound = CinematicAudio::Sound {
        id: 99,
        pan: 64,
        volume: 127,
        slot: None,
        repeat: false,
    };
    definition.presentation.audio = vec![
        CinematicAudioEvent {
            tick: 2,
            before_dialogue: true,
            action: sound.clone(),
        },
        CinematicAudioEvent {
            tick: 2,
            before_dialogue: true,
            action: CinematicAudio::Volume {
                slot: 7,
                volume: 32,
            },
        },
        CinematicAudioEvent {
            tick: 2,
            before_dialogue: false,
            action: sound.clone(),
        },
        CinematicAudioEvent {
            tick: 2,
            before_dialogue: false,
            action: sound.clone(),
        },
        // Native tables can declare events after the camera has stopped.
        CinematicAudioEvent {
            tick: 5,
            before_dialogue: false,
            action: sound,
        },
    ];
    let mut playback = playback(definition)?;
    assert!(matches!(
        playback.initial_audio().as_slice(),
        [AudioCommand::RepeatSound {
            id: 42,
            pan: 20,
            volume: 30,
            slot: 7
        }]
    ));
    let mut audio = vec![];
    assert!(!playback.step(&mut audio));
    assert!(audio.is_empty());
    assert!(!playback.step(&mut audio));
    assert!(matches!(
        audio.as_slice(),
        [
            AudioCommand::Sound { id: 99, .. },
            AudioCommand::SoundVolume {
                slot: 7,
                volume: 32
            },
            AudioCommand::Voice { resource: 123, .. },
            AudioCommand::Sound { id: 99, .. },
            AudioCommand::Sound { id: 99, .. }
        ]
    ));
    assert_eq!(playback.camera().position, [2.; 3]);
    audio.clear();
    assert!(!playback.step(&mut audio));
    assert!(!playback.step(&mut audio));
    assert!(matches!(audio.as_slice(), [AudioCommand::StopSound(7)]));
    assert_eq!(playback.fade(), (0., true));
    audio.clear();
    assert!(!playback.step(&mut audio));
    assert_eq!(playback.fade(), (128. / 255., true));
    assert!(playback.step(&mut audio));
    assert_eq!(playback.fade(), (1., true));
    assert_eq!(playback.ticks(), 4);
    assert!(audio.is_empty());
    Ok(())
}

#[test]
fn changed_fade_rate_controls_completion_without_a_scene_id_override() -> Result<()> {
    let mut definition = fixture();
    definition.presentation.fade.end_rate = 17;
    let mut playback = playback(definition)?;
    let mut audio = vec![];
    for _ in 0..18 {
        assert!(!playback.step(&mut audio));
    }
    assert_eq!(playback.fade(), (238. / 255., true));
    assert!(playback.step(&mut audio));
    assert_eq!(playback.fade(), (1., true));
    Ok(())
}
