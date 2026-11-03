//! One native enemy package becomes the same rules/artwork owner as GC.
use super::*;
use crate::{monsters, output, package::Package, source::Source};
use resonance_content::{
    AtlasChannel,
    animation::{Skeleton, Transform, euler_rotation},
    battle_enemy::{EntryRow, WeaponVisibility, document::Entry, rules::Rules},
    battle_model::{
        AttachedModel, AttachmentPlacement,
        document::{
            Model,
            enemy::{Definition as Artwork, Secondary},
        },
    },
    model_preview::ModelPreview,
};
use std::fs;

#[path = "rig_names.rs"]
mod rig_names;

fn body_nodes(library: &Path, preview: &ModelPreview) -> Result<Vec<String>> {
    Ok(
        Skeleton::from_glb(&fs::read(library.join(&preview.parts[0].scene.mesh))?)?
            .bones
            .into_iter()
            .map(|bone| bone.name)
            .collect(),
    )
}

fn artwork(bytes: &[u8], name: &str, preview: ModelPreview, nodes: &[String]) -> Result<Artwork> {
    let metadata = monsters::metadata(bytes)?;
    let owner = format!("meshes/monsters/{name}");
    let path = format!("{owner}/model.json");
    preview.validate_definition()?;
    let mut attachment_nodes = BTreeMap::new();
    for name in nodes {
        let lower = name.to_ascii_lowercase();
        if let Some(digits) = lower.strip_prefix("kk").and_then(|suffix| suffix.get(..2))
            && let Ok(slot) = digits.parse::<u8>()
        {
            ensure!(slot < 8, "enemy carried anchor exceeds directory");
            ensure!(
                attachment_nodes.insert(slot, name.clone()).is_none(),
                "duplicate enemy carried anchor"
            );
        }
    }
    let mut secondary_layers = Vec::new();
    let mut attachments = BTreeMap::<u8, Model>::new();
    for part in preview.parts.into_iter().skip(2) {
        if part.scene.mesh.starts_with(&format!("{owner}/secondary/")) {
            secondary_layers.push(part);
            continue;
        }
        let node = part
            .attached_to
            .as_ref()
            .context("enemy carried layer has no anchor")?;
        let slot = *attachment_nodes
            .iter()
            .find(|(_, name)| *name == node)
            .context("enemy carried anchor has no slot")?
            .0;
        let model = attachments.entry(slot).or_insert_with(|| Model {
            layers: Vec::new(),
            attachment_nodes: Default::default(),
            independent_layers: Default::default(),
        });
        if !model.layers.is_empty() {
            model.independent_layers.insert(part.scene.resource);
        }
        model.layers.push(part);
    }
    let secondary_body = if secondary_layers.is_empty() {
        None
    } else {
        Some(Secondary {
            model_slot: metadata[0xc0],
            motion_offset: u16::from(metadata[0xc1]),
            model: Model {
                layers: secondary_layers,
                attachment_nodes: Default::default(),
                independent_layers: Default::default(),
            },
        })
    };
    let mut attached_models = Vec::new();
    let model_scale = f32::from_bits(pe::word(metadata, 0x84)?);
    if metadata[0x1f2] != 0 {
        let rows =
            crate::preview_model::member(bytes, 0x1d8)?.context("missing attached-model table")?;
        for row in rows
            .get(..usize::from(metadata[0x1f2]) * 40)
            .context("truncated attached-model table")?
            .chunks_exact(40)
        {
            ensure!(
                matches!(row[3], 0 | 0x10 | 0x70 | 0x80) && row[0] < 6 && row[1] < 80,
                "unsupported attached-model controller"
            );
            let vector = |at| -> Result<[f32; 3]> {
                Ok([
                    f32::from_bits(pe::word(row, at)?),
                    f32::from_bits(pe::word(row, at + 4)?),
                    f32::from_bits(pe::word(row, at + 8)?),
                ])
            };
            let mut transform = Transform {
                translation: vector(4)?,
                rotation: euler_rotation(vector(16)?),
                scale: vector(28)?.map(|value| value * model_scale),
            };
            let placement = if row[3] & 0x80 != 0 {
                ensure!(
                    section(bytes, 12)?[8] == 16,
                    "unsupported independent attached model"
                );
                transform.translation = [0.; 3];
                transform.rotation = euler_rotation([-90., 0., 0.]);
                AttachmentPlacement::Independent
            } else {
                AttachmentPlacement::Bone(row[2])
            };
            transform.validate()?;
            attached_models.push(AttachedModel {
                model_slot: row[0],
                motion_slot: (row[1] != 0).then_some(row[1]),
                placement,
                visible: row[2] != 0,
                additive: row[3] & 0x10 != 0,
                inherit_tint: row[3] & 0x20 == 0,
                wing_layers: row[3] & 0x40 != 0,
                transform,
            });
        }
    }
    Ok(Artwork {
        model: path,
        attachment_nodes,
        secondary_body,
        attached_models,
        attachments,
        body_variant: (metadata[0xf3] != 0).then_some(AtlasChannel {
            texture: usize::from(metadata[0xf2]),
            frames: metadata[0xf3],
        }),
        uv_animation: Default::default(),
        weapon_variants: Default::default(),
        trails: Default::default(),
    })
}

/// `selected` is only for focused repairs; the full cook always visits all owners.
pub(crate) fn cook(source: &Source, library: &Path, selected: Option<&[u16]>) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    presentation::verify(&image)?;
    let count = monsters::count(&image)?;
    ensure!(
        selected.is_none_or(|ids| ids.iter().all(|&id| usize::from(id) < count)),
        "enemy selection exceeds catalogue"
    );
    let usual = source.require("btlusual.pac")?;
    let package = Package::parse(&usual)?;
    let common = package.member(8)?.context("missing common enemy actions")?;
    let durations = package
        .member(12)?
        .context("missing enemy presentation durations")?;
    let defaults = crate::recoil::strategy_defaults(&image)?;
    let mut failures = Vec::new();
    let mut prepared = 0;
    let mut unmapped_immunities = 0;
    let mut pending_presentation = Vec::new();
    let mut carried_counts = [0usize; 3];
    let mut unavailable_artes = Vec::new();
    for id in 0..count as u16 {
        if selected.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let result = (|| -> Result<()> {
            let bytes = source.require(monsters::package(&image, id)?)?;
            let name = monsters::symbol(id);
            let model_path = format!("meshes/monsters/{name}/model.json");
            let preview: ModelPreview =
                serde_json::from_slice(&fs::read(library.join(model_path))?)?;
            let metadata = monsters::metadata(&bytes)?;
            let nodes = body_nodes(library, &preview)?;
            let (mut profile, unmapped) = battle_profiles::enemy(metadata, &nodes)?;
            unmapped_immunities += usize::from(unmapped);
            let mut tables = actions_with_source(source, &bytes, common, library, id)?;
            rig_names::bind(&mut tables.actions.policy, &nodes)?;
            for (index, row) in tables.actions.rows.iter().enumerate() {
                if let Some(reason) = &row.unsupported_reason {
                    unavailable_artes.push((id, index, reason.clone()));
                }
            }
            let strategy = section(&bytes, 6)?;
            let entry_row = match strategy[2] {
                0 => EntryRow::Random,
                1 => EntryRow::Front,
                2 => EntryRow::Middle,
                3 => EntryRow::Back,
                other => anyhow::bail!("enemy entry strategy {other} is not decoded"),
            };
            let position = if strategy[2] == 0 {
                defaults[19]
            } else {
                strategy[2]
            };
            let count = u32::from_le_bytes(bytes[0x14..0x18].try_into()?).max(31) as usize;
            ensure!(count <= 80, "enemy motion count exceeds directory");
            let null_motions = (0..count)
                .filter_map(|slot| match pe::word(&bytes, 0x20 + slot * 4) {
                    Ok(0) => Some(Ok(slot as u16)),
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                })
                .collect::<Result<_>>()?;
            let (presentation, deferred) =
                presentation::read_package(&bytes, durations, &mut profile)?;
            let rules = Rules {
                profile,
                null_motions,
                weapon_visibility: match id {
                    105 => [(1, WeaponVisibility::Alternating)].into(),
                    208 | 209 => (0..4)
                        .map(|slot| (slot, WeaponVisibility::Hidden))
                        .collect(),
                    236..=238 => [(0, WeaponVisibility::Hidden)].into(),
                    _ => Default::default(),
                },
                actions: tables.actions,
                attacks: tables.attacks,
                contacts: tables.contacts,
                projectiles: tables.projectiles,
                target_strategy: Some(strategy[0].try_into()?),
                entry_row,
                guard_recovery_bonus: resonance_content::battle_recoil::strategy_guard_recovery(
                    position,
                ),
                presentation,
            };
            rules.clone().into_definition()?;
            let mut artwork = artwork(&bytes, &name, preview, &nodes)?;
            super::carried::bind(source, library, &bytes, &name, &mut artwork)?;
            carried_counts[0] += artwork.trails.len()
                + artwork
                    .weapon_variants
                    .values()
                    .filter(|variant| variant.trail.is_some())
                    .count();
            carried_counts[1] += artwork.uv_animation.len()
                + artwork
                    .weapon_variants
                    .values()
                    .filter(|variant| variant.animation.is_some())
                    .count();
            carried_counts[2] += artwork.weapon_variants.len();
            // These existing source controllers still need media/choreography
            // owners. Keep the omissions visible while the importer is a WIP.
            if deferred {
                pending_presentation.push(id);
            }
            let entry = Entry {
                rules,
                artwork: serde_json::to_value(artwork)?,
                effects: Some(format!("battle/enemies/{name}/effects/effects.json")),
            };
            output::json(
                &library.join(format!("battle/enemies/{name}/enemy.json")),
                &entry,
            )?;
            prepared += 1;
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("  Enemy {id} owner: {error:#}");
            failures.push(id);
        }
    }
    eprintln!(
        "  Enemy owners: {prepared}; unmapped source immunity 0x40 on {unmapped_immunities} profiles"
    );
    eprintln!("  Pending enemy entrance choreography: {pending_presentation:?}");
    eprintln!(
        "  Enemy carried artwork: {} trails, {} UV/effect controllers, {} alternate weapons",
        carried_counts[0], carried_counts[1], carried_counts[2]
    );
    eprintln!("  Unavailable enemy artes: {unavailable_artes:?}");
    ensure!(
        failures.is_empty(),
        "enemy owners remain unconverted: {failures:?}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; recooks only Kilia's enemy owner"]
    fn installed_kilia_controller_binds_existing_source_rig() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let library = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &library, Some(&[186]))?;
        let name = monsters::symbol(186);
        let entry: Entry = serde_json::from_slice(&fs::read(
            library.join(format!("battle/enemies/{name}/enemy.json")),
        )?)?;
        let preview: ModelPreview = serde_json::from_slice(&fs::read(
            library.join(format!("meshes/monsters/{name}/model.json")),
        )?)?;
        let nodes = body_nodes(&library, &preview)?;
        let Some(resonance_content::battle_action::EnemyBehavior::Transformation {
            revealed_bones,
            ..
        }) = entry.rules.actions.policy.behavior
        else {
            anyhow::bail!("Kilia has no native transformation controller");
        };
        ensure!(
            revealed_bones.len() == 8 && revealed_bones.iter().all(|name| nodes.contains(name)),
            "Kilia controller must reference its eight existing source joints"
        );
        ensure!(
            revealed_bones[0] == "SC_BONE_UDE_L",
            "Kilia controller did not retain the source joint spelling"
        );
        Ok(())
    }
    use crate::Language;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; recooks only enemy226"]
    fn installed_final_fury_enemy_owner_uses_shared_callbacks() -> Result<()> {
        use resonance_content::{
            battle_action::{EnemyEvent, EnemyExecution},
            battle_normal::Event,
        };
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &root, Some(&[226]))?;
        let captured = resonance_content::battle_enemy::document::capture(
            &mut resonance_content::library::Library::open(&root, &[])?.snapshot(),
            226,
            &resonance_content::diagnostics::Diagnostics::new(true),
            &|| false,
        )?;
        let row = &captured.definition.actions.rows[6];
        let Some(EnemyExecution::Physical {
            technique: 152,
            attack,
        }) = &row.execution
        else {
            anyhow::bail!("missing Final Fury physical action")
        };
        ensure!(row.unsupported_reason.is_none(), "Final Fury unavailable");
        ensure!(attack.events.iter().filter(|(_,e)|matches!(e,EnemyEvent::AuthoredCallback {skill:200,event} if matches!(event.as_ref(),Event::EmitProjectile{..}))).count()==2,"missing Final Fury skill projectiles");
        ensure!(
            attack
                .events
                .iter()
                .any(|(_, e)| matches!(e,EnemyEvent::Contact(c) if c.hit.effect==Some(2))),
            "missing Final Fury contact program2"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with body motions/effects"]
    fn installed_enemy_owners_cook_all_rules_and_artwork() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let library = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &library, None)
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_LIBRARY with cooked Steam enemy owners, body motions and effects"]
    fn cooked_enemy_owners_prepare_through_shared_reader() -> Result<()> {
        use resonance_content::{
            battle_enemy::document, diagnostics::Diagnostics, library::Library,
        };
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let library = Library::open(&root, &[])?;
        let diagnostics = Diagnostics::new(true);
        for id in [0, 84, 139, 180, 208, 226, 248] {
            let captured = document::capture(&mut library.snapshot(), id, &diagnostics, &|| false)
                .with_context(|| format!("prepare Steam enemy {id}"))?;
            let artwork = captured.artwork.context("missing enemy body artwork")?;
            ensure!(
                artwork
                    .source
                    .body
                    .skeleton
                    .bones
                    .iter()
                    .any(|bone| bone.name == captured.definition.profile.head_anchor.bone),
                "missing cooked head anchor"
            );
            ensure!(captured.effects.is_some(), "missing enemy effect owner");
            ensure!(
                captured
                    .definition
                    .actions
                    .rows
                    .iter()
                    .any(|row| row.execution.is_some()),
                "enemy has no prepared actions"
            );
            if id == 84 {
                ensure!(
                    !artwork.source.attachments.is_empty(),
                    "Soldier lost carried artwork"
                );
            }
            if id == 226 {
                let action = &captured.definition.actions.rows[6];
                ensure!(
                    action.unsupported_reason.is_none(),
                    "Final Fury is unavailable"
                );
                let Some(resonance_content::battle_action::EnemyExecution::Physical {
                    technique: 152,
                    attack,
                }) = &action.execution
                else {
                    anyhow::bail!("Final Fury physical recipe missing")
                };
                ensure!(attack.events.iter().filter(|(_,event)|matches!(event,resonance_content::battle_action::EnemyEvent::AuthoredCallback {event,..} if matches!(event.as_ref(),resonance_content::battle_normal::Event::EmitProjectile {..}))).count()==2,"Final Fury callback launches missing");
                ensure!(attack.events.iter().any(|(_,event)|matches!(event,resonance_content::battle_action::EnemyEvent::Contact(contact) if contact.hit.effect==Some(2))),"Final Fury skill contact artwork missing");
            }
        }
        Ok(())
    }
}
