//! Decode voice storage and publish explicit audio references.
use anyhow::{Context, Result, ensure};
use resonance_content::battle_voice::{Sound, Voices};

pub(crate) const PARTY_ROLES: usize = 58;
pub(crate) const ENEMY_ROLES: usize = 10;

pub(crate) struct Source {
    relative: Vec<Sound>,
}

pub(crate) fn sound(encoded: u16) -> Option<Sound> {
    match encoded {
        0 => None,
        value if value & 0x8000 != 0 => Some(Sound::Stream(value & 0x7fff)),
        value => Some(Sound::Cue(value + 501)),
    }
}

impl Source {
    pub fn actor_voices(&self, base: u32, count: usize) -> Result<Option<Voices>> {
        if base == 0 {
            return Ok(None);
        }
        let base = usize::try_from(base)?;
        let end = base.checked_add(count).context("voice range overflow")?;
        let lines = self
            .relative
            .get(base..end)
            .context("missing actor voices")?;
        let line = |index| lines.get(index).copied();
        Ok(Some(Voices {
            hurt: [line(3), line(4)],
            defeat: line(6),
            critical: line(8),
            guard: line(9),
            ally_defeated: [line(10), line(11)],
            technique_command: line(13),
            item_use: line(14),
            entry: [line(16), line(17), line(18)],
            major_enemy: line(19),
            outnumbered: line(20),
            weaker_enemies: line(21),
            stronger_enemies: line(22),
            repeated_encounter: line(23),
            victory: (28..32).filter_map(line).collect(),
            interrupted_cast: line(49),
            escape_request: line(51),
            escape_success: line(52),
            escape_cancel: line(53),
            taunt: line(56),
            contact_recovery: line(57),
        }))
    }
}

pub(crate) fn read(usual: &[u8]) -> Result<Source> {
    let streams = crate::source_assets::section(usual, 11)?;
    let durations = crate::source_assets::section(usual, 12)?;
    ensure!(
        !durations.is_empty()
            && durations.len().is_multiple_of(2)
            && durations.len() / 2 <= streams.len() * 8
            && durations.len() / 2 <= 32768,
        "invalid battle voice data"
    );
    let relative = (0..durations.len() / 2)
        .map(|index| {
            if streams[index / 8] & (1 << (index % 8)) != 0 {
                Sound::Stream(index as u16)
            } else {
                Sound::Cue(index as u16 + 501)
            }
        })
        .collect();
    Ok(Source { relative })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_storage_becomes_named_sounds() -> Result<()> {
        let mut bytes = vec![0; 56];
        bytes[..4].copy_from_slice(&13_u32.to_be_bytes());
        bytes[48..52].copy_from_slice(&56_u32.to_be_bytes());
        bytes[52..56].copy_from_slice(&57_u32.to_be_bytes());
        bytes.push(0b10);
        bytes.extend([0_u16, 62, 37].into_iter().flat_map(u16::to_be_bytes));
        let source = read(&bytes)?;
        assert_eq!(source.relative[1..], [Sound::Stream(1), Sound::Cue(503)]);
        let roles = Source {
            relative: (0..=58).map(Sound::Stream).collect(),
        };
        let voices = roles.actor_voices(1, PARTY_ROLES)?.unwrap();
        assert_eq!(
            voices.hurt,
            [Some(Sound::Stream(4)), Some(Sound::Stream(5))]
        );
        assert_eq!(
            voices.victory,
            (29..33).map(Sound::Stream).collect::<Vec<_>>()
        );
        assert!(roles.actor_voices(0, PARTY_ROLES)?.is_none());
        assert_eq!(sound(0), None);
        assert_eq!(sound(0x8002), Some(Sound::Stream(2)));
        assert_eq!(sound(2), Some(Sound::Cue(503)));
        assert!(read(&bytes[..bytes.len() - 1]).is_err());
        assert!(source.actor_voices(2, 2).is_err());
        Ok(())
    }
}
