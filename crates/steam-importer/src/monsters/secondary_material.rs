//! Source-owned secondary effects retain their alpha, motions and layer count.
use super::*;
use std::{fs, path::PathBuf};

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM; optional RESONANCE_SECONDARY_OUTPUT writes selected model owners"]
fn installed_secondary_effect_bodies_keep_authored_layers_and_full_range_rgb() -> Result<()> {
    let source = Source::open(
        &PathBuf::from(
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
        ),
        crate::Language::English,
    )?;
    let image = pe::Image::parse(&source.executable)?;
    let temporary = tempfile::tempdir()?;
    let library = std::env::var_os("RESONANCE_SECONDARY_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_owned());
    let mut selected = Vec::new();
    for id in 0..count(&image)? as u16 {
        let bytes = source.require(package(&image, id)?)?;
        let metadata = metadata(&bytes)?;
        if metadata[0x130] & 1 != 0 || pe::half(metadata, 0xb8)? & 0x8400 == 0 {
            continue;
        }
        let descriptor =
            preview_model::reference(&bytes, 0x180)?.context("secondary body missing")?;
        let outline = preview_model::reference(&bytes, 0x198)?;
        let descriptor_bytes = source.require(&descriptor)?;
        let payload = crate::package::dependencies(&descriptor_bytes)?;
        let mut native =
            crate::model::Model::read(&descriptor_bytes, &source.require(&payload[0])?)?;
        // Undine's actual source water ribbon has neutral half-range vertices,
        // not a black texture or a second authored outline resource.
        if id == 195 {
            ensure!(outline.is_none(), "Undine now owns an outline");
            ensure!(
                native
                    .meshes
                    .iter()
                    .flat_map(|m| &m.colors)
                    .all(|c| c[..3] == [119. / 255., 128. / 255., 119. / 255.] && c[3] == 1.),
                "Undine source color contract changed"
            );
        }
        for mesh in &mut native.meshes {
            for color in &mut mesh.colors {
                for channel in &mut color[..3] {
                    *channel = (*channel * 2.).min(1.);
                }
            }
        }
        let stem = descriptor
            .rsplit_once('.')
            .context("descriptor extension missing")?
            .0;
        let (tints, _, _) = crate::model::material_parameters(
            &source.require(&format!("{stem}.mxd"))?,
            &native.materials,
        )?;
        let expected = native.glb(&tints)?;
        models_selected(&source, &library, Some(&[id]))?;
        let preview: resonance_content::model_preview::ModelPreview = serde_json::from_slice(
            &fs::read(library.join(format!("meshes/monsters/{}/model.json", symbol(id))))?,
        )?;
        let secondary = preview
            .parts
            .iter()
            .filter(|part| part.scene.mesh.contains("/secondary/"))
            .collect::<Vec<_>>();
        ensure!(
            secondary.len() == 1 + usize::from(outline.is_some()),
            "enemy{id} secondary lost or invented an authored layer"
        );
        ensure!(
            fs::read(library.join(&secondary[0].scene.mesh))? == expected,
            "enemy{id} secondary changed geometry, UVs, alpha or normalized RGB"
        );
        selected.push(id);
    }
    ensure!(selected.contains(&195), "Undine secondary fixture absent");
    if std::env::var_os("RESONANCE_SECONDARY_PUBLISH_ENEMIES").is_some() {
        // An opt-in private library supplies unchanged effect banks needed by
        // the shared action compiler. Rebuild inline artwork from the same
        // source preview rather than hand-editing its repeated layer fields.
        crate::battle_enemies::cook(&source, &library, Some(&selected))?;
    }
    eprintln!("Verified source secondary owners {selected:?}");
    Ok(())
}
