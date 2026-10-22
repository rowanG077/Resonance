//! Enemy texture banks and model templates.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_effect::{Art, SourceBank},
    battle_model::{ModelPart, enemy_effects_path},
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, path::Path};

pub(super) fn carried_motions(
    bytes: &[u8],
    actions: &resonance_content::battle_action::EnemyActions,
    source: &mut resonance_content::battle_model::Enemy,
    output: &Path,
) -> Result<Vec<String>> {
    use resonance_content::battle_action::{EnemyBehavior, EnemyEvent, EnemyExecution};
    let requested: std::collections::BTreeSet<_> = actions
        .rows
        .iter()
        .filter_map(|row| match row.execution.as_ref()? {
            EnemyExecution::Attack(attack) | EnemyExecution::Physical { attack, .. } => {
                Some(attack)
            }
            EnemyExecution::Spell(_) | EnemyExecution::LeaveArena => None,
        })
        .flat_map(|attack| &attack.events)
        .filter_map(|(_, event)| match event {
            EnemyEvent::CarriedPose { slot, clip, .. } => Some((*slot, *clip)),
            _ => None,
        })
        .collect();
    let mut slots = source
        .attachments
        .keys()
        .copied()
        .chain(requested.iter().map(|&(slot, _)| slot))
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(EnemyBehavior::CarriedAnimationRate { slot, .. }) = &actions.policy.behavior {
        slots.insert(*slot);
    }
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let mut decoded = crate::scene::decoded::Package::default();
    let mut paths = Vec::new();
    for slot in slots {
        let part = source
            .attachments
            .get_mut(&slot)
            .context("carried pose has no model")?;
        let package = members
            .model(0x160 + usize::from(slot) * 4)?
            .context("carried pose has no source model")?;
        let (_, resource) =
            crate::geometry::model_resource(crate::source_assets::section(package, 1)?)?;
        let model =
            crate::model::Model::parse(&resource[crate::geometry::skeleton_range(resource)?])?;
        let bindings = crate::animation::ModelBindings::from_model(&model);
        ensure!(
            bindings
                .names
                .as_ref()
                .is_some_and(|names| names.iter().eq(part
                    .rig
                    .skeleton
                    .bones
                    .iter()
                    .map(|bone| &bone.name))),
            "carried pose skeleton differs from its artwork"
        );
        let mut publish =
            |clip, animation: &crate::animation::AuthoredAnimation, initial| -> Result<()> {
                let motion = animation.motion(&bindings)?;
                ensure!(
                    !motion.tracks.is_empty(),
                    "carried pose has no bound tracks"
                );
                motion.validate(&part.rig.skeleton)?;
                let encoded = motion.encode()?;
                let path = format!("clips/{}.motion", crate::digest(&encoded));
                crate::write_atomic(&output.join(&path), &encoded)?;
                source.files.insert(
                    path.clone(),
                    File {
                        sha256: crate::digest(&encoded),
                        bytes: encoded.len() as u64,
                        roles: [Role::Data].into(),
                    },
                );
                let layer = part
                    .layers
                    .first_mut()
                    .context("missing carried model layer")?;
                layer
                    .scene
                    .clips
                    .retain(|value| value.resource_slot != clip);
                layer.scene.clips.push(resonance_content::SceneClip {
                    motion: path.clone(),
                    resource_slot: clip,
                    duration_seconds: motion.duration_frames
                        / resonance_content::animation::FRAME_HZ,
                    animation_resource: None,
                    secondary_pose_nodes: motion
                        .tracks
                        .iter()
                        .filter(|track| track.times.len() > 2)
                        .map(|track| track.bone)
                        .collect(),
                });
                layer.scene.clips.sort_by_key(|value| value.resource_slot);
                if initial {
                    layer.animation = Some(clip);
                }
                paths.push(path);
                Ok(())
            };
        if let Some(range) = crate::field::sections(package)?
            .get(3)
            .and_then(Clone::clone)
        {
            publish(0, &crate::animation::read_member(&package[range])?, true)?;
        }
        for &(_, clip) in requested.iter().filter(|(part, _)| *part == slot) {
            let field = 0x20 + usize::from(clip) * 4;
            ensure!(field < 0x160, "carried clip exceeds the enemy motion table");
            let animation = members
                .animation(field, &mut decoded)?
                .context("missing carried pose animation")?;
            publish(clip, &animation, false)?;
        }
    }
    Ok(paths)
}

pub(super) fn publish(
    bytes: &[u8],
    id: u8,
    body: &resonance_content::animation::Skeleton,
    shared: &mut crate::battle_effect::SharedPalettes,
    output: &Path,
) -> Result<BTreeMap<String, File>> {
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)
        .with_context(|| format!("enemy {id} resource pointer table"))?;
    let mut files = BTreeMap::new();
    let bank = members
        .model(0x1cc)
        .with_context(|| format!("enemy {id} effect member 0x1cc"))?;
    let (mut source, palette_strides) = match bank {
        Some(bank) => crate::battle_effect::read_enemy_with_palettes(bank, body)
            .with_context(|| format!("enemy {id} effect member 0x1cc"))?,
        // Artwork can be declared without effect commands. An empty bank lets
        // the existing descriptor retain those resources without inventing events.
        None => (
            SourceBank {
                source_sha256: crate::digest(bytes),
                programs: Vec::new(),
                actors: Vec::new(),
                art: None,
            },
            Vec::new(),
        ),
    };
    let mut textures = BTreeMap::new();
    // The package owns relative bank 2 and its separate slot 14. Runtime
    // preparation binds these to the selected enemy resource generation.
    for (field, slot) in [(0x1d0, 2), (0x1d4, 14)] {
        if let Some(bytes) = members
            .model(field)
            .with_context(|| format!("enemy {id} texture member {field:#x}, slot {slot}"))?
        {
            textures.insert(
                slot,
                crate::battle_effect::art::bank_textures(
                    bytes,
                    &source,
                    &palette_strides,
                    slot,
                    output,
                )
                .with_context(|| format!("enemy {id} texture member {field:#x}, slot {slot}"))?,
            );
        }
    }
    crate::battle_effect::art::bind_palettes(&mut source, &palette_strides, &mut textures)?;
    shared.bind(&mut source, &palette_strides)?;
    let textures: BTreeMap<_, _> = textures
        .into_iter()
        .map(|(slot, textures)| {
            (
                slot,
                textures
                    .into_iter()
                    .map(|texture| texture.effect)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let profile = usize::from(crate::read::u16(bytes, 4)?);
    let count = usize::from(
        *bytes
            .get(profile + 0x1e8)
            .context("missing enemy model count")?,
    );
    ensure!(count <= 6, "enemy effect model count exceeds pointer table");
    let mut models = BTreeMap::new();
    let mut decoded = crate::scene::decoded::Package::default();
    for slot in 0..count {
        let Some(model) = members.model(0x180 + slot * 4)? else {
            continue;
        };
        let animation = members.animation(0x1b0 + slot * 4, &mut decoded)?;
        let clips = animation
            .as_ref()
            .map(|animation| crate::character::Clip {
                slot: 0,
                resource: None,
                animation,
            })
            .into_iter()
            .collect::<Vec<_>>();
        let mut layers = Vec::new();
        crate::model_preview::layers_with_clips(
            crate::model_preview::Layer {
                model,
                outline: members.model(0x198 + slot * 4)?,
                animation: None,
                attached_to: None,
                additive: false,
            },
            &mut layers,
            &format!("battle/enemies/{id:03}/effect-models/{slot}"),
            &clips,
            output,
            &mut decoded,
        )?;
        models.insert(
            slot as u8,
            ModelPart {
                rig: super::rig(model, super::RigKind::Effect)?,
                layer_skeletons: BTreeMap::new(),
                layers,
            },
        );
    }
    if bank.is_none() && models.is_empty() && textures.values().all(Vec::is_empty) {
        return Ok(files);
    }
    let mut art_files = super::files(
        models
            .values()
            .flat_map(|part| part.layers.iter().map(|layer| &layer.scene)),
        output,
    )?;
    for image in textures
        .values()
        .flatten()
        .flat_map(|texture| &texture.images)
    {
        art_files.insert(
            image.path.clone(),
            File {
                sha256: crate::media::hash_file(&output.join(&image.path))?,
                bytes: std::fs::metadata(output.join(&image.path))?.len(),
                roles: [Role::Texture].into(),
            },
        );
    }
    files.extend(art_files.clone());
    source.art = Some(Art {
        source_sha256: crate::digest(bytes),
        textures,
        models,
        files: art_files,
    });
    write(&mut files, output, enemy_effects_path(id), &source)?;
    Ok(files)
}

fn write(
    files: &mut BTreeMap<String, File>,
    output: &Path,
    path: String,
    value: &impl serde::Serialize,
) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    crate::write_atomic(&output.join(&path), &bytes)?;
    files.insert(
        path,
        File {
            sha256: crate::digest(&bytes),
            bytes: bytes.len() as u64,
            roles: [Role::Data].into(),
        },
    );
    Ok(())
}

/// Reuse the published carried geometry. Rig metadata is read from the same
/// member; the battle loader owns attachment and visibility.
pub(super) fn attachments(
    bytes: &[u8],
    preview: &resonance_content::model_preview::ModelPreview,
) -> Result<(
    BTreeMap<u8, ModelPart>,
    BTreeMap<u8, resonance_content::battle_model::TrailMaterial>,
)> {
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let profile = usize::from(crate::read::u16(bytes, 4)?);
    let count = usize::from(
        *bytes
            .get(profile + 0x1e4)
            .context("missing enemy attachment count")?,
    );
    ensure!(count <= 8, "enemy attachment count exceeds pointer table");
    let mut parts = BTreeMap::new();
    let mut trails = BTreeMap::new();
    for slot in 0..count {
        let package = members
            .model(0x160 + slot * 4)?
            .context("missing enemy attachment")?;
        let sections = crate::field::sections(package)?;
        let Some(range) = sections.get(1).and_then(Option::as_ref) else {
            continue;
        };
        let metadata = sections
            .first()
            .and_then(Option::as_ref)
            .context("missing enemy weapon metadata")?;
        trails.insert(
            slot as u8,
            super::weapon::trail_material(&package[metadata.clone()])?,
        );
        let prefix = format!("kk0{slot}");
        let layers: Vec<_> = preview
            .parts
            .iter()
            .filter(|part| {
                part.attached_to
                    .as_ref()
                    .is_some_and(|name| name.starts_with(&prefix))
            })
            .cloned()
            .collect();
        ensure!(!layers.is_empty(), "missing published enemy attachment");
        let mut layer_skeletons = BTreeMap::new();
        if slot == 0
            && let Some(extra) = sections.get(5).and_then(Option::as_ref)
        {
            ensure!(layers.len() == 2, "enemy extra attachment layers differ");
            let skeleton = super::rig(&package[extra.clone()], super::RigKind::Effect)?.skeleton;
            let layer = &layers[1];
            ensure!(
                skeleton
                    .bones
                    .iter()
                    .map(|bone| &bone.name)
                    .eq(layer.scene.bone_names.iter()),
                "enemy extra attachment skeleton differs from its mesh"
            );
            layer_skeletons.insert(layer.scene.resource, skeleton);
        }
        parts.insert(
            slot as u8,
            ModelPart {
                rig: super::rig(&package[range.clone()], super::RigKind::Weapon)?,
                layer_skeletons,
                layers,
            },
        );
    }
    Ok((parts, trails))
}

pub(super) fn weapon_variants(
    bytes: &[u8],
    output: &Path,
) -> Result<BTreeMap<u8, resonance_content::battle_model::WeaponVariant>> {
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let profile = usize::from(crate::read::u16(bytes, 4)?);
    let count = usize::from(
        *bytes
            .get(profile + 0x1e4)
            .context("missing enemy attachment count")?,
    );
    ensure!(count <= 8, "enemy attachment count exceeds pointer table");
    let mut result = BTreeMap::new();
    let mut decoded = crate::scene::decoded::Package::default();
    for variant in count..8 {
        let Some(package) = members.model(0x160 + variant * 4)? else {
            continue;
        };
        ensure!(count != 0, "weapon variant has no equipped attachment");
        let metadata = crate::source_assets::section(package, 0)?;
        let model = crate::source_assets::section(package, 1)?;
        let animation = crate::field::sections(package)?
            .get(3)
            .and_then(Clone::clone)
            .map(|range| crate::animation::read_member(&package[range]))
            .transpose()?;
        let mut layers = Vec::new();
        crate::model_preview::layers(
            crate::model_preview::Layer {
                model,
                outline: None,
                animation: animation.as_ref(),
                attached_to: None,
                additive: bytes[profile + 0x124] & 0x10 != 0,
            },
            &mut layers,
            &format!("battle/enemy-weapon-variants/{variant}"),
            output,
            &mut decoded,
        )?;
        result.insert(
            variant as u8,
            resonance_content::battle_model::WeaponVariant {
                model: ModelPart {
                    rig: super::rig(model, super::RigKind::Weapon)?,
                    layer_skeletons: BTreeMap::new(),
                    layers,
                },
                trail: super::weapon::trail_material(metadata)?,
                animation: Some(super::weapon::attachment_animation(metadata)?),
            },
        );
    }
    Ok(result)
}

pub(super) fn attachment_animation(
    bytes: &[u8],
    source: &mut resonance_content::battle_model::Enemy,
) -> Result<()> {
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let read = |slot: u8| {
        ensure!(slot < 8, "enemy attachment slot exceeds pointer table");
        let package = members
            .model(0x160 + usize::from(slot) * 4)?
            .context("missing enemy attachment settings")?;
        super::weapon::attachment_animation(crate::source_assets::section(package, 0)?)
    };
    source.uv_animation = source
        .attachments
        .keys()
        .map(|&slot| Ok((slot, read(slot)?)))
        .collect::<Result<_>>()?;
    for (&slot, variant) in &mut source.weapon_variants {
        variant.animation = Some(read(slot)?);
    }
    Ok(())
}
