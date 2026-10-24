use super::{
    tests::{equivalent, no_inventory},
    *,
};
use resonance_content::{
    arte, battle_formation, battle_profile, battle_rules as rules,
    battle_ui::{self, document as ui},
    battle_unison,
    diagnostics::Diagnostics,
    library::{Library, Mod},
    prepared::{Cache, Files},
};

fn write(root: &Path, path: &str, value: &impl Serialize) -> Result<()> {
    fs::create_dir_all(root.join(path).parent().unwrap())?;
    Ok(fs::write(
        root.join(path),
        serde_json::to_vec_pretty(value)?,
    )?)
}

#[test]
fn named_formations_validate_gameplay_and_retain_the_effective_snapshot() -> Result<()> {
    let output = tempfile::tempdir()?;
    write(
        output.path(),
        "content.json",
        &library::Descriptor {
            version: 1,
            game: "GQSEAF".into(),
            revision: 0,
        },
    )?;
    let table = serde_json::json!({
        "version":1,
        "records":[{"settings":{"play_music":true,"celebrate":true,"entry_voice":true,"escape_restricted":false},
            "resources":[{"enemy":7,"hidden_name":false}],
            "actors":[{"resource":0,"variant":0,"position":null,"unsupported_reason":null}]}],
        "pools":[[{"formation":0,"weight":2}]]
    });
    write(output.path(), battle_formation::PATH, &table)?;
    let library = Library::open(output.path(), &[])?;
    let capture = || -> Result<Files> {
        let mut snapshot = library.snapshot();
        snapshot.read(battle_formation::PATH, 1024 * 1024)?;
        Files::from_snapshot(
            snapshot,
            &mut Cache::default(),
            || false,
            Diagnostics::default(),
        )
    };
    let retained = capture()?;
    let original = rules::formations(&retained)?;
    original.validate()?;
    assert!(original.source_sha256.is_empty());
    assert_eq!(original.select(0, u32::MAX)?, 0);
    let mut edited = table.clone();
    edited["records"][0]["resources"][0]["enemy"] = 12.into();
    write(output.path(), battle_formation::PATH, &edited)?;
    assert_eq!(
        rules::formations(&capture()?)?.records[0].resources[0].enemy,
        12
    );
    assert_eq!(
        rules::formations(&retained)?.records[0].resources[0].enemy,
        7
    );
    for bad in [
        {
            let mut v = table.clone();
            v["pools"][0][0]["formation"] = 1.into();
            v
        },
        {
            let mut v = table.clone();
            v["pools"][0][0]["weight"] = 0.into();
            v
        },
        {
            let mut v = table.clone();
            v["records"][0]["actors"][0]["resource"] = 1.into();
            v
        },
        {
            let mut v = table.clone();
            v["version"] = 2.into();
            v
        },
        {
            let mut v = table.clone();
            v["source_sha256"] = "a".repeat(64).into();
            v
        },
        {
            let mut v = table.clone();
            v["files"] = serde_json::json!({});
            v
        },
    ] {
        write(output.path(), battle_formation::PATH, &bad)?;
        assert!(
            rules::formations(&capture()?)
                .and_then(|table| table.validate())
                .is_err()
        );
    }
    assert!(rules::capture(&mut library.snapshot(), &|| true).is_err());
    Ok(())
}

#[test]
#[ignore = "requires original disc 1; compares native shared battle rules and UI"]
fn original_battle_rules_and_ui_preserve_current_editable_inputs() -> Result<()> {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let native = crate::battle_rules::read(&extracted)?;
    let native_art = crate::battle_ui::prepare(&extracted, source.path())?;
    let mut publisher = Publisher::new(source.path(), output.path())?;
    publisher.battle_rules(&native)?;
    publisher.battle_ui(native_art.clone())?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = Library::open(output.path(), &[])?;
    let diagnostics = Diagnostics::new(true);
    let mut snapshot = library.snapshot();
    let current = rules::capture(&mut snapshot, &|| false)?;
    let art = ui::capture(&mut snapshot, &diagnostics, &|| false)?;
    let files = Files::from_snapshot(snapshot, &mut Cache::default(), || false, diagnostics)?;

    let mut formations = native.formations;
    formations.source_sha256.clear();
    let mut party = native.party;
    party.source_sha256.clear();
    party.collider_recipe_sha256.clear();
    let mut defense = native.defense;
    defense.source_sha256.clear();
    defense.recipe_sha256.clear();
    let mut unison = native.unison;
    unison.source_sha256.clear();
    unison.host.source_sha256.clear();
    let mut projectiles = native.projectiles;
    projectiles.source_sha256.clear();
    let mut recoil = native.recoil;
    recoil.source_sha256.clear();
    let techniques = native.techniques;
    for (before, captured, loaded) in [
        (
            serde_json::to_value(&formations)?,
            serde_json::to_value(&current.formations)?,
            serde_json::to_value(rules::formations(&files)?)?,
        ),
        (
            serde_json::to_value(&party)?,
            serde_json::to_value(&current.party)?,
            serde_json::to_value(rules::party(&files)?)?,
        ),
        (
            serde_json::to_value(&defense)?,
            serde_json::to_value(&current.defense)?,
            serde_json::to_value(rules::defense(&files)?)?,
        ),
        (
            serde_json::to_value(&unison)?,
            serde_json::to_value(&current.unison)?,
            serde_json::to_value(rules::unison(&files)?)?,
        ),
        (
            serde_json::to_value(&projectiles)?,
            serde_json::to_value(&current.projectiles)?,
            serde_json::to_value(rules::projectiles(&files)?)?,
        ),
        (
            serde_json::to_value(&recoil)?,
            serde_json::to_value(&current.recoil)?,
            serde_json::to_value(rules::recoil(&files)?)?,
        ),
        (
            serde_json::to_value(&techniques)?,
            serde_json::to_value(&current.techniques)?,
            serde_json::to_value(files.json::<arte::Catalogue>(arte::PATH)?)?,
        ),
    ] {
        assert_eq!(before, captured);
        assert_eq!(captured, loaded);
    }
    equivalent(
        &native_art,
        &art,
        source.path(),
        output.path(),
        &mut BTreeMap::new(),
    )?;
    assert_eq!(
        serde_json::to_value(&art)?,
        serde_json::to_value(battle_ui::Art::load(&files)?)?
    );
    for path in art.files() {
        assert!(files.contains_key(path), "uncaptured UI image {path}");
    }
    assert!(!files.contains_key("battle/ui.json"));
    assert!(!files.contains_key(battle_profile::PARTY_PATH));
    for path in publisher
        .published
        .keys()
        .filter(|path| path.ends_with(".json"))
    {
        let data: serde_json::Value = serde_json::from_slice(&fs::read(output.path().join(path))?)?;
        no_inventory(&data, path);
        let text = serde_json::to_string(&data)?;
        assert!(!text.contains("recipe_sha256"), "provenance in {path}");
    }

    let modifications = tempfile::tempdir()?;
    let mut party_entry: rules::Party =
        serde_json::from_slice(&fs::read(output.path().join(rules::PARTY))?)?;
    party_entry.normal_attacks = "battle/party/custom-normal-attacks.json".into();
    write(modifications.path(), rules::PARTY, &party_entry)?;
    let mut normals = party.normal_attacks.clone();
    normals[0][0].recovery += 3;
    write(modifications.path(), &party_entry.normal_attacks, &normals)?;
    let modified = Library::open(
        output.path(),
        &[Mod {
            name: "combat changes".into(),
            root: modifications.path().to_owned(),
        }],
    )?;
    let edited = rules::capture(&mut modified.snapshot(), &|| false)?;
    assert_eq!(
        edited.party.normal_attacks[0][0].recovery,
        party.normal_attacks[0][0].recovery + 3
    );
    assert_eq!(
        rules::party(&files)?.normal_attacks[0][0].recovery,
        party.normal_attacks[0][0].recovery
    );
    // The rest of the table and all other roots are inherited from the base.
    assert_eq!(
        serde_json::to_value(&edited.unison)?,
        serde_json::to_value(&current.unison)?
    );
    fs::remove_file(modifications.path().join(&party_entry.normal_attacks))?;
    assert!(rules::capture(&mut modified.snapshot(), &|| false).is_err());

    let mut invalid_unison = rules::Unison::from(unison.clone());
    invalid_unison.host.prelude.camera_target = invalid_unison.host.prelude.camera_eye;
    write(modifications.path(), &party_entry.normal_attacks, &normals)?;
    write(modifications.path(), battle_unison::PATH, &invalid_unison)?;
    assert!(rules::capture(&mut modified.snapshot(), &|| false).is_err());
    fs::remove_file(modifications.path().join(battle_unison::PATH))?;

    let mut descriptor: battle_ui::Art<String> =
        serde_json::from_slice(&fs::read(output.path().join(ui::PATH))?)?;
    descriptor.feedback.item_label = "ITEM".into();
    write(modifications.path(), ui::PATH, &descriptor)?;
    let mut font = art.font.clone();
    font.glyphs
        .get_mut(&'0')
        .context("missing zero glyph")?
        .advance += 1;
    write(modifications.path(), ui::FONT, &font)?;
    let edited_art = ui::capture(&mut modified.snapshot(), &Diagnostics::new(true), &|| false)?;
    assert_eq!(edited_art.feedback.item_label, "ITEM");
    assert_eq!(
        edited_art.font.glyphs[&'0'].advance,
        art.font.glyphs[&'0'].advance + 1
    );
    assert_eq!(
        battle_ui::Art::load(&files)?.font.glyphs[&'0'].advance,
        art.font.glyphs[&'0'].advance
    );
    descriptor.portraits[0].as_mut().unwrap().path = "ui/battle/portraits/missing.png".into();
    write(modifications.path(), ui::PATH, &descriptor)?;
    let tolerant = Diagnostics::default();
    let partial = ui::capture(&mut modified.snapshot(), &tolerant, &|| false)?;
    assert!(partial.portraits[0].is_none() && tolerant.has_errors());
    assert!(ui::capture(&mut modified.snapshot(), &Diagnostics::new(true), &|| false).is_err());
    assert!(ui::capture(&mut modified.snapshot(), &tolerant, &|| true).is_err());
    descriptor.commands.texture.path = "ui/battle/commands/missing.png".into();
    write(modifications.path(), ui::PATH, &descriptor)?;
    assert!(ui::capture(&mut modified.snapshot(), &Diagnostics::default(), &|| false).is_err());
    assert!(rules::capture(&mut library.snapshot(), &|| true).is_err());

    eprintln!(
        "verified {} formations, {} pools, {} party profiles, {} normal-attack banks, {} martial members, {} Unison combinations, {} projectile templates, {} techniques, {} UI image bindings and {} named publications",
        formations.records.len(),
        formations.pools.len(),
        party.records.len(),
        party.normal_attacks.len(),
        party.martial_attacks.members.len(),
        unison.combinations.len(),
        projectiles.records.len(),
        techniques.definitions.len(),
        art.files().count(),
        publisher.published.len()
    );
    Ok(())
}
