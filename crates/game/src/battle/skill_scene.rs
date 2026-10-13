//! Stored skill artwork joins the ordinary effect and model resource preparation.
use anyhow::{Context, Result};
use resonance_content::{
    battle_effect::SourceBank,
    battle_scene,
    prepared::{Cache, Files},
};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub(crate) fn contains(catalogue: u16) -> bool {
    matches!(catalogue, 51 | 228)
}

pub(crate) fn load(
    root: &Path,
    mut files: Files,
    catalogues: impl IntoIterator<Item = u16>,
    cache: &mut Cache,
    cancelled: impl Fn() -> bool,
) -> Result<(Files, BTreeMap<u16, Arc<SourceBank>>)> {
    let mut banks = BTreeMap::new();
    for catalogue in catalogues {
        let scene: battle_scene::Scene = files.json(&battle_scene::skill_path(catalogue))?;
        files = files.with_dependencies(root, scene.files, cache, &cancelled)?;
        if let Some(bank) = super::effect_program::load(&files, &scene.effects)? {
            banks.insert(catalogue, bank);
        }
    }
    Ok((files, banks))
}

/// Falcon's Crest keeps its artwork on the same elapsed schedule as its prepared attack.
pub(crate) fn falcon<S: FnMut(super::voice::Sound) -> Result<Option<resonance_battle::Sound>>>(
    resources: &mut super::encounter::resources::Resources<'_, S>,
    attack: &mut resonance_battle::PreparedAttack,
) -> Result<()> {
    use resonance_battle::{AttackEvent, AttackOrigin, EffectAppearance, SpellCamera, StageTint};
    const CATALOGUE: u16 = 228;
    const LLOYD: u8 = 1;
    const SWORD_APPEARS: u16 = 27;
    const SWORD_RESTORES: u16 = 306;
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
        .get(&(LLOYD, CATALOGUE))
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
            AttackEvent::ActionEffect {
                appearance: effect(1),
                origin: AttackOrigin::ActorRoot,
            },
        ),
        (SWORD_APPEARS, AttackEvent::TemporaryWeapons(weapons)),
        (
            SWORD_APPEARS,
            AttackEvent::WeaponVisibility(vec![(1, false)]),
        ),
        (
            PORTRAIT_START,
            AttackEvent::Cutin {
                catalogue: CATALOGUE,
                duration: PORTRAIT_DURATION,
            },
        ),
        (SWORD_RESTORES, AttackEvent::RestoreWeapons),
    ]);
    for at in [SWORD_APPEARS, SWORD_RESTORES] {
        attack.events.push((
            at,
            AttackEvent::EffectAt {
                appearance: effect(2),
                origin: AttackOrigin::ActorAim,
                heading: None,
                scale: Some(1.),
                follow: false,
            },
        ));
    }
    attack.events.sort_by_key(|(at, _)| *at);
    Ok(())
}
