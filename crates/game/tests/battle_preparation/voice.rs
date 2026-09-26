use super::*;
use battle::{
    model::ModelSource,
    voice::{self, Phase, Sound},
};
use resonance_battle::{SoundBinding, VoiceLine};
use resonance_content::{battle_model, battle_profile, battle_voice};

fn resolve(sound: Sound) -> Result<SoundBinding> {
    Ok(match sound {
        Sound::Cue(index) => SoundBinding { resource: 1, index },
        Sound::Stream(index) => SoundBinding { resource: 2, index },
    })
}

fn line(sound: Sound, duration: u16) -> Result<VoiceLine> {
    Ok(VoiceLine {
        sound: resolve(sound)?,
        duration,
    })
}

fn snapshot() -> Result<Files> {
    let mut files = Files::default();
    let mut records = vec![profile::profile(); 11];
    records[1].voice_base = 0;
    let mut voice_sequences = vec![vec![]; 10];
    voice_sequences[0] = vec![battle_profile::VoiceSequence {
        technique: 237,
        chant: 0x81ca,
        release: 0x81a7,
    }];
    files.bytes.insert(
        battle_profile::PARTY_PATH.into(),
        serde_json::to_vec(&battle_profile::Table {
            default_strategy: [[0; 3]; 10],
            companion_policy: Default::default(),
            placement: Default::default(),
            entry: Default::default(),
            source_sha256: "a".repeat(64),
            chant: vec![],
            voice_sequences,
            death_voice_pairs: vec![],
            contact_sounds: Default::default(),
            records,
        })?
        .into(),
    );
    files.bytes.insert(
        battle_voice::PATH.into(),
        serde_json::to_vec(&battle_voice::Table {
            source_sha256: "b".repeat(64),
            streams: (0..64)
                .map(|index| if index == 1 { 0b10 } else { 0 })
                .collect(),
            durations: (0..512).collect(),
        })?
        .into(),
    );
    let enemy = battle_model::Enemy {
        name: "enemy".into(),
        hidden_name_units: 2,
        actions: Default::default(),
        target_strategy: 0,
        guard_preference: 0,
        attachments: Default::default(),
        trails: Default::default(),
        source_sha256: "c".repeat(64),
        profile: profile::profile(),
        body: battle_model::Rig {
            skeleton: resonance_content::animation::Skeleton { bones: vec![] },
            transform_kinds: vec![],
            target_bones: vec![],
            volumes: vec![],
            attack_groups: Default::default(),
            attachments: Default::default(),
        },
        files: Default::default(),
    };
    files.bytes.insert(
        battle_model::enemy_path(36),
        serde_json::to_vec(&enemy)?.into(),
    );
    Ok(files)
}

#[test]
fn absolute_lines_preserve_stream_flags_and_only_gate_on_profile_eligibility() -> Result<()> {
    let files = snapshot()?;
    let actors = [
        ModelSource::Party(1),
        ModelSource::Party(2),
        ModelSource::Enemy(36),
    ];
    for (value, expected) in [
        (0, None),
        // The storage bit for line 9 is set. Absolute requests do not consult it.
        (9, Some(line(Sound::Cue(510), 9)?)),
        (0x8009, Some(line(Sound::Stream(9), 9)?)),
        // The enemy >=10 restriction belongs to relative requests only.
        (42, Some(line(Sound::Cue(543), 42)?)),
    ] {
        assert_eq!(
            voice::absolute(&files, &actors, value, resolve)?,
            vec![expected, None, expected]
        );
    }
    Ok(())
}

#[test]
fn relative_lines_select_original_storage_and_keep_empty_actor_slots() -> Result<()> {
    let files = snapshot()?;
    let actors = [
        ModelSource::Party(1),
        ModelSource::Party(2),
        ModelSource::Enemy(36),
    ];
    assert_eq!(
        voice::relative(&files, &actors, 8, resolve)?,
        vec![
            Some(line(Sound::Stream(9), 9)?),
            None,
            Some(line(Sound::Stream(9), 9)?),
        ]
    );
    assert_eq!(
        voice::relative(&files, &actors, 42, resolve)?,
        vec![Some(line(Sound::Cue(544), 43)?), None, None,]
    );
    // The original addition truncates to a halfword before table lookup.
    assert_eq!(
        voice::relative(&files, &actors[..1], u16::MAX, resolve)?,
        vec![None]
    );
    Ok(())
}

#[test]
fn technique_lines_use_last_match_and_original_chant_fallback() -> Result<()> {
    let mut files = snapshot()?;
    let actors = [
        ModelSource::Party(1),
        ModelSource::Party(2),
        ModelSource::Enemy(36),
    ];
    assert_eq!(
        voice::technique(&files, &actors, 237, Phase::Release, resolve)?,
        vec![Some(line(Sound::Stream(423), 423)?), None, None,]
    );
    assert_eq!(
        voice::technique(&files, &actors, 237, Phase::Chant, resolve)?[0],
        Some(line(Sound::Stream(458), 458)?)
    );
    assert_eq!(
        voice::technique(&files, &actors, 237, Phase::SelfChant, resolve)?[0],
        Some(line(Sound::Stream(8), 8)?)
    );
    assert_eq!(
        voice::technique(&files, &actors, 999, Phase::Chant, resolve)?[0],
        Some(line(Sound::Stream(8), 8)?)
    );
    assert_eq!(
        voice::technique(&files, &actors, 999, Phase::Release, resolve)?,
        vec![None; 3]
    );
    let mut table: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
    table.voice_sequences[0].push(battle_profile::VoiceSequence {
        technique: 237,
        chant: 0,
        release: 42,
    });
    files.bytes.insert(
        battle_profile::PARTY_PATH.into(),
        serde_json::to_vec(&table)?.into(),
    );
    assert_eq!(
        voice::technique(&files, &actors, 237, Phase::Release, resolve)?[0],
        Some(line(Sound::Cue(543), 42)?)
    );
    assert_eq!(
        voice::technique(&files, &actors, 237, Phase::Chant, resolve)?[0],
        Some(line(Sound::Stream(8), 8)?)
    );
    Ok(())
}

#[test]
fn idle_casting_fallback_uses_absolute_lines_and_colettes_two_overrides() -> Result<()> {
    let mut files = snapshot()?;
    let mut profiles: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
    profiles.records[1].voice_base = 121;
    profiles.voice_sequences[1].push(battle_profile::VoiceSequence {
        technique: 237,
        chant: 0x81ca,
        release: 0,
    });
    files.bytes.insert(
        battle_profile::PARTY_PATH.into(),
        serde_json::to_vec(&profiles)?.into(),
    );
    let mut voices: battle_voice::Table = files.json(battle_voice::PATH)?;
    voices.streams[1] |= 1; // Relative line 7 would stream; this absolute fallback does not.
    files.bytes.insert(
        battle_voice::PATH.into(),
        serde_json::to_vec(&voices)?.into(),
    );
    let actors = [ModelSource::Party(1), ModelSource::Party(2)];
    assert_eq!(
        voice::technique(&files, &actors, 237, Phase::Fallback, resolve)?,
        vec![
            Some(line(Sound::Cue(509), 8)?),
            Some(line(Sound::Cue(629), 128)?),
        ]
    );
    assert_eq!(
        voice::technique(&files, &actors, 237, Phase::SelfChant, resolve)?[1],
        Some(line(Sound::Stream(458), 458)?)
    );
    for (technique, expected) in [(268, 232), (269, 231)] {
        assert_eq!(
            voice::technique(&files, &actors, technique, Phase::Fallback, resolve)?[1],
            Some(line(Sound::Stream(expected), expected)?)
        );
    }
    Ok(())
}

#[test]
fn invalid_voice_dependencies_fail_preparation() -> Result<()> {
    let files = snapshot()?;
    for source in [
        ModelSource::Party(0),
        ModelSource::Party(12),
        ModelSource::Weapon(1),
        ModelSource::Scene(237),
    ] {
        assert!(voice::absolute(&files, &[source], 42, resolve).is_err());
        assert!(voice::relative(&files, &[source], 42, resolve).is_err());
        assert!(voice::technique(&files, &[source], 237, Phase::Release, resolve).is_err());
    }
    assert!(voice::absolute(&files, &[ModelSource::Party(1)], 512, resolve).is_err());
    assert!(
        voice::absolute(&files, &[ModelSource::Party(1)], 1, |_| bail!(
            "voice resource missing"
        ))
        .is_err()
    );
    assert!(voice::absolute(&Files::default(), &[ModelSource::Party(1)], 1, resolve).is_err());
    assert!(voice::relative(&files, &[ModelSource::Party(1)], 512, resolve).is_err());
    assert!(
        voice::relative(&files, &[ModelSource::Party(1)], 42, |_| bail!(
            "voice resource missing"
        ))
        .is_err()
    );
    assert!(voice::relative(&Files::default(), &[ModelSource::Party(1)], 42, resolve).is_err());
    let mut truncated = files.clone();
    let mut voices: battle_voice::Table = files.json(battle_voice::PATH)?;
    voices.durations.truncate(43);
    truncated.bytes.insert(
        battle_voice::PATH.into(),
        serde_json::to_vec(&voices)?.into(),
    );
    assert!(voice::relative(&truncated, &[ModelSource::Party(1)], 42, resolve).is_err());
    assert!(
        voice::technique(
            &truncated,
            &[ModelSource::Party(1)],
            237,
            Phase::Release,
            resolve
        )
        .is_err()
    );
    let mut malformed = files;
    malformed
        .bytes
        .insert(battle_voice::PATH.into(), Arc::from(b"{}".as_slice()));
    assert!(voice::relative(&malformed, &[ModelSource::Party(1)], 42, resolve).is_err());
    Ok(())
}

#[test]
fn missing_or_changed_voice_files_fail_even_with_live_cached_bytes() -> Result<()> {
    use resonance_content::{field_preload, prepared};
    use sha2::{Digest, Sha256};
    let root = std::env::temp_dir().join(format!(
        "resonance-battle-voices-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("battle"))?;
    let mut inventory = std::collections::BTreeMap::new();
    for (path, bytes) in snapshot()?.bytes {
        if path == battle_voice::PATH || path == battle_profile::PARTY_PATH {
            std::fs::write(root.join(&path), &bytes)?;
            inventory.insert(
                path,
                field_preload::File {
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                    bytes: bytes.len() as u64,
                    roles: [field_preload::Role::Data].into(),
                },
            );
        }
    }
    let mut cache = prepared::Cache::default();
    let active =
        Files::default().with_dependencies(&root, inventory.clone(), &mut cache, || false)?;
    let actors = [ModelSource::Party(1)];
    let before = voice::relative(&active, &actors, 42, resolve)?;
    let path = root.join(battle_voice::PATH);
    let original = std::fs::read(&path)?;
    let mut changed = original.clone();
    let last = changed.iter().rposition(|&b| b == b'0').unwrap();
    changed[last] = b'1';
    std::fs::write(&path, changed)?;
    assert!(
        Files::default()
            .with_dependencies(&root, inventory.clone(), &mut cache, || false)
            .is_err()
    );
    std::fs::remove_file(&path)?;
    assert!(
        Files::default()
            .with_dependencies(&root, inventory, &mut cache, || false)
            .is_err()
    );
    assert_eq!(voice::relative(&active, &actors, 42, resolve)?, before);
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
#[ignore = "requires the complete current cooked library; no devices"]
fn cold_voice_bindings_match_original_nurse_and_lightning_requests() -> Result<()> {
    let files = Files::load(
        &common::asset_root(),
        &["fields/map-340.preload.json"],
        &mut resonance_content::prepared::Cache::default(),
        || false,
    )?;
    let voices: battle_voice::Table = files.json(battle_voice::PATH)?;
    let formations: resonance_content::battle_formation::Formations =
        files.json(resonance_content::battle_formation::PATH)?;
    assert_eq!(voices.source_sha256, formations.source_sha256);
    let actors = [
        ModelSource::Party(1),
        ModelSource::Party(3),
        ModelSource::Party(4),
        ModelSource::Enemy(36),
    ];
    assert_eq!(
        voice::relative(&files, &actors, 42, resolve)?,
        vec![
            Some(line(Sound::Cue(544), 25)?),
            Some(line(Sound::Cue(784), 38)?),
            Some(line(Sound::Cue(905), 41)?),
            None,
        ]
    );
    assert_eq!(
        voice::technique(&files, &actors, 237, Phase::Release, resolve)?,
        vec![None, None, Some(line(Sound::Stream(423), 37)?), None,]
    );
    assert_eq!(
        voice::technique(&files, &actors[2..3], 237, Phase::Chant, resolve)?,
        vec![Some(line(Sound::Stream(458), 62)?)]
    );
    assert_eq!(
        voice::technique(&files, &actors[2..3], 237, Phase::SelfChant, resolve)?,
        vec![Some(line(Sound::Stream(369), 41)?)]
    );
    assert_eq!(
        voice::technique(&files, &actors[2..3], 237, Phase::Fallback, resolve)?,
        vec![Some(line(Sound::Cue(870), 41)?)]
    );
    assert_eq!(
        voice::technique(&files, &actors[1..2], 216, Phase::Chant, resolve)?,
        vec![Some(line(Sound::Stream(248), 37)?)]
    );
    assert_eq!(
        voice::technique(&files, &actors[1..2], 216, Phase::Release, resolve)?,
        vec![Some(line(Sound::Stream(312), 45)?)]
    );
    Ok(())
}

#[test]
fn contact_inventory_uses_profile_tables_before_audio_packages_exist() -> Result<()> {
    let mut files = snapshot()?;
    let mut profiles: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
    profiles.contact_sounds = battle_profile::ContactSounds {
        party: [46, 46, 54, 54, 54, 46, 46, 54, 46],
        elements: [54, 55, 56, 54, 58, 59, 54, 54, 54],
    };
    profiles.records[0].death_voice = 117;
    files.bytes.insert(
        battle_profile::PARTY_PATH.into(),
        serde_json::to_vec(&profiles)?.into(),
    );
    let mut enemy: battle_model::Enemy = files.json(&battle_model::enemy_path(36))?;
    enemy.profile.voice_base = 0;
    files.bytes.insert(
        battle_model::enemy_path(36),
        serde_json::to_vec(&enemy)?.into(),
    );
    assert!(
        !files
            .bytes
            .contains_key(resonance_content::battle_audio::PATH)
    );
    let mut requested = Vec::new();
    let audio = battle::contact_audio::prepare(
        &files,
        &[
            ModelSource::Party(1),
            ModelSource::Party(2),
            ModelSource::Enemy(36),
        ],
        |sound| {
            requested.push(sound);
            resolve(sound)
        },
    )?;
    assert_eq!(audio.actors[0].neutral.index, 46);
    assert_eq!(
        audio.actors[0].voices.alternate_defeat,
        Some(resolve(Sound::Cue(618))?)
    );
    assert_eq!(audio.actors[1].voices.hurt, [None; 2]);
    assert!(audio.actors[1].voices.stunned_override);
    assert_eq!(audio.actors[2].voices.hurt, [None; 2]);
    for cue in [46, 54, 55, 56, 58, 59, 65, 66, 618] {
        assert!(requested.contains(&Sound::Cue(cue)));
    }
    Ok(())
}
