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
fn attachment_styles_survive_when_the_weapon_starts_unequipped() -> Result<()> {
    let mut bytes = [0; BYTES];
    bytes[0x124] = 0x11;
    bytes[0x124 + 7 * 24] = 0x92;
    for initial_count in [0, 1, 4, 8] {
        bytes[0x1e4] = initial_count;
        let profile = read(&bytes)?;
        assert_eq!(
            profile.weapon_styles[0],
            WeaponStyle {
                before_body: true,
                toon: false,
                additive: true,
            }
        );
        assert_eq!(
            profile.weapon_styles[7],
            WeaponStyle {
                before_body: false,
                toon: true,
                additive: true,
            }
        );
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
    table.validate()?;
    assert_eq!(
        table.body_colliders.map(|body| (body.radius, body.height)),
        [
            (30., 150.),
            (30., 150.),
            (24., 110.),
            (30., 150.),
            (30., 150.),
            (30., 150.),
            (24., 110.),
            (30., 150.),
            (30., 150.)
        ]
    );
    let mut invalid = table.clone();
    invalid.version = 0;
    assert!(invalid.validate().is_err());
    invalid.version = table.version;
    invalid.body_colliders[0].height = 1.;
    assert!(invalid.validate().is_err());
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
