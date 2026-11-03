//! Steam's inserted profile words and numeric storage stop at this boundary.
use crate::{battle_normal::canonical, battle_voices, pe};
use anyhow::{Context, Result, ensure};
use resonance_content::{battle_profile::Profile, battle_voice::Voices};

pub(crate) fn party(image: &pe::Image<'_>) -> Result<Vec<Profile>> {
    ensure!(
        image.slice(0x442ad3, 14)?
            == [
                0xc1, 0xe6, 9, 0x8d, 0xb6, 0x18, 0x1d, 0x8e, 0, 0xb9, 0x80, 0, 0, 0
            ],
        "party profile layout changed"
    );
    let selectors = battle_voices::selectors(image)?;
    let mut profiles = image
        .slice(0x8e1f18, 9 * 512)?
        .as_chunks::<512>()
        .0
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let voices = battle_voices::actor(&selectors, row)?;
            let mut profile =
                decode(row, voices).with_context(|| format!("party profile {}", index + 1))?;
            // The shared fields name joints in the cooked body. Steam's GLBs
            // retain these source labels rather than GC's renamed joints.
            profile.head_anchor.bone = "Bone_atama".into();
            profile.overlimit_bone = "Bone_sebone01".into();
            Ok(profile)
        })
        .collect::<Result<Vec<_>>>()?;
    battle_voices::party_extras(image, &mut profiles)?;
    Ok(profiles)
}

fn decode(bytes: &[u8], voices: Option<Voices>) -> Result<Profile> {
    resonance_importer::decode_party_profile(&canonical_profile(bytes)?, voices)
}

pub(crate) fn canonical_profile(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(bytes.len() >= 512, "truncated Steam actor profile");
    let mut row = [0; 496];
    row[..0xac].copy_from_slice(&bytes[..0xac]);
    row[0xac..0x104].copy_from_slice(&bytes[0xb0..0x108]);
    row[0x104..].copy_from_slice(&bytes[0x110..0x1fc]);
    // These are whole condition masks, not two independently ordered words.
    for at in [0x10, 0x18, 0x38] {
        row[at..at + 8].reverse();
    }
    let fields: Vec<_> = [0x2a, 0x56, 0x58, 0xae, 0xb4, 0xbe]
        .into_iter()
        .map(|at| (at, 2))
        .chain(
            [0x20, 0x24, 0x5c, 0xa0, 0xb0, 0x104]
                .into_iter()
                .chain((0x60..0x94).step_by(4))
                .map(|at| (at, 4)),
        )
        .collect();
    canonical(&row, 496, &fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_conditions::Condition;

    #[test]
    fn shifted_fields_and_whole_condition_masks_use_the_shared_decoder() -> Result<()> {
        let mut row = [0; 512];
        row[0x10..0x18].copy_from_slice(&(1_u64 << 32).to_le_bytes());
        row[0x18..0x20].copy_from_slice(&0x20_u64.to_le_bytes());
        row[0x20..0x24].copy_from_slice(&3.5_f32.to_le_bytes());
        row[0x84..0x88].copy_from_slice(&1_f32.to_le_bytes());
        row[0xa0..0xa4].copy_from_slice(&0.5_f32.to_le_bytes());
        row[0xb1] = 20;
        row[0xb2..0xb4].copy_from_slice(&200_i16.to_le_bytes());
        row[0xb4..0xb8].copy_from_slice(&1.5_f32.to_le_bytes());
        row[0x114..0x118].copy_from_slice(&[16, 32, 48, 176]);
        row[0x127] = 3;
        row[0x130] = 0x11;
        let profile = decode(&row, None)?;
        assert!(profile.initial_conditions.contains(Condition::Quartz));
        assert!(profile.immunities.contains(Condition::Petrified));
        assert_eq!(profile.walk_speed, 3.5);
        assert_eq!(
            (
                profile.overlimit_gain,
                profile.initial_overlimit,
                profile.overlimit_scale
            ),
            (20, 200, 1.5)
        );
        assert_eq!(profile.shadow.unwrap().color, [16, 32, 48, 176]);
        assert_eq!(profile.armor, 3);
        assert!(profile.weapon_styles[0].before_body && profile.weapon_styles[0].additive);
        assert!(decode(&row[..511], None).is_err());
        Ok(())
    }
}
