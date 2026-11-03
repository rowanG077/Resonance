//! Stored skill artwork joins the ordinary effect and model resource preparation.
use anyhow::{Context, Result};
use resonance_content::battle_scene;

pub(crate) fn contains(catalogue: u16) -> bool {
    matches!(catalogue, 51 | 153 | 228 | 249)
}

/// These callbacks initialize their two stored programs independently of the
/// ordinary technique effect bank. Times include the native pose blend holds.
pub(crate) fn holy_binds(
    resources: &mut super::encounter::resources::Resources<'_>,
    attack: &mut resonance_battle::PreparedAttack,
    catalogue: u16,
) -> Result<()> {
    let second_at = match catalogue {
        249 => 44, // Native action 148: raw tick 34, opening blend 10.
        153 => 12, // Native action 151: raw tick 4, opening blend 8.
        _ => return Ok(()),
    };
    let bank = resources
        .skill_scenes
        .get(&catalogue)
        .context("Holy Binds has no prepared skill artwork")?
        .clone();
    let first = resources.prepared.effect_member(&bank, 1)?;
    let second = resources.prepared.effect_member(&bank, 2)?;
    holy_binds_effects(attack, first, second, second_at);
    Ok(())
}

fn holy_binds_effects(
    attack: &mut resonance_battle::PreparedAttack,
    first: resonance_battle::EffectAppearance,
    second: resonance_battle::EffectAppearance,
    second_at: u16,
) {
    use resonance_battle::{AttackEvent, AttackOrigin};
    for (at, appearance) in [(0, first), (second_at, second)] {
        attack.events.push((
            at,
            AttackEvent::InitializedActionEffect {
                appearance,
                origin: AttackOrigin::ActorRoot,
            },
        ));
    }
    attack.events.sort_by_key(|(at, _)| *at);
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::{
        AttackEvent, AttackOrigin, EffectAppearance, PreparedAttack, ResourceId,
    };

    #[test]
    fn skill_initialization_preserves_existing_technique_effect_ownership() {
        let technique = EffectAppearance {
            resource: ResourceId(1),
            member: 3,
        };
        let first = EffectAppearance {
            resource: ResourceId(2),
            member: 1,
        };
        let second = EffectAppearance {
            resource: ResourceId(2),
            member: 2,
        };
        let mut attack = PreparedAttack::new(350);
        attack.events.push((
            0,
            AttackEvent::ActionEffect {
                appearance: technique,
                origin: AttackOrigin::ActorRoot,
            },
        ));
        holy_binds_effects(&mut attack, first, second, 44);
        assert!(
            matches!(attack.events[0], (0, AttackEvent::ActionEffect { appearance, .. }) if appearance == technique)
        );
        assert!(
            matches!(attack.events[1], (0, AttackEvent::InitializedActionEffect { appearance, .. }) if appearance == first)
        );
        assert!(
            matches!(attack.events[2], (44, AttackEvent::InitializedActionEffect { appearance, .. }) if appearance == second)
        );
    }
}

/// Falcon's Crest keeps its artwork on the same elapsed schedule as its prepared attack.
pub(crate) fn falcon(
    resources: &mut super::encounter::resources::Resources<'_>,
    attack: &mut resonance_battle::PreparedAttack,
    model: &resonance_battle::ModelDefinition,
) -> Result<()> {
    use resonance_battle::{AttackEvent, AttackOrigin, EffectAppearance, SpellCamera, StageTint};
    const CATALOGUE: u16 = 228;
    const SWORD_APPEARS: u16 = 25;
    const SWORD_RESTORES: u16 = 304;
    const EFFECT_HOLD_START: u16 = 141;
    const EFFECT_HOLD_DURATION: u16 = 32;
    const PORTRAIT_START: u16 = 142;
    const PORTRAIT_DURATION: u16 = 64;

    let mut bank = resources
        .skill_scenes
        .get(&CATALOGUE)
        .context("Falcon's Crest has no prepared scene")?
        .clone();
    bank.members = vec![1, 2];
    resources.prepared.effect(bank.clone())?;
    let weapons = resources
        .skill_weapons
        .get(&(resonance_battle::Character::Lloyd, CATALOGUE))
        .context("Falcon's Crest has no prepared sword")?
        .clone();
    let effect = |member| EffectAppearance {
        resource: bank.resource,
        member,
    };
    attack.events.extend([
        (
            0,
            AttackEvent::StageTint(StageTint {
                color: [0; 3],
                duration: 316,
            }),
        ),
        (
            0,
            AttackEvent::SceneCamera(SpellCamera {
                bounds: battle_scene::CameraBounds {
                    minimum_distance: 2050.,
                    minimum_elevation: 20.5,
                },
                duration: 301,
            }),
        ),
        (
            0,
            AttackEvent::InitializedActionEffect {
                appearance: effect(1),
                origin: AttackOrigin::ActorRoot,
            },
        ),
        (SWORD_APPEARS, AttackEvent::TemporaryWeapons(weapons)),
        (
            EFFECT_HOLD_START,
            AttackEvent::HoldEffects(EFFECT_HOLD_DURATION),
        ),
        (
            SWORD_APPEARS,
            AttackEvent::WeaponVisibility(vec![(Some(resonance_battle::WeaponSlot(1)), false)]),
        ),
        (
            PORTRAIT_START,
            AttackEvent::Cutin {
                catalogue: resonance_battle::TechniqueId(CATALOGUE),
                duration: PORTRAIT_DURATION,
            },
        ),
        (SWORD_RESTORES, AttackEvent::RestoreWeapons),
    ]);
    for (at, name) in [
        (SWORD_APPEARS, "kk00"),
        (SWORD_RESTORES, "kk00"),
        (SWORD_RESTORES, "kk01"),
    ] {
        let bone = model
            .skeleton
            .bones
            .iter()
            .position(|bone| bone.name == name)
            .with_context(|| format!("Falcon's Crest is missing sword attachment {name}"))?;
        attack.events.push((
            at,
            AttackEvent::EffectAt {
                appearance: effect(2),
                origin: AttackOrigin::Bone(resonance_battle::MeleeAnchor {
                    resource: model.resource,
                    bone: u16::try_from(bone)?,
                }),
                heading: None,
                scale: Some(1.),
                follow: false,
            },
        ));
    }
    attack.events.sort_by_key(|(at, _)| *at);
    Ok(())
}
