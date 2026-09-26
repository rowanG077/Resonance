use super::*;
use resonance_battle::{ActionPhase, ModelDefinition, Playback};
use resonance_content::animation::{Motion, Skeleton};

#[test]
fn death_adapter_preserves_optional_source_clips_and_last_victim_flag() {
    let mut profile = profile::profile();
    let mut model = ModelDefinition {
        resource: 7,
        skeleton: Skeleton { bones: vec![] },
        motions: [3, 7, 9]
            .map(|clip| {
                (
                    clip,
                    Motion {
                        duration_frames: 3.,
                        tracks: vec![],
                    },
                )
            })
            .into(),
        secondary_motion: vec![],
        initial: Playback {
            clip: 3,
            frame: 0.,
            rate: 0.,
            repeat: false,
        },
        hurt_motions: [None; 2],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        stun: None,
        knockdown: None,
        anchors: vec![],
        weapons: vec![],
        hurt_bones: vec![],
        approach_bones: vec![],
        target_bones: vec![],
        shadow: None,
        target_marker: None,
        suppress_root_translation: [false; 3],
    };
    profile.flags = 0;
    profile.death_motion = 0;
    let (binding, motions) = battle::death::prepare(&profile, &model, 40, 41);
    assert!(!binding.wait_for_motion);
    assert!(!binding.integrate);
    assert!(binding.darken_immediately);
    assert_eq!(motions[0].unwrap().clip, 3);
    assert!(motions[1].is_none());
    profile.flags = 0x0024_0000;
    let (binding, motions) = battle::death::prepare(&profile, &model, 40, 41);
    assert!(binding.wait_for_motion && binding.integrate);
    assert!(!binding.darken_immediately);
    assert_eq!(motions[1].unwrap().clip, 7);
    profile.death_motion = 9;
    let (_, motions) = battle::death::prepare(&profile, &model, 40, 41);
    assert_eq!(motions[1].unwrap().clip, 9);
    model.motions.remove(&9);
    assert!(battle::death::prepare(&profile, &model, 40, 41).1[1].is_none());
    profile.flags |= 0x0800_0000;
    assert!(
        battle::death::prepare(&profile, &model, 40, 41)
            .1
            .iter()
            .all(Option::is_none)
    );
    assert!(
        battle::death::bindings(40, 41)
            .iter()
            .all(|a| a.phase == ActionPhase::Controller && a.tp_cost == 0)
    );
}

#[test]
fn death_feedback_binds_real_voice_modes_and_source_relationship_filters() -> Result<()> {
    use battle::{model::ModelSource, voice::Sound};
    use resonance_content::{battle_profile, battle_voice};
    let mut files = Files::default();
    let mut records = vec![profile::profile(); 11];
    for (index, record) in records.iter_mut().enumerate() {
        record.voice_base = (index as u32 + 1) * 20;
        record.overlimit_gain = [15, 11, 12][index % 3];
    }
    let table = battle_profile::Table {
        default_strategy: [[0; 3]; 10],
        companion_policy: Default::default(),
        source_sha256: "a".repeat(64),
        placement: Default::default(),
        entry: Default::default(),
        chant: vec![],
        voice_sequences: vec![vec![]; 10],
        records,
        contact_sounds: Default::default(),
        death_voice_pairs: vec![
            [1, 2],
            [1, 3],
            [2, 3],
            [4, 3],
            [3, 4],
            [7, 3],
            [5, 6],
            [7, 6],
            [2, 6],
            [4, 6],
            [7, 8],
            [1, 9],
            [6, 5],
            [4, 5],
        ],
    };
    files.bytes.insert(
        battle_profile::PARTY_PATH.into(),
        serde_json::to_vec(&table)?.into(),
    );
    let mut streams = vec![0; 40];
    streams[50 / 8] |= (1 << (50 % 8)) | (1 << (51 % 8));
    files.bytes.insert(
        battle_voice::PATH.into(),
        serde_json::to_vec(&battle_voice::Table {
            source_sha256: "b".repeat(64),
            streams,
            durations: (1000..1320).collect(),
        })?
        .into(),
    );
    let members: Vec<_> = (0..9).map(|_| serde_json::json!({
        "affinity": 0, "level": 1, "experience": 0, "base_stats": [100, 20, 30, 40, 50, 60, 70],
        "hp": 100, "tp": 20, "conditions": 0, "luck": 50, "overlimit": 0,
        "equipment": [0, 0, 0, 0, 0, 0], "techniques": [], "shortcuts": [0, 0, 0, 0],
    })).collect();
    let mut party: resonance_events::party::Party = serde_json::from_value(serde_json::json!({
        "members": members, "formation": [1,2,3], "items": {}, "found_items": [], "recent_items": [],
        "gald": 0, "spent_gald": 0, "settings": resonance_events::party::Settings::default(),
    }))?;
    let actors = [
        ModelSource::Party(1),
        ModelSource::Party(2),
        ModelSource::Party(3),
        ModelSource::Enemy(36),
    ];
    let resolve = |sound| {
        Ok(match sound {
            Sound::Cue(index) => resonance_battle::SoundBinding { resource: 7, index },
            Sound::Stream(index) => resonance_battle::SoundBinding { resource: 8, index },
        })
    };
    let feedback = battle::death::feedback(&files, &actors, &party, 3, false, resolve)?;
    let enemy = feedback.enemy.unwrap();
    assert_eq!(
        (enemy.appearance.resource, enemy.appearance.member),
        (3, 14)
    );
    assert_eq!(enemy.sound, resolve(Sound::Cue(73))?);
    let colette = feedback.allies[0][1].unwrap();
    assert_eq!((colette.priority, colette.overlimit_gain), (3, 110));
    for variant in 0..2 {
        let voice = colette.voices[variant].unwrap();
        assert_eq!(voice.sound, resolve(Sound::Stream(50 + variant as u16))?);
        assert_eq!(voice.duration, 1050 + variant as u16);
    }
    assert!(feedback.allies[0][2].is_none()); // Lloyd branch bypasses the fixed [1,3] pair.
    assert_eq!(feedback.allies[1][0].unwrap().priority, 1);
    assert_eq!(
        feedback.allies[1][2].unwrap().voices[0].unwrap().sound,
        resolve(Sound::Cue(571))?
    );
    assert!(feedback.allies[2].iter().all(Option::is_none));
    assert!(feedback.allies[3].iter().all(Option::is_none));
    let boosted = battle::death::feedback(&files, &actors, &party, 3, true, resolve)?;
    assert_eq!(boosted.allies[0][1].unwrap().overlimit_gain, 165);
    party.members[3].affinity = 100; // Absent Raine outranks every combat participant.
    let absent = battle::death::feedback(&files, &actors, &party, 3, false, resolve)?;
    assert!(absent.allies[0].iter().all(Option::is_none));
    assert!(absent.allies[1][0].is_none());
    assert!(absent.allies[1][2].is_some()); // Fixed pairs are independent of Lloyd's affinity rank.
    files.bytes.remove(battle_voice::PATH);
    assert!(battle::death::feedback(&files, &actors, &party, 3, false, resolve).is_err());
    Ok(())
}
