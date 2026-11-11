use super::*;

#[test]
fn shared_voice_loader_preserves_native_pcm_and_uses_the_playback_clock() -> Result<()> {
    use resonance_content::field_audio::Voice;
    let pcm: [i16; 4] = [1234, -2345, 32767, -32768];
    let wave = |rate| -> Result<Vec<u8>> {
        let mut bytes = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut bytes,
            hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        for sample in pcm {
            writer.write_sample(sample)?;
        }
        writer.finalize()?;
        Ok(bytes.into_inner())
    };
    let bytes: Arc<[u8]> = wave(32000)?.into();
    let mut voice = Voice {
        path: "audio/streams/test.wav".into(),
        frames: 4,
        channels: 1,
        sample_rate: 32028,
        source_sample_rate: 32000,
    };
    voice.validate()?;
    let clip = Clip::prepare(bytes.clone(), &voice)?;
    assert!(
        Arc::ptr_eq(&clip.bytes, &bytes),
        "PCM should share its verified WAV storage"
    );
    assert_eq!(
        (0..clip.sample_count())
            .map(|i| clip.value(i))
            .collect::<Vec<_>>(),
        pcm
    );
    assert_eq!(clip.rate, 32028);
    assert_eq!(clip.channels, 1);
    assert!(
        Clip::prepare(wave(32028)?.into(), &voice).is_err(),
        "relabelled WAVs must be recooked"
    );
    voice.source_sample_rate = 22050;
    assert!(Clip::prepare(bytes.clone(), &voice).is_err());
    voice.source_sample_rate = 32000;
    assert!(Clip::prepare(bytes[..bytes.len() - 1].into(), &voice).is_err());
    Ok(())
}
