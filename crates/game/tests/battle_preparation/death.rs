use super::*;

#[test]
fn death_feedback_uses_affinity_and_configured_pairs() -> Result<()> {
    use battle::{model::ModelSource, voice::Sound};
    use resonance_content::{battle_profile, battle_voice};
    let files = Files::default();
    let mut records = vec![profile::profile(); 3];
    for (index, record) in records.iter_mut().enumerate() {
        record.voices = Some(battle_voice::Voices {
            ally_defeated: match index {
                0 => [Some(Sound::Cue(531)), Some(Sound::Cue(532))],
                1 => [Some(Sound::Stream(50)), Some(Sound::Stream(51))],
                _ => [Some(Sound::Cue(571)), Some(Sound::Cue(572))],
            },
            ..Default::default()
        });
        record.overlimit_gain = [15, 11, 12][index % 3];
    }
    let table = battle_profile::Table {
        lethal_rescue_names: ["Angel Tear", "Revive", "Resurrect", "Ring", "Doll"]
            .map(str::to_owned),
        overlimit_voices: [None; 9],
        default_strategy: [[0; 3]; 10],
        companion_policy: Default::default(),
        source_sha256: "a".repeat(64),
        entry: Default::default(),
        voice_sequences: vec![Default::default(); 3],
        records,
        contact_sounds: Default::default(),
        death_voice_pairs: vec![[1, 2], [1, 3], [2, 3]],
    };
    let members: Vec<_> = (0..9).map(|_| serde_json::json!({
        "affinity": 0, "level": 1, "experience": 0, "base_stats": [100, 20, 30, 40, 50, 60, 70],
        "hp": 100, "tp": 20, "luck": 50, "overlimit": 0,
        "ailments": resonance_events::party::Ailments::default(), "queued_buffs": [],
        "equipment": [0, 0, 0, 0, 0, 0], "techniques": [], "shortcuts": [0, 0, 0, 0],

    })).collect();
    let mut party: resonance_events::party::Party = serde_json::from_value(serde_json::json!({
        "members": members, "formation": [1,2,3], "items": {}, "found_items": [], "recent_items": [],
        "battles": resonance_events::party::BattleStatistics::default(),
        "gald": 0, "spent_gald": 0, "settings": resonance_events::party::Settings::default(),
    }))?;
    let actors = [
        ModelSource::Party(1),
        ModelSource::Party(2),
        ModelSource::Party(3),
        ModelSource::Enemy(36),
    ];
    let resolve = |sound| Ok(Some(sound));
    let enemy_profile = profile::profile();
    let resolver = battle::voice::Resolver::new(&files, &table, [(36, &enemy_profile)]);
    let feedback = battle::death::feedback(&resolver, &actors, &party, 3, resolve)?;
    assert_eq!(
        (feedback.appearance.resource, feedback.appearance.member),
        (3, 14)
    );
    assert_eq!(feedback.sound, resolve(Sound::Cue(73))?);
    let selected: Vec<_> = feedback
        .allies
        .iter()
        .map(|r| (r.victim.index(), r.recipient.index(), r.voices))
        .collect();
    assert_eq!(
        selected,
        vec![
            (0, 1, [Some(Sound::Stream(50)), Some(Sound::Stream(51))]),
            (1, 0, [Some(Sound::Cue(531)), Some(Sound::Cue(532))]),
            (1, 2, [Some(Sound::Cue(571)), Some(Sound::Cue(572))]),
        ]
    );
    party.members[3].affinity = 100; // Absent Raine outranks every combat participant.
    let absent = battle::death::feedback(&resolver, &actors, &party, 3, resolve)?;
    assert_eq!(
        absent
            .allies
            .iter()
            .map(|r| (r.victim.index(), r.recipient.index()))
            .collect::<Vec<_>>(),
        [(1, 2)]
    );
    Ok(())
}
