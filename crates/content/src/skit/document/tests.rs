use super::*;

#[test]
fn skit_availability_uses_names_and_omits_unconditional_defaults() -> Result<()> {
    let entry: SkitDefinition = serde_json::from_value(serde_json::json!({
        "id":501,"title":"A conversation","trigger":"entry",
        "party":["lloyd","colette"],"location":"sylvarant",
        "condition":{"not":{"member":"sheena"}}
    }))?;
    assert_eq!(
        entry.party,
        [PartyCharacter::Lloyd, PartyCharacter::Colette]
    );
    assert!(matches!(entry.trigger, SkitTrigger::Entry));
    assert!(matches!(entry.location, SkitLocation::Sylvarant));
    let mut entry = entry;
    entry.party.clear();
    entry.location = SkitLocation::Anywhere;
    entry.condition = SkitCondition::None;
    assert_eq!(
        serde_json::to_value(entry)?,
        serde_json::json!({
            "id":501,"title":"A conversation","trigger":"entry"
        })
    );
    assert!(
        serde_json::from_value::<SkitDefinition>(serde_json::json!({
            "id":501,"title":"A conversation","trigger":"entry","party":["unknown"]
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SkitDefinition>(serde_json::json!({
            "id":501,"title":"A conversation"
        }))
        .is_err()
    );
    Ok(())
}

#[test]
fn skit_speed_preserves_native_integer_timing_for_every_supported_source_rate() -> Result<()> {
    let media: Media = serde_json::from_str(r#"{"ids":[],"speed":1.000875}"#)?;
    for source_rate in 8000..=96000 {
        let native = (u64::from(source_rate) * 32028 / 32000) as u32;
        if native <= 96000 {
            assert_eq!(media.playback_rate(source_rate)?, native, "{source_rate}");
        } else {
            assert!(media.playback_rate(source_rate).is_err());
        }
    }
    Ok(())
}

#[test]
fn skit_media_defaults_to_source_speed_and_rejects_invalid_selection() -> Result<()> {
    let media: Media = serde_json::from_str(r#"{"ids":[42]}"#)?;
    let mapping = br#"{"42":{"name":"arrival/colette","silence":{"frames":123,"sample_rate":44100,"channels":1}}}"#;
    let mut read = |path: &str, _: usize| -> Result<Option<Arc<[u8]>>> {
        assert_eq!(path, SPEECH);
        Ok(Some(Arc::from(mapping.as_slice())))
    };
    let resolved = media.resolve(&mut read)?;
    assert_eq!(
        (resolved[&42].frames, resolved[&42].sample_rate),
        (123, 44100)
    );
    assert_eq!(
        serde_json::to_value(&media)?,
        serde_json::json!({"ids":[42]})
    );
    let mut invalid = media.clone();
    invalid.ids.insert(43);
    assert!(
        format!("{:#}", invalid.resolve(&mut read).unwrap_err()).contains("missing skit speech 43")
    );
    for speed in [0.0, -1.0, f64::NAN, f64::INFINITY, 0.001, 1000.0] {
        invalid.ids = media.ids.clone();
        invalid.speed = speed;
        assert!(invalid.resolve(&mut read).is_err(), "speed {speed}");
    }
    assert!(
        Media::default()
            .resolve(&mut |_, _| panic!("empty selection reads no speech"))
            .unwrap()
            .is_empty()
    );
    Ok(())
}
