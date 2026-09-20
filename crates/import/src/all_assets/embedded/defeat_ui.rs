//! Defeat screen text and its independently declared title-image binding.
use super::text::{FixedText, TextPool, TextSource};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

const FAMILY: &str = "defeat-ui";
const TEXT_SLOTS: [(u32, usize); 4] = [
    (0x8017d870, 10),
    (0x8017d87c, 35),
    (0x801f9c48, 10),
    (0x801f9c54, 10),
];

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Catalogue {
    texts: Vec<String>,
    background_declaration: FixedText,
    pub(crate) background_source: String,
    pub(crate) background_image: usize,
    caption: FixedText,
    choices: [FixedText; 2],
}

impl Catalogue {
    pub(crate) fn caption(&self) -> &str {
        &self.texts[self.caption.text.0]
    }

    pub(crate) fn choices(&self) -> [&str; 2] {
        self.choices
            .each_ref()
            .map(|text| self.texts[text.text.0].as_str())
    }
}

fn read(extracted: &Path, executable: &[u8]) -> Result<(Catalogue, Vec<TextSource>)> {
    let mut texts = TextPool::default();
    let mut slot = |index: usize| {
        let (address, size) = TEXT_SLOTS[index];
        texts.fixed(executable, address, size)
    };
    let background_declaration = slot(0)?;
    let caption = slot(1)?;
    let choices = [slot(2)?, slot(3)?];
    let background_source = crate::all_assets::roles::declared_path(
        &extracted.join("files"),
        &texts.values[background_declaration.text.0],
    )?;
    Ok((
        Catalogue {
            texts: texts.values,
            background_declaration,
            background_source,
            // The defeat entry point selects this image before drawing its full-screen quad.
            background_image: 14,
            caption,
            choices,
        },
        texts.sources,
    ))
}

pub(crate) fn cook(extracted: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let (catalogue, sources) = read(extracted, executable)?;
    crate::embedded::write(
        &extracted.join("sys/main.dol"),
        output,
        FAMILY,
        &catalogue,
        serde_json::json!({
            "consumer": 0x8007202cu32,
            "text_slots": TEXT_SLOTS.map(|(address, source_size)| serde_json::json!({"address":address,"source_size":source_size})),
            "texts": sources,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dol;
    use std::fs;

    fn reconstruct(catalogue: &Catalogue, executable: &[u8]) -> Result<()> {
        for ((address, size), slot) in TEXT_SLOTS.into_iter().zip([
            &catalogue.background_declaration,
            &catalogue.caption,
            &catalogue.choices[0],
            &catalogue.choices[1],
        ]) {
            let (encoded, _, invalid) =
                encoding_rs::SHIFT_JIS.encode(&catalogue.texts[slot.text.0]);
            assert!(!invalid);
            let bytes: Vec<_> = encoded
                .iter()
                .copied()
                .chain([0])
                .chain(slot.storage.iter().copied())
                .collect();
            assert_eq!(bytes, dol::slice(executable, address, size)?);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both extracted discs; publishes only defeat JSON"]
    fn original_defeat_ui_preserves_text_storage_and_independent_background_declaration()
    -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("defeat-ui"));
        let result = (|| -> Result<()> {
            for disc in ["disc1", "disc2"] {
                let extracted = root.join(disc);
                let executable = fs::read(extracted.join("sys/main.dol"))?;
                let data = output.join(disc);
                cook(&extracted, &executable, &data)?;
                let original = crate::embedded::read::<Catalogue>(&data, FAMILY, "main.dol")?;
                reconstruct(&original, &executable)?;
                assert_eq!(original.background_source, "title.tpl");
                assert_eq!(original.background_image, 14);
                assert_eq!(original.caption(), dol::text(&executable, TEXT_SLOTS[1].0)?);
                assert_eq!(
                    original.choices(),
                    [
                        dol::text(&executable, TEXT_SLOTS[2].0)?,
                        dol::text(&executable, TEXT_SLOTS[3].0)?,
                    ]
                );

                let changed_root = data.join("changed");
                fs::create_dir_all(changed_root.join("sys"))?;
                fs::create_dir_all(changed_root.join("files"))?;
                fs::write(changed_root.join("files/ALT.TPL"), [])?;
                let mut changed = executable.clone();
                for (address, bytes) in [
                    (TEXT_SLOTS[0].0, b"alt.tpl\0".as_slice()),
                    (TEXT_SLOTS[1].0, b"\0".as_slice()),
                    (TEXT_SLOTS[3].0, b"N\0".as_slice()),
                ] {
                    let offset = dol::slice(&changed, address, bytes.len())?.as_ptr() as usize
                        - changed.as_ptr() as usize;
                    changed[offset..offset + bytes.len()].copy_from_slice(bytes);
                }
                fs::write(changed_root.join("sys/main.dol"), &changed)?;
                cook(&changed_root, &changed, &data)?;
                let restored = crate::embedded::read::<Catalogue>(&data, FAMILY, "main.dol")?;
                reconstruct(&restored, &changed)?;
                assert_eq!(restored.background_source, "ALT.TPL");
                assert!(restored.caption().is_empty());
                assert_eq!(restored.caption.storage.len(), 34);
                assert_eq!(restored.choices()[1], "N");
                assert_eq!(restored.choices[1].storage.len(), 8);
            }
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(output);
        result?;
        cleanup?;
        Ok(())
    }
}
