use super::*;
use battle::{
    model::ModelSource,
    voice::{Phase, Resolver, Sound},
};
use resonance_content::{battle_profile, battle_voice, diagnostics::Diagnostics};

fn resolve(sound: Sound) -> Result<Option<Sound>> {
    Ok(Some(sound))
}

fn snapshot(paranoid: bool) -> (Files, battle_profile::Table) {
    let files = Files::new(Diagnostics::new(paranoid));
    let mut records = vec![profile::profile(); 2];
    records[0].voices = Some(battle_voice::Voices {
        critical: Some(Sound::Stream(1)),
        ..Default::default()
    });
    let mut voice_sequences = vec![battle_profile::VoicePolicy::default(); 2];
    voice_sequences[0].default = battle_profile::VoiceSequence {
        chant: Some(Sound::Stream(1)),
        self_chant: Some(Sound::Stream(1)),
        fallback: Some(Sound::Cue(509)),
        release: None,
    };
    let default = voice_sequences[0].default;
    voice_sequences[0].techniques.insert(
        42,
        battle_profile::VoiceSequence {
            release: Some(Sound::Stream(2)),
            ..default
        },
    );
    (
        files,
        battle_profile::Table {
            lethal_rescue_names: Default::default(),
            overlimit_voices: [None; 9],
            default_strategy: [[0; 3]; 10],
            companion_policy: Default::default(),
            entry: Default::default(),
            source_sha256: "a".repeat(64),
            voice_sequences,
            death_voice_pairs: vec![],
            contact_sounds: Default::default(),
            records,
        },
    )
}

#[test]
fn prepared_lines_use_supplied_profiles_and_spell_overrides_without_reloading() -> Result<()> {
    let (files, profiles) = snapshot(true);
    let resolver = Resolver::new(&files, &profiles, [(36, &profiles.records[0])]);
    for source in [ModelSource::Party(1), ModelSource::Enemy(36)] {
        assert_eq!(
            resolver.absolute(source, Some(Sound::Cue(510)), resolve)?,
            resolve(Sound::Cue(510))?
        );
        assert_eq!(
            resolver.select(source, |v| v.critical, resolve)?,
            Some(Sound::Stream(1))
        );
        assert_eq!(resolver.select(source, |v| v.guard, resolve)?, None);
    }
    let speaker = ModelSource::Party(1);
    assert_eq!(resolver.absolute(speaker, None, resolve)?, None);
    assert_eq!(
        resolver.absolute(ModelSource::Party(2), Some(Sound::Cue(510)), resolve)?,
        None
    );
    assert_eq!(
        resolver.technique(speaker, 42, Phase::Release, resolve)?,
        Some(Sound::Stream(2))
    );
    for phase in [Phase::Chant, Phase::SelfChant] {
        assert_eq!(
            resolver.technique(speaker, 42, phase, resolve)?,
            Some(Sound::Stream(1))
        );
    }
    assert_eq!(
        resolver.technique(speaker, 42, Phase::Fallback, resolve)?,
        Some(Sound::Cue(509))
    );
    assert_eq!(
        resolver.technique(speaker, 99, Phase::Release, resolve)?,
        None
    );
    for source in [ModelSource::Party(2), ModelSource::Enemy(36)] {
        assert_eq!(
            resolver.technique(source, 42, Phase::Release, resolve)?,
            None
        );
    }
    for source in [ModelSource::Party(0), ModelSource::Weapon(1)] {
        assert!(
            resolver
                .absolute(source, Some(Sound::Cue(510)), |_| {
                    panic!("invalid actors must fail before sound loading")
                })
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn selected_voice_failures_follow_diagnostics_and_leave_other_lines_available() -> Result<()> {
    for paranoid in [false, true] {
        for fault in ["spell", "sound"] {
            let (files, mut profiles) = snapshot(paranoid);
            profiles.records[1] = profiles.records[0].clone();
            profiles.records[1].voices.as_mut().unwrap().critical = Some(Sound::Stream(2));
            if fault == "spell" {
                profiles.voice_sequences.truncate(1);
            }
            let resolver = Resolver::new(&files, &profiles, []);
            let mut sound = |request| {
                if fault == "sound" && request == Sound::Stream(2) {
                    return files
                        .diagnostics()
                        .attempt("test audio", Err(anyhow::anyhow!("missing selected sound")));
                }
                resolve(request)
            };
            let failed = if fault == "spell" {
                resolver.technique(ModelSource::Party(2), 42, Phase::Release, &mut sound)
            } else {
                resolver.select(ModelSource::Party(2), |v| v.critical, &mut sound)
            };
            if paranoid {
                assert!(failed.is_err());
            } else {
                assert!(failed?.is_none());
            }
            assert_eq!(files.diagnostics().entries().len(), 1);
            assert_eq!(
                resolver.absolute(ModelSource::Party(1), Some(Sound::Cue(510)), resolve)?,
                resolve(Sound::Cue(510))?
            );
        }
    }
    Ok(())
}

#[test]
fn contact_inventory_prepares_without_audio_packages() -> Result<()> {
    let (files, mut profiles) = snapshot(true);
    profiles.contact_sounds = battle_profile::ContactSounds {
        party: [46, 46, 54, 54, 54, 46, 46, 54, 46],
        elements: [54, 55, 56, 54, 58, 59, 54, 54, 54],
    };
    profiles.records[0].voices.as_mut().unwrap().defeat = Some(Sound::Cue(618));
    let enemy = profile::profile();
    let resolver = Resolver::new(&files, &profiles, [(36, &enemy)]);
    let mut requested = Vec::new();
    let audio = battle::contact_audio::prepare(
        &resolver,
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
    assert!(!files.contains_key(resonance_content::battle_audio::PATH));
    assert_eq!(audio.actors[0].neutral, Some(Sound::Cue(46)));
    assert_eq!(audio.actors[0].voices.defeat, Some(Sound::Cue(618)));
    assert_eq!(audio.actors[1].voices.hurt, [None; 2]);
    assert_eq!(audio.actors[1].voices.stunned, None);
    assert_eq!(audio.actors[2].voices.hurt, [None; 2]);
    for cue in [46, 54, 55, 56, 58, 59, 65, 66, 618] {
        assert!(requested.contains(&Sound::Cue(cue)));
    }
    Ok(())
}
