//! Original death descriptor selection. The authored controller shares one set
//! of per-actor optional motion bindings, so it never discovers resources live.
use super::ActionBinding;
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActionPhase, DeathBinding, ModelDefinition, MotionBinding};
use resonance_content::battle_profile::Profile;

pub const FALL: &str = "battle/motions/death/fall";
pub const REST: &str = "battle/motions/death/rest";

pub fn bindings(fall: u16, initial: u16) -> [ActionBinding; 2] {
    [(fall, "fall"), (initial, "initial")].map(|(id, entry)| ActionBinding {
        id,
        phase: ActionPhase::Controller,
        module: "battle::actor_death".into(),
        entry: entry.into(),
        duration: 0,
        tp_cost: 0,
    })
}

/// fn2C05C ignores absent native slots and the no-body-motion descriptor bit.
/// This is optional *source* content; a present clip still must load correctly.
pub fn prepare(
    profile: &Profile,
    model: &ModelDefinition,
    fall: u16,
    initial: u16,
) -> (DeathBinding, [Option<MotionBinding>; 2]) {
    let retains_body = profile.death_motion != 0 || profile.flags & 0x40000 != 0;
    let motion = |clip| {
        (profile.flags & 0x0800_0000 == 0 && model.motions.contains_key(&clip)).then_some(
            MotionBinding {
                model: model.resource,
                clip,
            },
        )
    };
    let rest = if retains_body {
        motion(if profile.death_motion != 0 {
            u16::from(profile.death_motion)
        } else {
            7
        })
    } else {
        None
    };
    (
        DeathBinding {
            fall,
            initial,
            wait_for_motion: profile.flags & 0x0020_0000 != 0,
            integrate: retains_body,
            darken_immediately: !retains_body,
            revival_motion: motion(9),
        },
        [motion(3), rest],
    )
}

/// 19DC4's exchange order matters when affinities tie. Lloyd participates in
/// sorting but is skipped when ranks are assigned; all nine persistent members
/// participate even when absent from this encounter.
fn closest(affinities: [i32; 9]) -> u8 {
    let mut order = [1u8, 2, 3, 4, 5, 6, 7, 8, 9];
    for first in 0..order.len() {
        for other in first + 1..order.len() {
            if affinities[usize::from(order[first] - 1)] < affinities[usize::from(order[other] - 1)]
            {
                order.swap(first, other);
            }
        }
    }
    *order.iter().find(|&&character| character != 1).unwrap()
}

/// Prepare the source relationship filters and both real relative voice lines
/// before activation. `overlimit_boost` is persistent battle flag 0x4 (19F0C).
/// Runtime still checks recipient availability and uses one shared battle draw.
pub fn feedback(
    files: &resonance_content::prepared::Files,
    actors: &[super::model::ModelSource],
    party: &resonance_events::party::Party,
    common_resource: u32,
    overlimit_boost: bool,
    mut resolve: impl FnMut(super::voice::Sound) -> Result<resonance_battle::SoundBinding>,
) -> Result<resonance_battle::DeathFeedback> {
    let affinities = party
        .members
        .iter()
        .take(9)
        .map(|member| member.affinity)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing party affinities"))?;
    let table: resonance_content::battle_profile::Table =
        files.json(resonance_content::battle_profile::PARTY_PATH)?;
    ensure!(
        table.records.len() == 11 && table.death_voice_pairs.len() == 14,
        "invalid death voice table"
    );
    let pairs: std::collections::BTreeSet<_> = table.death_voice_pairs.iter().copied().collect();
    ensure!(
        pairs.len() == table.death_voice_pairs.len()
            && pairs.iter().flatten().all(|id| (1..=9).contains(id)),
        "invalid death voice pair"
    );
    let nearest = closest(affinities);
    let voices = [
        super::voice::relative(files, actors, 10, &mut resolve)?,
        super::voice::relative(files, actors, 11, &mut resolve)?,
    ];
    let mut allies = vec![vec![None; actors.len()]; actors.len()];
    for (victim_index, &victim) in actors.iter().enumerate() {
        let super::model::ModelSource::Party(victim) = victim else {
            continue;
        };
        for (recipient_index, &recipient) in actors.iter().enumerate() {
            let super::model::ModelSource::Party(recipient) = recipient else {
                continue;
            };
            let selected = if victim == 1 {
                recipient == nearest
            } else if recipient == 1 {
                victim == nearest
            } else {
                pairs.contains(&[victim, recipient])
            };
            if !selected {
                continue;
            }
            let profile = table
                .records
                .get(usize::from(
                    recipient.checked_sub(1).context("zero party character")?,
                ))
                .context("missing death recipient profile")?;
            let base = 10 * u16::from(profile.overlimit_gain);
            allies[victim_index][recipient_index] = Some(resonance_battle::AllyDeathReaction {
                voices: [voices[0][recipient_index], voices[1][recipient_index]],
                priority: if victim == 1 { 3 } else { 1 },
                overlimit_gain: base + if overlimit_boost { base >> 1 } else { 0 },
            });
        }
    }
    Ok(resonance_battle::DeathFeedback {
        enemy: Some(resonance_battle::DeathEffect {
            appearance: resonance_battle::EffectAppearance {
                resource: common_resource,
                member: 14,
            },
            sound: resolve(super::voice::Sound::Cue(73))?,
        }),
        allies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relationship_rank_includes_absent_members_and_preserves_exchange_ties() {
        assert_eq!(closest([0; 9]), 2);
        assert_eq!(closest([100, 0, 1, 0, 0, 0, 0, 0, 0]), 3);
        // The signed comparison includes noncombatant members before ranking.
        assert_eq!(closest([-5, 4, 4, 9, 0, 0, 0, 0, 0]), 4);
        assert_eq!(closest([0, -3, -2, -1, 0, 1, 2, 3, 4]), 9);
    }
}
