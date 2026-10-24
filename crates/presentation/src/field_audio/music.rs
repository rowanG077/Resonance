//! Recorded music on the mixer's block clock, bypassing the live effects studio.
use super::*;

pub(super) struct MusicPlayer {
    pub(super) player: resonance_audio::recording::Player,
    end: Option<u64>,
}

impl MusicPlayer {
    pub(super) fn recording(recording: &crate::audio::Recording, battle: bool) -> Result<Self> {
        Ok(Self {
            player: recording.start(battle)?,
            end: None,
        })
    }

    pub(super) fn prepare(
        &mut self,
        gains: [f32; CONTROLS_PER_BLOCK],
        frame: u64,
        block: &mut [[f32; 2]; BLOCK_FRAMES],
    ) -> Result<()> {
        if frame.is_multiple_of(BLOCK_FRAMES as u64) && self.end.is_none() {
            for (index, output) in block.iter_mut().enumerate() {
                let Some(pcm) = self.player.next_frame()? else {
                    self.end = Some(frame + index as u64);
                    break;
                };
                let gain = gains[index / CONTROL_FRAMES] / 32768.;
                *output = pcm.map(|s| s * gain);
            }
        }
        Ok(())
    }

    pub(super) fn finished(&self, frame: u64) -> bool {
        self.end.is_some_and(|end| frame >= end)
    }

    pub(super) fn pause(&mut self, paused: bool) {
        self.player.pause(paused);
    }
}

#[cfg(test)]
mod tests {
    use super::super::named_tests::{fixture, wav, write};
    use super::*;
    use resonance_content::library::Library;

    fn prepare(root: &Path) -> Result<Arc<Assets>> {
        Cache::default().capture_named(
            "fields/iselia/audio.json",
            &mut Library::open(root, &[])?.snapshot(),
            Diagnostics::new(true),
            || false,
        )
    }

    fn full_volume(frames: &mut Frames) -> Result<()> {
        frames.command(AudioCommand::MusicVolume {
            volume: 127,
            duration_ticks: 0,
        })
    }

    fn value(sample: i16) -> [f32; 2] {
        [f32::from(sample) * std::f32::consts::FRAC_1_SQRT_2 / 32768.; 2]
    }

    #[test]
    fn recordings_bypass_reverb_keep_queued_handoffs_and_retain_selected_files() -> Result<()> {
        let root = fixture()?;
        let path = "audio/music/test-song/music.wav";
        let samples: Vec<_> = (0..10000)
            .map(|i| ((i % 1000) * 20 - 9000) as i16)
            .collect();
        write(root.path(), path, &wav(&samples, RATE)?)?;
        write(root.path(), "audio/music/test-song/music.json", br#"{"version":1,"audio":"audio/music/test-song/music.wav","loop":{"start_frame":0,"end_frame":10000}}"#)?;
        let assets = prepare(root.path())?;
        let (source, mut control) = assets.session();
        let mut frames = source.decoder();
        frames.command(AudioCommand::Music(MusicCommand::Play(7)))?;
        full_volume(&mut frames)?;
        for &sample in &samples[..173] {
            assert_eq!(frames.frame()?.unwrap(), value(sample));
        }
        frames.command(AudioCommand::Music(MusicCommand::Play(7)))?;
        assert_eq!(frames.frame()?.unwrap(), value(samples[173]));
        assert!(
            frames.studio.is_silent(),
            "baked music must not excite the live studio"
        );

        write(root.path(), "replacement.wav", &wav(&[-6000; 10000], RATE)?)?;
        fs::rename(root.path().join("replacement.wav"), root.path().join(path))?;
        let replacement = prepare(root.path())?;
        control.enter_field(replacement)?;
        // Cross a WAV page boundary after replacement. The playing capture must
        // continue reading its retained file, not reopen the replacement path.
        for &sample in &samples[174..5003] {
            assert_eq!(frames.frame()?.unwrap(), value(sample));
        }
        frames.command(AudioCommand::Music(MusicCommand::Stop))?;
        frames.command(AudioCommand::Music(MusicCommand::Play(7)))?;
        full_volume(&mut frames)?;
        let boundary = 5003usize.next_multiple_of(BLOCK_FRAMES);
        for &sample in &samples[5003..boundary] {
            assert_eq!(frames.frame()?.unwrap(), value(sample));
        }
        for _ in 0..BLOCK_FRAMES {
            assert_eq!(frames.frame()?.unwrap(), value(-6000));
        }
        control.check()?;
        Ok(())
    }

    #[test]
    fn recorded_music_keeps_fades_movie_clock_and_jingle_resume_behavior() -> Result<()> {
        let root = fixture()?;
        write(
            root.path(),
            "audio/music/mappings.json",
            br#"{"7":"test-song","8":"jingle"}"#,
        )?;
        write(
            root.path(),
            "audio/music/jingle/music.json",
            br#"{"version":1,"audio":"audio/music/jingle/music.wav"}"#,
        )?;
        write(
            root.path(),
            "audio/music/jingle/music.wav",
            &wav(&[-5000; 240], RATE)?,
        )?;
        let mut definition: resonance_content::field_audio::document::Definition =
            serde_json::from_slice(&fs::read(root.path().join("fields/iselia/audio.json"))?)?;
        definition.music[0].ids.insert(8);
        definition.music_reverbs.selectors.push(0);
        write(
            root.path(),
            "fields/iselia/audio.json",
            &serde_json::to_vec(&definition)?,
        )?;
        let (source, mut control) = prepare(root.path())?.session();
        let mut frames = source.decoder();
        frames.command(AudioCommand::Music(MusicCommand::Play(7)))?;
        let fade = Fade::new(0., 1., 100)?;
        for frame in 0..4000u64 {
            let gain = fade.value_at(frame / CONTROL_FRAMES as u64 * CONTROL_FRAMES as u64);
            let expected = value(16384).map(|v| v * gain);
            let actual = frames.frame()?.unwrap();
            for (a, b) in actual.into_iter().zip(expected) {
                assert!((a - b).abs() < 1e-7);
            }
        }
        control.movie(true)?;
        for _ in 0..BLOCK_FRAMES * 2 {
            assert_eq!(frames.frame()?.unwrap(), [0.; 2]);
        }
        let player = &frames.music.as_ref().unwrap().player.player;
        assert_eq!(
            player.position(),
            4320 % 128,
            "movie muting must leave the WAV clock running"
        );
        control.movie(false)?;
        for _ in 0..RATE * 2 + BLOCK_FRAMES as u32 {
            frames.frame()?;
        }
        assert_eq!(frames.frame()?.unwrap(), value(16384));
        while !frames.frame.is_multiple_of(BLOCK_FRAMES as u64) {
            frames.frame()?;
        }
        frames.command(AudioCommand::Music(MusicCommand::PlayJingle(8)))?;
        full_volume(&mut frames)?;
        for _ in 0..240 {
            assert_eq!(frames.frame()?.unwrap(), value(-5000));
        }
        assert_eq!(frames.frame()?.unwrap(), [0.; 2]);
        assert!(frames.music.is_none());
        assert_eq!(frames.background_music, Some(7));
        frames.command(AudioCommand::Music(MusicCommand::Resume))?;
        full_volume(&mut frames)?;
        while !frames.frame.is_multiple_of(BLOCK_FRAMES as u64) {
            assert_eq!(frames.frame()?.unwrap(), [0.; 2]);
        }
        assert_eq!(frames.frame()?.unwrap(), value(16384));
        frames.command(AudioCommand::Music(MusicCommand::Suspend))?;
        while !frames.frame.is_multiple_of(BLOCK_FRAMES as u64) {
            frames.frame()?;
        }
        assert_eq!(frames.frame()?.unwrap(), [0.; 2]);
        assert_eq!(frames.background_music, Some(7));
        control.check()?;
        Ok(())
    }
}
