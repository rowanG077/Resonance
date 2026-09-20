//! Stream physical visual resources without requiring battle controllers or AI.
use super::*;
use crate::battle::effect_program::{MagicArchive, SkillArchive};
use resonance_content::{
    battle::{
        effect_program::ModelRef,
        pose::Skeleton,
        unison::PowWeapon,
        visual::{EffectModel, LinkedWeaponMotions, PartyModel, TrailStyle},
    },
    menu_data::Costume,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Asset {
    Arena(u16),
    Party { character: u8, costume: Costume },
    Enemy(u8),
    EnemyAppearance { monster: u8, row: u8 },
    Weapon(u16),
    WeaponMotions { item: u16, costume: Costume },
    PowWeapon(PowWeapon),
    EffectModel(ModelRef),
    ToonRamp,
    Shadow,
}

impl Asset {
    pub(super) fn dependencies(self) -> Vec<Self> {
        match self {
            Self::EnemyAppearance { monster, .. } => vec![Self::Enemy(monster)],
            Self::WeaponMotions { item, costume } => vec![
                Self::Weapon(item),
                Self::Party {
                    character: 3,
                    costume,
                },
            ],
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Visual {
    Arena(ArenaVisuals),
    Party(PartyModel),
    Enemy(ModelVisuals),
    EnemyAppearance { texture: u16, rows: u8, row: u8 },
    Weapon(WeaponVisuals),
    WeaponMotions(LinkedWeaponMotions),
    EffectModel(EffectModel),
    PackageModel(PackageModel),
    Texture(String),
}

/// Authored effect data is independent of the controller selected for playback.
#[derive(Debug, Serialize, Deserialize)]
pub struct PackageModel {
    pub binding: ModelRef,
    pub model: ModelPreview,
    pub rig: Rig,
    pub outline: Option<Skeleton>,
}

impl PackageModel {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            matches!(
                self.binding,
                ModelRef::Magic { .. } | ModelRef::Skill { .. }
            ),
            "expected a magic or skill package model"
        );
        self.model.validate()?;
        self.rig.validate()?;
        ensure!(
            self.model.parts.len() == 1 + usize::from(self.outline.is_some()),
            "effect model has incomplete authored layers"
        );
        for (part, skeleton) in self
            .model
            .parts
            .iter()
            .zip(std::iter::once(&self.rig.skeleton).chain(self.outline.as_ref()))
        {
            skeleton.validate()?;
            ensure!(
                part.scene
                    .bone_names
                    .iter()
                    .map(String::as_str)
                    .eq(skeleton.bones.iter().map(|bone| bone.name.as_str())),
                "effect skeleton does not match its authored mesh"
            );
            ensure!(
                part.scene
                    .clips
                    .iter()
                    .map(|clip| clip.resource_slot)
                    .collect::<BTreeSet<_>>()
                    == self.rig.motions.keys().copied().collect(),
                "effect mesh does not retain all authored clips"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Cooked {
    pub asset: Asset,
    pub visual: Visual,
    /// Runtime files only; the caller persists this typed record separately.
    pub output_paths: Vec<String>,
    /// Shared records must also be published before this asset is usable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<Asset>,
}

/// Compressed archives are shared; decoded bodies and rigs leave with each result.
/// Only the current enemy and skill packages are cached, never the full roster.
pub struct Cooker<'a> {
    extracted: &'a Path,
    files: PathBuf,
    output: PathBuf,
    rel: Vec<u8>,
    executable: Vec<u8>,
    backgrounds: Vec<u8>,
    directory: Vec<u8>,
    enemies: Vec<u8>,
    weapons: Vec<u8>,
    owners: Vec<u16>,
    excluded: Vec<String>,
    trails: trails::Cooker,
    magic: Option<MagicArchive>,
    skill: SkillArchive,
    skill_package: Option<(u16, Vec<u8>)>,
    enemy_effects: Option<(u8, Vec<u8>)>,
}

impl<'a> Cooker<'a> {
    pub fn new(extracted: &'a Path, output: &Path) -> Result<Self> {
        let files = extracted.join("files");
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let sources = crate::battle::all::Sources::read(extracted)?;
        let directory = fs::read(files.join(&sources.usual))?;
        let excluded = excluded_bones(&files)?;
        Ok(Self {
            rel: fs::read(files.join("US_r_Top2Btl.rel"))?,
            backgrounds: fs::read(files.join(sources.archive(crate::battle::all::Archive::Arena)))?,
            enemies: fs::read(files.join(&sources.enemy))?,
            weapons: fs::read(files.join(sources.archive(crate::battle::all::Archive::Weapon)))?,
            owners: crate::session::equipment_owners(
                &executable,
                &crate::item::read(&executable)?,
            )?,
            trails: trails::Cooker::new(&directory, output)?,
            extracted,
            files,
            output: output.into(),
            executable,
            directory,
            excluded,
            magic: None,
            skill: SkillArchive::read(extracted)?,
            skill_package: None,
            enemy_effects: None,
        })
    }

    pub(crate) fn set_output(&mut self, output: &Path) {
        self.output = output.into();
        self.trails.set_output(output);
    }

    pub fn cook(&mut self, asset: Asset) -> Result<Cooked> {
        let visual = match asset {
            Asset::Arena(id) => {
                Visual::Arena(arena(&self.rel, &self.backgrounds, id, &self.output)?)
            }
            Asset::Party { character, costume } => {
                let body = party::body(&self.executable, &self.files, character, costume as u8)?;
                let archive =
                    party::archive(&self.executable, &self.files, character, costume as u8)?;
                let namespace = if costume == Costume::Standard {
                    format!("battle/{character}")
                } else {
                    format!("battle/{character}/costumes/{}", costume as u8)
                };
                let visual = party_model(
                    &self.rel,
                    &self
                        .files
                        .join(crate::all_assets::roles::victory_path(self.extracted)?),
                    character,
                    &namespace,
                    &body,
                    &archive,
                    &self.excluded,
                    &self.output,
                )?;
                Visual::Party(PartyModel {
                    head_bone: costumes::head_bone(&self.rel, &visual.rig)?,
                    visual,
                    // These item/costume combinations are separate streaming units.
                    weapon_motions: Default::default(),
                })
            }
            Asset::Enemy(id) => Visual::Enemy(enemy_model(
                &self.directory,
                &self.enemies,
                id,
                &self.excluded,
                &mut self.trails,
                &self.output,
            )?),
            Asset::EnemyAppearance { monster, row } => {
                let bytes = self.enemy_package(monster)?;
                let metadata = bytes
                    .get(usize::from(half(bytes, 4)?)..)
                    .context("enemy metadata exceeds package")?;
                let layer =
                    variant_texture(metadata)?.context("enemy has no body appearance rows")?;
                ensure!(
                    row < layer.frames,
                    "enemy appearance row exceeds authored atlas"
                );
                let model = bytes
                    .get(word(bytes, 0x18)? as usize..)
                    .context("enemy body exceeds package")?;
                let tpl = model
                    .get(word(model, 0)? as usize..word(model, 4)? as usize)
                    .context("enemy body texture table exceeds model")?;
                let textures = crate::tpl::parse_tpl(tpl)?;
                let texture = textures
                    .get(usize::from(layer.texture))
                    .context("enemy appearance texture exceeds body atlas")?;
                ensure!(
                    texture.height.is_multiple_of(u16::from(layer.frames)),
                    "enemy appearance rows do not divide the body atlas"
                );
                Visual::EnemyAppearance {
                    texture: layer.texture,
                    rows: layer.frames,
                    row,
                }
            }
            Asset::Weapon(id) => Visual::Weapon(weapon(
                &self.rel,
                &self.weapons,
                &self.executable,
                &self.files,
                id,
                weapon_owners(&self.owners, id),
                &mut self.trails,
                &self.output,
            )?),
            Asset::WeaponMotions { item, costume } => {
                const GENIS: u8 = 3;
                ensure!(
                    weapon_owners(&self.owners, item) & (1 << (GENIS - 1)) != 0,
                    "item {item} has no linked weapon motion owner"
                );
                let archive = party::archive(&self.executable, &self.files, GENIS, costume as u8)?;
                Visual::WeaponMotions(costumes::linked_weapon_motions(
                    &self.rel,
                    &self.weapons,
                    item,
                    &archive,
                )?)
            }
            Asset::PowWeapon(kind) => {
                self.load_magic()?;
                Visual::Weapon(pow_blade::from_archive(
                    self.magic.as_ref().unwrap(),
                    &mut self.trails,
                    &self.output,
                    kind,
                )?)
            }
            Asset::EffectModel(binding) => self.effect_model(binding)?,
            Asset::ToonRamp => {
                Visual::Texture(crate::field_lighting::cook(self.extracted, &self.output)?)
            }
            Asset::Shadow => Visual::Texture(shadow_texture(&self.directory, &self.output)?),
        };
        let output_paths = visual.output_paths()?;
        for path in &output_paths {
            resonance_content::validate_asset_path(path)?;
            ensure!(
                self.output.join(path).is_file(),
                "cooked visual dependency {path} is missing"
            );
        }
        Ok(Cooked {
            asset,
            visual,
            output_paths,
            dependencies: asset.dependencies(),
        })
    }

    fn load_magic(&mut self) -> Result<()> {
        if self.magic.is_none() {
            self.magic = Some(MagicArchive::read(self.extracted)?);
        }
        Ok(())
    }

    fn enemy_package(&mut self, monster: u8) -> Result<&[u8]> {
        if self
            .enemy_effects
            .as_ref()
            .is_none_or(|(id, _)| *id != monster)
        {
            let table = word(&self.directory, 0x2c)? as usize;
            let start = word(&self.directory, table + usize::from(monster) * 4)? as usize;
            let end = word(&self.directory, table + (usize::from(monster) + 1) * 4)? as usize;
            let bytes = compression::decode(
                self.enemies
                    .get(start..end)
                    .context("enemy package exceeds archive")?,
            )?;
            ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
            self.enemy_effects = Some((monster, bytes));
        }
        Ok(&self.enemy_effects.as_ref().unwrap().1)
    }

    fn effect_model(&mut self, binding: ModelRef) -> Result<Visual> {
        match binding {
            ModelRef::ColetteWeapon => {
                anyhow::bail!("live equipped weapon requires an item binding")
            }
            ModelRef::Magic { package, .. } => {
                self.load_magic()?;
                effects::package_model(
                    self.magic.as_ref().unwrap().package(package)?,
                    binding,
                    &self.output,
                )
                .map(Visual::PackageModel)
            }
            ModelRef::Skill { package, .. } => {
                if self
                    .skill_package
                    .as_ref()
                    .is_none_or(|(id, _)| *id != package)
                {
                    self.skill_package = Some((package, self.skill.package(package)?));
                }
                effects::package_model(
                    &self.skill_package.as_ref().unwrap().1,
                    binding,
                    &self.output,
                )
                .map(Visual::PackageModel)
            }
            ModelRef::Common { index } => {
                ensure!(index > 0, "common model zero is a live equipped weapon");
                let bank = crate::battle::actions::member(&self.directory, 6)?;
                let model = crate::battle::actions::member(bank, usize::from(index - 1))?;
                effects::cook_model(
                    binding,
                    crate::model_preview::Layer {
                        model,
                        outline: None,
                        animation: None,
                        attached_to: None,
                        additive: false,
                    },
                    &format!("battle/effects/models/common/{index}"),
                    &self.output,
                )
                .map(Visual::EffectModel)
            }
            ModelRef::Enemy { monster, .. } | ModelRef::EnemyAnimated { monster, .. } => {
                let output = self.output.clone();
                effects::enemy_model(self.enemy_package(monster)?, binding, &output)
                    .map(Visual::EffectModel)
            }
        }
    }
}

impl Visual {
    pub(super) fn output_paths(&self) -> Result<Vec<String>> {
        let mut paths = BTreeSet::new();
        match self {
            Self::Arena(visual) => model_paths(&visual.model, &mut paths)?,
            Self::Party(visual) => actor_paths(&visual.visual, &mut paths)?,
            Self::Enemy(visual) => actor_paths(visual, &mut paths)?,
            Self::EnemyAppearance { .. } => {}
            Self::Weapon(visual) => {
                for model in visual.slots.values() {
                    model_paths(model, &mut paths)?;
                }
                for rig in visual.rigs.values() {
                    rig.validate()?;
                }
                for trail in visual.trails.values() {
                    trail_paths(&trail.style, &mut paths)?;
                }
            }
            Self::WeaponMotions(visual) => {
                for rig in visual.rigs.values() {
                    rig.validate()?;
                }
            }
            Self::EffectModel(visual) => {
                model_paths(&visual.model, &mut paths)?;
                if let Some(rig) = &visual.rig {
                    rig.validate()?;
                }
            }
            Self::PackageModel(visual) => {
                visual.validate()?;
                model_paths(&visual.model, &mut paths)?;
            }
            Self::Texture(path) => {
                paths.insert(path.clone());
            }
        }
        Ok(paths.into_iter().collect())
    }
}

fn model_paths(model: &ModelPreview, paths: &mut BTreeSet<String>) -> Result<()> {
    model.validate()?;
    for part in &model.parts {
        paths.insert(part.scene.mesh.clone());
        paths.extend(part.scene.textures.iter().cloned());
    }
    Ok(())
}

fn actor_paths(visual: &ModelVisuals, paths: &mut BTreeSet<String>) -> Result<()> {
    model_paths(&visual.model, paths)?;
    visual.rig.validate()?;
    if let Some(table) = &visual.authored_motions {
        table.validate(
            &visual.rig,
            visual.victory.values().map(|motion| motion.clip),
        )?;
    }
    for attachment in visual.attachments.values() {
        attachment.rig.validate()?;
        if let Some(style) = &attachment.trail_style {
            trail_paths(style, paths)?;
        }
    }
    for trail in visual.trails.values() {
        trail_paths(&trail.style, paths)?;
    }
    Ok(())
}

fn trail_paths(style: &TrailStyle, paths: &mut BTreeSet<String>) -> Result<()> {
    style.validate()?;
    paths.extend(
        style
            .textures
            .values()
            .flatten()
            .map(|texture| texture.path.clone()),
    );
    Ok(())
}
