use super::*;
use crate::all_assets::rename_ui::Catalogue;
use resonance_content::menu_data::RenameData;

pub(super) fn cook(source: &Catalogue) -> Result<RenameData> {
    let text = |reference| -> Result<String> { Ok(source.required_text(reference)?.to_owned()) };
    let labels = &source.labels;
    let data = RenameData {
        defaults: source
            .defaults
            .map(text)
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        keyboard: text(source.keyboard.bound_cells)?,
        heading: text(labels.heading)?,
        delete: text(labels.delete)?,
        default: text(labels.default)?,
        commands: [
            text(labels.decision)?,
            text(labels.restore)?,
            text(labels.cancel)?,
        ],
    };
    data.validate()?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both original executables; publishes only rename JSON"]
    fn original_rename_ui_preserves_menu_projection() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("rename-ui"));
        let result = (|| -> Result<()> {
            let mut payloads = std::collections::BTreeSet::new();
            for disc in [1, 2] {
                let file = local.join(format!("disc{disc}/sys/main.dol"));
                let executable = fs::read(&file)?;
                let (source, payload) =
                    crate::all_assets::rename_ui::tests::recover(&file, &executable, &output)?;
                payloads.insert(payload);
                let label = |index: u32| -> Result<String> {
                    let pointer =
                        crate::read::u32(dol::slice(&executable, 0x8019d3e8 + index * 4, 4)?, 0)?;
                    dol::text(&executable, pointer)
                };
                let keyboard = crate::read::u32(dol::slice(&executable, 0x8035cf68, 4)?, 0)?;
                let expected = serde_json::json!({
                    "defaults":(6..15).map(label).collect::<Result<Vec<_>>>()?,
                    "keyboard":dol::text(&executable,keyboard)?,
                    "heading":label(0)?,"delete":label(1)?,"default":label(2)?,
                    "commands":[label(3)?,label(4)?,label(5)?],
                });
                assert_eq!(serde_json::to_value(cook(&source)?)?, expected);
            }
            assert_eq!(
                payloads.len(),
                1,
                "identical rename data must share a publication"
            );
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
