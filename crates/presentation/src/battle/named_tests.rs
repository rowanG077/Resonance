use anyhow::{Context as ContextExt, Result, ensure};
use resonance_content::{
    battle_stage, diagnostics::Diagnostics, library::Library, prepared::Files,
};
use resonance_game::battle::{effect_program, stage};
use std::{collections::BTreeSet, fs, sync::Arc};
#[path = "named_tests/encounter.rs"]
mod encounter;

fn named_library() -> Result<Library> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS to a current named library")?;
    Library::open(&root, &[])
}

#[test]
#[ignore = "requires a complete named library at RESONANCE_TEST_ASSETS; compares all formation bindings and shared battle rule consumers"]
fn named_shared_battle_rules_bind_original_encounters_and_controls() -> Result<()> {
    use resonance_content::{battle_rules, battle_ui, menu_data::MenuData};
    use resonance_game::battle::{encounter, recoil};
    let library = named_library()?;
    let mut snapshot = library.snapshot();
    let rules = battle_rules::capture(&mut snapshot, &|| false)?;
    ensure!(
        rules.formations.records.len() == 1000,
        "incomplete formation fixture"
    );
    battle_ui::document::capture(&mut snapshot, &Diagnostics::new(true), &|| false)?;
    resonance_content::preparation::menu_resources(
        &mut snapshot,
        &Diagnostics::new(true),
        &|| false,
    )?;
    let files = Files::from_snapshot(snapshot, || false, Diagnostics::new(true))?;
    let menu = MenuData::load(&files)?;
    let mut spawns = 0;
    let mut resources = 0;
    let mut unavailable = Vec::new();
    for id in 0..u16::try_from(rules.formations.records.len())? {
        let current = encounter::enemies(&files, menu.monsters()?, menu.items.len(), &[id]);
        let current = match current {
            Err(error) => {
                assert_eq!(
                    format!("{error:#}"),
                    "formation 90 requires missing enemy 191 variant 3"
                );
                unavailable.push(id);
                continue;
            }
            Ok(current) => current,
        };
        resources += current.resources.len();
        spawns += current.spawns.len();
        assert!(
            current
                .spawns
                .iter()
                .all(|spawn| spawn.resource < current.resources.len())
        );
    }
    assert_eq!(unavailable, [90]);
    for ids in [[0, 1, 2], [4, 99, 999]] {
        let combined = encounter::enemies(&files, menu.monsters()?, menu.items.len(), &ids)?;
        assert!(!combined.spawns.is_empty());
    }
    let current = recoil::Parameters::load(&files)?;
    for character in 1..=9 {
        let character = resonance_battle::Character::try_from(character)?;
        for position in 0..=3 {
            current.guard_recovery_bonus(position, character)?;
        }
    }
    use resonance_content::battle_profile::RecoilWeight;
    for profile in &rules.party.records {
        for weight in [
            RecoilWeight::Normal,
            RecoilWeight::Light,
            RecoilWeight::Heavy,
            RecoilWeight::Grounded,
        ] {
            current.profile(weight, &profile.traits);
        }
    }
    let techniques = resonance_content::arte::Catalogue::load(&files)?;
    assert!(!techniques.definitions.is_empty());
    let art = battle_ui::Art::load(&files)?;
    art.validate()?;
    for path in art.files() {
        files.read(path)?;
    }
    eprintln!(
        "verified 999 runtime formations with {resources} declared enemy resources and {spawns} actor spawns, preserved the source failure for formation 90, combined encounters, recoil/strategy controls, {} resolved techniques and {} UI image bindings",
        techniques.definitions.len(),
        art.files().count()
    );
    Ok(())
}

#[test]
#[ignore = "requires a complete named library at RESONANCE_TEST_ASSETS; admits every stored scene model and both named event scripts"]
fn named_choreography_prepares_scene_models_and_scoped_event_bindings() -> Result<()> {
    use resonance_content::{
        battle_event::document as events,
        battle_scene::document::{self as scenes, Kind},
        library::Mappings,
    };
    let library = named_library()?;
    let ids = library
        .snapshot()
        .mappings(scenes::TECHNIQUES)?
        .iter()
        .map(|(id, _)| u16::try_from(id).map_err(Into::into))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(ids.len() == 87, "incomplete stored-scene fixture");

    let mut models = 0;
    for (kind, ids) in [
        (Kind::Technique, ids),
        (Kind::Skill, BTreeSet::from([51, 228])),
    ] {
        for id in ids {
            let diagnostics = Diagnostics::new(true);
            let mut snapshot = library.snapshot();
            scenes::capture(&mut snapshot, kind, id, &diagnostics, &|| false)?;
            let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
            let scene = scenes::load(&files, kind, id)?;
            let bank = effect_program::load(&files, &scene.effects)?
                .context("missing named scene bank")?;
            assert!(bank.art.is_some());
            for (&slot, part) in &bank.art.as_ref().unwrap().models {
                let resource = resonance_battle::ResourceId(u32::from(slot));
                resonance_game::battle::model::effect(&files, part, resource, true)
                    .with_context(|| format!("named {kind:?} {id} model {slot}"))?;
                models += 1;
            }
            // All source paths used above belong to the captured view, including
            // the merged mapping. Later disk edits cannot alter runtime loading.
            let mapping: Mappings = files.json(kind.mapping())?;
            assert!(files.contains_key(&mapping.resolve(
                kind.mapping(),
                u32::from(id),
                Some("scene.json")
            )?));
            assert!(!files.diagnostics().has_errors());
        }
    }
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    resonance_content::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
    let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
    let session = resonance_content::session::SessionData::load(&files)?;
    let party = resonance_events::party::Party::new(&session, Default::default())?;
    let mut poses = 0;
    for id in [2, 3] {
        let diagnostics = Diagnostics::new(true);
        let mut snapshot = library.snapshot();
        let scene = Arc::new(events::capture(&mut snapshot, id, &diagnostics, &|| false)?);
        let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
        let prepared = resonance_game::battle::event::Prepared::load(&files, scene)?;
        let current = prepared.enter(&party)?;
        let current = current.events.resources();
        assert!(!current.bindings.is_empty());
        assert!(
            !current
                .blink
                .as_ref()
                .context("event blink is absent")?
                .frames
                .is_empty()
        );
        assert!(!current.models.is_empty());
        for model in current.models.values() {
            for pose in model.attachments.clips.values() {
                for name in [model.names.first(), model.names.last()]
                    .into_iter()
                    .flatten()
                {
                    for tick in [0., 1., 30., 300.] {
                        pose.sample_matrix(name, tick)?;
                        poses += 1;
                    }
                }
            }
        }
        assert!(!files.contains_key("data/embedded/field-effects.json"));
        assert!(!files.diagnostics().has_errors());
    }
    eprintln!(
        "verified 89 stored-scene runtime readers, {models} native effect models, and both event scripts with {poses} sampled attachment poses and scoped actor/motion bindings"
    );
    Ok(())
}

#[test]
#[ignore = "requires a complete named library at RESONANCE_TEST_ASSETS; compares every costume's runtime victory preparation"]
fn named_party_victories_bind_costumes_without_body_hashes() -> Result<()> {
    use resonance_content::{
        battle_model::document as models, battle_victory::document as victories,
    };
    use resonance_game::battle::victory;
    let library = named_library()?;
    let selections = (1..=9)
        .flat_map(|character| {
            (0..=4)
                .filter(move |&costume| character != 9 || costume != 4)
                .map(move |costume| (character, costume))
        })
        .collect::<Vec<_>>();

    let mut total = 0;
    for (character, costume) in selections {
        let diagnostics = Diagnostics::new(true);
        let mut snapshot = library.snapshot();
        let (selection, body) =
            models::capture_party(&mut snapshot, character, costume, &diagnostics, &|| false)?;
        let performances = victories::capture(
            &mut snapshot,
            [(character, costume, &selection)],
            &diagnostics,
            &|| false,
        )?;
        let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
        let model = victory_model(&files, &body, character)?;
        let selected_character = resonance_battle::Character::try_from(character)?;
        let (prepared, selected) =
            victory::prepare(&files, &performances, selected_character, costume, &model)?;
        assert_eq!(selected.len(), 5);
        assert!(
            victory::prepare(
                &files,
                &performances,
                selected_character,
                costume + 100,
                &model
            )?
            .1
            .is_empty()
        );
        for performance in &selected {
            let motion = &prepared.motions[&performance.motion.clip];
            motion.validate(&prepared.skeleton)?;
            assert!(motion.duration_frames > 0.);
        }
        assert!(!files.diagnostics().has_errors());
        total += selected.len();
    }
    eprintln!(
        "verified runtime victory selection and motion data for 44 costumes and {total} performances"
    );
    Ok(())
}

fn victory_model(
    files: &Files,
    body: &resonance_content::battle_model::Party,
    character: u8,
) -> Result<resonance_battle::ModelDefinition> {
    use resonance_content::animation::Skeleton;
    let mut motions = std::collections::BTreeMap::new();
    for part in body.parts.iter().chain(body.wings.iter()) {
        let skeleton = Skeleton::from_glb(&files.read(&part.mesh)?)?;
        ensure!(
            skeleton
                .bones
                .iter()
                .map(|bone| &bone.name)
                .eq(part.bone_names.iter()),
            "party mesh bone indices differ"
        );
        for clip in &part.clips {
            let motion = clip.decode_for_skeleton(&files.read(&clip.motion)?, &skeleton)?;
            motion.validate(&skeleton)?;
        }
    }
    for clip in &body.parts[0].clips {
        let motion = clip.decode_for_skeleton(&files.read(&clip.motion)?, &body.body.skeleton)?;
        motion.validate(&body.body.skeleton)?;
        motions.insert(clip.resource_slot, motion);
    }
    Ok(victory_definition(
        body.body.skeleton.clone(),
        motions,
        character,
    ))
}

fn victory_definition(
    skeleton: resonance_content::animation::Skeleton,
    motions: std::collections::BTreeMap<u16, resonance_content::animation::Motion>,
    character: u8,
) -> resonance_battle::ModelDefinition {
    resonance_battle::ModelDefinition {
        tint: [64, 64, 64, 255],
        fade_on_defeat: false,
        resource: resonance_battle::ResourceId(u32::from(character)),
        skeleton,
        body_spheres: vec![],
        reception_spheres: None,
        motions,
        secondary_motion: vec![],
        initial: resonance_battle::Playback {
            clip: 0,
            frame: 0.,
            rate: 1.,
            repeat: true,
        },
        reactions: Default::default(),
        idle_motions: [Some(0), None],
        idle_expression: [0; 4],
        active_expression: [0; 4],
        forced_eye_frame: None,
        blinks: vec![],
        weapons: vec![],
        shadow: None,
        suppress_root_translation: [false; 3],
    }
}

#[test]
fn named_victory_keeps_character_costume_scope_and_current_clip_bytes() -> Result<()> {
    use resonance_content::{
        animation::{Bone, Motion, Skeleton},
        battle_victory::document as victories,
    };
    use resonance_game::battle::victory;
    let work = tempfile::tempdir()?;
    fs::write(
        work.path().join("content.json"),
        br#"{"version":94,"game":"tales-of-symphonia"}"#,
    )?;
    fs::create_dir_all(work.path().join("battle/victory"))?;
    fs::write(work.path().join(victories::GROUPS), br#"[]"#)?;
    for (name, frames) in [("default", 30.), ("costume", 60.), ("other", 90.)] {
        let definition = serde_json::json!({
            "costumes":{"test":{"healthy-party":{"motion":format!("{name}.glb"),"timeline":"celebration"}}},
            "timelines":{"celebration":{}}
        });
        fs::write(
            work.path().join(format!("{name}.json")),
            serde_json::to_vec(&definition)?,
        )?;
        fs::write(
            work.path().join(format!("{name}.glb")),
            Motion {
                duration_frames: frames,
                tracks: vec![],
            }
            .encode(&Skeleton::single("root"))?,
        )?;
    }
    let library = Library::open(work.path(), &[])?;
    let skeleton = Skeleton {
        bones: vec![Bone {
            name: "root".into(),
            parent: None,
            parent_transform: None,
            shadow_bounds: true,
            bind: Default::default(),
        }],
    };
    let model = victory_definition(
        skeleton,
        [(
            0,
            Motion {
                duration_frames: 30.,
                tracks: vec![],
            },
        )]
        .into(),
        1,
    );
    let selection = |name: &str| victories::Selection {
        file: format!("{name}.json"),
        costume: "test".into(),
    };
    let default = selection("default");
    let costume = selection("costume");
    let other = selection("other");
    let capture = || -> Result<_> {
        let mut snapshot = library.snapshot();
        let diagnostics = Diagnostics::new(true);
        let performances = victories::capture(
            &mut snapshot,
            [(1, 0, &default), (1, 1, &costume), (2, 0, &other)],
            &diagnostics,
            &|| false,
        )?;
        let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
        Ok((files, performances))
    };
    let (files, performances) = capture()?;
    for (character, costume, duration) in [(1, 0, 30.), (1, 1, 60.), (2, 0, 90.)] {
        let (prepared, selected) = victory::prepare(
            &files,
            &performances,
            resonance_battle::Character::try_from(character)?,
            costume,
            &model,
        )?;
        assert_eq!(selected.len(), 1);
        assert_eq!(
            prepared.motions[&selected[0].motion.clip].duration_frames,
            duration
        );
    }
    assert!(
        victory::prepare(
            &files,
            &performances,
            resonance_battle::Character::Lloyd,
            2,
            &model
        )?
        .1
        .is_empty()
    );
    fs::write(
        work.path().join("costume.glb"),
        Motion {
            duration_frames: 120.,
            tracks: vec![],
        }
        .encode(&Skeleton::single("root"))?,
    )?;
    let (fresh, changed) = capture()?;
    let (prepared, selected) = victory::prepare(
        &fresh,
        &changed,
        resonance_battle::Character::Lloyd,
        1,
        &model,
    )?;
    assert_eq!(
        prepared.motions[&selected[0].motion.clip].duration_frames,
        120.
    );
    let (prepared, selected) = victory::prepare(
        &files,
        &performances,
        resonance_battle::Character::Lloyd,
        1,
        &model,
    )?;
    assert_eq!(
        prepared.motions[&selected[0].motion.clip].duration_frames,
        60.
    );
    assert!(
        victories::capture(
            &mut library.snapshot(),
            [(1, 0, &default), (1, 0, &costume)],
            &Diagnostics::default(),
            &|| false
        )
        .is_err()
    );
    assert!(
        victories::capture(
            &mut library.snapshot(),
            [(1, 0, &default)],
            &Diagnostics::default(),
            &|| true
        )
        .is_err()
    );
    let mut definition: serde_json::Value =
        serde_json::from_slice(&fs::read(work.path().join("default.json"))?)?;
    definition["costumes"]["test"]["injured-leader"] =
        serde_json::json!("malformed optional performance");
    fs::write(
        work.path().join("default.json"),
        serde_json::to_vec(&definition)?,
    )?;
    fs::write(work.path().join(victories::GROUPS), br#"[false]"#)?;
    let diagnostics = Diagnostics::default();
    let selected = victories::capture(
        &mut library.snapshot(),
        [(1, 0, &default), (1, 1, &costume)],
        &diagnostics,
        &|| false,
    )?;
    assert_eq!(selected.ordinary.len(), 2);
    assert!(selected.groups.is_empty());
    assert_eq!(diagnostics.entries().len(), 2);
    Ok(())
}

#[test]
#[ignore = "requires a complete named library at RESONANCE_TEST_ASSETS; compares all arena rigs and runtime poses"]
fn named_arenas_prepare_native_rigs_and_poses() -> Result<()> {
    let library = named_library()?;
    let ids = library
        .snapshot()
        .mappings(battle_stage::document::MAPPINGS)?
        .iter()
        .map(|(id, _)| u16::try_from(id).map_err(Into::into))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(ids.len() == 86, "incomplete arena fixture");

    for path in [
        resonance_content::battle_effect::document::COMMON,
        resonance_content::battle_effect::document::TECHNIQUES,
    ] {
        let mut snapshot = library.snapshot();
        let diagnostics = Diagnostics::new(true);
        resonance_content::battle_effect::document::capture(
            &mut snapshot,
            path,
            &diagnostics,
            &|| false,
        )?;
        let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
        let bank = effect_program::load(&files, path)?.context("named shared effect bank")?;
        for (&slot, model) in &bank.art.as_ref().context("shared effect art")?.models {
            resonance_game::battle::model::effect(
                &files,
                model,
                resonance_battle::ResourceId(u32::from(slot)),
                true,
            )
            .with_context(|| format!("{path} model {slot}"))?;
        }
        assert!(!files.diagnostics().has_errors());
    }
    let mut poses = 0;
    for id in ids {
        let (named, arena) = stage::prepare_named(&library, id, Diagnostics::new(true), || false)?;
        let models = stage::Model::load(&named, &arena)?;
        assert_eq!(models.len(), arena.layers.len());
        for model in models.values() {
            for tick in [0, 1, 30, 300, 3600] {
                assert_eq!(model.pose(tick)?.len(), model.skeleton.bones.len());
                poses += 1;
            }
        }
        if let Some(effects) = &arena.effects {
            let bank = effect_program::load(&named, effects)?.context("named effects")?;
            if arena.recurring_interval.is_some() {
                effect_program::prepare(
                    &bank,
                    resonance_battle::ResourceId(9),
                    &[1],
                    Default::default(),
                    &mut |_| Ok(None),
                    named.diagnostics(),
                )?;
            }
        }
        assert!(!named.diagnostics().has_errors());
    }
    eprintln!("verified 86 named arenas and {poses} native layer poses");
    Ok(())
}
