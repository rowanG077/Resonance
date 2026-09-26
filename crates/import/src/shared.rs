//! Shared field dependencies prepared once before any field is assembled.
use anyhow::{Context, Result};
use resonance_content::font::{BitmapFont, TextSpan};
use std::{collections::BTreeMap, path::Path};

pub(crate) struct Prepared {
    pub catalogue: crate::resource::Catalogue,
    pub resource_catalogue: String,
    pub font: BitmapFont,
    pub effects: crate::field_effects::Prepared,
    pub toon_ramp: String,
    pub save_point_tutorial: Vec<TextSpan>,
    pub files: BTreeMap<String, String>,
}

pub(crate) fn prepare(
    extracted: &Path,
    output: &Path,
    sources: &BTreeMap<String, String>,
    executable: &[u8],
    catalogues: &crate::all_assets::Catalogues,
    battle_sources: &crate::source_assets::Sources,
    usual: &[u8],
) -> Result<Prepared> {
    let crate::font::PreparedDialogue { font, art } = crate::font::prepare(extracted, output)?;
    let game_over_path = crate::game_over::publish(extracted, output)?;
    let game_over: resonance_content::game_over::Art =
        serde_json::from_slice(&std::fs::read(output.join(&game_over_path))?)?;
    crate::menu::cook(
        extracted,
        output,
        executable,
        catalogues,
        battle_sources,
        usual,
    )?;
    let weapons = crate::battle_model::weapon::publish_source(extracted, battle_sources, output)?;
    let techniques = crate::arte::publish(&catalogues.menu.arte, output)?;
    let session = crate::session::cook(executable, &catalogues.menu, output)?;
    let text = crate::session::cook_text(executable, &catalogues.menu, output)?;
    let skits = crate::skit::cook(extracted, output)?;
    let effects = crate::field_effects::cook(extracted, output)?;
    let toon_ramp = crate::field_lighting::cook(extracted, output)?;
    let catalogue = &catalogues.resources;
    let resource_catalogue = "game/resource-catalogue.json".to_owned();
    crate::write_atomic(
        &output.join(&resource_catalogue),
        &resource_sources(catalogue, sources)?,
    )?;
    let save_point_tutorial =
        crate::font::system_text(crate::dol::slice(executable, 0x8017A274, 256)?)?;
    let battle_ui: resonance_content::battle_ui::Art = serde_json::from_slice(&std::fs::read(
        output.join(resonance_content::battle_ui::PATH),
    )?)?;
    battle_ui.validate()?;
    let effect_bank: resonance_content::battle_effect::SourceBank = serde_json::from_slice(
        &std::fs::read(output.join(resonance_content::battle_effect::COMMON_PATH))?,
    )?;
    let effect_files = effect_bank
        .art
        .context("missing ordinary effect artwork")?
        .files;
    let files = [
        resource_catalogue.clone(),
        "ui/dialogue.json".into(),
        "ui/story-subtitles.json".into(),
        "ui/menu.json".into(),
        "game/menu-data.json".into(),
        game_over_path,
        resonance_content::battle_formation::PATH.into(),
        resonance_content::battle_voice::PATH.into(),
        resonance_content::battle_ui::PATH.into(),
        resonance_content::battle_victory::PATH.into(),
        resonance_content::battle_effect::COMMON_PATH.into(),
        resonance_content::battle_effect::TECHNIQUES_PATH.into(),
        resonance_content::battle_effect::TINTS_PATH.into(),
        resonance_content::battle_projectile::PATH.into(),
        resonance_content::battle_action::MARTIAL_PATH.into(),
        resonance_content::battle_action::SPELL_PATH.into(),
        resonance_content::battle_action::NORMAL_PATH.into(),
        resonance_content::battle_recoil::PATH.into(),
        resonance_content::battle_profile::PARTY_PATH.into(),
        weapons,
        resonance_content::battle_scene::path(237),
        resonance_content::battle_stage::path(13),
        techniques,
        art.font,
        art.cursor.path,
        font.texture.clone(),
        effects.path.clone(),
        toon_ramp.clone(),
        session,
        text,
        skits,
    ]
    .into_iter()
    .chain(
        (0..resonance_content::monster::MONSTER_COUNT)
            .map(|id| resonance_content::battle_model::enemy_path(id as u8)),
    )
    .chain(art.textures.into_iter().map(|texture| texture.path))
    .chain(battle_ui.files().map(str::to_owned))
    .chain(game_over.files.into_keys())
    .chain(effect_files.into_keys())
    .chain(effects.files.iter().cloned())
    .chain(
        resonance_script_content::FILES
            .iter()
            .filter(|(path, _)| path.ends_with(".sym"))
            .map(|(path, _)| format!("scripts/{path}")),
    )
    .map(|path| Ok((path.clone(), crate::media::hash_file(&output.join(path))?)))
    .collect::<Result<_>>()?;
    Ok(Prepared {
        catalogue: catalogue.clone(),
        resource_catalogue,
        font,
        effects,
        toon_ramp,
        save_point_tutorial,
        files,
    })
}

/// Preserve indexed native lookup domains without materializing every possible
/// model/animation pairing. Paths identify unified physical packages; group
/// member indices are resolved through each package's ordinary directory.
fn resource_sources(
    catalogue: &crate::resource::Catalogue,
    sources: &BTreeMap<String, String>,
) -> Result<Vec<u8>> {
    #[derive(serde::Serialize)]
    #[serde(tag = "status", rename_all = "snake_case")]
    enum Resource {
        Cooked { path: String },
        SourceAbsent { name: String },
    }
    #[derive(serde::Serialize)]
    struct Catalogue {
        version: u8,
        standalone: Vec<Option<Resource>>,
        groups: Vec<Option<Resource>>,
    }
    let mut available = BTreeMap::new();
    for (source, hash) in sources {
        if let Some((_, path)) = source.split_once('/') {
            available.entry(path.to_ascii_lowercase()).or_insert(hash);
        }
    }
    let resolve = |name: &Option<String>| {
        name.as_ref()
            .map(|name| match available.get(&name.to_ascii_lowercase()) {
                Some(hash) => Resource::Cooked {
                    path: format!("assets/{hash}"),
                },
                None => Resource::SourceAbsent { name: name.clone() },
            })
    };
    Ok(serde_json::to_vec(&Catalogue {
        version: 1,
        standalone: catalogue.standalone.iter().map(resolve).collect(),
        groups: catalogue
            .groups
            .iter()
            .map(|group| resolve(&group.path))
            .collect(),
    })?)
}

#[test]
fn resource_lookup_preserves_ids_aliases_and_absent_original_sources() -> Result<()> {
    let catalogue = crate::resource::Catalogue {
        save_point: "save-point.bin".into(),
        standalone: vec![
            Some("Model.bin".into()),
            None,
            Some("missing.bin".into()),
            Some("alias.bin".into()),
        ],
        groups: vec![
            crate::resource::Group {
                path: Some("models.cab".into()),
                storage: [0; 3],
            },
            crate::resource::Group {
                path: None,
                storage: [0; 3],
            },
        ],
        party_bodies: vec![],
        party_battle_motions: vec![],
        party_field_motions: vec![],
        field_services: vec![],
    };
    let sources = BTreeMap::from([
        ("disc2/MODEL.bin".into(), "later".into()),
        ("disc1/model.bin".into(), "shared".into()),
        ("disc1/alias.bin".into(), "shared".into()),
        ("disc2/models.cab".into(), "group".into()),
    ]);
    let value: serde_json::Value =
        serde_json::from_slice(&resource_sources(&catalogue, &sources)?)?;
    assert_eq!(value["standalone"][0]["path"], "assets/shared");
    assert_eq!(value["standalone"][0], value["standalone"][3]);
    assert!(value["standalone"][1].is_null());
    assert_eq!(
        value["standalone"][2],
        serde_json::json!({"status":"source_absent", "name":"missing.bin"})
    );
    assert_eq!(value["groups"][0]["path"], "assets/group");
    assert!(value["groups"][1].is_null());
    Ok(())
}
