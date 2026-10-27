use super::*;
use std::{sync::Arc, time::Duration};

struct Samples<I>(I);
impl<I: Iterator<Item = f32>> Iterator for Samples<I> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.0.next()
    }
}
impl<I: Iterator<Item = f32> + Send + 'static> Source for Samples<I> {
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SOURCE_RATE).unwrap()
    }
}

#[test]
fn scheduled_sources_pause_resume_and_complete_on_consumed_frames() {
    let (control, mut output) = Offline::new();
    let handle = control
        .schedule(NativeFrame(2), false, || {
            Ok(Box::new(Samples([1., 2., 3., 4., 5., 6.].into_iter())))
        })
        .unwrap();
    assert_eq!(
        output.by_ref().take(6).collect::<Vec<_>>(),
        [0., 0., 0., 0., 1., 2.]
    );
    assert_eq!(handle.audible_frames(), 1);
    handle.pause();
    assert_eq!(output.by_ref().take(20).collect::<Vec<_>>(), vec![0.; 20]);
    assert_eq!(handle.audible_frames(), 1);
    handle.play();
    assert_eq!(
        output.by_ref().take(4).collect::<Vec<_>>(),
        [3., 4., 5., 6.]
    );
    assert_eq!(handle.audible_frames(), 3);
    assert!(!handle.empty()); // EOF has not been observed yet.
    output.next();
    assert!(handle.empty());
    let next = control
        .play(false, || Ok(Box::new(Samples(std::iter::repeat(8.)))))
        .unwrap();
    assert_ne!(next.epoch, handle.epoch);
    handle.stop();
    output.next(); // finish prior frame
    assert_eq!(output.next(), Some(8.)); // old stop cannot cancel this epoch

    next.stop();
    output.next();
    let fast = control
        .play(false, || Ok(Box::new(Samples((1..=12).map(|v| v as f32)))))
        .unwrap();
    fast.set_transport(false, Speed::Double);
    assert_eq!(output.by_ref().take(2).collect::<Vec<_>>(), [2., 3.]);
    assert_eq!(fast.audible_frames(), 2);
    fast.set_transport(true, Speed::Double);
    assert_eq!(output.by_ref().take(8).collect::<Vec<_>>(), [0.; 8]);
    assert_eq!(fast.audible_frames(), 2);
    fast.set_transport(false, Speed::Normal);
    assert_eq!(output.by_ref().take(2).collect::<Vec<_>>(), [5., 6.]);
    fast.set_transport(false, Speed::Double);
    assert_eq!(
        output.by_ref().take(6).collect::<Vec<_>>(),
        [8., 9., 11., 12., 0., 0.]
    );
    assert_eq!(fast.audible_frames(), 6);
    assert!(fast.empty());
}

struct Identity;

#[test]
fn slow_audio_rates_preserve_samples_clock_and_completion_across_block_boundaries() {
    for (speed, repeat) in [(Speed::Half, 2usize), (Speed::Fifth, 5), (Speed::Tenth, 10)] {
        for chunk in [1, 7, 13] {
            let clock = Arc::new(Clock::new(SOURCE_RATE));
            clock.manual(NativeFrame(0));
            let (control, mut mixer) = Mixer::new(clock.clone());
            let handle = control
                .play(false, || {
                    Ok(Box::new(Samples(
                        (1..=6).flat_map(|i| [i as f32, -(i as f32)]),
                    )))
                })
                .unwrap();
            handle.set_transport(false, speed);
            let mut rendered = 0;
            let mut output = vec![[0.; 2]; chunk];
            while rendered < repeat * 6 + chunk {
                mixer.render(&mut output).unwrap();
                for (i, &sample) in output.iter().enumerate() {
                    let position = rendered + i;
                    let value = if position < repeat * 6 {
                        (position / repeat + 1) as f32
                    } else {
                        0.
                    };
                    assert_eq!(sample, [value, -value]);
                    clock.manual(NativeFrame((position + 1) as u64));
                    assert_eq!(
                        handle.audible_frames(),
                        ((position + 1) / repeat).min(6) as u64
                    );
                    if position + 1 < repeat * 6 {
                        assert!(!handle.empty());
                    }
                }
                rendered += chunk;
            }
            assert!(handle.empty());
            assert_eq!(handle.rendered_frames(), 6);
        }
    }
}

#[test]
fn fractional_audio_position_survives_pause_and_mid_sample_speed_changes() {
    let (control, mut output) = Offline::new();
    let handle = control
        .play(false, || {
            Ok(Box::new(Samples(
                (1..).step_by(2).flat_map(|i| [i as f32, -(i as f32)]),
            )))
        })
        .unwrap();
    handle.set_transport(false, Speed::Tenth);
    assert_eq!(
        output.by_ref().take(6).collect::<Vec<_>>(),
        [1., -1., 1., -1., 1., -1.]
    );
    handle.set_transport(true, Speed::Half);
    assert_eq!(output.by_ref().take(8).collect::<Vec<_>>(), [0.; 8]);
    assert_eq!(handle.audible_frames(), 0);
    handle.set_transport(false, Speed::Half);
    assert_eq!(output.by_ref().take(2).collect::<Vec<_>>(), [1., -1.]);
    let next: Vec<_> = output.by_ref().take(2).collect();
    assert!((next[0] - 2.2).abs() < 1e-6);
    assert!((next[1] + 2.2).abs() < 1e-6);
    assert_eq!(handle.audible_frames(), 1);
    handle.set_transport(false, Speed::Normal);
    let next: Vec<_> = output.by_ref().take(2).collect();
    assert!((next[0] - 3.6).abs() < 1e-6);
    assert!((next[1] + 3.6).abs() < 1e-6);
    assert_eq!(handle.audible_frames(), 2);
}

impl Converter for Identity {
    fn convert(&mut self, input: &[[f32; 2]], output: &mut Vec<[f32; 2]>) -> anyhow::Result<()> {
        output.extend_from_slice(input);
        Ok(())
    }
}
#[test]
fn worker_keeps_producing_without_control_thread_updates() {
    let clock = Arc::new(Clock::new(SOURCE_RATE));
    let ring = Output::new(clock.clone(), 256);
    let (control, mixer) = Mixer::new(clock);
    let _source = control
        .play(false, || {
            Ok(Box::new(Samples(std::iter::repeat_n(0.75, 32028 * 4))))
        })
        .unwrap();
    let worker = Worker::start(mixer, Box::new(Identity), ring.clone()).unwrap();
    let began = std::time::Instant::now();
    while !ring.ready() {
        assert!(began.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut callback = Callback::new(ring.clone(), true);
    let mut samples = [f32::NAN; 512];
    // No control/renderer pumping while device consumption continues.
    for _ in 0..32 {
        callback.render(&mut samples, 2, Duration::ZERO, |s| s);
        assert!(samples.iter().all(|&sample| sample == 0.));
        std::thread::sleep(Duration::from_secs_f64(256. / f64::from(SOURCE_RATE)));
    }
    assert_eq!(ring.diagnostics().underrun_frames, 0);
    assert_eq!(ring.diagnostics().callbacks, 32);
    drop(worker);
}

#[test]
fn callback_recovers_from_underruns() {
    let ring = Output::new(Arc::new(Clock::new(48000)), 256);
    let mut callback = Callback::new(ring.clone(), false);
    let mut samples = [1.; 1024];
    callback.render(&mut samples, 2, Duration::ZERO, |s| s);
    assert_eq!(samples, [0.; 1024]);
    assert_eq!(ring.diagnostics().underrun_frames, 512);
    assert!(ring.check().is_ok());
    assert_eq!(ring.clock.audible(), NativeFrame(0));
    ring.device_error(true, false);
    assert!(ring.check().is_ok());

    let (control, mixer) = Mixer::new(ring.clock.clone());
    let _source = control
        .play(false, || Ok(Box::new(Samples(std::iter::repeat(0.5)))))
        .unwrap();
    let worker = Worker::start(mixer, Box::new(Identity), ring.clone()).unwrap();
    let started = std::time::Instant::now();
    while !ring.ready() {
        assert!(started.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1));
    }
    callback.render(&mut samples, 2, Duration::ZERO, |s| s);
    assert_eq!(samples, [0.5; 1024]);
    assert!(ring.check().is_ok());
    drop(worker);
    ring.device_error(false, false);
    assert!(ring.check().is_err());
}

#[test]
fn decoded_pcm_is_bounded_contiguous_and_reports_missing_content() {
    let pcm = Arc::new(Pcm::new(8));
    pcm.push(0, vec![1., 2., 3., 4., 5., 6., 7., 8.]).unwrap();
    assert!(!pcm.needs_data());
    assert!(pcm.push(4, vec![9., 10.]).is_err());
    let mut source = pcm.source(false, 1.);
    assert_eq!(source.next(), Some(1.));
    assert_eq!(source.next(), Some(2.));
    assert!(pcm.push(8, vec![9., 10.]).is_err());
    pcm.push(4, vec![9., 10.]).unwrap();
    pcm.finish();
    assert_eq!(
        source.collect::<Vec<_>>(),
        vec![3., 4., 5., 6., 7., 8., 9., 10.]
    );
    assert_eq!(pcm.underruns(), 0);
}

#[test]
fn replacing_output_replays_unheard_pcm_without_restarting_the_source() {
    let clock = Arc::new(Clock::new(SOURCE_RATE));
    let ring = Output::new(clock.clone(), 256);
    let (control, mixer) = Mixer::new(clock.clone());
    let handle = control
        .play(false, || {
            Ok(Box::new(Samples(
                (0..2000).flat_map(|i| [i as f32 / 4000.; 2]),
            )))
        })
        .unwrap();
    let worker = Worker::start(mixer, Box::new(Identity), ring.clone()).unwrap();
    let started = std::time::Instant::now();
    while !ring.ready() {
        assert!(started.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1));
    }
    // The device has heard 100 frames; synthesis has already rendered ahead.
    clock.manual(NativeFrame(100));
    let rendered = handle.rendered_frames();
    assert!(rendered > 100);
    let suspended = worker.suspend().unwrap();
    assert_eq!(handle.audible_frames(), 100);
    let (worker, ring) = suspended
        .resume(SOURCE_RATE, 256, Box::new(Identity))
        .unwrap();
    while !ring.ready() {
        assert!(started.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut callback = Callback::new(ring.clone(), false);
    let mut samples = [0.; 512];
    callback.render(&mut samples, 2, Duration::ZERO, |s| s);
    for (index, frame) in samples.chunks_exact(2).enumerate() {
        assert_eq!(frame, &[(100 + index) as f32 / 4000.; 2]);
    }
    assert_eq!(handle.epoch, 1);
    assert_eq!(ring.diagnostics().underrun_frames, 0);
    drop(worker);
}

#[test]
fn decoded_starvation_refills_without_advancing_the_movie_clock() {
    let pcm = Arc::new(Pcm::new(8));
    pcm.push(0, vec![1., 2.]).unwrap();
    let source = pcm.source(false, 1.);
    let (control, mut output) = Offline::new();
    let handle = control.play(false, move || Ok(Box::new(source))).unwrap();
    assert_eq!(
        output.by_ref().take(6).collect::<Vec<_>>(),
        [1., 2., 0., 0., 0., 0.]
    );
    assert!(!handle.empty());
    assert!(!pcm.finished());
    assert_eq!(handle.audible_frames(), 1);
    assert_eq!(pcm.underruns(), 1);

    // A partial refill stays buffered until the normal lookahead is restored.
    pcm.push(1, vec![3., 4.]).unwrap();
    assert_eq!(output.by_ref().take(8).collect::<Vec<_>>(), [0.; 8]);
    assert_eq!(handle.audible_frames(), 1);
    assert_eq!(pcm.underruns(), 1);
    pcm.push(2, vec![5., 6., 7., 8., 9., 10.]).unwrap();
    assert_eq!(
        output.by_ref().take(8).collect::<Vec<_>>(),
        [3., 4., 5., 6., 7., 8., 9., 10.]
    );
    assert_eq!(handle.audible_frames(), 5);

    // EOF releases a short final tail after another underrun.
    assert_eq!(output.by_ref().take(2).collect::<Vec<_>>(), [0.; 2]);
    pcm.push(5, vec![11., 12.]).unwrap();
    pcm.finish();
    assert_eq!(
        output.by_ref().take(4).collect::<Vec<_>>(),
        [11., 12., 0., 0.]
    );
    assert_eq!(handle.audible_frames(), 6);
    assert!(handle.empty());
}
