//! Authored enemy voice holds use the shared battle presentation clock.
use super::*;
use resonance_content::{
    battle_enemy::{Opening, Presentation},
    battle_profile::Profile,
    battle_voice::TimedVoice,
};

#[path = "entrances.rs"]
mod entrances;

pub(super) fn verify(image: &pe::Image<'_>) -> Result<()> {
    entrances::verify(image)?;
    for (address, bytes) in [
        (0x401035, &[0x8b, 0x80, 0xf8, 0, 0, 0][..]),
        (0x401042, &[0x8b, 0x91, 0x10, 1, 0, 0]),
        (0x401048, &[0x52, 0x6a, 6]),
        (0x4010bc, &[0x8b, 0x89, 0xfc, 0, 0, 0]),
        (0x4010df, &[0x81, 0xc1, 0x8c, 2, 0, 0]),
        (0x4010eb, &[0x0f, 0xb7, 0x0c, 0x4a]),
        (0x4632b6, &[0xa9, 0, 0, 0xff, 0xff]),
        (0x4632c2, &[0x05, 0x8c, 2, 0, 0]),
        (0x4632cd, &[0x0f, 0xb7, 4, 0x41]),
        (0x4632d1, &[0xa8, 1, 0x74, 4, 0x40, 0x0f, 0xb7, 0xc0]),
        (0x467af1, &[0x8b, 0x48, 0x34]),
        (0x467afa, &[0x89, 0x87, 0x3c, 0x9a, 0, 0]),
    ] {
        ensure!(
            image.slice(address, bytes.len())? == bytes,
            "enemy voice-hold consumer changed at {address:#x}"
        );
    }
    Ok(())
}

fn ticks(durations: &[u8], encoded: u32) -> Result<u16> {
    let index = if encoded >> 16 != 0 {
        encoded & 0xffff
    } else {
        encoded
            .checked_add(652)
            .context("presentation voice index overflow")?
    };
    let value = u32::from(pe::half(durations, usize::try_from(index)? * 2)?);
    let rounded = (value + 1) & !1;
    ensure!(
        rounded <= u32::from(u16::MAX - 60),
        "enemy voice hold exceeds battle clock"
    );
    Ok(rounded as u16)
}

pub(super) fn read(
    selector: u8,
    metadata: &[u8],
    durations: &[u8],
    profile: &mut Profile,
) -> Result<(Option<Presentation>, bool)> {
    let has_entrance = entrances::has_callback(selector)?;
    if !matches!(
        selector,
        2 | 3 | 6 | 8 | 10 | 12 | 13 | 15 | 16 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27
    ) {
        return Ok((None, has_entrance));
    }
    let override_id = pe::word(metadata, 0xf8)?;
    let defeat_id = if override_id != 0 {
        profile.voices.get_or_insert_with(Default::default).defeat =
            battle_voices::sound(override_id)?;
        override_id
    } else {
        pe::word(metadata, 0x110)?
            .checked_add(1)
            .context("defeat voice base overflow")?
            & 0xffff
    };
    let defeat = TimedVoice {
        sound: profile.voices.as_ref().and_then(|voices| voices.defeat),
        ticks: ticks(durations, defeat_id)?,
    };
    let voiced_opening = matches!(selector, 3 | 6 | 8 | 19 | 20 | 23 | 24);
    let opening = if voiced_opening {
        let id = pe::word(metadata, 0xfc)?;
        Some(Opening::Voice(TimedVoice {
            sound: battle_voices::sound(id)?,
            ticks: ticks(durations, id)?,
        }))
    } else {
        let fallback = if selector == 22 {
            let id = pe::word(metadata, 0xfc)?;
            serde_json::to_value(TimedVoice {
                sound: battle_voices::sound(id)?,
                ticks: ticks(durations, id)?,
            })?
        } else {
            serde_json::Value::Null
        };
        entrances::read(selector, profile, fallback)?
    };
    let deferred = has_entrance && opening.is_none();
    Ok((Some(Presentation { opening, defeat }), deferred))
}

/// Package-relative launch commands join the ordinary shared presentation data.
pub(super) fn read_package(
    package: &[u8],
    durations: &[u8],
    profile: &mut Profile,
) -> Result<(Option<Presentation>, bool)> {
    let selector = super::section(package, 12)?[8];
    let metadata = crate::monsters::metadata(package)?;
    let (mut presentation, deferred) = read(selector, metadata, durations, profile)?;
    if selector == 12 {
        presentation
            .as_mut()
            .context("missing Efreet presentation")?
            .opening = Some(entrances::falling(package, metadata, profile)?);
        return Ok((presentation, false));
    }
    Ok((presentation, deferred))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actor_and_stream_voice_holds_round_up_on_the_authored_clock() -> Result<()> {
        let mut durations = vec![0; 656 * 2];
        durations[2..4].copy_from_slice(&31u16.to_le_bytes());
        durations[654 * 2..655 * 2].copy_from_slice(&84u16.to_le_bytes());
        assert_eq!(ticks(&durations, 0x0a0001)?, 32);
        assert_eq!(ticks(&durations, 2)?, 84);
        assert!(ticks(&durations, u32::MAX).is_err());
        durations[2..4].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(ticks(&durations, 0x0a0001).is_err());
        Ok(())
    }
}
