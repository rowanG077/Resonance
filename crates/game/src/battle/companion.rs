//! Companion normal-policy inputs from the verified strategy and technique tables.
use anyhow::{Result, ensure};
use resonance_battle::{CompanionDefinition, PolicyLimits};
use resonance_content::battle_profile;
use resonance_events::party::Member;

pub fn prepare(
    profiles: &battle_profile::Table,
    character: u8,
    member: &Member,
    level_difference: i8,
) -> Result<CompanionDefinition> {
    ensure!(
        (1..=9).contains(&character),
        "unsupported companion character"
    );
    let defaults = profiles.default_strategy[usize::from(character - 1)];
    ensure!(
        (1..=8).contains(&defaults[0])
            && (1..=8).contains(&defaults[1])
            && (1..=6).contains(&defaults[2]),
        "unprepared companion defaults {defaults:?}"
    );
    let strategy: [u8; 3] = std::array::from_fn(|i| {
        if member.strategy[i] == 0 {
            defaults[i]
        } else {
            member.strategy[i]
        }
    });
    ensure!(
        (1..=8).contains(&strategy[0])
            && (1..=8).contains(&strategy[1])
            && (1..=6).contains(&strategy[2]),
        "unprepared companion strategy {strategy:?}"
    );
    let limits: [PolicyLimits; 9] = std::array::from_fn(|index| PolicyLimits {
        tp: profiles.companion_policy.tp_limits[index],
        healing: profiles.companion_policy.healing_limits[index],
        support_level: profiles.companion_policy.support_level_limits[index],
    });
    ensure!(
        limits.iter().all(|row| row.tp <= 100 && row.healing <= 100),
        "invalid companion policy limits"
    );
    Ok(CompanionDefinition {
        initial_policy: member.strategy,
        defaults,
        limits,
        level: member.level,
        level_difference,
    })
}
