use super::*;
use anyhow::Context;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};
use symphonia_constants::BattleEventId;
use symphonia_script_compiler::scenario::{self, Context as ScriptContext};

#[test]
#[ignore = "requires both complete extracted GameCube discs at local/extracted"]
fn original_disc_script_inputs_are_identical() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_EXTRACTED")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted"));
    let first = root.join("disc1");
    let second = root.join("disc2");
    let mut compared = 0;
    let mut compare = |relative: &Path| -> Result<()> {
        let original = fs::read(first.join(relative))
            .with_context(|| format!("read disc1 {}", relative.display()))?;
        let other = fs::read(second.join(relative))
            .with_context(|| format!("read disc2 {}", relative.display()))?;
        anyhow::ensure!(
            original == other,
            "disc-specific script input: {}",
            relative.display()
        );
        compared += 1;
        Ok(())
    };
    compare(Path::new("sys/main.dol"))?;
    compare(Path::new("files/field_sil.so"))?;
    compare(Path::new("files/btl.so"))?;
    for event in BattleEventId::ALL {
        compare(Path::new(&format!("files/btl_ev{:03}.so", event.raw())))?;
    }
    for (folder, extension, expected) in [("MAP", "bin", 501), ("CHT", "skp", 487)] {
        let files = |disc: &Path| -> Result<BTreeSet<_>> {
            let mut names = BTreeSet::new();
            for entry in fs::read_dir(disc.join("files").join(folder))? {
                let entry = entry?;
                if entry.file_type()?.is_file()
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|value| value == extension)
                {
                    names.insert(entry.file_name());
                }
            }
            Ok(names)
        };
        let names = files(&first)?;
        anyhow::ensure!(
            names == files(&second)?,
            "disc-specific {folder} file inventory"
        );
        assert_eq!(names.len(), expected, "{folder}");
        for name in names {
            compare(&Path::new("files").join(folder).join(name))?;
        }
    }
    assert_eq!(compared, 999);
    println!("{compared} original script input files are identical on both discs");
    Ok(())
}

#[test]
#[ignore = "requires the complete extracted GameCube input at local/extracted"]
fn original_scenarios_roundtrip_before_gameplay_corrections() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_EXTRACTED")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted"));
    let executable = fs::read(root.join("disc1/sys/main.dol"))?;
    let mut checked = 0;
    let mut matched_bytes = 0;
    let mut verify = |name: &str, original: &[u8]| -> Result<()> {
        let result = scenario::decompile_with_context(
            original,
            definitions(),
            ScriptContext::from_path(Path::new(name)),
        )
        .with_context(|| format!("decompile unmodified original {name}"))?;
        let compiled = scenario::compile(name, &result.source, definitions())?;
        anyhow::ensure!(compiled == original, "original bytes changed: {name}");
        checked += 1;
        matched_bytes += original.len();
        Ok(())
    };

    let phases = crate::dol::slice(
        &executable,
        crate::field_core::PHASES,
        crate::field_core::FIELD_COUNT * 24,
    )?;
    let mut fields = 0;
    let mut corrected_fields = BTreeSet::new();
    for (id, row) in phases.as_chunks::<24>().0.iter().enumerate() {
        let Some(archive) = crate::field_core::native_archive(&root, &executable, row, id)? else {
            continue;
        };
        let original = crate::field_core::optional_section(&archive, 6)?
            .context("field archive has no scenario section")?;
        let name = format!(
            "fields/{}/scripts/scenario.ssb",
            crate::field_core::field_name(id)
        );
        // Verify the archive member itself. Prepared gameplay corrections must
        // never become the input to this original-byte comparison.
        verify(&name, original)?;
        if crate::field_gameplay::prepare(id, original)?.as_ref() != original {
            corrected_fields.insert(id);
        }
        fields += 1;
    }
    assert_eq!(fields, 501);
    assert_eq!(corrected_fields, [126, 127, 139, 377, 413, 524].into());
    println!("{fields} unmodified field archive members matched");

    let (mappings, skits) = crate::skit_assets::sources(&root)?;
    let mut skit_owners = BTreeMap::new();
    let mut mapped_paths = BTreeSet::new();
    for (&id, path) in &skits {
        let name = mappings.target(u32::from(id))?;
        let (original, _) = crate::skit_assets::script(path)?;
        verify(&format!("skits/{name}/scenario.ssb"), &original)?;
        if let Some(previous) = skit_owners.insert(name, original.clone()) {
            assert_eq!(previous, original, "skit alias {id}: {name}");
        }
        mapped_paths.insert(path.clone());
    }
    assert_eq!(skits.len(), 475);
    assert_eq!(mapped_paths.len(), 475);
    assert_eq!(skit_owners.len(), 434);
    let mut unassigned = 0;
    for entry in fs::read_dir(root.join("disc1/files/CHT"))? {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "skp")
            || mapped_paths.contains(&path)
        {
            continue;
        }
        let (original, _) = crate::skit_assets::script(&path)?;
        verify(
            &format!(
                "skits/unassigned/{}/scenario.ssb",
                path.file_name().unwrap().to_str().unwrap()
            ),
            &original,
        )?;
        unassigned += 1;
    }
    assert_eq!(unassigned, 12);
    println!("487 original skit package members matched; 446 published owners");

    verify(
        "world/scripts/scenario.ssb",
        &fs::read(root.join("disc1/files/field_sil.so"))?,
    )?;
    let mut event_owners = BTreeSet::new();
    for event in BattleEventId::ALL {
        let id = event.raw();
        let original = fs::read(root.join(format!("disc1/files/btl_ev{id:03}.so")))?;
        verify(&format!("battle/scripts/event-{id:04}.ssb"), &original)?;
        event_owners.insert(original);
    }
    verify(
        "battle/scripts/common.ssb",
        &fs::read(root.join("disc1/files/btl.so"))?,
    )?;
    assert_eq!(event_owners.len(), 4);
    assert_eq!(
        fields + skit_owners.len() + unassigned + event_owners.len() + 2,
        953
    );
    assert_eq!(checked, 998);
    println!(
        "{checked} original scenario references matched ({matched_bytes} bytes); \
         953 published owners; exactly six existing field gameplay corrections"
    );
    Ok(())
}
