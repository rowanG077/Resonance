//! Enumerate the fixed playable set without constructing a disposable battle.
use super::*;
use resonance_content::{battle_martial, battle_voice::Sound};

pub(super) fn cue(sound: Sound) -> Option<u16> {
    match sound {
        Sound::Cue(id) if id != 0 => Some(id),
        _ => None,
    }
}

pub(super) fn player_sounds(common: &Common) -> BTreeSet<Sound> {
    let mut sounds = BTreeSet::new();
    let party = &common.rules.party;
    sounds.extend(
        crate::battle::audio::COMMON_CUES
            .iter()
            .copied()
            .map(Sound::Cue),
    );
    sounds.extend(
        crate::battle::audio::COMMON_STREAMS
            .iter()
            .copied()
            .map(Sound::Stream),
    );
    sounds.extend(&common.extra_sounds);
    sounds.extend(party.contact_sounds.party.map(Sound::Cue));
    sounds.extend(
        party
            .contact_sounds
            .elements
            .map(|id| Sound::Cue(u16::from(id))),
    );
    sounds.extend(party.overlimit_voices.into_iter().flatten());
    sounds.extend(
        party
            .records
            .iter()
            .filter_map(|profile| profile.voices.as_ref())
            .flat_map(|voices| voices.sounds()),
    );
    sounds.extend(
        party
            .voice_sequences
            .iter()
            .flat_map(|policy| std::iter::once(&policy.default).chain(policy.techniques.values()))
            .flat_map(|sequence| sequence.sounds()),
    );
    sounds.extend(
        party
            .normal_attacks
            .iter()
            .flatten()
            .flat_map(|attack| &attack.events)
            .flat_map(|(_, event)| event.sounds()),
    );
    let variants: BTreeSet<_> = party
        .martial_attacks
        .bindings
        .values()
        .flat_map(|characters| characters.iter())
        .filter(|entry| (1..=9).contains(entry.0))
        .map(|(_, binding)| (binding.member, binding.variant))
        .collect();
    sounds.extend(
        variants
            .into_iter()
            .filter_map(|(member, variant)| {
                party
                    .martial_attacks
                    .variant(battle_martial::Binding { member, variant })
                    .ok()
            })
            .flat_map(battle_martial::Attack::sounds),
    );
    sounds.extend(
        common
            .scenes
            .values()
            .flat_map(|scene| scene.0.actions.values())
            .filter_map(|action| action.prepared().ok())
            .flat_map(battle_martial::Attack::sounds),
    );
    sounds.remove(&Sound::Cue(0));
    sounds
}
