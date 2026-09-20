//! Bind complete physical visuals into the runtime catalogue without conversion.
use super::all::{Asset, Cooked, Visual};
use crate::battle::all::{Archive, Sources};
use anyhow::{Result, ensure};
use resonance_content::{
    battle::visual::{ArenaVisuals, ModelVisuals, MotionSlot, TrailStyle},
    menu_data::Costume,
    model_preview::ModelPreview,
};
#[cfg(test)]
use std::fs;
use std::{collections::BTreeMap, path::Path};
mod effects;
mod party;
pub(super) use effects::effect_packages;
pub(super) use party::{party, party_metadata, pow_weapons, weapons};

pub(in crate::battle) fn require_null_party_motion(
    output: &Path,
    disc: u8,
    sources: &Sources,
    character: u8,
    slot: u16,
) -> Result<()> {
    ensure!(
        (1..=9).contains(&character),
        "invalid party character {character}"
    );
    let costume = Costume::Standard;
    let name = format!("party-{character}-costume-{}", costume as u8);
    let Visual::Party(model) = Directory::open(output, disc, &sources.usual)?
        .read(Asset::Party { character, costume }, &name)?
    else {
        anyhow::bail!("cooked {name} contains another visual kind");
    };
    ensure!(
        matches!(model.visual.motion_slot(slot)?, MotionSlot::AuthoredNull),
        "party {character} standard costume requires authored null motion {slot}"
    );
    Ok(())
}

pub(super) fn arenas(
    root: &Path,
    disc: u8,
    sources: &Sources,
    ids: &[u16],
) -> Result<BTreeMap<u16, ArenaVisuals>> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let directory = Directory::open(root, disc, sources.archive(Archive::Arena))?;
    ids.iter()
        .map(|&id| {
            let Visual::Arena(arena) = directory.read(Asset::Arena(id), &format!("arena-{id}"))?
            else {
                anyhow::bail!("cooked arena {id} contains another visual kind");
            };
            arena.validate()?;
            Ok((id, arena))
        })
        .collect()
}

pub(super) fn enemies(
    root: &Path,
    disc: u8,
    sources: &Sources,
    ids: &[u8],
) -> Result<BTreeMap<u8, ModelVisuals>> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let directory = Directory::open(root, disc, &sources.enemy)?;
    ids.iter()
        .map(|&id| Ok((id, enemy(&directory, id)?)))
        .collect()
}

fn enemy(directory: &Directory<'_>, id: u8) -> Result<ModelVisuals> {
    let Visual::Enemy(enemy) = directory.read(Asset::Enemy(id), &format!("enemy-{id}"))? else {
        anyhow::bail!("cooked enemy {id} contains another visual kind");
    };
    Ok(enemy)
}

pub(super) fn textures(root: &Path, disc: u8, sources: &Sources) -> Result<(String, String)> {
    let directory = Directory::open(root, disc, &sources.usual)?;
    let texture = |asset, name| {
        let Visual::Texture(path) = directory.read(asset, name)? else {
            anyhow::bail!("cooked {name} contains another visual kind");
        };
        Ok(path)
    };
    Ok((
        texture(Asset::ToonRamp, "toon-ramp")?,
        texture(Asset::Shadow, "shadow")?,
    ))
}

pub(in crate::battle) use crate::cooked::Source as Directory;

impl Directory<'_> {
    fn read(&self, asset: Asset, name: &str) -> Result<Visual> {
        let (directories, bytes) = self.candidates(&format!("battle/all/visuals/{name}.json"))?;
        let directory = directories[0];
        let mut cooked: Cooked = serde_json::from_slice(&bytes)?;
        ensure!(
            cooked.asset == asset && cooked.dependencies == asset.dependencies(),
            "wrong cooked {name} identity or dependencies"
        );
        ensure!(
            cooked.output_paths == cooked.visual.output_paths()?,
            "incomplete cooked {name} file inventory"
        );
        for path in &cooked.output_paths {
            self.verify_file(&directories, path)?;
        }
        match &mut cooked.visual {
            Visual::Arena(visual) => Self::model(directory, &mut visual.model),
            Visual::Party(visual) => Self::actor(directory, &mut visual.visual),
            Visual::Enemy(visual) => Self::actor(directory, visual),
            Visual::Weapon(visual) => {
                for model in visual.slots.values_mut() {
                    Self::model(directory, model);
                }
                for trail in visual.trails.values_mut() {
                    Self::trail(directory, &mut trail.style);
                }
            }
            Visual::EffectModel(visual) => Self::model(directory, &mut visual.model),
            Visual::PackageModel(visual) => Self::model(directory, &mut visual.model),
            Visual::Texture(path) => Self::path(directory, path),
            Visual::EnemyAppearance { .. } | Visual::WeaponMotions(_) => {}
        }
        Ok(cooked.visual)
    }

    fn path(directory: &str, path: &mut String) {
        *path = format!("{directory}/{path}");
    }

    fn model(directory: &str, model: &mut ModelPreview) {
        for part in &mut model.parts {
            for path in std::iter::once(&mut part.scene.mesh).chain(&mut part.scene.textures) {
                Self::path(directory, path);
            }
        }
    }

    fn actor(directory: &str, actor: &mut ModelVisuals) {
        Self::model(directory, &mut actor.model);
        for style in actor
            .trails
            .values_mut()
            .map(|trail| &mut trail.style)
            .chain(
                actor
                    .attachments
                    .values_mut()
                    .filter_map(|attachment| attachment.trail_style.as_mut()),
            )
        {
            Self::trail(directory, style);
        }
    }

    fn trail(directory: &str, style: &mut TrailStyle) {
        for texture in style.textures.values_mut().flatten() {
            Self::path(directory, &mut texture.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{
        SceneClip, ScenePart,
        model_preview::{ModelPreview, PreviewPart},
    };

    fn rebase(value: &mut serde_json::Value, paths: &BTreeMap<&str, String>) {
        match value {
            serde_json::Value::String(value) => {
                if let Some(path) = paths.get(value.as_str()) {
                    *value = path.clone();
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    rebase(value, paths);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    rebase(value, paths);
                }
            }
            _ => {}
        }
    }

    pub(super) fn assert_bound(
        directory: &Directory<'_>,
        name: &str,
        visual: &Visual,
    ) -> Result<()> {
        let (namespace, bytes) = directory.resolve(&format!("battle/all/visuals/{name}.json"))?;
        let cooked: Cooked = serde_json::from_slice(&bytes)?;
        let paths = cooked
            .output_paths
            .iter()
            .map(|path| (path.as_str(), format!("{namespace}/{path}")))
            .collect();
        let mut expected = serde_json::to_value(&cooked.visual)?;
        rebase(&mut expected, &paths);
        assert_eq!(serde_json::to_value(visual)?, expected, "{name}");
        Ok(())
    }

    #[test]
    fn binds_renamed_declared_arena_and_rejects_missing_or_wrong_assets() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("resonance-arena-binding"));
        fs::create_dir(&root)?;
        let result = (|| -> Result<()> {
            let sources = Sources::fixture(&root.join("extracted"))?;
            let source = [
                (
                    format!("disc1/{}", sources.archive(Archive::Arena)),
                    vec!["assets/other", "assets/arena"],
                ),
                (
                    format!("disc2/{}", sources.archive(Archive::Arena)),
                    vec!["assets/arena", "assets/other"],
                ),
            ]
            .into_iter()
            .collect::<BTreeMap<_, _>>();
            crate::write_atomic(&root.join("sources.json"), &serde_json::to_vec(&source)?)?;
            let mesh = "battle/arenas/1/body.glb";
            let texture = "battle/arenas/1/body.ktx2";
            let mut cooked = Cooked {
                asset: Asset::Arena(1),
                visual: Visual::Arena(ArenaVisuals {
                    source_sha256: "a".repeat(64),
                    model: ModelPreview {
                        scale: 1.,
                        elevation: 0.,
                        hidden_geometry: vec![],
                        node_scales: vec![],
                        parts: vec![PreviewPart {
                            animation: None,
                            scene: ScenePart {
                                resource: 0,
                                mesh: mesh.into(),
                                textures: vec![texture.into()],
                                materials: vec![],
                                appearance: None,
                                translation: [0.; 3],
                                clips: vec![SceneClip {
                                    resource_slot: 0,
                                    duration_seconds: 2.,
                                    animation_resource: None,
                                    secondary_pose_nodes: vec![],
                                }],
                                autoplay: true,
                                texture_animations: vec![],
                                bone_names: vec![],
                                material_nodes: vec![],
                                outline_color: None,
                                secondary_motion: Default::default(),
                            },
                            attached_to: None,
                            additive: false,
                            uv_offsets: vec![],
                        }],
                    },
                    yaw_degrees: 120.,
                    camera_pitch_offset: 0.,
                    translation: [0.; 3],
                    animation_rates: vec![0.5],
                    uv_channels: vec![vec![]],
                    ambient: [255; 4],
                    actor_ambient: Some([255; 3]),
                    light_position: [0., 100., 0.],
                }),
                output_paths: vec![mesh.into(), texture.into()],
                dependencies: vec![],
            };
            let directory = root.join("assets/arena");
            let record = directory.join("battle/all/visuals/arena-1.json");
            crate::write_atomic(&record, &serde_json::to_vec(&cooked)?)?;
            crate::write_atomic(&directory.join(mesh), b"existing mesh")?;
            crate::write_atomic(&directory.join(texture), b"existing texture")?;
            cooked.asset = Asset::Arena(2);
            crate::write_atomic(
                &root.join("assets/other/battle/all/visuals/arena-2.json"),
                &serde_json::to_vec(&cooked)?,
            )?;
            crate::write_atomic(&root.join("assets/other").join(mesh), b"other mesh")?;
            crate::write_atomic(&root.join("assets/other").join(texture), b"other texture")?;
            cooked.asset = Asset::Arena(1);
            for disc in [1, 2] {
                let bound = arenas(&root, disc, &sources, &[1, 2])?;
                let arena = &bound[&1];
                assert_eq!(
                    bound[&2].model.parts[0].scene.mesh,
                    format!("assets/other/{mesh}")
                );
                assert_eq!(
                    arena.model.parts[0].scene.mesh,
                    format!("assets/arena/{mesh}")
                );
                assert_eq!(
                    arena.model.parts[0].scene.textures,
                    [format!("assets/arena/{texture}")]
                );
                assert_eq!(arena.model.parts[0].scene.clips[0].duration_seconds, 2.);
                assert_eq!(arena.animation_rates, [0.5]);
            }
            fs::remove_file(directory.join(texture))?;
            assert!(arenas(&root, 2, &sources, &[1]).is_err());
            crate::write_atomic(&directory.join(texture), b"existing texture")?;
            let duplicate = root.join("assets/other/battle/all/visuals/arena-1.json");
            crate::write_atomic(&duplicate, &serde_json::to_vec(&cooked)?)?;
            assert!(
                arenas(&root, 2, &sources, &[1])
                    .unwrap_err()
                    .to_string()
                    .contains("conflicting")
            );
            crate::write_atomic(&root.join("assets/other").join(mesh), b"existing mesh")?;
            crate::write_atomic(
                &root.join("assets/other").join(texture),
                b"existing texture",
            )?;
            assert!(arenas(&root, 2, &sources, &[1]).is_ok());
            cooked.asset = Asset::Arena(2);
            crate::write_atomic(&duplicate, &serde_json::to_vec(&cooked)?)?;
            assert!(
                arenas(&root, 2, &sources, &[1])
                    .unwrap_err()
                    .to_string()
                    .contains("conflicting")
            );
            fs::remove_file(duplicate)?;
            cooked.asset = Asset::Arena(2);
            crate::write_atomic(&record, &serde_json::to_vec(&cooked)?)?;
            assert!(arenas(&root, 2, &sources, &[1]).is_err());
            cooked.asset = Asset::Arena(1);
            cooked.visual = Visual::Texture(texture.into());
            crate::write_atomic(&record, &serde_json::to_vec(&cooked)?)?;
            assert!(arenas(&root, 2, &sources, &[1]).is_err());
            fs::remove_file(record)?;
            assert!(arenas(&root, 2, &sources, &[1]).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "requires cook-all arenas and original offset tables; no conversion or codecs"]
    fn original_cooked_arena_library_binds_both_discs() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        for disc in [1, 2] {
            let files = local.join(format!("extracted/disc{disc}/files"));
            let sources = Sources::read(files.parent().unwrap())?;
            let rel = fs::read(files.join("US_r_Top2Btl.rel"))?;
            let data = super::super::rel_data(&rel)?;
            let offsets = (0..98)
                .map(|id| crate::read::u32(data, 0x3b90 + id * 4))
                .collect::<Result<Vec<_>>>()?;
            let ids: Vec<_> = crate::battle::all::physical_ranges(
                &offsets,
                0,
                files
                    .join(sources.archive(Archive::Arena))
                    .metadata()?
                    .len(),
            )?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
            assert_eq!(ids.len(), 86);
            let bound = arenas(&local.join("all-assets"), disc, &sources, &ids)?;
            assert_eq!(bound.keys().copied().collect::<Vec<_>>(), ids);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires cook-all enemy visuals and original offset tables; no conversion or codecs"]
    fn original_cooked_enemy_library_preserves_bodies_motions_and_appearances() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let root = local.join("all-assets");
        for disc in [1, 2] {
            let files = local.join(format!("extracted/disc{disc}/files"));
            let sources = Sources::read(files.parent().unwrap())?;
            let usual = fs::read(files.join(&sources.usual))?;
            let offsets = crate::battle::actions::member(&usual, 10)?
                .chunks_exact(4)
                .map(|bytes| crate::read::u32(bytes, 0))
                .collect::<Result<Vec<_>>>()?;
            let ranges = crate::battle::all::physical_ranges(
                &offsets,
                0,
                files.join(&sources.enemy).metadata()?.len(),
            )?;
            let directory = Directory::open(&root, disc, &sources.enemy)?;
            let mut totals = [0; 6];
            // Stream one complete body at a time; animation JSON dominates corpus memory.
            for (id, _) in ranges {
                let id = u8::try_from(id)?;
                let (namespace, bytes) =
                    directory.resolve(&format!("battle/all/visuals/enemy-{id}.json"))?;
                let cooked: Cooked = serde_json::from_slice(&bytes)?;
                let Visual::Enemy(visual) = &cooked.visual else {
                    anyhow::bail!("wrong enemy visual");
                };
                totals[0] += 1;
                totals[1] += visual.rig.motions.len();
                totals[2] += usize::from(visual.paired_body.is_some());
                totals[3] += visual.attachments.len();
                totals[4] += visual.trails.len();
                if let Some(layer) = &visual.variant_texture {
                    for row in 0..layer.frames {
                        let (_, bytes) = directory.resolve(&format!(
                            "battle/all/visuals/enemy-{id}-appearance-{row}.json"
                        ))?;
                        let appearance: Cooked = serde_json::from_slice(&bytes)?;
                        assert_eq!(
                            appearance.asset,
                            Asset::EnemyAppearance { monster: id, row }
                        );
                        assert_eq!(appearance.dependencies, [Asset::Enemy(id)]);
                        assert!(appearance.output_paths.is_empty());
                        assert!(matches!(appearance.visual,
                            Visual::EnemyAppearance { texture, rows, row: selected }
                            if texture == layer.texture && rows == layer.frames && selected == row));
                        totals[5] += 1;
                    }
                }
                let paths = cooked
                    .output_paths
                    .iter()
                    .map(|path| (path.as_str(), format!("{namespace}/{path}")))
                    .collect();
                let mut expected = serde_json::to_value(&cooked.visual)?;
                rebase(&mut expected, &paths);
                let bound = enemies(&root, disc, &sources, &[id])?.remove(&id).unwrap();
                assert_eq!(
                    serde_json::to_value(Visual::Enemy(bound))?,
                    expected,
                    "enemy {id} disc {disc}"
                );
            }
            assert_eq!(totals, [251, 4582, 3, 196, 20, 40]);
        }
        Ok(())
    }
}
