use super::*;
use resonance_content::{
    field_audio::{Asset, Voice},
    skit::SkitMedia,
};

pub(super) fn cook(
    extracted: &Path,
    output: &Path,
    decoder: &Path,
    executable: &[u8],
) -> Result<BTreeMap<u32, SkitMedia>> {
    let decoder = crate::media::Tool::resolve(decoder)?;
    let directory = output.join("intermediate/skits/media");
    fs::create_dir_all(&directory)?;
    fs::create_dir_all(output.join("audio/skits"))?;
    let mut result = BTreeMap::new();
    for row in dol::slice(executable, 0x802a30a0, 0xd8)?.chunks_exact(12) {
        let pointer = u32::from_be_bytes(row[..4].try_into()?);
        if pointer == 0 {
            continue;
        }
        let path = source_path(executable, pointer)?;
        if !path.starts_with("CV/cht_") {
            continue;
        }
        let group = u32::from_be_bytes(row[4..8].try_into()?);
        let archive = fs::read(extracted.join("files").join(&path))?;
        for (index, member) in crate::afs::parse(&archive)?.iter().enumerate() {
            let id = group + index as u32;
            let source_sha256 = crate::digest(member.data);
            let cache = directory.join(format!("{id:08x}.json"));
            if let Ok(bytes) = fs::read(&cache)
                && let Ok((hash, previous)) = serde_json::from_slice::<(String, SkitMedia)>(&bytes)
                && hash == decoder.hash
                && previous.source_sha256 == source_sha256
                && previous.voice.as_ref().is_none_or(|v| {
                    crate::media::hash_file(&output.join(&v.asset.path))
                        .ok()
                        .as_ref()
                        == Some(&v.asset.sha256)
                })
            {
                result.insert(id, previous);
                continue;
            }
            let raw = directory.join(format!("{id:08x}.ahx"));
            write_atomic(&raw, member.data)?;
            let wav = directory.join(format!("{id:08x}.wav"));
            let process = std::process::Command::new(&decoder.path)
                .args(["-i", "-o"])
                .arg(&wav)
                .arg(&raw)
                .output()?;
            ensure!(
                process.status.success(),
                "decode skit media {id:x}: {}",
                String::from_utf8_lossy(&process.stderr)
            );
            let mut reader = hound::WavReader::open(&wav)?;
            let spec = reader.spec();
            ensure!(
                spec.bits_per_sample == 16
                    && (8000..=48000).contains(&spec.sample_rate)
                    && spec.sample_format == hound::SampleFormat::Int
                    && (1..=2).contains(&spec.channels),
                "invalid skit PCM {id:x}: {spec:?}"
            );
            let frames = reader.duration();
            let pcm: Vec<i16> = reader.samples().collect::<std::result::Result<_, _>>()?;
            // US skits use silent AHX tracks as subtitle clocks. Do not infer
            // silence from a region or filename; verify every decoded sample.
            let sample_rate = (u64::from(spec.sample_rate) * 32028 / 32000) as u32;
            let voice = if pcm.iter().all(|&s| s == 0) {
                None
            } else {
                let path = format!("audio/skits/{id:08x}.wav");
                let target = output.join(&path);
                let mut writer = hound::WavWriter::create(
                    &target,
                    hound::WavSpec {
                        sample_rate,
                        ..spec
                    },
                )?;
                for sample in pcm {
                    writer.write_sample(sample)?;
                }
                writer.finalize()?;
                Some(Voice {
                    asset: Asset {
                        sha256: crate::media::hash_file(&target)?,
                        path,
                    },
                    frames,
                    sample_rate,
                    source_sample_rate: spec.sample_rate,
                    channels: spec.channels,
                    source_name: member.name.into(),
                    source_sha256: source_sha256.clone(),
                })
            };
            let media = SkitMedia {
                voice,
                frames,
                sample_rate,
                source_sha256,
            };
            write_atomic(&cache, &serde_json::to_vec(&(&decoder.hash, &media))?)?;
            result.insert(id, media);
        }
    }
    println!(
        "Cooked {} skit media clocks ({} audible tracks)",
        result.len(),
        result.values().filter(|m| m.voice.is_some()).count()
    );
    Ok(result)
}
