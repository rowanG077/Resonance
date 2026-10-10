//! Prepared ordinary-item resources and a borrowed view of the session inventory.
use super::{model::ModelSource, voice};
use anyhow::{Result, ensure};
use resonance_battle::{ActorId, EffectAppearance, ModelDefinition, Sound, item};
use resonance_content::{battle_effect::Tints, battle_profile::Profile, menu_data::Item};
use std::{collections::BTreeMap, sync::Arc};

/// Menu observations borrow the same Party that the synchronous release spends.
/// Live conditions, availability and vitals belong to Battle.
pub struct Inventory<'a> {
    pub counts: &'a BTreeMap<u16, u8>,
    pub definitions: &'a [Item],
    pub recent: &'a [u16],
    pub roster: &'a [(ActorId, u8)],
    pub names: [&'a str; 9],
}

/// Scene feedback for committed item releases. Each use requests one user voice.
pub struct Feedback {
    pub use_effect: EffectAppearance,
    pub revival_effect: EffectAppearance,
    pub sound: Option<Sound>,
    pub buff_tint: Option<[u8; 3]>,
    pub scan_tint: Option<[u8; 3]>,
    pub voices: Vec<ItemVoice>,
}

#[derive(Default)]
pub struct ItemVoice {
    pub used: Option<Sound>,
    pub discovery: Option<Sound>,
}

/// Bind item timing and optional scene feedback in the same actor visit.
pub fn prepare<'a>(
    resolver: &voice::Resolver<'_>,
    tints: Option<&Tints>,
    actors: impl IntoIterator<Item = (ModelSource, Option<&'a ModelDefinition>, &'a Profile, bool)>,
    common_resource: u32,
    mut resolve: impl FnMut(voice::Sound) -> Result<Option<Sound>>,
) -> Result<(item::Definition, Feedback)> {
    let mut rows = Vec::new();
    let mut targets = Vec::new();
    let mut voices = Vec::new();
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
            voices.push(ItemVoice::default());
            continue;
        };
        rows.push(Some(item::ActorDefinition {
            motion: super::model::common_motion(model, super::model::MotionRole::Item),
            quick,
        }));
        voices.push(ItemVoice {
            used: resolver.select(source, |v| v.item_use, &mut resolve)?,
            discovery: if character == super::party::Character::Raine as u8 {
                resolver.absolute(source, Some(voice::Sound::Cue(878)), &mut resolve)?
            } else {
                None
            },
        });
    }
    Ok((
        item::Definition {
            policies: Arc::new(policies()),
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
            voices,
        },
    ))
}

/// Interpret the current game's inventory identities once, before battle starts.
pub(super) fn policies() -> BTreeMap<u16, item::Policy> {
    use item::Effect::*;
    use resonance_battle::{
        Element,
        conditions::{Buff, Cure},
    };
    [
        (1, Recover { hp: 30, tp: 0 }),
        (2, Recover { hp: 60, tp: 0 }),
        (3, Recover { hp: 0, tp: 30 }),
        (4, Recover { hp: 0, tp: 60 }),
        (5, Recover { hp: 30, tp: 30 }),
        (6, Recover { hp: 60, tp: 60 }),
        (7, FullRecovery),
        (8, PartyRecover { hp: 30, tp: 0 }),
        (9, PartyRecover { hp: 0, tp: 30 }),
        (10, item::Effect::Cure(Cure::Physical)),
        (11, Revive),
        (12, item::Effect::Cure(Cure::All)),
        (13, item::Effect::Cure(Cure::AntiMagic)),
        (14, item::Effect::Buff(Buff::Flare)),
        (15, item::Effect::Buff(Buff::Flare)),
        (16, item::Effect::Buff(Buff::Guard)),
        (17, item::Effect::Buff(Buff::Acuity)),
        (
            18,
            item::Effect::Buff(Buff::PhysicalAilmentGuard { persistent: true }),
        ),
        (
            19,
            item::Effect::Buff(Buff::PhysicalAilmentGuard { persistent: false }),
        ),
        (
            20,
            item::Effect::Buff(Buff::MagicalAilmentGuard { persistent: true }),
        ),
        (
            21,
            item::Effect::Buff(Buff::MagicalAilmentGuard { persistent: false }),
        ),
        (37, Scan),
        (38, AllDivide),
        (39, Hourglass),
        (44, item::Effect::Buff(Buff::Quartz(Element::Water))),
        (45, item::Effect::Buff(Buff::Quartz(Element::Wind))),
        (46, item::Effect::Buff(Buff::Quartz(Element::Fire))),
        (47, item::Effect::Buff(Buff::Quartz(Element::Earth))),
        (48, item::Effect::Buff(Buff::Quartz(Element::Ice))),
        (49, item::Effect::Buff(Buff::Quartz(Element::Lightning))),
        (50, item::Effect::Buff(Buff::Quartz(Element::Darkness))),
        (51, item::Effect::Buff(Buff::Quartz(Element::Light))),
    ]
    .into_iter()
    .map(|(id, effect)| {
        (
            id,
            item::Policy {
                effect,
                records_gel_use: (1..=6).contains(&id),
            },
        )
    })
    .collect()
}
