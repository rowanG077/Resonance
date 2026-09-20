use super::*;
use resonance_audio_cook::{bank::Bank, compile, song::Song};
use symphonia_script::{NativeCall, scenario};

#[test]
#[ignore = "requires original extracted US scripts, music and instrument bank; no playback"]
fn original_guardian_entries_keep_their_authored_arena_and_music() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    for (map, formation, first_argument) in [(501, 77, 0x49ec), (500, 78, 0xb1c4)] {
        let archive =
            crate::field::MapArchive::open(&crate::field::source_for_id(&extracted, map).unwrap())
                .unwrap();
        let (_, analysis) = scenario::disassemble(archive.section(6).unwrap()).unwrap();
        let mut instructions = analysis
            .instructions
            .values()
            .filter(|instruction| instruction.offset >= first_argument);
        for value in [formation, 71, 0, 105, 0, 0, 0, 0, 0, 0, 0, 0] {
            for (mnemonic, operands) in
                [("push.s8", vec![value]), ("calc", vec![0]), ("arg", vec![])]
            {
                let instruction = instructions.next().unwrap();
                assert_eq!(instruction.mnemonic, mnemonic);
                assert_eq!(instruction.operands, operands);
            }
        }
        let call = instructions.next().unwrap();
        assert_eq!(call.mnemonic, "proc");
        assert_eq!(call.operands, [NativeCall::StartBattle as i64]);
    }

    let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
    let music = BattleMusic::Guardian;
    assert_eq!(music.id(), 105);
    assert!(BattleMusic::ALL.contains(&music));
    let path = media::music_path(&executable, music.id() as u16).unwrap();
    assert_eq!(path, "S/bgm_b013.song");
    assert_eq!(
        media::song_reverbs(&executable, music.id() as u16).unwrap(),
        media::song_reverbs(&executable, BattleMusic::Sylvarant.id() as u16).unwrap()
    );
    let song = fs::read(extracted.join("files").join(path)).unwrap();
    assert_eq!(
        crate::digest(&song),
        "2119888b1fbfce3959a1960f871c52c3904e6033f93e497c22f862085557a9b1"
    );
    let song = Song::parse(&song).unwrap();
    let [instruments, _] = roles::resident_banks(&extracted, &executable).unwrap();
    let bank = fs::read(extracted.join("files").join(instruments)).unwrap();
    let bank = Bank::parse(&bank).unwrap();
    let setup = bank.music_setup(0, music.id() as u16).unwrap();
    let (resources, score) = compile::music(&bank, &song, &setup).unwrap();
    resources.validate().unwrap();
    score.validate(&resources).unwrap();
    assert!(!resources.samples.is_empty());
    assert!(!score.first_events.is_empty());
    assert!(!score.loop_events.is_empty());
}

#[test]
#[ignore = "requires original battle module, voice banks and cook-all actor records; no playback"]
fn original_opening_voice_branches_and_native_preparation_have_complete_dependencies() {
    use resonance_content::battle::audio::{AudioActor, OpeningLine, ResolvedVoice};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let rel = super::super::actions::Rel::read(&root.join("files/US_r_Top2Btl.rel")).unwrap();
    // Reviewed complete consumers, including the actor-link loop and voice admission.
    for (start, bytes, expected) in [
        (
            0x40c8,
            0x15e0,
            "edb266ca45e120be947fbb4110c3b44e4b2231df25bf0090a23990d351e79f5f",
        ),
        (
            0x35b8c,
            0x38,
            "5008df4ff6b5a255ccda0322572396d8af321c492ee5404bf74abce0f380d1c9",
        ),
        (
            0x35b18,
            0x6c,
            "ebe69dc59c04f46b3bef8277c907c8d2172bfb2ed5a6af55f7c1043dd1c4668c",
        ),
        (
            0x355ec,
            0x52c,
            "938a6d1561e58cf95404520567d4043106c05e8ebaa8683a442f02815a083d8d",
        ),
        (
            0x10a8,
            0x240,
            "c4187088e27f94885cd0d30c909bfc27d3808507d023eff96d0b6b214db28f77",
        ),
        (
            0x71d90,
            0xe8,
            "037c412c2af41e1ba6f1900e0d2e661ea41f6cdb761438a7dfc2b65fabcace8f",
        ),
        (
            0x71674,
            0x544,
            "4d890f57e302e134ef558d4571185d310ea7ef166726964f221518c36d08c46d",
        ),
    ] {
        assert_eq!(
            crate::digest(&rel.at((1, start)).unwrap()[..bytes]),
            expected
        );
    }
    for (selector, callback) in [0x35b18, 0x35b84, 0x35b84, 0x355ec].into_iter().enumerate() {
        assert_eq!(rel.pointer(5, 0xd20 + selector * 4).unwrap(), (1, callback));
    }
    let executable = fs::read(root.join("sys/main.dol")).unwrap();
    let sources = super::super::all::Sources::read(&root).unwrap();
    let resident = roles::resident_banks(&root, &executable).unwrap();
    let banks = selection::banks(&root, &executable, &resident).unwrap();
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let profiles =
        selection::voices(&output, 1, &sources, &banks, &(1..=9).collect(), &[]).unwrap();
    let archive = fs::read(
        root.join("files")
            .join(binding::source(&root, &executable).unwrap()),
    )
    .unwrap();
    let streams = crate::afs::parse(&archive).unwrap();
    for (&character, profile) in &profiles.party {
        for line in OpeningLine::ALL {
            let ResolvedVoice::Cue(id) = profile
                .relative(AudioActor::Party(character), line as u8)
                .unwrap()
            else {
                panic!("initial party voice profile unexpectedly silent");
            };
            if id & 0x8000 != 0 {
                let stream = &streams[usize::from(id & 0x7fff)];
                assert!(stream.name.ends_with(".adx"));
                assert!(!stream.data.is_empty());
            }
            assert!(profiles.cues().any(|prepared| prepared == id));
        }
    }
    assert_eq!(
        profiles.party[&3]
            .relative(AudioActor::Party(3), OpeningLine::Ready as u8)
            .unwrap(),
        ResolvedVoice::Cue(33025)
    );
}

#[test]
fn renamed_bank_aliases_and_cooked_voice_settings_preserve_selection() -> Result<()> {
    use resonance_content::battle::audio::NativeVoices;
    let root = crate::temporary_path(&std::env::temp_dir().join("battle-audio-selection"));
    let result = (|| -> Result<()> {
        let sources = super::super::all::Sources::fixture(&root)?;
        let files = root.join("files");
        let mut bank = vec![0; 93];
        for (at, value) in [
            (0, 4u32),
            (4, 36),
            (8, 36),
            (12, 72),
            (16, 16),
            (20, 88),
            (24, 4),
            (28, 92),
            (32, 1),
            (36, u32::MAX),
            (64, 32),
            (88, u32::MAX),
        ] {
            bank[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        bank[40..44].copy_from_slice(&[0, 1, 0, 1]);
        fs::write(files.join("Common.song"), &bank)?;
        fs::write(files.join("Alias.bin"), &bank)?;
        bank[41] = 2;
        fs::write(files.join("Event.data"), &bank)?;
        bank[41] = 10;
        fs::write(files.join("Party.data"), &bank)?;
        fs::write(files.join("PartyAlias.data"), &bank)?;
        bank[41] = 19;
        fs::write(files.join("Actor.data"), &bank)?;
        fs::write(files.join("ActorAlias.data"), &bank)?;
        let paths = [
            "Alias.bin",
            "Actor.data",
            "ActorAlias.data",
            "Event.data",
            "PartyAlias.data",
        ]
        .map(String::from);
        let party = ["Party.data".to_owned()];
        let mut banks = selection::read_banks(&files, "Common.song", &party, paths.clone())?;
        assert_eq!(
            banks.keys().map(String::as_str).collect::<Vec<_>>(),
            ["Actor.data", "Common.song", "Party.data"]
        );
        bank[92] = 1;
        fs::write(files.join("ActorAlias.data"), bank)?;
        assert!(selection::read_banks(&files, "Common.song", &party, paths).is_err());
        banks.get_mut("Common.song").unwrap().ids.insert(4);
        banks.get_mut("Party.data").unwrap().ids = BTreeSet::from([502, 503, 504]);
        banks.get_mut("Actor.data").unwrap().ids.insert(505);
        assert_eq!(
            selection::sounds(&banks, &BTreeSet::from([4, 503, 505]))?,
            [
                ("Actor.data".into(), vec![505]),
                ("Common.song".into(), vec![4]),
                ("Party.data".into(), vec![503])
            ]
        );
        assert!(selection::sounds(&banks, &BTreeSet::from([5])).is_err());
        let output = root.join("cooked");
        crate::write_atomic(
            &output.join("sources.json"),
            &serde_json::to_vec(&BTreeMap::from([
                (format!("disc1/{}", sources.usual), vec!["assets/shared"]),
                (format!("disc1/{}", sources.enemy), vec!["assets/enemy"]),
            ]))?,
        )?;
        let actor = json!({"effects":{"voice_base":1,"death_voice":0x8002},"combat":{"flags":16}});
        crate::embedded::write(
            &files.join("US_r_Top2Btl.rel"),
            &output.join("data"),
            "battle-party-settings",
            &json!([{"character":1,"settings":actor}]),
            json!({}),
        )?;
        crate::write_atomic(
            &output.join("assets/shared/battle/all/usual/11.json"),
            &serde_json::to_vec(&json!({"voice_count":8,"streamed_voices":[1,3]}))?,
        )?;
        let enemy = output.join("assets/enemy/battle/all/enemy-7/header-4.json");
        crate::write_atomic(&enemy, &serde_json::to_vec(&actor)?)?;
        let profiles = selection::voices(&output, 1, &sources, &banks, &BTreeSet::from([1]), &[7])?;
        for profile in [&profiles.party[&1], &profiles.enemies[&7]] {
            assert_eq!(
                profile.voices,
                NativeVoices::Voiced {
                    base: 1,
                    ids: vec![0x8001, 2, 0x8003]
                }
            );
            assert_eq!(profile.death_override, 0x8002);
            assert!(profile.low_hp);
        }
        fs::remove_file(enemy)?;
        assert!(
            selection::voices(&output, 1, &sources, &banks, &BTreeSet::from([1]), &[7]).is_err()
        );
        banks.get_mut("Common.song").unwrap().ids.insert(503);
        assert!(selection::sounds(&banks, &BTreeSet::from([503])).is_err());
        Ok(())
    })();
    let cleanup = fs::remove_dir_all(root);
    result?;
    cleanup?;
    Ok(())
}

#[test]
#[ignore = "requires both original discs and cook-all records; no conversion or playback"]
fn original_cooked_voice_profiles_match_every_actor_and_native_bank_owner() -> Result<()> {
    use crate::read::{u16 as half, u32 as word};
    use resonance_content::battle::audio::NativeVoices;
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    for disc in [1, 2] {
        let extracted = local.join(format!("extracted/disc{disc}"));
        let files = extracted.join("files");
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let sources = super::super::all::Sources::read(&extracted)?;
        let resident = roles::resident_banks(&extracted, &executable)?;
        let banks = selection::banks(&extracted, &executable, &resident)?;
        let mut original_paths = BTreeSet::from([resident[1].clone()]);
        for entry in fs::read_dir(files.join("S"))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("v_") && name.ends_with(".snd") {
                original_paths.insert(format!("S/{name}"));
            }
        }
        let identities = |paths: Vec<&String>| -> Result<BTreeMap<u16, String>> {
            paths
                .into_iter()
                .map(|path| {
                    let bytes = fs::read(files.join(path))?;
                    Ok((
                        resonance_audio_cook::bank::Bank::parse(&bytes)?.group()?,
                        crate::digest(&bytes),
                    ))
                })
                .collect()
        };
        assert_eq!(
            identities(banks.keys().collect())?,
            identities(original_paths.iter().collect())?
        );
        for path in roles::party_banks(&extracted, &executable)? {
            assert!(banks.contains_key(&path));
        }
        let profiles = selection::voices(
            &local.join("all-assets"),
            disc,
            &sources,
            &banks,
            &(1..=9).collect(),
            &(0..251).collect::<Vec<_>>(),
        )?;
        let usual = fs::read(files.join(&sources.usual))?;
        let bitmap = super::super::actions::member(&usual, 11)?;
        let rel = super::super::actions::Rel::read(&files.join("US_r_Top2Btl.rel"))?;
        let archive = fs::read(files.join(&sources.enemy))?;
        let directory = super::super::actions::member(&usual, 10)?;
        let check =
            |row: &[u8], profile: &resonance_content::battle::audio::VoiceProfile| -> Result<()> {
                let base = word(row, 0x104)? as u16;
                let expected = if base == 0 {
                    NativeVoices::Voiceless
                } else {
                    let bank = banks
                        .values()
                        .find(|bank| bank.ids.contains(&(base + 501)))
                        .unwrap();
                    let end = bank.ids.last().unwrap() + 1 - 501;
                    NativeVoices::Voiced {
                        base,
                        ids: (base..end)
                            .map(|id| {
                                id | if bitmap[usize::from(id / 8)] & (1 << (id % 8)) != 0 {
                                    0x8000
                                } else {
                                    0
                                }
                            })
                            .collect(),
                    }
                };
                assert_eq!(profile.voices, expected);
                assert_eq!(profile.death_override, half(row, 0xf4)?);
                assert_eq!(profile.low_hp, word(row, 0x5c)? & 16 != 0);
                Ok(())
            };
        for (&id, profile) in &profiles.party {
            check(
                rel.at((5, 0x3d30 + (usize::from(id) - 1) * 0x1f0))?,
                profile,
            )?;
        }
        for (&id, profile) in &profiles.enemies {
            let at = usize::from(id) * 4;
            let bytes = crate::compression::decode(
                &archive[word(directory, at)? as usize..word(directory, at + 4)? as usize],
            )?;
            check(&bytes[usize::from(half(&bytes, 4)?)..], profile)?;
        }
    }
    Ok(())
}
