use super::*;

fn source_bytes(profile: &Profile) -> [u8; BYTES] {
    let mut bytes = [0; BYTES];
    for span in &profile.storage {
        bytes[span.offset..span.offset + span.bytes.len()].copy_from_slice(&span.bytes);
    }
    bytes[0xce] = profile.texture_channels.len() as u8;
    for (index, channel) in profile.texture_channels.iter().enumerate() {
        bytes[0xcf + index] = channel.frames;
        bytes[0xd3 + index] = channel.texture;
        bytes[0xd7 + index] = profile.idle_expression[index];
    }
    bytes[0x1e4] = profile.weapon_draw_flags.len() as u8;
    for (index, &flags) in profile.weapon_draw_flags.iter().enumerate() {
        bytes[0x124 + 24 * index] = flags;
    }
    for (offset, value) in [
        (0x10, profile.condition_flags[0]),
        (0x14, profile.condition_flags[1]),
        (0x18, profile.condition_immunity[0]),
        (0x1c, profile.condition_immunity[1]),
        (0x20, profile.walk_speed.bits()),
        (0x24, profile.run_speed.bits()),
        (0x38, profile.intrinsic_conditions[0]),
        (0x3c, profile.intrinsic_conditions[1]),
        (0x5c, profile.flags),
        (0x60, profile.center_offset[0].bits()),
        (0x64, profile.center_offset[1].bits()),
        (0x68, profile.center_offset[2].bits()),
        (0x6c, profile.stun_offset[0].bits()),
        (0x70, profile.stun_offset[1].bits()),
        (0x74, profile.stun_offset[2].bits()),
        (0x78, profile.target_offset[0].bits()),
        (0x7c, profile.target_offset[1].bits()),
        (0x80, profile.target_offset[2].bits()),
        (0x84, profile.model_scale.bits()),
        (0x88, profile.shadow_scale.bits()),
        (0x8c, profile.effect_scale.bits()),
        (0x90, profile.ground_offset.bits()),
        (0xa0, profile.casting.animation_rate.bits()),
        (0x104, profile.voice_base),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    for (offset, value) in [
        (0x56, profile.casting.base_ticks as u16),
        (0x58, profile.guard_pressure_limit as u16),
        (0xb4, profile.body_flags),
        (0xec, profile.casting.command_index as u16),
        (0xf0, profile.camera_minimum_radius as u16),
        (0xf4, profile.death_voice),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    for (offset, value) in [
        (0x28, profile.weight),
        (0x2c, profile.turn_ticks),
        (0x2d, profile.stun_resistance),
        (0x2e, profile.stagger_threshold),
        (0x2f, profile.guard_reduction),
        (0x30, profile.casting.motion_flags),
        (0x51, profile.target_bone),
        (0x55, profile.stagger_ticks),
        (0x94, profile.idle_ticks),
        (0x95, profile.idle_variation),
        (0x97, profile.initial_motion),
        (0x9c, profile.death_motion),
        (0xc1, profile.initial_motion_override),
        (0x96, profile.casting.effect_interval),
        (0x9d, profile.casting.loop_start),
        (0x9f, profile.head_bone),
        (0xa4, profile.casting.resume_start),
        (0xa5, profile.casting.resume_blend),
        (0xa6, profile.casting.stored_recovery_clip),
        (0xa7, profile.camera_yaw_offset),
        (0xa8, profile.casting.resume_loop_start),
        (0xad, profile.overlimit_gain),
        (0xe7, profile.camera_category),
        (0x11b, profile.armor),
    ] {
        bytes[offset] = value;
    }
    bytes[0x108..0x10c].copy_from_slice(&profile.shadow_color);
    bytes
}

#[test]
fn profiles_preserve_every_source_byte_including_nonfinite_operands() -> Result<()> {
    let mut bytes: [u8; BYTES] = std::array::from_fn(|i| i as u8);
    bytes[0x1e4] = 8;
    for (offset, bits) in [
        (0x84, 0x7fc12345_u32),
        (0x88, 0x7fc98765),
        (0x8c, 0xffc54321),
        (0x90, 0x80000000),
        (0xa0, 0xff800000),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&bits.to_be_bytes());
    }
    for channels in 0..=4 {
        bytes[0xce] = channels;
        let profile: Profile = serde_json::from_slice(&serde_json::to_vec(&read(&bytes)?)?)?;
        assert_eq!(
            &profile.idle_expression[..usize::from(channels)],
            &bytes[0xd7..0xd7 + usize::from(channels)]
        );
        assert!(
            profile.idle_expression[usize::from(channels)..]
                .iter()
                .all(|&value| value == 0)
        );
        assert_eq!(source_bytes(&profile), bytes);
    }
    assert!(read(&bytes[..BYTES - 1]).is_err());
    Ok(())
}

#[test]
#[ignore = "requires both extracted original discs"]
fn original_party_profiles_roundtrip_all_eleven_slots_on_both_discs() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut previous = None;
    for disc in [1, 2] {
        let file = local.join(format!("disc{disc}/files/US_r_Top2Btl.rel"));
        let module = Rel::read(&file)?;
        let output = tempfile::tempdir()?;
        let path = publish_party(&file, output.path(), "battle")?;
        assert_eq!(path, resonance_content::battle_profile::PARTY_PATH);
        let json = std::fs::read(output.path().join(path))?;
        let variant = publish_party(&file, output.path(), "battle/variants/test")?;
        assert_eq!(std::fs::read(output.path().join(variant))?, json);
        let table: Table = serde_json::from_slice(&json)?;
        assert_eq!(table.source_sha256, crate::digest(&module.bytes));
        assert_eq!(table.records.len(), 11);
        assert_eq!(table.entry.bootstrap_eye, [0., 120., 1750.]);
        assert_eq!(table.entry.bootstrap_focus, [0., 120., 0.]);
        assert_eq!(table.entry.initial_yaw, 85.);
        assert_eq!(table.entry.radius, 4200.);
        assert_eq!(table.entry.focus_x, 700.);
        assert_eq!(table.entry.focus_speed_scale, 1_f32 / 59.);
        assert_eq!(table.entry.replacement_motion_rate, 0.5);
        assert_eq!(table.entry.fade_color, [128; 3]);
        let screen = &table.entry.screen_break;
        assert_eq!(screen.points.len(), 43);
        assert_eq!(screen.triangles.len(), 62);
        assert_eq!(screen.triangles[0], [10, 14, 15]);
        let points: Vec<_> = screen
            .points
            .iter()
            .flatten()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        assert_eq!(points, module.at((5, 0xc0))?[..43 * 12]);
        let triangles: Vec<_> = screen
            .triangles
            .iter()
            .flatten()
            .flat_map(|&v| u32::from(v).to_be_bytes())
            .collect();
        assert_eq!(triangles, module.at((5, 0x2c4))?[..62 * 12]);
        assert_eq!(screen.viewport, [640., 480.]);
        assert_eq!(screen.viewport_center, [320., 240.]);
        assert_eq!(screen.center_weight, 0.333);
        assert_eq!(screen.center_expansion, 0.025);
        assert_eq!(screen.velocity_scale, 2.5);
        assert_eq!(screen.angular_base, 1.);
        assert_eq!(screen.angular_variation, 0.15);
        assert_eq!(screen.radians_per_degree, 0.017453292);
        assert_eq!(screen.secondary_rotation_scale, 1.5);
        assert_eq!(screen.draw_depth, -0.5);
        assert_eq!(table.placement.leader_z, [0., -250., 250., -450.]);
        assert_eq!(table.placement.front_x, -300.);
        assert_eq!(table.placement.row_step, -200.);
        assert_eq!(table.placement.member_x, -50.);
        assert_eq!(table.placement.member_z, -400.);
        assert_eq!(table.placement.other_row_center, 200.);
        assert_eq!(table.placement.single_row_z, [150., -150.]);
        let chant: Vec<_> = table
            .chant
            .iter()
            .flat_map(|row| {
                let mut bytes = row.time.to_be_bytes().to_vec();
                bytes.extend([
                    row.clip,
                    row.blend,
                    row.start,
                    row.end,
                    row.layer_flags,
                    row.resource as u8,
                ]);
                bytes.extend(row.rate.bits().to_be_bytes());
                bytes
            })
            .collect();
        assert_eq!(chant, module.at((5, 0x11e0))?[..48]);
        assert_eq!(
            table.chant.iter().map(|row| row.time).collect::<Vec<_>>(),
            [0, 52, 80, -2]
        );
        assert_eq!(table.records[2].effect_scale.bits(), 0.9_f32.to_bits());
        let restored: Vec<_> = table.records.iter().flat_map(source_bytes).collect();
        assert_eq!(restored, module.at((5, 0x3d30))?[..11 * BYTES]);
        assert_eq!(table.records[0].guard_reduction, 75);
        assert_eq!(table.records[0].stagger_threshold, 100);
        assert_eq!(table.records[0].stagger_ticks, 45);
        assert_eq!(table.records[0].voice_base, 1);
        assert_eq!(table.records[1].voice_base, 121);
        assert_eq!(table.records[2].voice_base, 241);
        assert_eq!(table.records[3].voice_base, 362);
        assert_eq!(table.voice_sequences.len(), 10);
        assert_eq!(
            table.voice_sequences[3]
                .iter()
                .find(|row| row.technique == 237),
            Some(&VoiceSequence {
                technique: 237,
                chant: 0x81ca,
                release: 0x81a7
            })
        );
        for (character, records) in table.voice_sequences.iter().enumerate() {
            if let Some(&pointer) = module.pointers.get(&(5, 0x59d8 + character * 4)) {
                let original = module.at(pointer)?;
                let restored: Vec<_> = records
                    .iter()
                    .flat_map(|row| [row.technique, row.chant, row.release])
                    .flat_map(u16::to_be_bytes)
                    .collect();
                assert_eq!(restored, original[..restored.len()]);
                assert_eq!(&original[restored.len()..restored.len() + 2], &[0, 0]);
            } else {
                assert!(records.is_empty());
            }
        }
        if let Some(previous) = previous {
            assert_eq!(json, previous);
        }
        previous = Some(json);
    }
    Ok(())
}
