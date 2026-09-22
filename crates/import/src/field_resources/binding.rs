//! Resolve original standalone files and bounded members of resource archives.
#[cfg(test)]
use crate::{all_assets::PhysicalDirectory, scene::binding::read};
use crate::{resource::Catalogue, scene::decoded::Package};
use anyhow::{Result, ensure};
use std::io::{Cursor, Read, Seek, SeekFrom};
#[cfg(test)]
use std::path::Path;

pub(crate) struct Resources<'a> {
    pub(crate) catalogue: &'a Catalogue,
    decoded: &'a Package,
}

impl<'a> Resources<'a> {
    pub(crate) fn decoded(catalogue: &'a Catalogue, decoded: &'a Package) -> Self {
        Self { catalogue, decoded }
    }

    pub(crate) fn package(&self) -> &'a Package {
        self.decoded
    }

    pub(crate) fn source(&self, declared: &str) -> Result<Vec<u8>> {
        crate::compression::payload((*self.decoded.source(declared)?).clone())
    }

    pub(crate) fn resource(&self, id: u32) -> Result<Vec<u8>> {
        let declared = self.catalogue.source(id)?;
        if id >> 16 == 0 {
            return self.source(declared);
        }
        let bytes = self.decoded.source(declared)?;
        read_member(Cursor::new(bytes.as_slice()), bytes.len() as u64, id)
    }
}

fn read_member(mut file: impl Read + Seek, size: u64, id: u32) -> Result<Vec<u8>> {
    let mut word = [0; 4];
    file.read_exact(&mut word)?;
    let count = u64::from(u32::from_be_bytes(word));
    let index = u64::from(id & 0xffff);
    let header = 4 + count * 8;
    ensure!(
        index < count && header <= size,
        "invalid resource archive index"
    );
    let mut entry = [0; 8];
    file.seek(SeekFrom::Start(4 + index * 8))?;
    file.read_exact(&mut entry)?;
    if entry[4..] == [0; 4] {
        file.seek(SeekFrom::Start(4))?;
        file.read_exact(&mut entry)?;
    }
    let start = u64::from(u32::from_be_bytes(entry[..4].try_into()?));
    let length = u64::from(u32::from_be_bytes(entry[4..].try_into()?));
    ensure!(
        start >= header && length > 0 && start + length <= size,
        "invalid resource archive payload"
    );
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes)?;
    crate::compression::payload(bytes)
}

#[cfg(test)]
pub(crate) fn members(root: &Path, directory: &str) -> Result<Vec<Option<String>>> {
    let members: PhysicalDirectory = read(root, &format!("{directory}/members.json"))?;
    members.validate()?;
    members
        .members
        .into_iter()
        .map(|member| {
            member
                .map(|index| payload(root, format!("{directory}/{index}")))
                .transpose()
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn payload(root: &Path, mut directory: String) -> Result<String> {
    resonance_content::validate_asset_path(&directory)?;
    for _ in 0..=16 {
        ensure!(
            root.join(&directory).is_dir(),
            "missing cooked resource directory {directory}"
        );
        let path = format!("{directory}/cabinet.json");
        if !root.join(&path).try_exists()? {
            return Ok(directory);
        }
        let members: Vec<String> = read(root, &path)?;
        let [member] = members.as_slice() else {
            anyhow::bail!("script resource requires one cabinet payload");
        };
        resonance_content::validate_asset_path(member)?;
        directory = format!("{directory}/{member}");
    }
    anyhow::bail!("cooked script resource nesting exceeds 16 levels")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_read_bounded_members_and_aliases() -> Result<()> {
        let mut bank: Vec<u8> = [3_u32, 28, 4, 0, 0, 28, 4, 0x12345678]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect();
        let mut decoded = crate::scene::decoded::Package::default();
        decoded.remember_source("Bank.bin", std::sync::Arc::new(bank.clone()));
        decoded.remember_source(
            "Standalone.tpl",
            std::sync::Arc::new(b"dependency".to_vec()),
        );
        let catalogue = Catalogue {
            standalone: vec![Some("standalone.TPL".into())],
            groups: vec![crate::resource::Group {
                path: Some("bank.BIN".into()),
                storage: [0; 3],
            }],
            party_bodies: vec![],
            party_battle_motions: vec![],
            party_field_motions: vec![],
            field_services: vec![],
            save_point: "unused.cab".into(),
        };
        let resources = Resources::decoded(&catalogue, &decoded);
        assert_eq!(resources.resource(0)?, b"dependency");
        for id in 0x10000..=0x10002 {
            assert_eq!(resources.resource(id)?, 0x12345678_u32.to_be_bytes());
        }
        assert!(resources.resource(0x10003).is_err());
        // Reject an aliased payload outside the file, then a missing fallback.
        bank[20..24].copy_from_slice(&32_u32.to_be_bytes());
        assert!(read_member(Cursor::new(&bank), bank.len() as u64, 0x10002).is_err());
        bank[8..12].fill(0);
        assert!(read_member(Cursor::new(&bank), bank.len() as u64, 0x10001).is_err());
        assert!(resources.source("missing.bin").is_err());
        Ok(())
    }
}
