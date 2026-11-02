//! Native event directories. Resource identities disappear into shared mappings.
use crate::{package::Package, pe, source::Source};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub(crate) struct Resources {
    pub entries: BTreeMap<u32, Option<String>>,
}

impl Resources {
    pub fn read(source: &Source) -> Result<Self> {
        let image = pe::Image::parse(&source.executable)?;
        ensure!(
            image.slice(0x578a6c, 3)? == [0x83, 0xff, 13],
            "unknown global bank count"
        );
        let mut entries = BTreeMap::new();
        for index in 0..13u32 {
            let at = 0x5788b6 + index * 28 + if index == 11 { 4 } else { 0 };
            let args = image.slice(at, 10)?;
            ensure!(
                args[0] == 0xba && args[5] == 0xb9,
                "unknown global bank registration"
            );
            let extension = image.text(pe::word(args, 1)?)?;
            ensure!(extension == "TOSBIN", "unknown global bank extension");
            let name = format!("{}.{extension}", image.text(pe::word(args, 6)?)?);
            let bytes = source.require(&name)?;
            let package = Package::parse(&bytes)?;
            for slot in 0..package.len() {
                entries.insert(
                    (index + 1) << 16 | slot as u32,
                    package.reference(slot)?.map(str::to_owned),
                );
            }
        }
        for (index, pointer) in image.slice(0x8adb10, 53 * 4)?.chunks_exact(4).enumerate() {
            let name = image.text(pe::word(pointer, 0)?)?;
            entries.insert(index as u32, Some(name.to_owned()));
        }
        Ok(Self { entries })
    }

    pub fn loaded(&self, resource: u32) -> Result<&str> {
        let entry = self
            .entries
            .get(&resource)
            .context("script resource exceeds native directory")?;
        entry
            .as_deref()
            .or_else(|| {
                self.entries
                    .get(&(resource & 0xffff0000))
                    .and_then(|e| e.as_deref())
            })
            .context("script resource has no fallback")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_bank_member_uses_zero_without_changing_requested_identity() -> Result<()> {
        let resources = Resources {
            entries: BTreeMap::from([
                (0x20000, Some("actor.pac".into())),
                (0x20001, None),
                (0x30000, None),
                (4, Some("object.pac".into())),
            ]),
        };
        assert_eq!(resources.loaded(0x20001)?, "actor.pac");
        assert_eq!(resources.entries[&0x20001], None);
        assert_eq!(resources.loaded(4)?, "object.pac");
        assert!(resources.loaded(0x30000).is_err());
        assert!(resources.loaded(53).is_err());
        Ok(())
    }
}
