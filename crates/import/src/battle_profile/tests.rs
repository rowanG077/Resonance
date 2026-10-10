use super::*;

fn read(bytes: &[u8]) -> Result<Profile> {
    let mut usual = vec![0; 59];
    usual[..4].copy_from_slice(&13_u32.to_be_bytes());
    usual[48..52].copy_from_slice(&56_u32.to_be_bytes());
    usual[52..56].copy_from_slice(&57_u32.to_be_bytes());
    super::read(
        bytes,
        &crate::battle_voice::read(&usual)?,
        crate::battle_voice::PARTY_ROLES,
    )
}

#[test]
fn profiles_decode_movement_expressions_and_all_attachment_slots() -> Result<()> {
    let mut bytes = [0; BYTES];
    bytes[0x20..0x24].copy_from_slice(&5_f32.to_be_bytes());
    bytes[0x24..0x28].copy_from_slice(&10_f32.to_be_bytes());
    bytes[0x9b] = 200;
    bytes[0xd7..0xdb].copy_from_slice(&[1, 2, 3, 4]);
    bytes[0xdf..0xe3].copy_from_slice(&[4, 3, 2, 1]);
    let flags = [0, 1, 2, 0x10, 0x80, 0x11, 0x82, 0x92];
    for (slot, value) in flags.into_iter().enumerate() {
        bytes[0x124 + slot * 24] = value;
    }
    let styles = [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
        (false, false, false),
        (true, false, true),
        (false, true, false),
        (false, true, true),
    ]
    .map(|(before_body, toon, additive)| WeaponStyle {
        before_body,
        toon,
        additive,
    });
    for channels in 0..=4 {
        bytes[0xce] = channels;
        let profile = read(&bytes)?;
        let count = usize::from(channels);
        assert_eq!(profile.walk_speed, 5.);
        assert_eq!(profile.run_speed, 10.);
        assert_eq!(profile.body_alpha, 200);
        assert_eq!(
            &profile.idle_expression[..count],
            &bytes[0xd7..0xd7 + count]
        );
        assert_eq!(
            &profile.rescue_expression[..count],
            &bytes[0xdf..0xdf + count]
        );
        assert!(profile.idle_expression[count..].iter().all(|&v| v == 0));
        assert!(profile.rescue_expression[count..].iter().all(|&v| v == 0));
        assert_eq!(profile.weapon_styles, styles);
    }
    // Equipment may enable attachments beyond the initial count.
    for initial_count in [0, 1, 4, 8] {
        bytes[0x1e4] = initial_count;
        assert_eq!(read(&bytes)?.weapon_styles, styles);
    }
    Ok(())
}

#[test]
fn profiles_prepare_blink_frames_and_validate_selected_atlas() -> Result<()> {
    let mut bytes = [0; BYTES];
    bytes[0x5c..0x60].copy_from_slice(&0x8000_u32.to_be_bytes());
    assert!(read(&bytes)?.blink.is_none());
    bytes[0xce] = 1;
    bytes[0xcf] = 16;
    let profile = read(&bytes)?;
    let blink = profile.blink.as_ref().unwrap();
    assert_eq!((blink.channel, blink.frames), (0, [1, 2]));
    assert_eq!(blink.excluded_expressions, [2, 3, 10]);
    for invalid in [
        Blink {
            channel: 1,
            ..blink.clone()
        },
        Blink {
            frames: [1, 16],
            ..blink.clone()
        },
    ] {
        assert!(invalid.validate(&profile.texture_channels).is_err());
    }
    bytes[0xcf] = 2;
    assert!(read(&bytes).is_err());
    Ok(())
}

#[test]
fn profiles_publish_named_capabilities_without_flag_words() -> Result<()> {
    let mut bytes = [0; BYTES];
    let ordinary = read(&bytes)?;
    assert_eq!(ordinary.traits, ProfileTraits::default());
    bytes[0x5c..0x60].copy_from_slice(&0x8828_8301_u32.to_be_bytes());
    bytes[0xb4..0xb6].copy_from_slice(&0x4221_u16.to_be_bytes());
    let profile = read(&bytes)?;
    assert!(profile.traits.flying && profile.traits.fixed_height);
    assert!(profile.blink.is_none());
    assert!(profile.traits.knockdown_immune && profile.traits.body_motion_disabled);
    assert!(profile.traits.enemy_contact_recovery);
    assert!(profile.traits.passes_allied_obstacles && profile.traits.passable_for_allies);
    assert!(profile.traits.suppress_hurt_motion && profile.traits.item_target_excluded);
    assert!(!profile.traits.hover_bobbing && !profile.traits.push_immovable);
    bytes[0x58..0x5a].copy_from_slice(&(-1_i16).to_be_bytes());
    assert_eq!(read(&bytes)?.guard_pressure_limit, 0);
    let encoded = serde_json::to_value(profile)?;
    assert!(encoded.get("flags").is_none() && encoded.get("body_flags").is_none());
    assert_eq!(encoded["traits"]["inactive_item_target"], true);
    Ok(())
}

#[test]
fn profiles_decode_named_conditions_and_immunities() -> Result<()> {
    use Condition::*;
    let mut bytes = [0; BYTES];
    bytes[0x14..0x18].copy_from_slice(&0x0400_0809_u32.to_be_bytes());
    bytes[0x18..0x1c].copy_from_slice(&0x100_u32.to_be_bytes());
    bytes[0x1c..0x20].copy_from_slice(&4_u32.to_be_bytes());
    bytes[0x38..0x3c].copy_from_slice(&0x8200_u32.to_be_bytes());
    let profile = read(&bytes)?;
    assert_eq!(
        profile.initial_conditions,
        ConditionSet::of(&[PoisonMild, Paralysis, PhysicalAffliction, DefenseHalved])
    );
    assert_eq!(profile.immunities, ConditionSet::of(&[Stun, ShortStun]));
    assert_eq!(
        profile.intrinsic_conditions,
        ConditionSet::of(&[AilmentResistance, CastingSpeed])
    );
    let json = serde_json::to_value(profile)?;
    assert!(json.get("condition_flags").is_none());
    assert!(json.get("condition_immunity").is_none());
    assert_eq!(
        json["immunities"],
        serde_json::json!(["short_stun", "stun"])
    );
    Ok(())
}

#[test]
fn profiles_reject_unknown_conditions_and_active_immunity_flags() {
    for offset in [0x14, 0x1c, 0x3c] {
        let mut bytes = [0; BYTES];
        bytes[offset..offset + 4].copy_from_slice(&0x40_u32.to_be_bytes());
        assert!(read(&bytes).is_err());
    }
    for offset in [0x10, 0x38] {
        for (word, flag) in [(0, 0x100_u32), (1, 4)] {
            let mut bytes = [0; BYTES];
            let offset = offset + word * 4;
            bytes[offset..offset + 4].copy_from_slice(&flag.to_be_bytes());
            assert!(read(&bytes).is_err());
        }
    }
}

#[test]
fn profiles_reject_truncated_records_invalid_counts_and_nonfinite_floats() {
    let mut bytes = [0; BYTES];
    assert!(read(&bytes[..BYTES - 1]).is_err());
    bytes[0xce] = 5;
    assert!(read(&bytes).is_err());
    bytes[0xce] = 0;
    bytes[0x1e4] = 9;
    assert!(read(&bytes).is_err());
    bytes[0x1e4] = 0;
    for offset in [0x20, 0x24, 0x60, 0x84, 0x88, 0x8c, 0x90] {
        bytes[offset..offset + 4].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(read(&bytes).is_err(), "float at {offset:#x}");
        bytes[offset..offset + 4].fill(0);
    }
}

#[test]
#[ignore = "requires extracted game assets"]
fn imports_party_profiles_and_battle_entry_settings() -> Result<()> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/extracted/disc1/files/US_r_Top2Btl.rel");
    let output = tempfile::tempdir()?;
    let extracted = file.parent().unwrap().parent().unwrap();
    let sources = crate::source_assets::Sources::read(extracted)?;
    let usual = std::fs::read(extracted.join("files").join(sources.usual))?;
    let path = publish_party(&file, &usual, output.path(), "battle")?;
    assert_eq!(path, resonance_content::battle_profile::PARTY_PATH);
    let table: Table = serde_json::from_slice(&std::fs::read(output.path().join(path))?)?;
    assert_eq!(table.records.len(), 11);
    assert_eq!(table.records[0].guard_reduction, 75);
    assert_eq!(table.records[0].stagger_threshold, 100);
    assert_eq!(table.records[0].stagger_ticks, 45);
    assert_eq!(table.records[2].effect_scale, 0.9);
    assert_eq!(table.entry.fade_color, [128; 3]);
    assert_eq!(table.voice_sequences.len(), 10);
    assert_eq!(
        table.voice_sequences[3].techniques.get(&237),
        Some(&VoiceSequence {
            chant: Some(resonance_content::battle_voice::Sound::Stream(458)),
            self_chant: Some(resonance_content::battle_voice::Sound::Stream(369)),
            release: Some(resonance_content::battle_voice::Sound::Stream(423)),
            fallback: Some(resonance_content::battle_voice::Sound::Cue(870)),
        })
    );
    Ok(())
}
