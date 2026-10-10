//! Bind prepared normal attacks and player movement before battle entry.
use super::model::MotionRole;
use anyhow::{Result, ensure};
use resonance_battle::{ControlDefinition, ControlMotions};
use resonance_content::battle_profile::Profile;

/// Bind player locomotion, using the walk clip when a rig has no separate run clip.
pub fn motions(model: Option<&resonance_battle::ModelDefinition>) -> Option<ControlMotions> {
    let motion = |clip| super::model::common_motion(model, clip);
    Some(ControlMotions {
        walk: motion(MotionRole::Walk)?,
        run: motion(MotionRole::Run)?,
        stop: motion(MotionRole::Stop)?,
        landing: motion(MotionRole::Landing)?,
    })
}

pub fn party(
    profile: &Profile,
    character: u8,
    actions: [resonance_battle::ActionKey; 7],
    motions: Option<ControlMotions>,
) -> Result<ControlDefinition> {
    Ok(ControlDefinition {
        normals: super::normal::controls(character, actions)?,
        shortcuts: [0; 4],
        walk_speed: profile.walk_speed,
        run_speed: profile.run_speed,
        turn_ticks: profile.turn_ticks,
        motions,
    })
}

/// Technique metadata shared by player control, companion policy, learning and commands.
pub fn techniques(
    catalogue: &resonance_content::arte::Catalogue,
    character: u8,
    actions: &std::collections::BTreeMap<u16, resonance_battle::ActionKey>,
) -> Result<Vec<resonance_battle::PreparedTechnique>> {
    let learned_by = catalogue.learned_by(character)?;
    actions
        .iter()
        .map(|(&id, &action)| {
            ensure!(
                learned_by.iter().any(|&learned| u16::from(learned) == id),
                "prepared technique has no character catalogue entry"
            );
            let descriptor = catalogue.definition(usize::from(id))?;
            ensure!(
                descriptor.capabilities.target != resonance_battle::TechniqueTarget::Unavailable,
                "technique target is unavailable"
            );
            let special_guard = super::martial::is_special_guard(character, id);
            if special_guard {
                return Ok(resonance_battle::PreparedTechnique {
                    action,
                    catalogue: id,
                    player_range: [0.; 2],
                    ai_range: [0.; 2],
                    capabilities: resonance_battle::TechniqueCapabilities {
                        family: Some(resonance_battle::ArteFamily::Arcane),
                        target: resonance_battle::TechniqueTarget::SelfTarget,
                        ..Default::default()
                    },
                    element: descriptor.element,
                });
            }
            let maximum = descriptor.action_range;
            ensure!(
                maximum.is_finite() && maximum > 0.,
                "invalid technique range"
            );
            let capabilities = descriptor.capabilities;
            let ai_minimum = if capabilities.uses_weapon_reach {
                0.
            } else if maximum < 1000. {
                (maximum - 100.).max(0.)
            } else {
                500.
            };
            Ok(resonance_battle::PreparedTechnique {
                action,
                catalogue: id,
                player_range: [0., maximum],
                ai_range: [ai_minimum, maximum],
                capabilities,
                element: descriptor.element,
            })
        })
        .collect()
}
