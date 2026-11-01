//! Resolve companion defaults and policy limits during encounter preparation.
use super::party::Character;
use resonance_battle::CompanionDefinition;
use resonance_content::battle_profile;
use resonance_events::party::Member;

pub fn prepare(
    profiles: &battle_profile::Table,
    character: Character,
    member: &Member,
) -> CompanionDefinition {
    CompanionDefinition {
        character,
        initial_policy: member.strategy,
        defaults: profiles.default_strategy[character.index()],
        target_strategies: profiles.target_strategies,
        positioning: profiles.companion_positioning,
        normal_selection: profiles.companion_normal_selection,
        limits: profiles.companion_policy,
    }
}
