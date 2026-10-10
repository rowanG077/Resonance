//! Decode enemy traits and decisions without opening body or animation resources.
use crate::read::u16 as half;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_enemy::Definition;

pub(crate) fn read(
    bytes: &[u8],
    enemy: u8,
    voices: &crate::battle_voice::Source,
    default_guard_bonus: u8,
) -> Result<Definition> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
    let metadata = bytes
        .get(usize::from(half(bytes, 4)?)..)
        .context("missing enemy profile")?;
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let projectiles = members
        .model(0x1c8)?
        .map(|member| {
            let start = crate::read::u32(bytes, 0x1c8)? as usize;
            crate::battle_projectile::read_package_member(member, start)
        })
        .transpose()?
        .map_or_else(Vec::new, |table| table.records);
    let strategy = enemy_strategy(bytes)?;
    let mut actions = crate::battle_action::read(bytes, enemy, &projectiles)?;
    let target_strategy =
        match resonance_content::battle_action::TargetPolicy::try_from(strategy[0]) {
            Ok(policy) => Some(policy),
            Err(error) => {
                actions
                    .policy
                    .unsupported_reason
                    .get_or_insert_with(|| error.to_string());
                None
            }
        };
    let (entry_row, guard_recovery_bonus) = guard_policy(strategy[2], default_guard_bonus)?;
    Ok(Definition {
        source_sha256: crate::digest(bytes),
        profile: crate::battle_profile::read(metadata, voices, crate::battle_voice::ENEMY_ROLES)?,
        actions,
        target_strategy,
        entry_row,
        guard_recovery_bonus,
    })
}

fn guard_policy(
    value: u8,
    default_bonus: u8,
) -> Result<(resonance_content::battle_enemy::EntryRow, u8)> {
    use resonance_content::battle_enemy::EntryRow;
    let row = match value {
        0 => EntryRow::Random,
        1 => EntryRow::Front,
        2 => EntryRow::Middle,
        3 => EntryRow::Back,
        _ => anyhow::bail!("invalid enemy entry row {value}"),
    };
    let bonus = if value == 0 {
        default_bonus
    } else {
        resonance_content::battle_recoil::strategy_guard_recovery(value)
    };
    Ok((row, bonus))
}

fn enemy_strategy(bytes: &[u8]) -> Result<[u8; 3]> {
    let start = usize::from(half(bytes, 6)?);
    Ok(bytes
        .get(start..start + 3)
        .context("truncated enemy strategy")?
        .try_into()
        .unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guard_policy_decodes_placement_and_resolves_actual_recovery_bonus() -> Result<()> {
        use resonance_content::battle_enemy::EntryRow;
        for (value, row, bonus) in [
            (0, EntryRow::Random, 7),
            (1, EntryRow::Front, 0),
            (2, EntryRow::Middle, 5),
            (3, EntryRow::Back, 25),
        ] {
            assert_eq!(guard_policy(value, 7)?, (row, bonus));
        }
        assert!(guard_policy(4, 7).is_err());
        Ok(())
    }
}
