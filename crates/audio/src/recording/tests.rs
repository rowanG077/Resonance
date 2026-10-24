use super::*;
use std::fs;

fn wave(path: &std::path::Path, rate: u32, frames: &[[i16; 2]]) -> Result<()> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for frame in frames {
        for &sample in frame {
            writer.write_sample(sample)?;
        }
    }
    writer.finalize()?;
    Ok(())
}

#[test]
fn recordings_preserve_intro_fractional_loops_pause_and_independent_cursors() -> Result<()> {
    let (root, _) = crate::package::tests::fixture();
    let path = root.0.join("music.wav");
    wave(
        &path,
        32000,
        &[[0, 0], [100, 200], [200, 400], [300, 600], [999, 999]],
    )?;
    let track = Prepared::new(File::open(&path)?, Some(1..4))?;
    let mut player = track.start(48000)?;
    let mut other = track.start(32000)?;
    let expected = [
        0.,
        200. / 3.,
        400. / 3.,
        200.,
        800. / 3.,
        700. / 3.,
        100.,
        500. / 3.,
    ];
    for expected in expected {
        let frame = player.next_frame()?.unwrap();
        assert!(
            (frame[0] - expected).abs() < 0.001,
            "{frame:?} != {expected}"
        );
        assert_eq!(frame[1], frame[0] * 2.);
    }
    assert_eq!(other.next_frame()?, Some([0., 0.]));
    assert_eq!(other.next_frame()?, Some([100., 200.]));
    let before = player.position();
    player.pause(true);
    assert_eq!(player.next_frame()?, Some([0.; 2]));
    assert_eq!(player.position(), before);
    player.pause(false);
    assert!(player.next_frame()?.unwrap()[0] > 0.);
    let mut once = Prepared::new(File::open(&path)?, None)?.start(32000)?;
    for frame in [
        [0., 0.],
        [100., 200.],
        [200., 400.],
        [300., 600.],
        [999., 999.],
    ] {
        assert_eq!(once.next_frame()?, Some(frame));
    }
    assert_eq!(once.next_frame()?, None);
    assert_eq!(once.next_frame()?, None);
    Ok(())
}

#[test]
fn recordings_pin_replaced_files_validate_current_bounds_and_use_bounded_pages() -> Result<()> {
    let (root, _) = crate::package::tests::fixture();
    let path = root.0.join("music.wav");
    let frames: Vec<_> = (0..PAGE_FRAMES * 5)
        .map(|index| [index as i16, -(index as i16)])
        .collect();
    wave(&path, 32028, &frames)?;
    let old = Prepared::new(
        File::open(&path)?,
        Some(PAGE_FRAMES + 7..PAGE_FRAMES * 4 + 3),
    )?;
    assert!(old.pages.len() <= 2);
    assert!(Prepared::new(File::open(&path)?, Some(0..PAGE_FRAMES * 5 + 1)).is_err());
    let replacement = root.0.join("replacement.wav");
    wave(&replacement, 48000, &[[77; 2]; 100])?;
    fs::rename(&replacement, &path)?;
    let fresh = Prepared::new(File::open(&path)?, None)?;
    assert_eq!((fresh.frames(), fresh.sample_rate()), (100, 48000));
    assert!(Prepared::new(File::open(&path)?, old.repeat()).is_err());
    let mut player = old.start(32028)?;
    for position in 0..PAGE_FRAMES * 9 {
        let expected = if position < PAGE_FRAMES * 4 + 3 {
            position
        } else {
            PAGE_FRAMES + 7 + (position - (PAGE_FRAMES + 7)) % (PAGE_FRAMES * 3 - 4)
        } as i16;
        assert_eq!(
            player.next_frame()?,
            Some([f32::from(expected), -f32::from(expected)])
        );
        assert!(player.pages.len() <= 2);
        assert!(
            player
                .pages
                .iter()
                .all(|page| page.frames.len() <= PAGE_FRAMES as usize)
        );
    }
    assert_eq!(fresh.start(48000)?.next_frame()?, Some([77.; 2]));
    let file = fs::OpenOptions::new().write(true).open(&path)?;
    file.set_len(45)?;
    assert!(Prepared::new(File::open(&path)?, None).is_err());
    Ok(())
}
