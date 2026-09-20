//! Select authored publications, then apply the native allocator's runtime policy.
use super::{
    all::{BankCook, ProjectileStatus},
    *,
};
use crate::{
    battle::all::{Archive, Sources},
    cooked::Source,
};
use resonance_content::battle::effect_inventory::EffectSourceBank;

pub(crate) fn bind(
    root: &Path,
    disc: u8,
    sources: &Sources,
    required: &BTreeSet<EffectId>,
    velocity_reset_scale: f32,
) -> Result<BattleEffects> {
    let mut banks = BTreeMap::new();
    collect(
        required
            .iter()
            .map(|&id| {
                let bank = match banks.entry(id.bank) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(load(root, disc, sources, id.bank)?)
                    }
                };
                project(bank, id, velocity_reset_scale)
                    .with_context(|| format!("projectile {id:?}"))
            })
            .collect(),
    )
}

/// Native contact controllers specialize this recipe before validating their result.
pub(crate) fn bind_one(
    root: &Path,
    disc: u8,
    sources: &Sources,
    id: EffectId,
    velocity_reset_scale: f32,
) -> Result<ProjectileRecipe> {
    project(
        &load(root, disc, sources, id.bank)?,
        id,
        velocity_reset_scale,
    )
    .with_context(|| format!("projectile {id:?}"))
}

fn load(root: &Path, disc: u8, sources: &Sources, bank: EffectBank) -> Result<BankCook> {
    let (source, name, expected) = match bank {
        EffectBank::Techniques => (
            sources.usual.as_str(),
            "techniques".to_owned(),
            EffectSourceBank::Techniques,
        ),
        EffectBank::Enemy(monster) => (
            sources.enemy.as_str(),
            format!("enemy-{monster}"),
            EffectSourceBank::Enemy {
                monster: u16::from(monster),
            },
        ),
        EffectBank::Magic(package) => (
            sources.archive(Archive::Magic),
            format!("magic-{package}"),
            EffectSourceBank::Magic { package },
        ),
        EffectBank::Common => bail!("common projectile recipes require a recovered source binding"),
        EffectBank::Skill(_) | EffectBank::Arena(_) => {
            bail!("projectile recipes for this resource bank are not implemented")
        }
    };
    let relative = format!("battle/all/projectiles/{name}.json");
    let (_, bytes) = Source::open(root, disc, source)?.resolve(&relative)?;
    let cooked: BankCook =
        serde_json::from_slice(&bytes).with_context(|| format!("invalid cooked {relative}"))?;
    ensure!(
        cooked.bank == expected,
        "projectile bank identity mismatch in {relative}"
    );
    ensure!(
        cooked.failure.is_none(),
        "failed cooked {relative}: {:?}",
        cooked.failure
    );
    ensure!(
        cooked
            .sha256
            .as_ref()
            .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit())),
        "missing or invalid projectile source digest in {relative}"
    );
    ensure!(
        cooked
            .roots
            .iter()
            .enumerate()
            .all(|(index, row)| usize::from(row.id) == index),
        "projectile root identity mismatch in {relative}"
    );
    Ok(cooked)
}

fn project(bank: &BankCook, id: EffectId, velocity_reset_scale: f32) -> Result<ProjectileRecipe> {
    let row = bank
        .roots
        .get(usize::from(id.id))
        .with_context(|| format!("missing selected projectile {id:?}"))?;
    let authored = match &row.status {
        ProjectileStatus::Cooked { authored } => authored,
        ProjectileStatus::AuthoredNull => &source::AuthoredProjectile::NULL,
        ProjectileStatus::Unsupported { error } => bail!("unsupported cooked projectile: {error}"),
    };
    lower(authored, id, velocity_reset_scale)
}

#[cfg(test)]
mod tests {
    use super::super::all::{Cooker, ProjectileOutcome};
    use super::*;
    use sha2::{Digest, Sha256};

    fn publish(root: &Path, directory: &str, bank: &BankCook) -> Result<()> {
        let path = root.join(directory).join("battle/all/projectiles");
        fs::create_dir_all(&path)?;
        fs::write(path.join("magic-1.json"), serde_json::to_vec(bank)?)?;
        Ok(())
    }

    #[test]
    fn binds_renamed_publications_without_archives_and_rejects_invalid_identity() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("projectile-binding"));
        let result = (|| -> Result<()> {
            let sources = Sources::fixture(&root)?;
            fs::remove_dir_all(root.join("files"))?;
            fs::remove_dir_all(root.join("sys"))?;
            fs::write(
                root.join("sources.json"),
                serde_json::to_vec(&BTreeMap::from([(
                    format!("disc2/{}", sources.archive(Archive::Magic)),
                    vec!["assets/shared", "assets/copy"],
                )]))?,
            )?;
            let mut row = [0; source::RECORD_BYTES];
            row[8..12].copy_from_slice(&0x400u32.to_be_bytes());
            row[12..14].copy_from_slice(&40u16.to_be_bytes());
            row[0x24..0x28].copy_from_slice(&1.5f32.to_be_bytes());
            row[0x5d] = 2;
            let mut bank = BankCook {
                bank: EffectSourceBank::Magic { package: 1 },
                sha256: Some(format!("{:x}", Sha256::digest(row))),
                roots: vec![
                    ProjectileOutcome {
                        id: 0,
                        status: ProjectileStatus::AuthoredNull,
                    },
                    ProjectileOutcome {
                        id: 1,
                        status: ProjectileStatus::Cooked {
                            authored: source::AuthoredProjectile::decode(&row)?,
                        },
                    },
                ],
                failure: None,
            };
            let id = EffectId {
                bank: EffectBank::Magic(1),
                id: 1,
            };
            publish(&root, "assets/shared", &bank)?;
            publish(&root, "assets/copy", &bank)?;
            let actual = bind(
                &root,
                2,
                &sources,
                &BTreeSet::from([EffectId { id: 0, ..id }, id]),
                0.5,
            )?;
            for (recipe, bytes) in actual
                .projectiles
                .iter()
                .zip([&[0; source::RECORD_BYTES], &row])
            {
                assert_eq!(
                    serde_json::to_value(recipe)?,
                    serde_json::to_value(projectile(bytes, recipe.id.unwrap(), 0.5)?)?
                );
            }
            assert_eq!(actual.projectiles[1].spawn_effect.unwrap().bank, id.bank);
            assert!(bind_one(&root, 1, &sources, id, 0.5).is_err());
            assert!(bind_one(&root, 2, &sources, EffectId { id: 2, ..id }, 0.5).is_err());

            bank.roots[1].id = 0;
            publish(&root, "assets/copy", &bank)?;
            assert!(
                bind_one(&root, 2, &sources, id, 0.5)
                    .unwrap_err()
                    .to_string()
                    .contains("conflicting cooked")
            );
            fs::remove_dir_all(root.join("assets/copy"))?;
            publish(&root, "assets/shared", &bank)?;
            assert!(
                bind_one(&root, 2, &sources, id, 0.5)
                    .unwrap_err()
                    .to_string()
                    .contains("root identity")
            );
            bank.roots[1].id = 1;
            bank.bank = EffectSourceBank::Magic { package: 2 };
            publish(&root, "assets/shared", &bank)?;
            assert!(
                bind_one(&root, 2, &sources, id, 0.5)
                    .unwrap_err()
                    .to_string()
                    .contains("bank identity")
            );
            bank.bank = EffectSourceBank::Magic { package: 1 };
            bank.failure = Some("malformed trailing bytes".into());
            publish(&root, "assets/shared", &bank)?;
            assert!(bind_one(&root, 2, &sources, id, 0.5).is_err());
            bank.failure = None;
            bank.sha256 = None;
            publish(&root, "assets/shared", &bank)?;
            assert!(bind_one(&root, 2, &sources, id, 0.5).is_err());
            bank.sha256 = Some("0".repeat(64));
            bank.roots[1].status = ProjectileStatus::Unsupported {
                error: "invalid shape".into(),
            };
            publish(&root, "assets/shared", &bank)?;
            assert!(bind_one(&root, 2, &sources, id, 0.5).is_err());
            assert!(bind_one(&root, 2, &sources, EffectId { id: 0, ..id }, 0.5).is_ok());
            bank.roots[1].status = ProjectileStatus::Cooked {
                authored: source::AuthoredProjectile {
                    flags: 0x800,
                    ..source::AuthoredProjectile::NULL
                },
            };
            publish(&root, "assets/shared", &bank)?;
            assert!(
                format!("{:#}", bind_one(&root, 2, &sources, id, 0.5).unwrap_err())
                    .contains("unsupported projectile movement flags")
            );
            for bank in [
                EffectBank::Common,
                EffectBank::Skill(1),
                EffectBank::Arena(1),
            ] {
                assert!(bind_one(&root, 2, &sources, EffectId { bank, ..id }, 0.5).is_err());
            }
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(root);
        result?;
        cleanup?;
        Ok(())
    }

    #[test]
    #[ignore = "requires private extracted discs and their cook-all publications"]
    fn published_projectiles_match_original_rows_on_both_discs() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let root = local.join("all-assets");
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let sources = Sources::read(&extracted)?;
            let rel = crate::battle::actions::Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
            let original =
                crate::battle::motion::read(&rel, &crate::battle::embedded::Layout::RETAIL)?;
            let motion: crate::battle::motion::MotionTables =
                Source::open(&root, disc, "US_r_Top2Btl.rel")?
                    .embedded("battle-motion", "US_r_Top2Btl.rel")?;
            let scale = motion.projectile_velocity_reset_scale()?;
            assert_eq!(scale, original.projectile_velocity_reset_scale()?);
            let mut cooker = Cooker::read(&extracted)?;
            let mut seen = BTreeSet::new();
            let mut compared = 0;
            for path in [
                sources.usual.as_str(),
                sources.enemy.as_str(),
                sources.archive(Archive::Magic),
            ] {
                let source = Source::open(&root, disc, path)?;
                for directory in source.publications() {
                    let directory = root.join(directory).join("battle/all/projectiles");
                    if !directory.is_dir() {
                        continue;
                    }
                    for entry in fs::read_dir(directory)? {
                        let published: BankCook =
                            serde_json::from_slice(&fs::read(entry?.path())?)?;
                        let bank = match published.bank {
                            EffectSourceBank::Techniques => EffectBank::Techniques,
                            EffectSourceBank::Enemy { monster } => {
                                EffectBank::Enemy(monster.try_into()?)
                            }
                            EffectSourceBank::Magic { package } => EffectBank::Magic(package),
                            _ => continue,
                        };
                        if !seen.insert(bank) {
                            continue;
                        }
                        let binding = load(&root, disc, &sources, bank);
                        if published.failure.is_some() {
                            assert!(binding.is_err());
                            continue;
                        }
                        let binding = binding?;
                        let (_, rows) = cooker.load(published.bank)?;
                        assert_eq!(
                            binding.sha256.as_deref(),
                            Some(format!("{:x}", Sha256::digest(&rows)).as_str())
                        );
                        assert_eq!(binding.roots.len(), rows.len() / source::RECORD_BYTES);
                        for (index, row) in rows.chunks_exact(source::RECORD_BYTES).enumerate() {
                            let Ok(id) = u8::try_from(index) else {
                                continue;
                            };
                            let id = EffectId { bank, id };
                            let original = projectile(row, id, scale);
                            let bound = project(&binding, id, scale);
                            match (original, bound) {
                                (Ok(original), Ok(bound)) => assert_eq!(
                                    serde_json::to_value(original)?,
                                    serde_json::to_value(bound)?,
                                    "disc{disc} {id:?}"
                                ),
                                (Err(_), Err(_)) => {}
                                (original, bound) => bail!(
                                    "disc{disc} {id:?}: original {original:?}, bound {bound:?}"
                                ),
                            }
                            compared += 1;
                        }
                    }
                }
            }
            ensure!(compared > 0, "no projectile publications on disc{disc}");
            eprintln!(
                "disc{disc}: compared {compared} projectile rows in {} banks",
                seen.len()
            );
        }
        Ok(())
    }
}
