//! Resolve companion defaults and policy limits during encounter preparation.
use super::party::Character;
use resonance_battle::{CompanionDefinition, PolicyLimits};
use resonance_content::battle_profile;
use resonance_events::party::Member;

pub fn prepare(
    profiles: &battle_profile::Table,
    character: Character,
    member: &Member,
) -> CompanionDefinition {
    CompanionDefinition {
        normal_attack_opening: matches!(character, Character::Kratos | Character::Zelos),
        initial_policy: member.strategy,
        defaults: profiles.default_strategy[character.index()],
        limits: std::array::from_fn(|index| PolicyLimits {
            tp: profiles.companion_policy.tp_limits[index],
            healing: profiles.companion_policy.healing_limits[index],
        }),
    }
}
