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
    bytes[0x30] = 4;
    bytes[0x20..0x24].copy_from_slice(&5_f32.to_be_bytes());
    bytes[0x24..0x28].copy_from_slice(&10_f32.to_be_bytes());
    bytes[0x51] = 1;
    for (axis, value) in [4_f32, 120., -6.].into_iter().enumerate() {
        bytes[0x78 + axis * 4..0x7c + axis * 4].copy_from_slice(&value.to_be_bytes());
    }
    bytes[0x9b] = 200;
    bytes[0x9d] = 12;
    bytes[0xa6] = 34;
    bytes[0xa8] = 12;
    bytes[0xa0..0xa4].copy_from_slice(&0.5_f32.to_be_bytes());
    bytes[0xa4] = 2;
    bytes[0xa5] = 4;
    bytes[0xbe..0xc0].copy_from_slice(&300_u16.to_be_bytes());
    bytes[0xc0] = 7;
    bytes[0xc3] = 13;
    bytes[0xc9] = 2;
    bytes[0xe8] = 60;
    bytes[0xe9] = 1;
    bytes[0xd7..0xdb].copy_from_slice(&[1, 2, 3, 4]);
    bytes[0xdb..0xdf].copy_from_slice(&[6, 7, 8, 9]);
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
        assert_eq!(profile.cast_recovery_motion, Some(34));
        assert!(profile.chant_once);
        assert_eq!(profile.chant_loop_start, 6.);
        assert_eq!(profile.cast_poses.release_loop_start, 6.);
        assert_eq!(profile.cast_poses.opening.clip, 13);
        assert_eq!(profile.cast_poses.release.start, 1.);
        assert_eq!(profile.cast_poses.release.rate, 0.5);
        assert_eq!(profile.cast_poses.release.blend, 4);
        assert!(!profile.cast_poses.release.repeat);
        assert_eq!(
            profile
                .passive_effects
                .iter()
                .map(|effect| (effect.program, effect.interval))
                .collect::<Vec<_>>(),
            [(7, 300), (2, 13)]
        );
        assert_eq!(
            profile
                .idle_effect
                .map(|effect| (effect.program, effect.interval)),
            Some((1, 60))
        );
        assert_eq!(profile.target_anchor.bone, 1);
        assert_eq!(profile.target_anchor.offset, [4., 120., -6.]);
        assert_eq!(
            &profile.idle_expression[..count],
            &bytes[0xd7..0xd7 + count]
        );
        assert_eq!(
            &profile.rescue_expression[..count],
            &bytes[0xdf..0xdf + count]
        );
        assert_eq!(
            &profile.active_expression[..count],
            &bytes[0xdb..0xdb + count]
        );
        assert!(profile.idle_expression[count..].iter().all(|&v| v == 0));
        assert!(profile.active_expression[count..].iter().all(|&v| v == 0));
        assert!(profile.rescue_expression[count..].iter().all(|&v| v == 0));
        assert_eq!(profile.weapon_styles, styles);
    }
    bytes[0xa6] = 0;
    bytes[0x30] = 3;
    assert_eq!(read(&bytes)?.cast_recovery_motion, None);
    assert!(!read(&bytes)?.chant_once);
    bytes[0xb4..0xb6].copy_from_slice(&0x0801_u16.to_be_bytes());
    let host = read(&bytes)?;
    assert!(host.traits.hosts_participants);
    assert!(host.hosted_body.is_none());
    bytes[0xb4..0xb6].copy_from_slice(&0x03e4_u16.to_be_bytes());
    let child = read(&bytes)?;
    assert!(!child.traits.hosts_participants);
    assert_eq!(
        child.hosted_body,
        Some(resonance_content::battle_profile::HostedBody {
            release_on_hurt: true,
            release_below_half_health: false,
            release_on_defeat: false,
            launch_on_release: true,
        })
    );
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
fn profiles_decode_capabilities_and_reject_non_finite_scales() -> Result<()> {
    let mut bytes = [0; BYTES];
    let ordinary = read(&bytes)?;
    assert_eq!(ordinary.traits, ProfileTraits::default());
    let mut encoded = serde_json::to_value(ordinary.traits)?;
    assert_eq!(encoded["hit_shake_disabled"], false);
    encoded
        .as_object_mut()
        .unwrap()
        .remove("hit_shake_disabled");
    assert!(!serde_json::from_value::<ProfileTraits>(encoded)?.hit_shake_disabled);
    bytes[0x5c..0x60].copy_from_slice(&0x8c28_8301_u32.to_be_bytes());
    bytes[0xb4..0xb6].copy_from_slice(&0x4221_u16.to_be_bytes());
    bytes[0xac] = 3;
    bytes[0xb0..0xb4].copy_from_slice(&1.25_f32.to_be_bytes());
    let profile = read(&bytes)?;
    assert_eq!(profile.overlimit_bone, 3);
    assert_eq!(profile.overlimit_scale, 1.25);
    assert!(profile.traits.flying && profile.traits.fixed_height);
    assert!(profile.blink.is_none());
    assert!(profile.traits.knockdown_immune && profile.traits.body_motion_disabled);
    assert!(profile.traits.hit_shake_disabled);
    assert!(profile.traits.enemy_contact_recovery);
    assert!(profile.traits.inactive_item_target);
    assert!(profile.traits.immune_to_unison);
    assert!(profile.traits.passes_allied_obstacles && profile.traits.passable_for_allies);
    assert!(profile.traits.suppress_hurt_motion && profile.traits.item_target_excluded);
    assert!(!profile.traits.hover_bobbing && !profile.traits.push_immovable);
    bytes[0x58..0x5a].copy_from_slice(&(-1_i16).to_be_bytes());
    assert_eq!(read(&bytes)?.guard_pressure_limit, 0);
    bytes[0xb0..0xb4].copy_from_slice(&f32::NAN.to_be_bytes());
    assert!(read(&bytes).is_err());
    Ok(())
}

#[test]
fn profiles_decode_named_conditions_and_immunities() -> Result<()> {
    use Condition::*;
    let mut bytes = [0; BYTES];
    bytes[0x10..0x14].copy_from_slice(&2_u32.to_be_bytes());
    bytes[0x14..0x18].copy_from_slice(&0x0480_0809_u32.to_be_bytes());
    bytes[0x18..0x1c].copy_from_slice(&0x100_u32.to_be_bytes());
    bytes[0x1c..0x20].copy_from_slice(&4_u32.to_be_bytes());
    bytes[0x38..0x3c].copy_from_slice(&0x8204_u32.to_be_bytes());
    bytes[0x3c..0x40].copy_from_slice(&0x0100_0000_u32.to_be_bytes());
    let profile = read(&bytes)?;
    assert_eq!(
        profile.initial_conditions,
        ConditionSet::of(&[
            PoisonMild,
            Paralysis,
            PhysicalAffliction,
            DefenseHalved,
            PhysicalProtection,
            PhysicalBarrier,
        ])
    );
    assert_eq!(profile.immunities, ConditionSet::of(&[Stun, ShortStun]));
    assert_eq!(
        profile.intrinsic_conditions,
        ConditionSet::of(&[
            AilmentResistance,
            CastingSpeed,
            MagicalProtection,
            MagicalBarrier,
        ])
    );
    Ok(())
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
    assert_eq!(table.chant_sequences.len(), 1);
    let chant = &table.chant_sequences[&3];
    assert_eq!(
        chant
            .iter()
            .map(|(at, pose)| (*at, pose.clip, pose.blend, pose.rate, pose.repeat))
            .collect::<Vec<_>>(),
        [
            (0, 30, 8, 0.5, false),
            (53, 31, 2, 0.5, false),
            (82, 32, 2, 0.5, true)
        ]
    );
    assert!(
        chant
            .iter()
            .all(|(_, pose)| pose.start == 0. && pose.end.is_none())
    );
    assert_eq!(table.voice_sequences.len(), 10);
    assert_eq!(
        table.voice_sequences[3].techniques.get(&237),
        Some(&VoiceSequence {
            chant: Some(resonance_content::battle_voice::Sound::Stream(458)),
            chant_ticks: 62,
            fallback_ticks: 41,
            self_chant: Some(resonance_content::battle_voice::Sound::Stream(369)),
            release: Some(resonance_content::battle_voice::Sound::Stream(423)),
            fallback: Some(resonance_content::battle_voice::Sound::Cue(870)),
        })
    );
    Ok(())
}
