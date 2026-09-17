//! Lossless reference frames selected by emulated presentation time, never image similarity.
use super::*;
use std::collections::BTreeSet;

#[derive(Deserialize, Serialize)]
struct Stream {
    codec_name: String,
    pix_fmt: String,
    width: u32,
    height: u32,
    time_base: String,
    r_frame_rate: String,
}
#[derive(Deserialize)]
struct Timestamp {
    pts: i64,
}
#[derive(Serialize)]
struct Frame {
    index: usize,
    pts: i64,
    vi: i64,
}

fn ratio(value: &str) -> Result<f64> {
    let (n, d) = value.split_once('/').context("invalid video time base")?;
    let value = n.parse::<u64>()? as f64 / d.parse::<u64>()? as f64;
    ensure!(value.is_finite() && value > 0., "invalid video time base");
    Ok(value)
}

fn index(timestamps: &[Timestamp], time_base: f64, first_vi: i64) -> Result<Vec<Frame>> {
    ensure!(
        timestamps.first().is_some_and(|f| f.pts == 0),
        "video must start at PTS zero"
    );
    let mut frames = Vec::with_capacity(timestamps.len());
    for (index, sample) in timestamps.iter().enumerate() {
        // GQSEAF uses the NTSC 60000/1001 Hz VI clock. Matroska rounds PTS to
        // milliseconds; permit that quantization, never a half-tick ambiguity.
        let tick = sample.pts as f64 * time_base * (60_000. / 1001.);
        ensure!(
            sample.pts >= 0 && tick <= f64::from(u32::MAX) && (tick - tick.round()).abs() <= 0.04,
            "timestamp is not on the NTSC VI clock"
        );
        let vi = first_vi
            .checked_add(tick.round() as i64)
            .context("VI index overflow")?;
        ensure!(
            frames
                .last()
                .is_none_or(|previous: &Frame| previous.vi < vi && previous.pts < sample.pts),
            "video timestamps repeat or go backward"
        );
        frames.push(Frame {
            index,
            pts: sample.pts,
            vi,
        });
    }
    Ok(frames)
}

/// Reuse only images bound to the same video and timestamp registration.
pub(super) fn reuse(
    cache: &Path,
    output: &Path,
    hash: &str,
    first_vi: i64,
    requested: &[u32],
) -> Result<bool> {
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(cache.join("frames.json"))?)?;
    if record["complete"] != true
        || record["video_sha256"] != hash
        || record["first_vi"] != first_vi
    {
        return Ok(false);
    }
    let images = record["images"]
        .as_array()
        .context("missing cached video images")?;
    let mut selected = Vec::new();
    for vi in requested.iter().copied().collect::<BTreeSet<_>>() {
        let Some(image) = images.iter().find(|i| i["vi"] == vi) else {
            return Ok(false);
        };
        let name = format!("vi-{vi:06}.png");
        ensure!(
            image["path"] == name && image["sha256"] == pair::file_hash(&cache.join(&name))?,
            "cached video image differs from its extraction manifest"
        );
        selected.push(image.clone());
    }
    ensure!(!output.exists(), "video-frame output already exists");
    fs::create_dir_all(output)?;
    for image in &selected {
        let name = image["path"].as_str().unwrap();
        fs::copy(cache.join(name), output.join(name))?;
    }
    record["images"] = serde_json::json!(selected);
    fs::write(
        output.join("frames.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;
    Ok(true)
}

pub(super) fn run(video: &Path, output: &Path, first_vi: i64, requested: &[u32]) -> Result<()> {
    ensure!(!output.exists(), "video-frame output already exists");
    let mut reader = resonance_media::VideoReader::open(video)?;
    let (width, height) = reader.dimensions();
    ensure!(
        (width, height) == (640, 480),
        "expected native-resolution lossless RGB FFV1"
    );
    let stream = Stream {
        codec_name: "ffv1".into(),
        pix_fmt: "bgr0".into(),
        width,
        height,
        time_base: "1/1000000000".into(),
        r_frame_rate: "60000/1001".into(),
    };
    let requested: BTreeSet<_> = requested.iter().copied().collect();
    let mut timestamps = Vec::new();
    let mut images = Vec::new();
    fs::create_dir_all(output)?;
    while let Some(frame) = reader.next_frame()? {
        let pts = i64::try_from(frame.timestamp.as_nanos())?;
        timestamps.push(Timestamp { pts });
        let tick = frame.timestamp.as_secs_f64() * (60_000. / 1001.);
        let vi = first_vi
            .checked_add(tick.round() as i64)
            .context("VI index overflow")?;
        if u32::try_from(vi).is_ok_and(|vi| requested.contains(&vi)) {
            let name = format!("vi-{vi:06}.png");
            let path = output.join(&name);
            let rgb = frame
                .rgba
                .chunks_exact(4)
                .flat_map(|p| p[..3].iter().copied())
                .collect();
            RgbImage::from_raw(width, height, rgb)
                .context("invalid decoded video image")?
                .save(&path)?;
            images.push(serde_json::json!({"vi":vi,"index":frame.index,"path":name,"sha256":pair::file_hash(&path)?}));
        }
    }
    // Validate every timestamp, including gaps and unrequested frames, before
    // marking an extraction complete. Never substitute a neighboring image.
    let frames = index(&timestamps, ratio(&stream.time_base)?, first_vi)?;
    for vi in requested {
        ensure!(
            frames.iter().any(|frame| frame.vi == i64::from(vi)),
            "no video presentation at VI {vi}"
        );
    }
    fs::write(
        output.join("frames.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "complete":true,"video_sha256":pair::file_hash(video)?,"stream":stream,
            "first_vi":first_vi,"vi_rate":"60000/1001","frames":frames,"images":images,
        }))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_frames_reject_wrong_origins_missing_samples_and_changed_pixels() {
        let root =
            std::env::temp_dir().join(format!("resonance-video-cache-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("vi-000010.png");
        RgbImage::new(2, 2).save(&source).unwrap();
        let manifest = serde_json::json!({"complete":true,"video_sha256":"video", "first_vi":0,
            "images":[{"vi":10,"path":"vi-000010.png","sha256":pair::file_hash(&source).unwrap()}]});
        fs::write(
            root.join("frames.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let output = root.join("reused");
        assert!(!reuse(&root, &output, "other-video", 0, &[10]).unwrap());
        assert!(!reuse(&root, &output, "video", 1, &[10]).unwrap());
        assert!(!reuse(&root, &output, "video", 0, &[10, 11]).unwrap());
        assert!(!output.exists());
        assert!(reuse(&root, &output, "video", 0, &[10]).unwrap());
        assert_eq!(
            fs::read(output.join("vi-000010.png")).unwrap(),
            fs::read(&source).unwrap()
        );
        fs::write(&source, b"changed pixels").unwrap();
        assert!(reuse(&root, &root.join("tampered"), "video", 0, &[10]).is_err());
        assert!(!root.join("tampered").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn extracts_exact_presentations_and_rejects_missing_vis() {
        let root =
            std::env::temp_dir().join(format!("resonance-video-extraction-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let video = root.join("capture.mkv");
        let mut writer = resonance_media::encode::MovieWriter::new(
            fs::File::create(&video).unwrap(),
            640,
            480,
            16683,
            32028,
        )
        .unwrap();
        for (i, millis) in [0, 17, 67].into_iter().enumerate() {
            let rgb = [i as u8 * 73, 17, 253].repeat(640 * 480);
            writer
                .video(&rgb, std::time::Duration::from_millis(millis))
                .unwrap();
        }
        writer.finish().unwrap();
        let output = root.join("frames");
        run(&video, &output, 100, &[104, 100, 104]).unwrap();
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("frames.json")).unwrap()).unwrap();
        assert_eq!(record["images"].as_array().unwrap().len(), 2);
        assert_eq!(record["frames"][1]["vi"], 101);
        assert_eq!(
            image::open(output.join("vi-000104.png"))
                .unwrap()
                .to_rgb8()
                .get_pixel(0, 0)
                .0,
            [146, 17, 253]
        );
        let missing = root.join("missing");
        assert!(run(&video, &missing, 100, &[102]).is_err());
        assert!(!missing.join("frames.json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ntsc_timestamps_preserve_gaps_and_reject_ambiguous_frames() {
        let samples = [0, 17, 67].map(|pts| Timestamp { pts });
        let frames = index(&samples, 0.001, 100).unwrap();
        assert_eq!(
            frames.iter().map(|f| f.vi).collect::<Vec<_>>(),
            [100, 101, 104]
        );
        assert!(!frames.iter().any(|f| f.vi == 102));
        let preroll = index(&samples, 0.001, -2).unwrap();
        assert_eq!(
            preroll.iter().map(|f| f.vi).collect::<Vec<_>>(),
            [-2, -1, 2]
        );
        for invalid in [[0, 8, 67], [0, 17, 17], [0, 67, 17], [0, -17, 67]] {
            assert!(index(&invalid.map(|pts| Timestamp { pts }), 0.001, 0).is_err());
        }
    }
}
