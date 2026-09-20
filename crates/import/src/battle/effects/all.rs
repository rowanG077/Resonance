//! Convert every authored projectile row before native allocation specializes it.
use super::source::{AuthoredProjectile, RECORD_BYTES};
use super::*;
use crate::battle::{actions::member, effect_program::SkillArchive};
use resonance_content::battle::effect_inventory::EffectSourceBank;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum ProjectileStatus {
    Cooked { authored: AuthoredProjectile },
    AuthoredNull,
    Unsupported { error: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ProjectileOutcome {
    pub id: u16,
    #[serde(flatten)]
    pub status: ProjectileStatus,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BankCook {
    pub bank: EffectSourceBank,
    pub sha256: Option<String>,
    pub roots: Vec<ProjectileOutcome>,
    pub failure: Option<String>,
}

pub(crate) struct Cooker {
    extracted: PathBuf,
    enemy: PathBuf,
    usual: Vec<u8>,
    magic: Option<MagicArchive>,
    skill: Option<SkillArchive>,
}

impl Cooker {
    pub fn read(extracted: &Path) -> Result<Self> {
        let sources = crate::battle::all::Sources::read(extracted)?;
        Ok(Self {
            extracted: extracted.to_owned(),
            enemy: extracted.join("files").join(&sources.enemy),
            usual: fs::read(extracted.join("files").join(&sources.usual))?,
            magic: None,
            skill: None,
        })
    }

    pub fn cook_bank(&mut self, bank: EffectSourceBank) -> BankCook {
        let mut cooked = BankCook {
            bank,
            sha256: None,
            roots: Vec::new(),
            failure: None,
        };
        match self.load(bank) {
            Ok((start, bytes)) => {
                cooked.sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
                let mut rows = bytes.chunks_exact(RECORD_BYTES);
                for (id, row) in rows.by_ref().enumerate() {
                    let Ok(id) = u16::try_from(id) else {
                        cooked.failure =
                            Some("projectile row count exceeds source identity range".into());
                        break;
                    };
                    cooked.roots.push(ProjectileOutcome {
                        id,
                        status: decode(row),
                    });
                }
                // The following member is aligned within its package. Projectile
                // tables themselves can begin at any word-aligned package offset.
                if !rows.remainder().is_empty()
                    && start + bytes.len()
                        != (start + cooked.roots.len() * RECORD_BYTES).next_multiple_of(32)
                {
                    cooked.failure = Some(format!(
                        "projectile section at {start:#x} has {} unexplained trailing bytes",
                        rows.remainder().len()
                    ));
                }
            }
            Err(error) => cooked.failure = Some(format!("{error:#}")),
        }
        cooked
    }

    pub(super) fn load(&mut self, bank: EffectSourceBank) -> Result<(usize, Vec<u8>)> {
        Ok(match bank {
            EffectSourceBank::Techniques => (
                word(&self.usual, 4 + 7 * 4)? as usize,
                member(&self.usual, 7)?.to_vec(),
            ),
            EffectSourceBank::Enemy { monster } => {
                let package = crate::battle::archive_directories::enemy_package(
                    &self.enemy,
                    &self.usual,
                    monster,
                )?;
                let start = word(&package, 0x1c8)? as usize;
                let bytes = crate::battle::enemy_inventory::offset_section(&package, start)?;
                (start, bytes.to_vec())
            }
            EffectSourceBank::Magic { package } => {
                if self.magic.is_none() {
                    self.magic = Some(MagicArchive::read(&self.extracted)?);
                }
                let package_bytes = self.magic.as_ref().unwrap().package(package)?;
                (
                    word(package_bytes, 252)? as usize,
                    magic_member(package_bytes, 252)?
                        .context("magic package has no projectile table")?
                        .to_vec(),
                )
            }
            EffectSourceBank::Skill { package } => {
                if self.skill.is_none() {
                    self.skill = Some(SkillArchive::read(&self.extracted)?);
                }
                let package_bytes = self.skill.as_ref().unwrap().package(package)?;
                (
                    word(&package_bytes, 252)? as usize,
                    magic_member(&package_bytes, 252)?
                        .context("skill package has no projectile table")?
                        .to_vec(),
                )
            }
            EffectSourceBank::Arena { .. } => {
                bail!("arena archives do not declare a projectile table")
            }
            _ => bail!("projectile source bank {bank:?} has no recovered table binding"),
        })
    }
}

fn decode(row: &[u8]) -> ProjectileStatus {
    if row.iter().all(|&byte| byte == 0) {
        return ProjectileStatus::AuthoredNull;
    }
    match AuthoredProjectile::decode(row) {
        Ok(authored) => ProjectileStatus::Cooked { authored },
        Err(error) => ProjectileStatus::Unsupported {
            error: format!("{error:#}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires privately extracted GameCube projectile packages"]
    fn unaligned_enemy_projectile_tables_use_package_alignment() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let mut cooker = Cooker::read(&extracted).unwrap();
        for monster in [155, 163, 164] {
            let cooked = cooker.cook_bank(EffectSourceBank::Enemy { monster });
            assert!(cooked.failure.is_none(), "{:?}", cooked.failure);
            assert_eq!(cooked.roots.len(), 1);
            assert!(matches!(
                cooked.roots[0].status,
                ProjectileStatus::Cooked { .. }
            ));
        }
    }

    #[test]
    fn authored_conversion_preserves_banks_and_keeps_independent_rows() {
        let mut row = [0; RECORD_BYTES];
        assert!(matches!(decode(&row), ProjectileStatus::AuthoredNull));
        row[8..12].copy_from_slice(&0x400u32.to_be_bytes());
        row[12..14].copy_from_slice(&20u16.to_be_bytes());
        row[0x5d] = 1; // Authored common-bank birth, overwritten by selected allocator.
        let ProjectileStatus::Cooked { authored } = decode(&row) else {
            panic!("authored projectile was rejected");
        };
        assert_eq!(
            authored.spawn_effect,
            source::EffectSelector { slot: 0, id: 1 }
        );
        assert_eq!(
            projectile(
                &row,
                EffectId {
                    bank: EffectBank::Magic(1),
                    id: 1
                },
                1.
            )
            .unwrap()
            .spawn_effect
            .unwrap()
            .bank,
            EffectBank::Magic(1)
        );
        row[0x15] = 255;
        assert!(matches!(decode(&row), ProjectileStatus::Unsupported { .. }));
        row[0x15] = 0;
        row[0x5c] = 2;
        let ProjectileStatus::Cooked { authored } = decode(&row) else {
            panic!("authored dynamic selector was rejected");
        };
        assert_eq!(
            authored.spawn_effect,
            source::EffectSelector { slot: 2, id: 1 }
        );
        for unsupported in [0x800_u32, 0x4000000] {
            row[8..12].copy_from_slice(&unsupported.to_be_bytes());
            let ProjectileStatus::Cooked { authored } = decode(&row) else {
                panic!("authored control was rejected");
            };
            assert_eq!(authored.flags, unsupported);
            assert!(
                projectile(
                    &row,
                    EffectId {
                        bank: EffectBank::Magic(1),
                        id: 1
                    },
                    1.
                )
                .is_err()
            );
        }
    }
}
