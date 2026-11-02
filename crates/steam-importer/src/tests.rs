use super::*;
use resonance_content::{arte, battle_rules, items, library::Mappings};
use std::{collections::BTreeMap, fs, path::PathBuf};
use symphonia_constants::{CharacterId, KnownId};

fn json<T: serde::de::DeserializeOwned>(root: &Path, path: &str) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(root.join(path))?)?)
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM pointing to an installed Steam copy"]
fn installed_categories_load_in_current_content_format() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let output = tempfile::tempdir()?;
    cook_tables(
        &source::Source::open(&installation, Language::English)?,
        output.path(),
    )?;
    let enemies: Mappings = json(
        output.path(),
        resonance_content::battle_enemy::document::MAPPINGS,
    )?;
    let formations: battle_rules::Formations = json(output.path(), "battle/formations.json")?;
    let formations = formations.prepare(&enemies)?;
    assert_eq!(formations.records.len(), 1000);
    assert_eq!(formations.pools.len(), 736);
    for (id, expected) in [(801, [251, 252, 253]), (802, [254, 255, 256])] {
        assert_eq!(
            formations.records[id]
                .resources
                .iter()
                .map(|r| r.enemy)
                .collect::<Vec<_>>(),
            expected
        );
    }
    let items: items::Document = json(output.path(), items::PATH)?;
    assert_eq!(items.prepare()?.inventory.len(), 531);
    for id in 528..531 {
        let name = items.mappings.target(id)?;
        let expected = ["Free Pass", "Theater Ticket", "Forget-Me-Not"][(id - 528) as usize];
        assert_eq!(items.items[name].text["name"], expected);
    }
    let techniques: arte::document::Document = json(output.path(), arte::PATH)?;
    let prepared = techniques.prepare()?;
    assert_eq!(techniques.mappings.target(253)?, "lightning-thrust");
    assert_eq!(techniques.mappings.target(260)?, "infernal-ruin");
    assert_eq!(techniques.mappings.target(261)?, "crimson-devastation");
    assert_eq!(prepared.catalogue.definitions.len(), 262);
    let sword_rain = techniques.mappings.id("sword-rain-alpha")? as usize;
    assert_eq!(
        prepared.menu[sword_rain]
            .alternatives
            .iter()
            .filter(|&&id| id != 0)
            .count(),
        5
    );
    let fields: Mappings = json(output.path(), resonance_content::library::FIELDS)?;
    assert_eq!(fields.iter().count(), 509);
    let forage: resonance_content::foraging::Document =
        json(output.path(), resonance_content::foraging::PATH)?;
    assert_eq!(forage.prepare(&items.mappings, &fields)?.pools.len(), 6);
    let ex: resonance_content::ex_skills::Document =
        json(output.path(), resonance_content::ex_skills::PATH)?;
    let ex = ex.prepare(&items.mappings)?;
    ex.validate_rules(531)?;
    let names = Mappings::new(
        CharacterId::ALL
            .iter()
            .filter_map(|id| id.resource_name().map(|n| (id.value(), n.to_owned())))
            .collect::<BTreeMap<_, _>>(),
    )?;
    let recoil: battle_rules::Recoil = json(output.path(), resonance_content::battle_recoil::PATH)?;
    assert_eq!(recoil.prepare(&names)?.impulses.len(), 19);
    // Reruns overwrite interrupted output directly, without freshness state.
    let before = fs::read(output.path().join(arte::PATH))?;
    fs::write(output.path().join(arte::PATH), b"interrupted cook")?;
    cook_tables(
        &source::Source::open(&installation, Language::English)?,
        output.path(),
    )?;
    assert_eq!(fs::read(output.path().join(arte::PATH))?, before);
    for language in [
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
    ] {
        cook_tables(
            &source::Source::open(&installation, language)?,
            output.path(),
        )?;
        let translated: items::Document = json(output.path(), items::PATH)?;
        translated.prepare()?;
        assert_ne!(
            translated.items[translated.mappings.target(1)?].text,
            items.items[items.mappings.target(1)?].text
        );
        let techniques: arte::document::Document = json(output.path(), arte::PATH)?;
        techniques.prepare()?;
        let ex: resonance_content::ex_skills::Document =
            json(output.path(), resonance_content::ex_skills::PATH)?;
        ex.prepare(&translated.mappings)?.validate_rules(531)?;
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optionally RESONANCE_TEST_STEAM_FIELD"]
fn installed_field_sources_convert() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let language = std::env::var("RESONANCE_TEST_STEAM_LANGUAGE")
        .ok()
        .map(|value| value.parse::<Language>())
        .transpose()?
        .unwrap_or_default();
    let source = source::Source::open(&installation, language)?;
    let output = tempfile::tempdir()?;
    field_directory::cook(&source, output.path())?;
    let selected = std::env::var("RESONANCE_TEST_STEAM_FIELD")
        .ok()
        .map(|id| id.parse::<u32>())
        .transpose()?;
    field_core::cook(&source, output.path(), selected)?;
    let fields: Mappings = json(output.path(), resonance_content::library::FIELDS)?;
    for (id, name) in fields
        .iter()
        .filter(|(id, _)| selected.is_none_or(|only| only == *id))
    {
        let base = output.path().join("fields").join(name);
        let bytes = fs::read(base.join("scripts/scenario.ssb"))?;
        symphonia_script::Program::decode(&bytes).with_context(|| format!("cooked field {id}"))?;
        if base.join("messages.json").exists() {
            let _: Vec<symphonia_script::message::Message> =
                serde_json::from_slice(&fs::read(base.join("messages.json"))?)?;
        }
        if base.join("collision.glb").exists() {
            resonance_content::field::collision_mesh::decode(&fs::read(
                base.join("collision.glb"),
            )?)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optionally RESONANCE_TEST_STEAM_FIELD"]
fn installed_scenery_and_skinned_body_use_shared_assets() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let output = tempfile::tempdir()?;
    field_directory::cook(&source, output.path())?;
    field_settings::cook(&source, output.path())?;
    let selected = std::env::var("RESONANCE_TEST_STEAM_FIELD")
        .ok()
        .map(|id| id.parse::<u32>())
        .transpose()?
        .unwrap_or(332);
    field_scene::cook(&source, output.path(), Some(selected))?;
    let names: Mappings = json(output.path(), resonance_content::library::FIELDS)?;
    let scene: resonance_content::field::document::Scene = json(
        output.path(),
        &format!("fields/{}/scene.json", names.target(selected)?),
    )?;
    assert!(!scene.parts.is_empty());
    for mut part in scene.parts {
        let bytes = fs::read(output.path().join(&part.mesh))?;
        part.prepare_bones(&bytes)?;
        assert!(!part.bone_names.is_empty());
        assert!(!part.materials.is_empty());
        let skeleton = resonance_content::animation::Skeleton::from_glb(&bytes)?;
        for clip in &part.clips {
            let motion =
                clip.decode_for_skeleton(&fs::read(output.path().join(&clip.motion))?, &skeleton)?;
            motion.validate(&skeleton)?;
            resonance_content::animation::pose::sample(
                &skeleton,
                &motion,
                motion.duration_frames * 0.5,
            )?;
        }
        for image in part.textures {
            resonance_asset_writer::texture::ktx2(&fs::read(output.path().join(image))?)?;
        }
    }
    for path in scene.camera_tracks.values() {
        let camera = resonance_content::camera_motion::CameraMotion::decode(&fs::read(
            output.path().join(path),
        )?)?;
        camera.sample(0.)?;
    }
    // The same converter must retain weighted joints for party bodies.
    let image = pe::Image::parse(&source.executable)?;
    let name = image.text(pe::word(image.slice(0x8cfe80, 4)?, 0)?)?;
    let bytes = source.require(name)?;
    let package = package::Package::parse(&bytes)?;
    let descriptor = package.reference(0)?.context("party package has no body")?;
    let (part, native) = model::cook(
        &source,
        output.path(),
        descriptor,
        "characters/lloyd",
        "body",
    )?;
    assert!(native.meshes.iter().any(|mesh| mesh.joints.is_some()));
    let bytes = fs::read(output.path().join(part.mesh))?;
    let skeleton = resonance_content::animation::Skeleton::from_glb(&bytes)?;
    skeleton.bind_pose()?;
    assert!(skeleton.bones.len() > 10);
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optionally RESONANCE_TEST_STEAM_CHARACTER"]
fn installed_party_models_and_motions_use_shared_reader() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let output = tempfile::tempdir()?;
    let selected = std::env::var("RESONANCE_TEST_STEAM_CHARACTER")
        .ok()
        .map(|id| id.parse::<u8>())
        .transpose()?;
    character_models::cook(&source, output.path(), selected)?;
    let mappings: resonance_content::field::document::ModelMappings =
        json(output.path(), resonance_content::field::document::MODELS)?;
    assert_eq!(mappings.0.len(), if selected.is_some() { 7 } else { 63 });
    let owners = mappings
        .0
        .values()
        .map(|target| target.model())
        .collect::<std::collections::BTreeSet<_>>();
    let mut clips = 0;
    for owner in owners {
        let mut model: resonance_content::field::document::Model =
            json(output.path(), &format!("meshes/{owner}/model.json"))?;
        model.validate()?;
        for part in &mut model.parts {
            let bytes = fs::read(output.path().join(&part.mesh))?;
            part.prepare_bones(&bytes)?;
            let skeleton = resonance_content::animation::Skeleton::from_glb(&bytes)?;
            assert!(!part.clips.is_empty());
            for clip in &part.clips {
                let motion = clip
                    .decode_for_skeleton(&fs::read(output.path().join(&clip.motion))?, &skeleton)?;
                motion.validate(&skeleton)?;
                for frame in [0., motion.duration_frames * 0.5, motion.duration_frames] {
                    resonance_content::animation::pose::sample(&skeleton, &motion, frame)?;
                }
                clips += 1;
            }
        }
        model.hidden_nodes(&model.hidden_geometry)?;
        assert!(model.parts.iter().any(|p| p.outline_color.is_some()));
    }
    assert!(clips > 30);
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM"]
fn installed_field_settings_use_shared_contract() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let output = tempfile::tempdir()?;
    for language in [
        Language::English,
        Language::French,
        Language::Spanish,
        Language::German,
        Language::Italian,
    ] {
        let source = source::Source::open(&installation, language)?;
        field_settings::cook(&source, output.path())?;
        let settings: resonance_content::field::settings::Settings =
            json(output.path(), resonance_content::field::settings::PATH)?;
        settings.effects.validate()?;
        settings.blink.validate()?;
        settings.contact_shadow.validate()?;
        assert!(!settings.save_point.tutorial.0.is_empty());
        assert!(!settings.save_point.no_gem.0.is_empty());
        for path in [
            &settings.toon_ramp,
            &settings.contact_shadow.texture,
            &settings.effects.emote_texture,
            &settings.effects.status_texture,
        ] {
            resonance_asset_writer::texture::ktx2(&fs::read(output.path().join(path))?)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM"]
fn installed_global_actor_models_use_shared_documents() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let output = tempfile::tempdir()?;
    character_models::cook(&source, output.path(), None)?;
    let models = field_models::cook(&source, output.path())?;
    assert!(models.mappings.0.len() > 200);
    for resource in [131072, 131073, 131074] {
        let path = models.mappings.model_path(resource)?;
        let mut model: resonance_content::field::document::Model = json(output.path(), &path)?;
        model.validate()?;
        for part in &mut model.parts {
            let bytes = fs::read(output.path().join(&part.mesh))?;
            part.prepare_bones(&bytes)?;
            let skeleton = resonance_content::animation::Skeleton::from_glb(&bytes)?;
            for clip in &part.clips {
                clip.decode_for_skeleton(&fs::read(output.path().join(&clip.motion))?, &skeleton)?
                    .validate(&skeleton)?;
            }
        }
        model.hidden_nodes(&model.hidden_geometry)?;
        if let Some(path) = model.collision {
            resonance_content::field::collision_mesh::decode_model(&fs::read(
                output.path().join(path),
            )?)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_LIBRARY pointing to a cooked Steam library"]
fn cooked_field_actor_documents_assemble() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let selected = std::env::var("RESONANCE_TEST_STEAM_FIELD")
        .ok()
        .map(|id| id.parse::<u32>())
        .transpose()?
        .unwrap_or(332);
    let fields: Mappings = json(&root, resonance_content::library::FIELDS)?;
    let path = format!("fields/{}/field.json", fields.target(selected)?);
    let definition: resonance_content::field::document::Definition = json(&root, &path)?;
    definition.validate()?;
    let assets = definition.assemble(selected, |path| {
        Ok(std::sync::Arc::from(fs::read(root.join(path))?))
    })?;
    assert!(assets.actors.len() >= 63);
    assert!(assets.actors.iter().any(|actor| actor.resource == 1));
    assert!(
        assets
            .actors
            .iter()
            .any(|actor| actor.resource > 9 && actor.resource >> 24 != 0x23)
    );
    assert!(
        assets
            .actors
            .iter()
            .flat_map(|actor| &actor.parts)
            .flat_map(|part| &part.clips)
            .any(|clip| clip.animation_resource.is_some())
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; writes the actor category into that library"]
fn installed_field_actors_cook() -> Result<()> {
    let installation = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
    );
    let root = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let source = source::Source::open(&installation, Language::English)?;
    let resources = resources::Resources::read(&source)?;
    let mut mappings: resonance_content::field::document::ModelMappings =
        json(&root, resonance_content::field::document::MODELS)?;
    // Start from the party/global model passes after an interrupted actor cook.
    mappings.0.retain(|id, _| {
        (1..=9).contains(id)
            || matches!(id >> 24, 0x23 | 0x24)
            || (*id >= 0x10000 && resources.entries.contains_key(id))
    });
    let mut sources = BTreeMap::new();
    for &resource in mappings.0.keys() {
        let id = if resource >> 24 == 0x24 {
            resource & 0xffff
        } else {
            resource
        };
        if let Ok(name) = resources.loaded(id) {
            sources.insert(resource, name.to_owned());
        }
    }
    let standalone = json(&root, "meshes/standalone/mappings.json")?;
    let mut models = field_models::Models {
        resources,
        mappings,
        standalone,
        sources,
    };
    let selected = std::env::var("RESONANCE_TEST_STEAM_FIELD")
        .ok()
        .map(|id| id.parse::<u32>())
        .transpose()?;
    field_actors::cook(&source, &root, &mut models, selected)
}
