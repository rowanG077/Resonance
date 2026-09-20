//! Physical filenames come from loader declarations; archive kinds remain semantic.
use super::{Archive, Asset, Job};
use crate::{all_assets::roles::declared_path, dol, field_resources::resolve_path, rel::Rel};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

#[derive(Clone, Copy, Serialize)]
pub(in crate::battle) struct SourceLayout {
    pub enemy: (usize, usize),
    /// Magic, skill, arena and weapon declarations, in Archive::ALL order.
    pub archives: [(usize, usize); 4],
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Sources {
    pub usual: String,
    pub enemy: String,
    pub(super) archives: [String; 4],
}

impl Sources {
    pub(crate) fn cooked(output: &Path, disc: u8) -> Result<Self> {
        crate::cooked::Source::open(output, disc, "US_r_Top2Btl.rel")?
            .embedded("battle-sources", "US_r_Top2Btl.rel")
    }

    pub(crate) fn publish(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
        let Some((_, layout)) = super::super::embedded::Layout::identify(file) else {
            return Ok(None);
        };
        crate::embedded::write(
            file,
            output,
            "battle-sources",
            &Self::for_module(file)?.context("missing battle source layout")?,
            serde_json::json!({"usual": 0x8017e53cu32, "module_declarations": layout.sources}),
        )
        .map(Some)
    }

    pub(crate) fn read(extracted: &Path) -> Result<Self> {
        let files = extracted.join("files");
        let module = resolve_path(&files, "US_r_Top2Btl.rel")?;
        Self::from_module(
            extracted,
            &Rel::read(&files.join(module))?,
            &super::super::embedded::Layout::RETAIL,
        )
    }

    pub(crate) fn for_module(file: &Path) -> Result<Option<Self>> {
        let Some((_, layout)) = super::super::embedded::Layout::identify(file) else {
            return Ok(None);
        };
        let extracted = file
            .parent()
            .and_then(Path::parent)
            .context("battle module outside extracted files")?;
        Self::from_module(extracted, &Rel::read(file)?, &layout).map(Some)
    }

    fn from_module(
        extracted: &Path,
        rel: &Rel,
        layout: &super::super::embedded::Layout,
    ) -> Result<Self> {
        Self::decode(
            &extracted.join("files"),
            &fs::read(extracted.join("sys/main.dol"))?,
            rel,
            layout.sources,
        )
    }

    fn decode(files: &Path, executable: &[u8], rel: &Rel, layout: SourceLayout) -> Result<Self> {
        let mut archives = Vec::new();
        for pointer in layout.archives {
            archives.push(declared_path(files, &rel.text(pointer)?)?);
        }
        Ok(Self {
            // The startup loader reads this standalone declaration into the usual-bank handle.
            usual: declared_path(files, &dol::text(executable, 0x8017e53c)?)?,
            enemy: declared_path(files, &rel.text(layout.enemy)?)?,
            archives: archives.try_into().unwrap(),
        })
    }

    pub(crate) fn archive(&self, archive: Archive) -> &str {
        &self.archives[match archive {
            Archive::Magic => 0,
            Archive::Skill => 1,
            Archive::Arena => 2,
            Archive::Weapon => 3,
        }]
    }

    pub(crate) fn owned_paths(&self) -> BTreeSet<String> {
        [&self.usual, &self.enemy]
            .into_iter()
            .chain(&self.archives)
            .cloned()
            .collect()
    }

    pub(crate) fn source_paths(&self, job: &Job) -> Vec<&str> {
        match job {
            Job::Shared => vec![&self.usual, self.archive(Archive::Magic)],
            Job::Archive { kind, .. } => vec![self.archive(*kind)],
            Job::Enemy { .. } => vec![&self.enemy],
            Job::Visual {
                asset: Asset::WeaponMotions { .. },
                ..
            } => vec![self.archive(Archive::Weapon)],
            _ => vec![&self.usual],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::embedded::Layout;
    use super::*;

    impl Sources {
        pub(in crate::battle) fn fixture(root: &Path) -> Result<Self> {
            let files = root.join("files");
            fs::create_dir_all(files.join("Data"))?;
            fs::create_dir_all(root.join("sys"))?;
            for name in ["Common", "Enemy", "Magic", "Skill", "Arena", "Weapon"] {
                fs::write(files.join(format!("Data/{name}.bin")), [])?;
            }
            let mut executable = vec![0; 0x140];
            for (at, value) in [(0, 0x100u32), (0x48, 0x8017e53c), (0x90, 0x40)] {
                executable[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            executable[0x100..0x110].copy_from_slice(b"data/common.bin\0");
            fs::write(root.join("sys/main.dol"), executable)?;
            let size = 0x2500;
            let mut module = vec![0; 0x100 + size];
            for (at, value) in [(12, 5u32), (16, 0x4c), (0x6c, 0x100), (0x70, size as u32)] {
                module[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            for ((_, offset), name) in Layout::RETAIL
                .sources
                .archives
                .into_iter()
                .zip(["Magic", "Skill", "Arena", "Weapon"])
                .chain([(Layout::RETAIL.sources.enemy, "Enemy")])
            {
                let name = format!("./data/{}.bin\0", name.to_lowercase());
                module[0x100 + offset..0x100 + offset + name.len()]
                    .copy_from_slice(name.as_bytes());
            }
            fs::write(files.join("US_r_Top2Btl.rel"), module)?;
            Self::read(root)
        }
    }

    #[test]
    fn declared_archive_roles_survive_renaming_and_reject_missing_or_invalid_sources() -> Result<()>
    {
        let root = crate::temporary_path(&std::env::temp_dir().join("battle-sources"));
        let result = (|| -> Result<()> {
            let sources = Sources::fixture(&root)?;
            let files = root.join("files");
            let path = files.join("US_r_Top2Btl.rel");
            let mut module = fs::read(&path)?;
            assert_eq!(sources.usual, "Data/Common.bin");
            assert_eq!(sources.enemy, "Data/Enemy.bin");
            for (kind, name) in Archive::ALL
                .into_iter()
                .zip(["Magic", "Skill", "Arena", "Weapon"])
            {
                assert_eq!(sources.archive(kind), format!("Data/{name}.bin"));
            }
            assert_eq!(sources.owned_paths().len(), 6);
            assert_eq!(
                sources.source_paths(&Job::Shared),
                ["Data/Common.bin", "Data/Magic.bin"]
            );
            // Exercise the archive loader through the renamed declaration too.
            let data = module.len();
            let skill = Layout::RETAIL.archives.skill;
            let size = skill + 16;
            module.resize(data + size, 0);
            for (at, value) in [(12, 6u32), (0x74, data as u32), (0x78, size as u32)] {
                module[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            for (index, value) in [0u32, 0, 3, 7].into_iter().enumerate() {
                let at = data + skill + index * 4;
                module[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            fs::write(&path, &module)?;
            fs::write(files.join(sources.archive(Archive::Skill)), b"onetwo!")?;
            let archive = crate::battle::effect_program::SkillArchive::read(&root)?;
            assert_eq!(archive.package(1)?, b"one");
            assert_eq!(archive.package(2)?, b"two!");
            assert!(archive.package(0).is_err());
            assert!(archive.package(3).is_err());
            let output = root.join("cooked");
            let prefix = "data/variants/renamed";
            let publications: Vec<_> = Sources::publish(&path, &output.join(prefix))?
                .unwrap()
                .into_iter()
                .map(|path| format!("{prefix}/{path}"))
                .collect();
            crate::write_atomic(
                &output.join("sources.json"),
                &serde_json::to_vec(&std::collections::BTreeMap::from([
                    ("disc1/US_r_Top2Btl.rel", &publications),
                    ("disc2/US_r_Top2Btl.rel", &publications),
                ]))?,
            )?;
            assert_eq!(Sources::cooked(&output, 1)?, sources);
            assert_eq!(Sources::cooked(&output, 2)?, sources);
            assert!(Sources::for_module(&files.join("unrelated.rel"))?.is_none());
            fs::remove_file(files.join("Data/Magic.bin"))?;
            assert!(Sources::read(&root).is_err());
            fs::write(files.join("Data/Magic.bin"), [])?;
            let offset = 0x100 + Layout::RETAIL.sources.archives[0].1;
            module[offset..offset + 64].fill(0);
            module[offset..offset + 11].copy_from_slice(b"../bad.bin\0");
            fs::write(path, module)?;
            assert!(Sources::read(&root).is_err());
            // Preparation only needs the published declarations, even if inputs disappear.
            fs::remove_file(root.join("sys/main.dol"))?;
            assert_eq!(Sources::cooked(&output, 1)?, sources);
            fs::write(output.join(&publications[0]), b"{}")?;
            assert!(
                Sources::cooked(&output, 1)
                    .unwrap_err()
                    .to_string()
                    .contains("data changed")
            );
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(root);
        result?;
        cleanup?;
        Ok(())
    }

    #[test]
    #[ignore = "requires both original discs; publishes only source declarations"]
    fn original_archive_declarations_match_native_relocations_on_both_discs() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("original-battle-sources"));
        let mut catalogues = BTreeSet::new();
        let result = (|| -> Result<()> {
            for disc in [1, 2] {
                let extracted = root.join(format!("disc{disc}"));
                for name in [
                    "US_r_Top2Btl.rel",
                    "r_Top2Btl.rel",
                    "US_Top2Btl.rel",
                    "US_m_Top2Btl.rel",
                    "Top2Btl.rel",
                    "m_Top2Btl.rel",
                    "Top2BtlD.rel",
                ] {
                    let path = extracted.join("files").join(name);
                    let rel = Rel::read(&path)?;
                    let (_, layout) = Layout::identify(&path).unwrap();
                    let sources = Sources::for_module(&path)?.unwrap();
                    assert_eq!(sources.usual, "BTL/BTLusual.dat");
                    assert_eq!(sources.enemy, "BTL/BTLenemy.dat");
                    for pointer in layout
                        .sources
                        .archives
                        .into_iter()
                        .chain([layout.sources.enemy])
                    {
                        assert!(
                            rel.local_targets().contains(&pointer),
                            "{disc}/{name}/{pointer:?}"
                        );
                    }
                    for archive in Archive::ALL {
                        assert_eq!(sources.archive(archive), format!("BTL/{}", archive.file()));
                    }
                    assert_eq!(sources.owned_paths().len(), 6);
                    let paths = Sources::publish(&path, &output)?.unwrap();
                    catalogues.insert(paths[0].clone());
                    assert_eq!(
                        crate::embedded::read::<Sources>(&output, "battle-sources", name)?,
                        sources
                    );
                }
            }
            assert_eq!(catalogues.len(), 1);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
