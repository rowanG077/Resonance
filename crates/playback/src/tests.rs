use super::*;
use std::{sync::Arc, time::Duration};

#[test]
fn scheduled_sources_pause_resume_and_complete_on_consumed_frames() {
    let (control, mut output) = Offline::new();
    let handle = control
        .schedule(NativeFrame(2), false, || {
            Ok(Box::new([1., 2., 3., 4., 5., 6.].into_iter()))
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
        .play(false, || Ok(Box::new(std::iter::repeat(8.))))
        .unwrap();
    assert_ne!(next.epoch, handle.epoch);
    handle.stop();
    output.next(); // finish prior frame
    assert_eq!(output.next(), Some(8.)); // old stop cannot cancel this epoch
}

struct Identity;
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
        .play(false, || Ok(Box::new(std::iter::repeat_n(0.75, 32028 * 4))))
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
fn exhausted_callback_returns_silence_and_reports_failure() {
    let ring = Output::new(Arc::new(Clock::new(48000)), 256);
    let mut callback = Callback::new(ring.clone(), false);
    let mut samples = [1.; 1024];
    callback.render(&mut samples, 2, Duration::ZERO, |s| s);
    assert_eq!(samples, [0.; 1024]);
    assert_eq!(ring.diagnostics().underrun_frames, 512);
    assert!(ring.check().is_err());
}

#[test]
fn decoded_pcm_is_bounded_contiguous_and_reports_missing_content() {
    let pcm = Arc::new(Pcm::new(8));
    pcm.push(0, vec![1., 2., 3., 4., 5., 6., 7., 8.]).unwrap();
    assert!(!pcm.needs_data());
    assert!(pcm.push(4, vec![9., 10.]).is_err());
    let mut source = pcm.source(false);
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
            Ok(Box::new((0..2000).flat_map(|i| [i as f32 / 4000.; 2])))
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
