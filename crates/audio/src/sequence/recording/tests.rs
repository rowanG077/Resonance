use super::*;
use crate::{data::*, package::tests::playback_data};
use std::io::Cursor;

fn score(looping: bool) -> Arc<Loaded> {
    let (mut bank, mut score, mut tables) = playback_data();
    bank.programs.insert(
        1,
        vec![
            Command::Interpolation {
                mode: Interpolation::Direct,
                coefficients: 0,
            },
            Command::Envelope {
                envelope: Envelope::Ordinary(crate::envelope::Parameters::default()),
            },
            Command::StartSample { sample: 2 },
            Command::Wait {
                milliseconds: Some(35),
                from_start: false,
                key_off: false,
                sample_end: false,
            },
            Command::StopSample,
            Command::End,
        ],
    );
    let sample = Arc::make_mut(bank.samples.get_mut(&2).unwrap());
    sample.pcm = vec![12000; 160];
    sample.loop_pcm = sample.pcm.clone();
    sample.loop_start = 0;
    sample.loop_length = 160;
    score.initial_bpm_1024 = 120 * 1024;
    score.end_tick = 384;
    score.loop_start_tick = 96;
    score.tempos = vec![Tempo {
        tick: 192,
        bpm_1024: 180 * 1024,
    }];
    let note = |tick, velocity| Event {
        tick,
        channel: 0,
        kind: EventKind::Notes {
            source: VoiceSource::Sequence {
                group: 0,
                program: 0,
                drums: false,
            },
            voices: vec![Note {
                macro_id: 1,
                key: 60,
                velocity,
                pan: 64,
                priority: 1,
                max_voices: 42,
            }],
            length: 48,
        },
    };
    score.first_events = vec![note(0, 127), note(96, 50), note(288, 90)];
    score.loop_events = if looping {
        vec![note(96, 50), note(288, 90)]
    } else {
        vec![]
    };
    tables.mix.volume = std::array::from_fn(|index| (index as f32 / 127.).min(1.));
    tables.mix.alternate_volume = tables.mix.volume;
    Arc::new(Loaded::new(bank, score, tables, [[0., 0., 0.05, 0., 0.]; 2]).unwrap())
}

fn render_bytes(loaded: Arc<Loaded>) -> Result<(Rendered, Vec<i16>)> {
    let mut bytes = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(
        &mut bytes,
        hound::WavSpec {
            channels: 2,
            sample_rate: SOURCE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    let result = render(
        loaded.clone(),
        loaded.reverbs(),
        &mut writer,
        &mut Cursor::new(Vec::new()),
        || false,
    )?;
    writer.finalize()?;
    bytes.set_position(0);
    let mut wave = hound::WavReader::new(bytes)?;
    assert_eq!(u64::from(wave.duration()), result.frames);
    Ok((result, wave.samples().collect::<Result<_, _>>()?))
}

#[test]
fn rendered_music_matches_live_pcm_and_sequencer_boundaries_with_tempo_changes() -> Result<()> {
    for looping in [false, true] {
        let loaded = score(looping);
        let (rendered, pcm) = render_bytes(loaded.clone())?;
        assert!(pcm.iter().any(|&sample| sample != 0));
        // Independent recorded preview provides the original sequencer's loop
        // markers and complete PCM, without using the renderer's selection code.
        let reference = super::super::render_preview(
            loaded.clone(),
            loaded.reverbs(),
            rendered.frames as u32 + BLOCK_FRAMES as u32,
        )?;
        assert_eq!(pcm, reference.pcm[..pcm.len()]);
        if let Some(repeat) = rendered.repeat {
            assert!(looping);
            assert!(reference.loop_starts.contains(&(repeat.start as u32)));
            assert!(reference.loop_starts.contains(&(repeat.end as u32)));
            assert_eq!(repeat.end, rendered.frames);
            assert!(repeat.start >= u64::from(reference.loop_starts[0]));
            let start = repeat.start as usize * 2;
            let end = repeat.end as usize * 2;
            let width = BLOCK_FRAMES * 2;
            assert_eq!(
                &reference.pcm[start - width..start + width],
                &reference.pcm[end - width..end + width],
                "both sides of the unmodified cut must recur"
            );
        } else {
            assert!(!looping);
        }
    }
    Ok(())
}

#[test]
fn music_rendering_cancels_and_bounds_unending_programs() -> Result<()> {
    let loaded = score(true);
    for cancelled in [false, true] {
        let mut bytes = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut bytes,
            hound::WavSpec {
                channels: 2,
                sample_rate: SOURCE_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        let result = render_bounded(
            loaded.clone(),
            loaded.reverbs(),
            &mut writer,
            &mut Cursor::new(Vec::new()),
            320,
            320,
            || cancelled,
        );
        let error = result.err().unwrap().to_string();
        assert!(error.contains(if cancelled {
            "cancelled"
        } else {
            "bounded rendering"
        }));
    }
    Ok(())
}

#[test]
fn a_recorded_repeat_retains_all_passes_needed_for_fractional_clock_phase() -> Result<()> {
    let base = score(true);
    let mut bank = Resources {
        programs: base.resources().programs.clone(),
        samples: base.resources().samples.clone(),
    };
    let mut song = base.score().clone();
    let sample = Arc::make_mut(bank.samples.get_mut(&2).unwrap());
    sample.pcm = (0..160)
        .map(|frame| ((frame % 31) * 700 - 10500) as i16)
        .collect();
    sample.loop_pcm = sample.pcm.clone();
    bank.programs.get_mut(&1).unwrap()[3] = Command::Wait {
        milliseconds: None,
        from_start: false,
        key_off: true,
        sample_end: false,
    };
    song.initial_bpm_1024 = 135 * 1024;
    song.tempos.clear();
    song.loop_start_tick = 0;
    song.end_tick = 768;
    song.first_events.truncate(1);
    song.first_events[0].tick = 8;
    let EventKind::Notes { length, .. } = &mut song.first_events[0].kind else {
        unreachable!()
    };
    *length = 760;
    song.loop_events = song.first_events.clone();
    let loaded = Arc::new(Loaded::new(
        bank,
        song,
        base.tables().clone(),
        base.reverbs(),
    )?);
    let (rendered, pcm) = render_bytes(loaded.clone())?;
    let repeat = rendered.repeat.unwrap();
    let reference = super::super::render_preview(
        loaded.clone(),
        loaded.reverbs(),
        rendered.frames as u32 + 2 * BLOCK_FRAMES as u32,
    )?;
    assert_eq!(
        pcm,
        reference.pcm[..pcm.len()],
        "rendering must not rewrite the join"
    );
    let start = reference
        .loop_starts
        .iter()
        .position(|&f| u64::from(f) == repeat.start)
        .unwrap();
    let end = reference
        .loop_starts
        .iter()
        .position(|&f| u64::from(f) == repeat.end)
        .unwrap();
    assert_eq!(end - start, 3);
    let window = |frame: u64| {
        let center = frame as usize * 2;
        &reference.pcm[center - 640..center + 640]
    };
    assert_ne!(
        window(repeat.start),
        window(u64::from(reference.loop_starts[start + 1]))
    );
    assert_eq!(window(repeat.start), window(repeat.end));
    Ok(())
}

#[test]
fn repeating_dry_music_bakes_its_own_reverb_tail_without_editing_the_join() -> Result<()> {
    let base = score(true);
    let mut bank = Resources {
        programs: base.resources().programs.clone(),
        samples: base.resources().samples.clone(),
    };
    bank.programs
        .get_mut(&1)
        .unwrap()
        .insert(2, Command::Auxiliary { bus: 0, value: 100 });
    let mut song = base.score().clone();
    song.initial_bpm_1024 = 137 * 1024;
    song.tempos.clear();
    song.loop_start_tick = 0;
    song.first_events.truncate(1);
    song.first_events[0].tick = 100;
    song.loop_events = song.first_events.clone();
    let reverbs = [[0.7, 0.8, 1.3, 0.4, 0.02]; 2];
    let loaded = Arc::new(Loaded::new(bank, song, base.tables().clone(), reverbs)?);
    let (rendered, pcm) = render_bytes(loaded.clone())?;
    let source = rendered
        .source_repeat
        .expect("fractional note timing requires baking the selected dry performance");
    let repeat = rendered.repeat.unwrap();
    assert!(repeat.start >= source.end);

    // Build an independent reference: retain the native buses through the
    // selected dry passage, then feed that passage repeatedly into one studio.
    // Compare every output sample, including both sides of the eventual cut.
    let mut stream = Stream::new(loaded.clone(), true)?;
    let mut dry = Vec::new();
    while dry.len() < source.end as usize {
        let mut block = [[[0; 2]; 3]; BLOCK_FRAMES];
        stream.render_block([LiveControls::default(); CONTROLS_PER_BLOCK], &mut block)?;
        dry.extend(block);
    }
    let mut studio = Studio::new(reverbs)?;
    let mut reference = Vec::new();
    for frame in 0..rendered.frames + WINDOW_FRAMES as u64 {
        let position = if frame < source.end {
            frame
        } else {
            source.start + (frame - source.end) % (source.end - source.start)
        };
        reference.extend(
            studio
                .process(dry[position as usize])
                .map(|sample| sample.clamp(i16::MIN as i32, i16::MAX as i32) as i16),
        );
    }
    assert_eq!(pcm, reference[..pcm.len()], "no PCM correction at the join");
    let window = |frame: u64| {
        let center = frame as usize * 2;
        &reference[center - WINDOW_FRAMES * 2..center + WINDOW_FRAMES * 2]
    };
    assert_ne!(window(source.start), window(source.end));
    assert_eq!(window(repeat.start), window(repeat.end));

    // Finding the dry passage consumes the search budget. Effects still need
    // enough time to repeat that passage, subject to a separate hard ceiling.
    let search_limit = source.end + WINDOW_FRAMES as u64 + 1;
    for (ceiling, succeeds) in [
        (search_limit, false),
        (rendered.frames + WINDOW_FRAMES as u64, true),
    ] {
        let mut bytes = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut bytes,
            hound::WavSpec {
                channels: 2,
                sample_rate: SOURCE_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        let result = render_bounded(
            loaded.clone(),
            reverbs,
            &mut writer,
            &mut Cursor::new(Vec::new()),
            search_limit,
            ceiling,
            || false,
        );
        if succeeds {
            assert_eq!(result?.repeat, Some(repeat.clone()));
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("bounded rendering")
            );
        }
    }
    Ok(())
}
