//! Resident effects use the same programs, RGBA atlases and models as GC.
use super::*;
use crate::package::Package;
use resonance_content::{
    battle_effect::{self, Tints},
    model_preview::PreviewPart,
};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let atlases = effect_atlas::Directory::read(&image)?;
    let residents = effect_atlas::residents(source, &image)?;
    let bytes = source.require("BTLusual.pac")?;
    let package = Package::parse(&bytes)?;
    let directory = Package::parse(
        package
            .member(6)?
            .context("missing resident effect models")?,
    )?;
    ensure!(
        directory.len() == 13,
        "resident effect model directory changed"
    );
    let mut models = BTreeMap::new();
    for index in 0..directory.len() {
        let slot = u8::try_from(index + 1)?;
        let name = directory
            .reference(index)?
            .context("missing resident effect model")?;
        let (mut scene, _) = model::cook(
            source,
            library,
            name,
            &format!("battle/effects/common/models/model-{slot}"),
            "body",
        )?;
        scene.autoplay = false;
        models.insert(
            slot,
            Model {
                attachment_nodes: Default::default(),
                independent_layers: Default::default(),
                layers: vec![PreviewPart {
                    scene,
                    animation: None,
                    attached_to: None,
                    additive: false,
                    uv_offsets: Vec::new(),
                }],
            },
        );
    }
    for (member, name) in [(2, "common"), (3, "techniques")] {
        let bank = package
            .member(member)?
            .context("missing resident effect bank")?;
        let include: &[u8] = if member == 2 { &[19] } else { &[] };
        let mut definition = decode_including(bank, &[], include, |stride, declaration| {
            if let Declaration::Particle { visual, .. } = declaration
                && let ParticleTexture::Atlas { palette_set, .. } = &mut visual.texture
            {
                *palette_set = stride;
            }
            Ok(())
        })
        .with_context(|| format!("resident {name} effect programs"))?;
        let owner = format!("battle/effects/{name}");
        for (index, program) in definition.programs.iter().enumerate() {
            for event in program {
                if let EffectOperation::Unsupported { reason } = &event.operation {
                    eprintln!("  Unavailable resident {name} program {index}: {reason}");
                }
            }
        }
        for (name, declaration) in definition.template_names.iter().zip(&definition.templates) {
            if let Declaration::Unsupported { reason } = declaration {
                eprintln!("  Unavailable resident template {name}: {reason}");
            }
        }
        let slots = definition
            .templates
            .iter()
            .filter_map(|declaration| declaration.particle_visual().ok()?.texture.slot())
            .collect::<BTreeSet<_>>();
        let mut textures = BTreeMap::new();
        for slot in slots {
            let resource = residents
                .get(&slot)
                .with_context(|| format!("unbound resident effect atlas {slot}"))?;
            let texture = effect_atlas::cook(
                source,
                library,
                &mut definition,
                resource,
                atlases.get(resource),
                &owner,
                slot,
            )?;
            textures.insert(slot, vec![texture]);
        }
        let images = textures
            .values()
            .flatten()
            .map(|texture| texture.images.len())
            .sum::<usize>();
        definition.artwork = Some(Artwork::from_models(textures, models.clone())?);
        output::json(&library.join(&owner).join("effects.json"), &definition)?;
        eprintln!(
            "  Resident {name} effects: {} programs, {} templates, {images} images",
            definition.programs.len(),
            definition.templates.len()
        );
    }
    // GC feedback colors are the intended rendering style. Contact program
    // identities still come from the Steam executable's elemental lookup.
    for (address, bytes) in [
        (0x465647, &[0x8a, 0x82, 0x44, 0x91, 0x91, 0][..]),
        (0x465748, &[0x8a, 0x80, 0x44, 0x91, 0x91, 0]),
    ] {
        ensure!(
            image.slice(address, bytes.len())? == bytes,
            "contact feedback table consumer changed"
        );
    }
    let mut tints: Tints = serde_json::from_str(include_str!("tints.json"))?;
    tints.contact_effects = image.slice(0x919144, 10)?.try_into()?;
    let common = package.member(2)?.context("missing common effect bank")?;
    ensure!(
        tints
            .contact_effects
            .iter()
            .all(|&member| member < common[4]),
        "contact feedback selects an absent common program"
    );
    output::json(&library.join(battle_effect::TINTS_PATH), &tints)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires RESONANCE_TEST_EXTRACTED; compares the intended feedback style with the current GC importer"]
    fn resident_feedback_style_matches_the_gc_importer() -> Result<()> {
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_EXTRACTED").context("set RESONANCE_TEST_EXTRACTED")?,
        );
        let output = tempfile::tempdir()?;
        resonance_importer::cook_battle_effect_tints(&root, output.path())?;
        let expected: serde_json::Value = serde_json::from_slice(&std::fs::read(
            output.path().join(battle_effect::TINTS_PATH),
        )?)?;
        let actual: serde_json::Value = serde_json::from_str(include_str!("tints.json"))?;
        assert_eq!(actual, expected);
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks and loads resident effect banks"]
    fn installed_resident_effects_cook_and_load_through_shared_reader() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let library = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &library)?;
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        for path in [
            battle_effect::document::COMMON,
            battle_effect::document::TECHNIQUES,
        ] {
            let bank = battle_effect::document::read(path, &diagnostics, |path| {
                Ok(std::fs::read(library.join(path))?.into())
            })?;
            let art = bank.art.context("missing resident artwork")?;
            ensure!(art.models.len() == 13, "missing resident effect models");
            for declaration in &bank.actors {
                if let Ok(visual) = declaration.particle_visual()
                    && let ParticleTexture::Atlas {
                        slot,
                        palette_set,
                        dual,
                    } = visual.texture
                {
                    ensure!(!dual, "resident effect retains separate alpha image");
                    let texture = art
                        .textures
                        .get(&slot)
                        .and_then(|bank| bank.first())
                        .context("missing resident atlas")?;
                    for selector in 0..=255 {
                        texture.palette_image(palette_set, selector)?;
                    }
                }
            }
            if path == battle_effect::document::COMMON {
                ensure!(
                    !bank.programs.iter().flatten().any(|event| matches!(
                        event.operation,
                        EffectOperation::Unsupported { .. }
                    )),
                    "common feedback contains an unavailable program"
                );
                ensure!(
                    !bank
                        .actors
                        .iter()
                        .any(|actor| matches!(actor, Declaration::Unsupported { .. })),
                    "common feedback contains an unavailable template"
                );
                ensure!(
                    bank.template_names
                        .iter()
                        .any(|name| name == battle_effect::document::STUN_BIRD),
                    "missing common stun bird template"
                );
            }
        }
        Ok(())
    }
}
