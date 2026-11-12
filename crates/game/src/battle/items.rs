//! Prepared ordinary-item resources and a borrowed view of the session inventory.
use super::{model::ModelSource, party::Character, voice};
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, EffectAppearance, ModelDefinition, Sound, item};
use resonance_content::{battle_effect::Tints, battle_profile::Profile, menu_data::Item};
use std::{collections::BTreeMap, sync::Arc};

/// Menu observations borrow the same Party that the synchronous release spends.
/// Live conditions, availability and vitals belong to Battle.
pub struct Inventory<'a> {
    pub counts: &'a BTreeMap<u16, u8>,
    pub definitions: &'a [Item],
    pub recent: &'a [u16],
    pub roster: &'a [(ActorId, Character)],
    pub names: [&'a str; 9],
}

/// Scene feedback for committed item releases. Each use requests one user voice.
pub struct Feedback {
    pub use_effect: EffectAppearance,
    pub revival_effect: EffectAppearance,
    pub sound: Option<Sound>,
    pub buff_tint: Option<[u8; 3]>,
    pub scan_tint: Option<[u8; 3]>,
    pub actors: Vec<ActorFeedback>,
}

#[derive(Default)]
pub struct ActorFeedback {
    pub use_offset: [f32; 3],
    pub used: Option<Sound>,
    pub discovery: Option<Sound>,
}

/// Bind item timing and optional scene feedback in the same actor visit.
pub fn prepare<'a>(
    resolver: &voice::Resolver<'_>,
    tints: Option<&Tints>,
    actors: impl IntoIterator<Item = (ModelSource, Option<&'a ModelDefinition>, &'a Profile, bool)>,
    common_resource: resonance_battle::ResourceId,
    policies: Arc<BTreeMap<u16, item::Policy>>,
    mut resolve: impl FnMut(voice::Sound) -> Result<Option<Sound>>,
) -> Result<(item::Definition, Feedback)> {
    let mut rows = Vec::new();
    let mut targets = Vec::new();
    let mut feedback = Vec::new();
    for (source, model, profile, quick) in actors {
        targets.push(item::TargetDefinition {
            inactive_selectable: profile.traits.inactive_item_target,
            excluded: profile.traits.item_target_excluded,
        });
        let ModelSource::Party(character) = source else {
            ensure!(
                matches!(source, ModelSource::Enemy(_)),
                "item roster contains a non-actor"
            );
            rows.push(None);
            feedback.push(ActorFeedback::default());
            continue;
        };
        rows.push(Some(item::ActorDefinition {
            motion: super::model::common_motion(model, super::model::MotionRole::Item),
            quick,
        }));
        feedback.push(ActorFeedback {
            use_offset: *resolver
                .party_table()
                .item_use_offsets
                .get(character.index())
                .context("missing item-use effect offset")?,
            used: resolver.select(source, |v| v.item_use, &mut resolve)?,
            discovery: if character == super::party::Character::Raine {
                resolver.absolute(source, Some(voice::Sound::Cue(878)), &mut resolve)?
            } else {
                None
            },
        });
    }
    Ok((
        item::Definition {
            policies,
            actors: rows,
            targets,
        },
        Feedback {
            use_effect: EffectAppearance {
                resource: common_resource,
                member: 9,
            },
            revival_effect: EffectAppearance {
                resource: common_resource,
                member: 42,
            },
            sound: resolve(voice::Sound::Cue(76))?,
            scan_tint: tints.map(|tints| tints.actors.scan[..3].try_into().unwrap()),
            buff_tint: tints.map(|tints| tints.actors.buff[..3].try_into().unwrap()),
            actors: feedback,
        },
    ))
}

/// Bind each item's authored battle use once, before battle starts. Item IDs
/// stay inventory and notice keys; the simulation never interprets them.
pub(super) fn policies(items: &[Item]) -> BTreeMap<u16, item::Policy> {
    use resonance_battle::conditions::{Buff, Cure};
    use resonance_content::items::{BattleBuff, BattleCure, BattleUse};
    items
        .iter()
        .enumerate()
        .filter_map(|(id, item)| {
            let effect = match item.battle_use? {
                BattleUse::Recover {
                    hp,
                    tp,
                    party: false,
                    ..
                } => item::Effect::Recover { hp, tp },
                BattleUse::Recover {
                    hp,
                    tp,
                    party: true,
                    ..
                } => item::Effect::PartyRecover { hp, tp },
                BattleUse::FullRecovery => item::Effect::FullRecovery,
                BattleUse::Revive => item::Effect::Revive,
                BattleUse::Cure { ailments } => item::Effect::Cure(match ailments {
                    BattleCure::Physical => Cure::Physical,
                    BattleCure::Magical => Cure::AntiMagic,
                    BattleCure::All => Cure::All,
                }),
                BattleUse::Buff { buff } => item::Effect::Buff(match buff {
                    BattleBuff::Flare => Buff::Flare,
                    BattleBuff::Guard => Buff::Guard,
                    BattleBuff::Acuity => Buff::Acuity,
                    BattleBuff::PhysicalAilmentGuard => Buff::PhysicalAilmentGuard,
                    BattleBuff::MagicalAilmentGuard => Buff::MagicalAilmentGuard,
                }),
                BattleUse::Quartz { element } => item::Effect::Buff(Buff::Quartz(element)),
                BattleUse::Scan => item::Effect::Scan,
                BattleUse::AllDivide => item::Effect::AllDivide,
                BattleUse::AllMultiply => item::Effect::AllMultiply,
                BattleUse::Hourglass => item::Effect::Hourglass,
                BattleUse::UnisonGauge => item::Effect::UnisonGauge,
            };
            Some((
                id as u16,
                item::Policy {
                    effect,
                    records_gel_use: matches!(
                        item.battle_use,
                        Some(BattleUse::Recover { gel: true, .. })
                    ),
                },
            ))
        })
        .collect()
}
