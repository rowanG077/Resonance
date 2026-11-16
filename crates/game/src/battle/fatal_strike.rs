//! Character choreography built from prepared Symphonia performances.
use resonance_battle::{
    ActionDefinitions, ActionExecution, ActorSetup, AttackEvent, AttackPose, Character, FatalColor,
    FatalPerformance, FatalStyle, ModelDefinition, NormalAttack,
};
use std::sync::Arc;
mod mage;
mod rig;
mod weapon;

/// Keep rig binding and source frame units at preparation; the scene owns gameplay timing.
pub(super) fn performances(
    model: &mut Arc<ModelDefinition>,
    setup: &ActorSetup,
    actions: &ActionDefinitions,
    character: Character,
    feedback: &super::feedback::Feedback,
    head: resonance_battle::HeadAnchor,
) -> anyhow::Result<[FatalPerformance; 3]> {
    let style = match character {
        Character::Lloyd => FatalStyle::TwinBlades,
        Character::Colette => FatalStyle::Chakram,
        Character::Genis => FatalStyle::Arcane,
        Character::Raine => FatalStyle::Sacred,
        Character::Sheena => FatalStyle::Seals,
        Character::Zelos => FatalStyle::Spellblade,
        Character::Presea => FatalStyle::HeavyAxe,
        Character::Regal => FatalStyle::Martial,
        Character::Kratos => FatalStyle::Seraph,
    };
    if style.caster() {
        return mage::performances(Arc::make_mut(model), setup, actions, style, feedback);
    }
    weapon::performances(Arc::make_mut(model), setup, actions, style, head)
}
