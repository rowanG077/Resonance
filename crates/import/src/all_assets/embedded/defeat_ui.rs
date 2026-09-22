//! Defeat screen text and its independently declared title-image binding.
use super::text::{TextPool, TextRef};
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
    pub(crate) background_source: String,
    pub(crate) background_image: usize,
    caption: TextRef,
    choices: [TextRef; 2],
}

#[cfg(test)]
impl Catalogue {
    pub(crate) fn caption(&self) -> &str {
        &self.texts[self.caption.0]
    }

    pub(crate) fn choices(&self) -> [&str; 2] {
        self.choices
            .each_ref()
            .map(|text| self.texts[text.0].as_str())
    }
}

fn read(extracted: &Path, executable: &[u8]) -> Result<Catalogue> {
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
        &texts.values[background_declaration.0],
    )?;
    Ok(Catalogue {
        texts: texts.values,
        background_source,
        // The defeat entry point selects this image before drawing its full-screen quad.
        background_image: 14,
        caption,
        choices,
    })
}

pub(crate) fn cook(extracted: &Path, executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let catalogue = read(extracted, executable)?;
    crate::embedded::write(&extracted.join("sys/main.dol"), output, FAMILY, &catalogue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dol;
    use std::fs;

    #[test]
    #[ignore = "requires both extracted discs; publishes only defeat JSON"]
    fn original_defeat_ui_preserves_labels_and_background_binding() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("defeat-ui"));
        let result = (|| -> Result<()> {
            for disc in ["disc1", "disc2"] {
                let extracted = root.join(disc);
                let executable = fs::read(extracted.join("sys/main.dol"))?;
                let data = output.join(disc);
                cook(&extracted, &executable, &data)?;
                let original = crate::embedded::read::<Catalogue>(&data, FAMILY, "main.dol")?;
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
                assert_eq!(restored.background_source, "ALT.TPL");
                assert!(restored.caption().is_empty());
                assert_eq!(restored.choices()[1], "N");
            }
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(output);
        result?;
        cleanup?;
        Ok(())
    }
}
