use super::{opening_options, opening_setup};
use anyhow::{Context, Result};
use resonance_content::menu_data::MenuData;

#[test]
#[ignore = "requires current opening battle publications; CPU preparation only"]
fn complete_opening_candidates_are_atomic_and_replay_the_same_prepared_generation() -> Result<()> {
    use resonance_battle::Sound;
    use resonance_events::{battle::DefeatPolicy, party::Party};
    use resonance_game::battle::{encounter::PrepareOptions, party::Character};
    use std::sync::Arc;

    let root = super::common::asset_root();
    let mut retained = resonance_content::preparation::capture(
        &resonance_content::library::Library::open(&root, &[])?,
        &[],
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    let mut menus: MenuData = resonance_content::menu_data::MenuData::load(&retained)?;
    for path in [
        resonance_content::menu_data::MANUAL_PATH,
        resonance_content::menu_data::FIGURINES_PATH,
        resonance_content::menu_data::SYNOPSIS_PATH,
        resonance_content::menu_data::CUSTOMIZE_PATH,
        resonance_content::menu_data::RENAME_PATH,
    ] {
        retained.remove(path);
    }
    retained.insert(
        resonance_content::menu_data::MANUAL_PATH.into(),
        b"{".as_slice().into(),
    );
    menus.presentation.world_map = None;
    assert!(menus.validate().is_err());
    menus.validate_gameplay()?;
    let mut data: resonance_content::session::SessionData =
        resonance_content::session::SessionData::load(&retained)?;
    data.rules = Some(Arc::new(menus.clone()));
    let mut party = Party::new(&data, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![1, 2, 3];
    for (index, level) in [(0, 3), (1, 1), (2, 2)] {
        party
            .raise_level(&data, index, level, None, || 7)
            .map_err(anyhow::Error::msg)?;
    }

    let mut invalid = menus.clone();
    invalid.presentation.monsters.as_mut().unwrap().records[36]
        .statistics
        .clear();
    assert!(
        resonance_game::battle::encounter::Assets::capture(
            &mut (&retained)
                .library()
                .context("named library is required")?
                .snapshot(),
            &invalid,
            &data,
            &party,
            opening_setup(1, DefeatPolicy::GameOver),
            (&retained).diagnostics().clone(),
            &(|| false)
        )
        .is_err(),
        "selected enemy data must still validate"
    );
    for formation in [1, 2] {
        let options = || PrepareOptions {
            event_flags: if formation == 2 {
                [19, 24, 29].into()
            } else {
                Default::default()
            },
            colette_state: if formation == 2 { 1000 } else { 0 },
            ..opening_options(0x2345)
        };
        let (mut assets, prepared) = {
            let mut snapshot = (&retained)
                .library()
                .context("named library is required")?
                .snapshot();
            let catalogue = resonance_content::battle_audio::document::Catalogue::capture(
                &mut snapshot,
                &(|| false),
            )?;
            let inputs = resonance_game::battle::encounter::Assets::capture(
                &mut snapshot,
                &menus,
                &data,
                &party,
                opening_setup(formation, DefeatPolicy::GameOver),
                (&retained).diagnostics().clone(),
                &(|| false),
            )?;
            let (assets, prepared, _) = inputs.prepare_named(&menus, options(), &catalogue)?;
            Ok::<_, anyhow::Error>((assets, prepared))
        }?;

        let audio = resonance_content::battle_audio::document::Catalogue::capture(
            &mut assets
                .files
                .library()
                .context("named library required")?
                .snapshot(),
            &|| false,
        )?;
        let sound = |request| -> Result<_> {
            match request {
                Sound::Cue(index) => {
                    anyhow::ensure!(
                        audio.resolved.effects.contains_key(&i16::try_from(index)?),
                        "unpublished battle cue {index}"
                    );
                }
                Sound::Stream(index) => {
                    anyhow::ensure!(
                        audio.resolved.voices.contains_key(&u32::from(index)),
                        "unpublished battle voice {index}"
                    );
                }
            };
            Ok(Some(request))
        };
        if formation == 1 {
            let mut changed = menus.clone();
            changed.items.clear();
            let error = assets
                .prepare(&changed, options(), sound)
                .err()
                .expect("changed selected equipment must be checked before deriving stats");
            assert!(error.to_string().contains("missing equipped item"));
        }
        assert_eq!(
            prepared.characters,
            [Character::Lloyd, Character::Colette, Character::Genis]
        );
        assert_eq!(
            prepared
                .core
                .models()
                .unwrap()
                .definition(resonance_battle::ActorId::from_index(1)?)
                .context("missing Colette model")?
                .forced_eye_frame,
            (formation == 2).then_some(15),
        );
        assert_eq!(prepared.results.formation, formation);
        let colette = prepared
            .core
            .models()
            .unwrap()
            .definition(resonance_battle::ActorId::from_index(1)?)
            .context("missing Colette model")?;
        let body = prepared
            .models
            .iter()
            .find(|body| body.resource == colette.resource)
            .context("missing Colette render model")?;
        let part = &body.parts[0];
        let crystal = part
            .scene
            .appearance
            .as_ref()
            .and_then(|a| a.costume)
            .context("missing crystal texture selector")?;
        let material = part
            .scene
            .materials
            .iter()
            .position(|material| {
                material
                    .color
                    .as_ref()
                    .is_some_and(|binding| binding.texture == crystal)
            })
            .context("missing crystal material")?;
        assert_eq!(
            part.uv_offsets[material][1],
            if formation == 2 { 0.5 } else { 0.75 }
        );
        assert_eq!(prepared.results.enemies.len(), usize::from(formation));
        let lloyd = prepared
            .core
            .models()
            .unwrap()
            .definition(resonance_battle::ActorId::from_index(0)?)
            .context("missing Lloyd model")?;
        let body = prepared
            .models
            .iter()
            .find(|body| body.resource == lloyd.resource)
            .context("missing Lloyd render model")?;
        let bandage = body
            .skeleton
            .bones
            .iter()
            .position(|bone| bone.name == "obj_nuno01")
            .context("missing Lloyd bandage")?;
        assert_eq!(
            body.suppressed_nodes.contains(&bandage),
            formation == 2,
            "battle appearance must preserve the story's uncovered Exsphere"
        );
        let first = prepared.core;
        let second = assets.prepare(&menus, options(), sound)?.core;
        assert_eq!(first.snapshot(), second.snapshot());
        assert_eq!(first.actors().len(), 3 + usize::from(formation));
        // The held snapshot must not consume one of the registered world visits.
        let seed = first.random_state();
        let _ = first.snapshot();
        assert_eq!(first.random_state(), seed);
        assets
            .files
            .remove(resonance_content::battle_projectile::PATH);
        // Replay uses captured rules, without decoding their source again.
        let replay = assets.prepare(&menus, options(), sound)?.core;
        assert_eq!(first.snapshot(), replay.snapshot());
        assert!(
            retained
                .read(resonance_content::battle_projectile::PATH)
                .is_ok()
        );
    }
    // A fresh capture still rejects a broken required document.
    let modification = tempfile::tempdir()?;
    let path = modification
        .path()
        .join(resonance_content::battle_projectile::PATH);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, b"{")?;
    let library = resonance_content::library::Library::open(
        &root,
        &[resonance_content::library::Mod {
            name: "broken projectiles".into(),
            root: modification.path().into(),
        }],
    )?;
    let error = resonance_game::battle::encounter::Assets::capture(
        &mut library.snapshot(),
        &menus,
        &data,
        &party,
        opening_setup(1, DefeatPolicy::GameOver),
        resonance_content::diagnostics::Diagnostics::new(true),
        &|| false,
    )
    .err()
    .context("fresh capture accepted broken projectile rules")?;
    assert!(format!("{error:#}").contains(resonance_content::battle_projectile::PATH));
    Ok(())
}
