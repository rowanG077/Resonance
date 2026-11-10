use crate::CameraTrack;
use resonance_content::camera_motion::CameraMotion;

impl CameraTrack {
    pub fn new(resource: u32, tick: u32) -> Self {
        Self {
            resource,
            start_tick: tick,
            frame: 0.,
            sampled_frames: [0.; 2],
            rate: 0.5,
            playing: true,
            repeat: 0,
            completed: false,
            target_actor: 0,
            target_offset: [0.; 3],
        }
    }

    pub fn finished(&self) -> bool {
        self.completed
    }

    pub fn frame(&self) -> f32 {
        self.frame
    }

    pub(crate) fn seek(&mut self, frame: f32) {
        self.frame = frame;
    }

    /// Native80048E40 divides duration by the range, then scales the offset
    /// in double precision and rounds only the final, bounded frame to f32.
    pub(crate) fn map_position(&mut self, position: i32, start: i32, end: i32, duration: f32) {
        let duration = f64::from(duration);
        let step = duration / (f64::from(end) - f64::from(start));
        let frame = (f64::from(position) - f64::from(start)) * step;
        self.seek(if frame.is_nan() {
            f32::NAN
        } else {
            frame.clamp(0., duration) as f32
        });
    }

    /// Native8005E268 advances once after publishing the sampled camera pose.
    /// Keep each single-precision operation and the original one-wrap branches.
    pub(crate) fn advance(&mut self, duration: f32) {
        if !self.playing {
            return;
        }
        self.frame += self.rate;
        if self.frame < 0. {
            self.frame += duration;
        }
        if self.frame > duration {
            self.completed = true;
            if self.repeat == 0 {
                self.playing = false;
                self.frame = duration;
            } else {
                self.frame -= duration;
            }
        }
    }

    pub(crate) fn configure(
        &mut self,
        operation: i32,
        value: i32,
        duration: f32,
    ) -> Result<i32, String> {
        Ok(match operation {
            0 => {
                self.playing = true;
                0
            }
            1 | 2 => {
                self.playing = false;
                if operation == 2 {
                    self.seek(0.);
                }
                0
            }
            3 => {
                self.seek(value as f32);
                0
            }
            4 => {
                self.rate = value as f32 / 200.;
                0
            }
            5 => {
                self.repeat = value as u8;
                0
            }
            // Gekko fctiwz returns MIN for an unordered mapped frame.
            6 => {
                if self.frame.is_nan() {
                    i32::MIN
                } else {
                    self.frame as i32
                }
            }
            7 => duration as i32,
            8 => self.rate as i32,
            9 => i32::from(self.repeat),
            10 => i32::from(self.completed),
            11 => {
                self.target_actor = value;
                value
            }
            12..=14 => {
                self.target_offset[(operation - 12) as usize] = value as f32;
                self.target_offset[(operation - 12) as usize] as i32
            }
            _ => 0,
        })
    }

    pub fn sample(&self, motion: &CameraMotion) -> anyhow::Result<resonance_content::CameraKey> {
        motion.sample(self.frame)
    }

    pub(crate) fn visit(
        &mut self,
        motion: &CameraMotion,
    ) -> anyhow::Result<resonance_content::CameraKey> {
        let key = self.sample(motion)?;
        self.sampled_frames = [self.sampled_frames[1], self.frame];
        Ok(key)
    }

    /// The title presents the preceding camera visit, including real seeks
    /// and rate changes, rather than reconstructing it from elapsed time.
    pub fn sample_previous(
        &self,
        motion: &CameraMotion,
    ) -> anyhow::Result<resonance_content::CameraKey> {
        motion.sample(self.sampled_frames[0])
    }
}

#[cfg(test)]
mod script_command_tests {
    use super::*;

    #[test]
    fn repeat_queries_keep_the_stored_byte_and_unknown_commands_return_zero() {
        let mut track = CameraTrack::new(22, 0);
        track.playing = false;
        for value in [0, 1, 2, 255, 256, 257, -1] {
            assert_eq!(track.configure(5, value, 20.).unwrap(), 0);
            assert_eq!(
                track.configure(9, 123, 20.).unwrap(),
                i32::from(value as u8)
            );
        }
        assert_eq!(track.configure(-1, 0, 20.).unwrap(), 0);
        assert_eq!(track.configure(15, 0, 20.).unwrap(), 0);
        assert_eq!(track.repeat, 255);
        assert!(!track.playing);
        track.playing = true;
        for _ in 0..42 {
            track.advance(20.);
        }
        assert_eq!(track.frame(), 1.);
    }

    #[test]
    fn offset_commands_return_the_float_storage_conversion_and_rate_reads_the_step() {
        let mut track = CameraTrack::new(22, 0);
        track.playing = false;
        for (command, value, expected) in [
            (12, 16777217, 16777216),
            (13, -16777217, -16777216),
            (14, i32::MAX, i32::MAX),
        ] {
            assert_eq!(track.configure(command, value, 20.).unwrap(), expected);
            assert_eq!(track.target_offset[(command - 12) as usize], value as f32);
        }
        assert_eq!(track.configure(4, 100, 20.).unwrap(), 0);
        assert_eq!(track.rate, 0.5);
        assert_eq!(track.configure(8, 100, 20.).unwrap(), 0);
        track.configure(4, -500, 20.).unwrap();
        assert_eq!(track.configure(8, 0, 20.).unwrap(), -2);
    }

    #[test]
    fn raw_seeks_do_not_wrap_clamp_or_set_completion_until_an_advance() {
        let mut track = CameraTrack::new(22, 0);
        track.configure(1, 0, 20.).unwrap();
        track.configure(3, 90, 20.).unwrap();
        assert_eq!(track.configure(6, 0, 20.).unwrap(), 90);
        assert_eq!(track.configure(10, 0, 20.).unwrap(), 0);
        track.advance(20.);
        assert_eq!(track.frame(), 90.);
        assert!(!track.finished());
        track.configure(0, 0, 20.).unwrap();
        track.advance(20.);
        assert_eq!(track.frame(), 20.);
        assert!(track.finished());
        assert!(!track.playing);
        track.configure(2, 0, 20.).unwrap();
        assert_eq!(track.frame(), 0.);
        assert!(
            track.finished(),
            "rewind preserves the native completion byte"
        );
        track.configure(3, -90, 20.).unwrap();
        assert_eq!(track.configure(6, 0, 20.).unwrap(), -90);
    }

    #[test]
    fn advance_uses_one_wrap_and_latches_only_on_crossing_the_upper_end() {
        let mut track = CameraTrack::new(22, 0);
        track.configure(5, 2, 20.).unwrap();
        track.configure(3, 20, 20.).unwrap();
        assert!(!track.finished());
        track.advance(20.);
        assert_eq!(track.frame(), 0.5);
        assert!(track.finished());
        track.configure(3, 0, 20.).unwrap();
        track.configure(4, -10000, 20.).unwrap();
        track.advance(20.);
        assert_eq!(track.frame(), -30., "negative overflow adds one duration");
        let mut fresh = CameraTrack::new(22, 0);
        fresh.configure(4, -10000, 20.).unwrap();
        fresh.advance(20.);
        assert_eq!(fresh.frame(), -30.);
        assert!(
            !fresh.finished(),
            "reverse playback does not set completion"
        );
        fresh.configure(5, 1, 20.).unwrap();
        fresh.configure(3, 0, 20.).unwrap();
        fresh.configure(4, 10000, 20.).unwrap();
        fresh.advance(20.);
        assert_eq!(
            fresh.frame(),
            30.,
            "positive overflow subtracts one duration"
        );
        assert!(fresh.finished());
        assert!(fresh.playing);
    }

    #[test]
    fn advance_matches_original_powerpc_instruction_snapshots() {
        // Captured by executing DOL8005E628..8005E6A8 under qemu-ppc.
        // Only the enclosing function epilogue was replaced by BLR. The
        // original-binary audit pins the complete advancement block.
        for (
            frame,
            duration,
            rate,
            repeat,
            completed,
            playing,
            updates,
            expected_frame,
            expected_completed,
            expected_playing,
        ) in [
            (
                0., 100., 0.005, 0, false, true, 10000, 0x42480183, false, true,
            ),
            (20., 20., 0.5, 0, false, true, 1, 0x41a00000, true, false),
            (20., 20., 0.5, 2, false, true, 1, 0x3f000000, true, true),
            (0., 20., -50., 0, false, true, 1, 0xc1f00000, false, true),
            (0., 20., 50., 1, false, true, 1, 0x41f00000, true, true),
            (90., 20., 0.5, 0, false, false, 1, 0x42b40000, false, false),
            (0., 20., 0.5, 0, true, true, 1, 0x3f000000, true, true),
            (19.5, 20., 0.5, 0, false, true, 1, 0x41a00000, false, true),
            (0., 0., 0.5, 1, false, true, 1, 0x3f000000, true, true),
        ] {
            let mut track = CameraTrack {
                frame,
                rate,
                repeat,
                completed,
                playing,
                ..CameraTrack::new(22, 0)
            };
            for _ in 0..updates {
                track.advance(duration);
            }
            assert_eq!(track.frame().to_bits(), expected_frame);
            assert_eq!(track.finished(), expected_completed);
            assert_eq!(track.playing, expected_playing);
        }
    }

    #[test]
    fn delayed_samples_retain_actual_visits_across_seeks_and_rate_changes() {
        let motion = CameraMotion::from_keys(&[
            resonance_content::CameraKey::looking_at(0., [0.; 3], [0., 10., 0.]),
            resonance_content::CameraKey::looking_at(20., [40., 0., 0.], [0., 10., 0.]),
        ])
        .unwrap();
        let mut track = CameraTrack::new(22, 0);
        for expected in [0., 0.5, 1.] {
            assert_eq!(track.visit(&motion).unwrap().time, expected);
            track.advance(20.);
        }
        assert_eq!(track.sample_previous(&motion).unwrap().time, 0.5);
        track.configure(3, 15, 20.).unwrap();
        track.configure(4, -100, 20.).unwrap();
        assert_eq!(track.visit(&motion).unwrap().time, 15.);
        track.advance(20.);
        assert_eq!(track.sample_previous(&motion).unwrap().time, 1.);
        assert_eq!(track.visit(&motion).unwrap().time, 14.5);
        assert_eq!(track.sample_previous(&motion).unwrap().time, 15.);
    }

    #[test]
    fn mapped_positions_match_original_powerpc_instruction_snapshots() {
        // Executed the complete original DOL80048E40..80048EF8 with the
        // original global frame and constant pools under qemu-ppc.
        for (duration, position, start, end, expected_bits) in [
            (f32::from_bits(0x443576db), 2537, -24, 8127, 0x43640f74),
            (20., 0, -100, 300, 0x40a00000),
            (20., -200, -100, 300, 0x00000000),
            (20., 400, -100, 300, 0x41a00000),
            (20., 50, 100, 0, 0x41200000),
            (100., i32::MAX, i32::MIN, i32::MAX, 0x42c80000),
            (20., 6, 5, 5, 0x41a00000),
            (20., 4, 5, 5, 0x00000000),
            (20., 5, 5, 5, 0x7fc00000),
            (0., 5, 0, 10, 0x00000000),
        ] {
            let mut track = CameraTrack::new(22, 0);
            track.completed = true;
            track.playing = false;
            track.map_position(position, start, end, duration);
            assert_eq!(track.frame().to_bits(), expected_bits);
            assert!(track.finished());
            assert!(!track.playing);
            if track.frame().is_nan() {
                assert_eq!(track.configure(6, 0, duration).unwrap(), i32::MIN);
            }
        }
    }
}
