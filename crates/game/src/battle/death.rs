//! Prepare optional feedback for a confirmed defeat.
use anyhow::{Result, ensure};
use resonance_battle::{ActorId, EffectAppearance, Sound};

pub struct Feedback {
    pub appearance: EffectAppearance,
    pub sound: Option<Sound>,
    pub allies: Vec<AllyReaction>,
}

pub struct AllyReaction {
    pub victim: ActorId,
    pub recipient: ActorId,
    pub voices: [Option<Sound>; 2],
}

/// The highest-affinity companion reacts to Lloyd; ties favor the lowest character ID.
fn closest(affinities: [i32; 9]) -> u8 {
    (2..=9)
        .max_by_key(|&id| (affinities[usize::from(id - 1)], std::cmp::Reverse(id)))
        .unwrap()
}

/// Prepare optional relationship voice lines and enemy defeat feedback.
pub fn feedback(
    voices: &super::voice::Resolver<'_>,
    actors: &[super::model::ModelSource],
    party: &resonance_events::party::Party,
    common_resource: u32,
    mut resolve: impl FnMut(super::voice::Sound) -> Result<Option<resonance_battle::Sound>>,
) -> Result<Feedback> {
    let affinities = party
        .members
        .iter()
        .take(9)
        .map(|member| member.affinity)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| anyhow::anyhow!("missing party affinities"))?;
    let table = voices.party_table();
    let pairs: std::collections::BTreeSet<_> = table.death_voice_pairs.iter().copied().collect();
    ensure!(
        pairs.len() == table.death_voice_pairs.len()
            && pairs.iter().flatten().all(|id| (1..=9).contains(id)),
        "invalid death voice pair"
    );
    let nearest = closest(affinities);
    let voices = actors
        .iter()
        .map(|&actor| {
            Ok([
                voices.select(actor, |v| v.ally_defeated[0], &mut resolve)?,
                voices.select(actor, |v| v.ally_defeated[1], &mut resolve)?,
            ])
        })
        .collect::<Result<Vec<_>>>()?;
    let mut allies = Vec::new();
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
            allies.push(AllyReaction {
                victim: ActorId::from_index(victim_index)?,
                recipient: ActorId::from_index(recipient_index)?,
                voices: voices[recipient_index],
            });
        }
    }
    Ok(Feedback {
        appearance: resonance_battle::EffectAppearance {
            resource: common_resource,
            member: 14,
        },
        sound: resolve(super::voice::Sound::Cue(73))?,
        allies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_companion_uses_affinity_then_character_order() {
        assert_eq!(closest([0; 9]), 2);
        assert_eq!(closest([100, 0, 1, 0, 0, 0, 0, 0, 0]), 3);
        // All companions participate, including those outside the active formation.
        assert_eq!(closest([-5, 4, 4, 9, 0, 0, 0, 0, 0]), 4);
        assert_eq!(closest([0, -3, -2, -1, 0, 1, 2, 3, 4]), 9);
    }
}
