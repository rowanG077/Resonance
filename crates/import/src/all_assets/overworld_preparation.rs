//! Bind world terrain and VM data from the same decoded packages as physical cooking.
use super::overworld_encounters::tiles::{TileAsset, TileCatalogue};
use super::{overworld_collision, overworld_encounters, overworld_landmarks, overworld_movement};
use crate::{
    scene::{decoded, source::Models},
    write_atomic,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{ScriptAsset, overworld::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(super) struct Plan {
    pub dependencies: BTreeSet<String>,
    pub visuals: super::overworld_visuals::Sources,
    tiles: TileCatalogue,
    movement: MovementParameters,
    collision: CollisionTables,
    encounters: EncounterTables,
    landmarks: Landmarks,
    guideposts: Vec<Guidepost>,
    pub(super) script: String,
}
impl Plan {
    pub(super) fn terrain_sources(&self) -> BTreeMap<String, TileAsset> {
        self.tiles
            .worlds
            .iter()
            .flat_map(|world| &world.tiles)
            .flat_map(|tile| std::iter::once(&tile.base).chain(&tile.alternate))
            .map(|asset| (asset.package["assets/".len()..].to_owned(), asset.clone()))
            .collect()
    }
}

pub(super) fn discover(
    extracted: &Path,
    sources: &BTreeMap<String, String>,
    executable: &[u8],
) -> Result<Plan> {
    let module = extracted.join("files/US_r_Top2field.rel");
    let disc = crate::disc_number(extracted)?;
    let source_hashes: BTreeMap<_, _> = sources
        .iter()
        .filter_map(|(path, hash)| {
            path.strip_prefix(&format!("disc{disc}/"))
                .map(|path| (path.to_owned(), hash.clone()))
        })
        .collect();
    let tiles = overworld_encounters::tiles::read(&module, &source_hashes)?
        .context("world terrain catalogue missing")?;
    let world = super::world_map::read(executable)?;
    let controller = crate::rel::Rel::read(&module)?;
    let mut landmarks = overworld_landmarks::prepare(&world)?;
    overworld_landmarks::bind_camera_clearance(&mut landmarks, &controller)?;
    super::overworld_entries::bind(&mut landmarks, &controller)?;
    let map = crate::menu::world_map(
        &world,
        &crate::field_catalogue::read(executable)?,
        &super::inventory_ui::read(executable)?,
    )?;
    let script = crate::field_resources::resolve_path(&extracted.join("files"), "field_sil.so")?;
    let mut dependencies: BTreeSet<String> = tiles
        .worlds
        .iter()
        .flat_map(|world| &world.tiles)
        .flat_map(|tile| std::iter::once(&tile.base).chain(&tile.alternate))
        .map(|asset| {
            source_hashes
                .get(&asset.source)
                .cloned()
                .context("world terrain source missing")
        })
        .collect::<Result<_>>()?;
    let visuals = super::overworld_visuals::Sources::read(
        extracted,
        &crate::rel::Rel::read(&module)?,
        executable,
    )?;
    dependencies.extend(visuals.dependencies.keys().cloned());
    Ok(Plan {
        dependencies,
        visuals,
        movement: overworld_movement::read(&controller)?,
        collision: overworld_collision::prepare(&module)?,
        encounters: overworld_encounters::prepare(&module)?,
        landmarks,
        guideposts: super::field_unlocks::read(&module, &map.locations)?
            .context("world guideposts missing")?,
        tiles,
        script,
    })
}

pub(super) fn tile(
    output: &Path,
    asset: &TileAsset,
    decoded: &decoded::Package,
) -> Result<TileResources> {
    let source = decoded.source(&asset.source)?;
    let archive = if source.starts_with(b"MSCF") {
        crate::field::MapArchive::decode(&source)?
    } else {
        crate::field::MapArchive {
            sections: crate::field::sections(&source)?,
            bytes: (*source).clone(),
            source_sha256: crate::digest(&source),
            member: String::new(),
        }
    };
    ensure!(
        asset.package == format!("assets/{}", archive.source_sha256),
        "world source digest differs"
    );
    let mut models = Models::new(output, decoded);
    for index in [0, 2] {
        let Some(source) = archive.optional_section(index) else {
            continue;
        };
        let animation = archive
            .optional_section(index + 1)
            .map(|bytes| decoded.animation(bytes))
            .transpose()?;
        models.add(
            &format!("{}/{index}", asset.source),
            source,
            source,
            |geometry, _, part, glb| {
                part.resource = index as u16;
                for material in &mut part.materials {
                    material.depth_write = index == 0;
                }
                if index == 2 {
                    part.texture_animations = terrain_texture_animations(source)?;
                }
                if let Some(animation) = animation {
                    crate::scene::binding::bind_clip(
                        part,
                        glb,
                        geometry
                            .bindings
                            .as_ref()
                            .context("terrain animation has no skeleton")?,
                        &animation,
                        None,
                    )?;
                    part.autoplay = true;
                }
                Ok(())
            },
        )?;
    }
    Ok(TileResources {
        ground: crate::field::collision_data::Mesh::read(
            archive.section(4)?,
            crate::field::collision_data::Format::Detect,
        )?
        .groups,
        parts: models
            .finish()
            .into_iter()
            .map(|layer| layer.part)
            .collect(),
    })
}

/// Map model-header bytes 7..12 to native texture slots 8..13.
/// Scroll the first two and step the four shoreline atlases.
fn terrain_texture_animations(source: &[u8]) -> Result<Vec<resonance_content::TextureAnimation>> {
    let slots = source
        .get(0x13..0x19)
        .context("terrain texture slots missing")?;
    Ok(slots
        .iter()
        .enumerate()
        .filter_map(|(slot, &id)| {
            let texture = usize::from(id.checked_sub(1)?);
            let offsets = if slot < 2 {
                (0..1000).map(|tick| [tick as f32 * 0.001; 2]).collect()
            } else {
                (0..160)
                    .map(|tick| [0., (tick / 20) as f32 * 0.125])
                    .collect()
            };
            Some(resonance_content::TextureAnimation {
                texture,
                delay_ticks: 0,
                loop_start: 0,
                offsets,
            })
        })
        .collect())
}

pub(super) fn prepare(
    plan: &Plan,
    extracted: &Path,
    output: &Path,
    terrain: &BTreeMap<String, TileResources>,
    visuals: Visuals,
) -> Result<Package> {
    let source = fs::read(extracted.join("files").join(&plan.script))?;
    let script =
        crate::all_assets::decode_script(&source)?.context("invalid world event script")?;
    let directory = format!("assets/{}", crate::digest(&source));
    let script_path = format!("{directory}/script.ssb");
    let messages = format!("{directory}/messages.json");
    write_atomic(&output.join(&script_path), script.bytes)?;
    write_atomic(
        &output.join(&messages),
        &serde_json::to_vec(&script.messages)?,
    )?;
    let mut worlds: [Vec<TerrainTile>; 2] = Default::default();
    let tile = |asset: &TileAsset| {
        terrain
            .get(&asset.package["assets/".len()..])
            .cloned()
            .context("prepared world terrain missing")
    };
    for (index, world) in plan.tiles.worlds.iter().enumerate() {
        for entry in &world.tiles {
            worlds[index].push(TerrainTile {
                column: entry.column,
                row: entry.row,
                base: tile(&entry.base)?,
                alternate: entry.alternate.as_ref().map(tile).transpose()?,
            });
        }
    }
    let package = Package {
        version: PACKAGE_VERSION,
        script: ScriptAsset {
            path: script_path,
            sha256: crate::digest(script.bytes),
        },
        messages,
        movement: plan.movement.clone(),
        collision: plan.collision.clone(),
        encounters: plan.encounters.clone(),
        landmarks: plan.landmarks.clone(),
        guideposts: plan.guideposts.clone(),
        worlds,
        visuals,
    };
    package.validate_content(|path| output.join(path).is_file())?;
    Ok(package)
}

#[test]
#[ignore = "reads a complete current cook at RESONANCE_WORLD_ASSETS and original disc 1"]
fn original_world_cinematic_packages_bind_cameras_and_all_actor_motion() -> Result<()> {
    let output = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let library = resonance_content::library::Library::open(&output, &[])?;
    let mut snapshot = library.snapshot();
    let (definition, world) =
        resonance_content::overworld::document::capture(&mut snapshot, &|| false)?;
    let original = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/extracted/disc1/files/US_r_Top2field.rel");
    let (expected, _) = super::overworld_cinematics::read(&crate::rel::Rel::read(&original)?)?;
    assert_eq!(world.visuals.cinematics.len(), 14);
    let audio: resonance_content::field_audio::document::Definition = snapshot.json(
        &definition.audio,
        resonance_content::preparation::JSON_LIMIT,
    )?;
    let audio = audio.resolve(&mut snapshot)?;
    assert_eq!(world.visuals.cinematics[&517].dialogue.len(), 9);
    for line in &world.visuals.cinematics[&517].dialogue {
        let voice = &audio.voices[&line.voice];
        assert!(
            !snapshot
                .read(&voice.path, resonance_content::preparation::ASSET_LIMIT)?
                .is_empty()
        );
    }
    for id in [
        24, 25, 26, 133, 160, 177, 182, 183, 193, 217, 282, 437, 438, 443, 445,
    ] {
        assert!(audio.effects.contains_key(&id));
    }
    let files = resonance_content::prepared::Files::from_snapshot(
        snapshot,
        &mut Default::default(),
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    for (&id, scene) in &world.visuals.cinematics {
        assert_eq!(
            serde_json::to_value(&scene.presentation)?,
            serde_json::to_value(&expected[&id].presentation)?,
            "scene {id}"
        );
        assert_eq!(
            serde_json::to_value(&scene.dialogue)?,
            serde_json::to_value(&expected[&id].dialogue)?
        );
        assert_eq!(
            scene.actors.len(),
            [1, 1, 1, 9, 8, 8, 2, 2, 4, 7, 7, 7, 2, 2][usize::from(id - 513)]
        );
        assert_eq!(
            scene.camera.last().unwrap().time,
            [
                830., 150., 150., 120., 960., 100., 150., 470., 210., 180., 180., 180., 150., 510.
            ][usize::from(id - 513)]
        );
        for parts in scene.actors.values() {
            for part in parts {
                files.read(&part.mesh)?;
                for clip in &part.clips {
                    resonance_content::animation::Motion::decode(&files.read(&clip.motion)?)?
                        .validate_bones(part.bone_names.len())?;
                }
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires original disc 1; prepares three terrain shapes in private temporary output"]
fn original_world_terrain_shapes_bind_source_geometry() -> Result<()> {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let temporary = tempfile::tempdir()?;
    let output = temporary.path();
    let _publication = crate::publication::Session::start_if_needed(output)?;
    // Shoreline base, story alternate, and opaque-only land. The complete world
    // inventory and travel behaviors are covered by the world consumer checks.
    for (name, shoreline) in [
        ("FIELD/00.dat", true),
        ("FIELD/06a.dat", true),
        ("FIELD/18.dat", false),
    ] {
        let bytes = std::sync::Arc::new(fs::read(extracted.join("files").join(name))?);
        let asset = TileAsset {
            source: name.into(),
            package: format!("assets/{}", crate::digest(&bytes)),
        };
        let mut decoded =
            decoded::Package::cook(&bytes, &asset.package, output, super::geometry::Input::File)?;
        decoded.remember_source(name, std::sync::Arc::clone(&bytes));
        let prepared = tile(output, &asset, &decoded)?;
        let sections = crate::field::sections(&bytes)?;
        let collision = sections[4]
            .as_ref()
            .context("missing source terrain collision")?;
        assert!(!prepared.ground.is_empty(), "{name} has no collision");
        assert_eq!(
            serde_json::to_value(&prepared.ground)?,
            serde_json::to_value(crate::field::collision(&bytes[collision.clone()])?)?,
            "{name} collision"
        );
        let expected_layers: &[u16] = if shoreline { &[0, 2] } else { &[0] };
        assert_eq!(
            prepared
                .parts
                .iter()
                .map(|part| part.resource)
                .collect::<Vec<_>>(),
            expected_layers,
            "{name} layers"
        );
        for part in &prepared.parts {
            let index = usize::from(part.resource);
            let section = sections[index]
                .as_ref()
                .context("missing source terrain layer")?;
            let glb = crate::scene::glb::Glb::read(&output.join(&part.mesh))?;
            crate::geometry::compare_field_layer(&bytes[section.clone()], part, &glb.json, 0)
                .with_context(|| format!("{name}/{index}"))?;
            assert!(
                sections[index + 1].is_none(),
                "fixture has skeletal animation"
            );
            assert!(!part.autoplay && part.clips.is_empty(), "{name}/{index}");
            assert_eq!(
                !part.texture_animations.is_empty(),
                shoreline && index == 2,
                "{name}/{index} shoreline animation"
            );
            for animation in &part.texture_animations {
                animation.validate()?;
                assert!(animation.texture < part.textures.len());
            }
            for texture in &part.textures {
                assert!(!fs::read(output.join(texture))?.is_empty(), "{texture}");
            }
        }
    }
    Ok(())
}

#[test]
fn terrain_header_selects_scroll_and_shoreline_atlas_textures() -> Result<()> {
    let mut header = [0; 32];
    header[0x13..0x19].copy_from_slice(&[1, 0, 3, 0, 0, 0]);
    let animations = terrain_texture_animations(&header)?;
    assert_eq!(animations.len(), 2);
    assert_eq!(animations[0].texture, 0);
    assert_eq!(animations[1].texture, 2);
    assert_eq!(animations[0].offset(1000), [0.; 2]);
    assert_eq!(animations[1].offset(19), [0.; 2]);
    assert_eq!(animations[1].offset(20), [0., 0.125]);
    assert_eq!(animations[1].offset(160), [0.; 2]);
    Ok(())
}
