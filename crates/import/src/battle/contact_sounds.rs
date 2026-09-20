//! Default contact cues for every character and original element selector.
use super::embedded::Layout;
use crate::{embedded, rel::Rel};
use anyhow::{Context, Result};
use resonance_content::battle::audio::ImpactSoundTable;
use std::path::Path;

const FAMILY: &str = "battle-contact-sounds";
const PARTY_BYTES: usize = 18;
const ELEMENT_BYTES: usize = 11;

fn parse(party: &[u8], elements: &[u8]) -> Result<ImpactSoundTable> {
    let party: &[u8; PARTY_BYTES] = party
        .get(..PARTY_BYTES)
        .context("truncated party contact sounds")?
        .try_into()?;
    let elements: &[u8; ELEMENT_BYTES] = elements
        .get(..ELEMENT_BYTES)
        .context("truncated elemental contact sounds")?
        .try_into()?;
    Ok(ImpactSoundTable {
        party: std::array::from_fn(|i| u16::from_be_bytes([party[i * 2], party[i * 2 + 1]])),
        elements: elements.map(u16::from),
    })
}

pub(super) fn read(rel: &Rel, layout: &Layout) -> Result<ImpactSoundTable> {
    // The native party copy reads nine halfwords. Element selector ten is an
    // authored zero entry; preserve it along with the other ten selectors.
    parse(
        rel.at((4, layout.party_contact_sounds))?,
        rel.at((5, layout.contact_sounds))?,
    )
}

pub(crate) fn cook_all(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    embedded::write(
        file,
        output,
        FAMILY,
        &read(&Rel::read(file)?, &layout)?,
        serde_json::json!({
            "party": {
                "section": 4, "offset": layout.party_contact_sounds,
                "count": 9, "stride": 2, "bytes": PARTY_BYTES,
            },
            "elements": {
                "section": 5, "offset": layout.contact_sounds,
                "count": 11, "stride": 1, "bytes": ELEMENT_BYTES,
            },
        }),
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    fn contact_sound_records_are_bounded_and_keep_every_selector() -> Result<()> {
        let party: Vec<u8> = (0x100u16..0x109).flat_map(u16::to_be_bytes).collect();
        let elements: Vec<u8> = (0..11).collect();
        let table = parse(&party, &elements)?;
        assert_eq!(table.party, std::array::from_fn(|i| 0x100 + i as u16));
        assert_eq!(table.elements, std::array::from_fn(|i| i as u16));
        for size in [0, PARTY_BYTES - 1] {
            assert!(parse(&party[..size], &elements).is_err());
        }
        for size in [0, ELEMENT_BYTES - 1] {
            assert!(parse(&party, &elements[..size]).is_err());
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; no media conversion"]
    fn original_contact_sound_tables_cover_every_module_and_match_binding() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join(FAMILY));
        let mut publications = BTreeSet::new();
        let result = (|| -> Result<()> {
            for disc in [1, 2] {
                let mut modules = 0;
                for file in fs::read_dir(local.join(format!("disc{disc}/files")))? {
                    let file = file?.path();
                    let Some((module, layout)) = Layout::identify(&file) else {
                        continue;
                    };
                    let rel = Rel::read(&file)?;
                    let table = read(&rel, &layout)?;
                    let party = rel.at((4, layout.party_contact_sounds))?;
                    let elements = rel.at((5, layout.contact_sounds))?;
                    assert_eq!(table.party, [46, 46, 54, 54, 54, 46, 46, 54, 46]);
                    assert_eq!(table.elements, [54, 55, 56, 54, 58, 59, 54, 54, 54, 54, 0]);
                    for (value, bytes) in table.party.iter().zip(party.chunks_exact(2)) {
                        assert_eq!(*value, u16::from_be_bytes(bytes.try_into()?));
                    }
                    for (value, byte) in table.elements.iter().zip(elements) {
                        assert_eq!(*value, u16::from(*byte));
                    }
                    // The next referenced tables follow zero-filled gaps;
                    // their data must never be interpreted as more sound IDs.
                    for (section, offset, bytes, length) in [
                        (4, layout.party_contact_sounds, party, PARTY_BYTES),
                        (5, layout.contact_sounds, elements, ELEMENT_BYTES),
                    ] {
                        let end = (offset + length).next_multiple_of(4);
                        assert!(rel.local_targets().contains(&(section, end)));
                        assert!(bytes[length..end - offset].iter().all(|&byte| byte == 0));
                    }
                    let paths = cook_all(&file, &output)?.unwrap();
                    assert_eq!(
                        fs::read(output.join(&paths[0]))?,
                        serde_json::to_vec(&table)?
                    );
                    let source: serde_json::Value =
                        serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                    assert_eq!(source["module"], module);
                    assert_eq!(source["source_sha256"], crate::digest(&rel.bytes));
                    assert_eq!(source["data"], paths[0]);
                    assert_eq!(
                        source["party"],
                        serde_json::json!({"section": 4, "offset": layout.party_contact_sounds,
                            "count": 9, "stride": 2, "bytes": PARTY_BYTES})
                    );
                    assert_eq!(
                        source["elements"],
                        serde_json::json!({"section": 5, "offset": layout.contact_sounds,
                            "count": 11, "stride": 1, "bytes": ELEMENT_BYTES})
                    );
                    let binding: ImpactSoundTable = embedded::read(&output, FAMILY, module)?;
                    assert_eq!(binding.party, table.party);
                    assert_eq!(binding.elements, table.elements);
                    publications.insert(paths[0].clone());
                    modules += 1;
                }
                assert_eq!(modules, 7);
            }
            assert_eq!(publications.len(), 1);
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
