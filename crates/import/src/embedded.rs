//! Publish semantic JSON once while retaining every source module's provenance.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub(crate) fn read<T: DeserializeOwned>(output: &Path, family: &str, module: &str) -> Result<T> {
    read_with(family, module, |path| Ok(fs::read(output.join(path))?))
}

pub(crate) fn read_with<T: DeserializeOwned>(
    family: &str,
    module: &str,
    load: impl Fn(&str) -> Result<Vec<u8>>,
) -> Result<T> {
    #[derive(Deserialize)]
    struct Source {
        module: String,
        data: String,
    }
    let path = format!("embedded/{family}/{module}.json");
    resonance_content::validate_asset_path(&path)?;
    let source: Source =
        serde_json::from_slice(&load(&path).with_context(|| {
            format!("missing cooked {family} for {module}; run cook-all first")
        })?)?;
    ensure!(source.module == module, "wrong cooked source module");
    resonance_content::validate_asset_path(&source.data)?;
    ensure!(
        source.data.starts_with(&format!("embedded/{family}/")),
        "wrong cooked data family"
    );
    let bytes = load(&source.data)?;
    ensure!(
        source.data == format!("embedded/{family}/{}.json", crate::digest(&bytes)),
        "cooked {family} data changed; run cook-all again"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

pub(crate) fn write(
    file: &Path,
    output: &Path,
    family: &str,
    value: &impl Serialize,
    mut provenance: Value,
) -> Result<Vec<String>> {
    let module = file
        .file_name()
        .and_then(|name| name.to_str())
        .context("invalid module filename")?;
    let bytes = serde_json::to_vec(value)?;
    let data = format!("embedded/{family}/{}.json", crate::digest(&bytes));
    crate::write_atomic(&output.join(&data), &bytes)?;
    provenance["module"] = json!(module);
    provenance["source_sha256"] = json!(crate::digest(&fs::read(file)?));
    provenance["data"] = json!(data);
    let source = format!("embedded/{family}/{module}.json");
    crate::write_atomic(&output.join(&source), &serde_json::to_vec(&provenance)?)?;
    Ok(vec![data, source])
}

#[test]
fn shared_data_keeps_source_identity_and_rejects_wrong_links() -> Result<()> {
    let output = crate::temporary_path(&std::env::temp_dir().join("embedded-binding"));
    fs::create_dir(&output)?;
    let result = (|| -> Result<()> {
        let file = output.join("source.rel");
        fs::write(&file, b"source")?;
        assert!(read::<Vec<u8>>(&output, "tables", "source.rel").is_err());
        let paths = write(&file, &output, "tables", &vec![1u8, 2], json!({}))?;
        assert_eq!(read::<Vec<u8>>(&output, "tables", "source.rel")?, [1, 2]);
        fs::write(output.join(&paths[0]), b"[3,4]")?;
        assert!(read::<Vec<u8>>(&output, "tables", "source.rel").is_err());
        fs::write(output.join(&paths[0]), b"[1,2]")?;
        for (module, data) in [
            ("other.rel", paths[0].as_str()),
            ("source.rel", "../outside.json"),
            ("source.rel", "embedded/other/data.json"),
        ] {
            fs::write(
                output.join(&paths[1]),
                serde_json::to_vec(&json!({"module": module, "data": data}))?,
            )?;
            assert!(read::<Vec<u8>>(&output, "tables", "source.rel").is_err());
        }
        Ok(())
    })();
    fs::remove_dir_all(output)?;
    result
}
