//! Original disc extraction, shared by the command-line and first-run importers.
use anyhow::{Context, Result, ensure};
use nod::{
    common::PartitionKind,
    read::{DiscOptions, DiscReader, PartitionOptions},
};
use std::{
    fs,
    io::Read,
    path::{Component, Path},
};

const SUPPLEMENT: [&str; 9] = [
    "EV/sce_084_090.afs",
    "EV/sce_091_100.afs",
    "EV/sce_101_110.afs",
    "EV/sce_111_120.afs",
    "EV/sce_121_132.afs",
    "MOV/as2.h4m",
    "MOV/as3.h4m",
    "MOV/s09.h4m",
    "MOV/s10.h4m",
];

/// Recognize reusable extraction from both discs without reading their contents.
/// Check the executable and every disc-2 supplement, not just empty directories.
pub fn extracted_discs_available(root: &Path) -> bool {
    let Ok(root) = std::path::absolute(root) else {
        return false;
    };
    let regular_directory = |path: &Path| fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
    let regular_file = |path: &Path| {
        fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
            && path
                .parent()
                .is_some_and(|parent| parent.ancestors().all(&regular_directory))
    };
    regular_directory(&root.join("disc1/files"))
        && regular_file(&root.join("disc1/sys/main.dol"))
        && SUPPLEMENT
            .iter()
            .all(|file| regular_file(&root.join("disc2/files").join(file)))
}

/// Validate the supported region, revision and disc number without writing anything.
pub fn validate_disc(disc_path: &Path, expected_disc: u8) -> Result<()> {
    ensure!(
        fs::symlink_metadata(disc_path)
            .with_context(|| format!("open disc image {}", disc_path.display()))?
            .is_file(),
        "Choose a regular disc image file."
    );
    let disc = DiscReader::new(disc_path, &DiscOptions::default())
        .with_context(|| format!("open disc image {}", disc_path.display()))?;
    let header = disc.header();
    ensure!(
        header.game_id_str() == "GQSEAF" && header.disc_version == 0 && header.is_gamecube(),
        "Please use North American GameCube discs (GQSEAF, revision 0)."
    );
    ensure!(
        header.disc_num == expected_disc,
        "Expected disc {}, found disc {}.",
        expected_disc + 1,
        header.disc_num + 1
    );
    Ok(())
}

/// Extract an original disc, optionally reading only disc 2's supplement.
pub fn extract_disc(
    disc_path: &Path,
    output: &Path,
    expected_disc: Option<u8>,
    progress: &dyn Fn(crate::ImportProgress),
) -> Result<()> {
    let disc = DiscReader::new(disc_path, &DiscOptions::default())
        .with_context(|| format!("open disc image {}", disc_path.display()))?;
    let header = disc.header();
    ensure!(
        header.game_id_str() == "GQSEAF" && header.disc_version == 0 && header.is_gamecube(),
        "expected GameCube GQSEAF revision 0"
    );
    if let Some(number) = expected_disc {
        ensure!(
            header.disc_num == number,
            "expected disc {} at {}, found disc {}",
            number + 1,
            disc_path.display(),
            header.disc_num + 1
        );
    }
    let supplement_only = expected_disc == Some(1);
    let mut partition =
        disc.open_partition_kind(PartitionKind::Data, &PartitionOptions::default())?;
    let meta = partition.meta()?;
    if !supplement_only {
        fs::create_dir_all(output.join("sys"))?;
        for (name, bytes) in [
            ("boot.bin", meta.raw_boot.as_slice()),
            ("bi2.bin", meta.raw_bi2.as_slice()),
            ("apploader.img", meta.raw_apploader.as_ref()),
            ("main.dol", meta.raw_dol.as_ref()),
            ("fst.bin", meta.raw_fst.as_ref()),
        ] {
            fs::write(output.join("sys").join(name), bytes)?;
        }
    }
    let fst = meta.fst().map_err(anyhow::Error::msg)?;
    let total = fst
        .iter()
        .filter(|(_, node, name)| {
            node.is_file() && (!supplement_only || SUPPLEMENT.contains(&name.as_str()))
        })
        .count();
    let mut count = 0;
    progress(crate::ImportProgress::Extracting {
        disc: header.disc_num + 1,
        files: 0,
        total,
    });
    for (_, node, name) in fst.iter() {
        if !node.is_file() || supplement_only && !SUPPLEMENT.contains(&name.as_str()) {
            continue;
        }
        ensure!(
            !name.is_empty()
                && Path::new(&name)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "unsafe disc path {name}"
        );
        let mut bytes = Vec::new();
        partition.open_file(node)?.read_to_end(&mut bytes)?;
        let target = output.join("files").join(&name);
        fs::create_dir_all(target.parent().expect("file has a parent"))?;
        fs::write(target, bytes)?;
        count += 1;
        if count % 100 == 0 || count == total {
            progress(crate::ImportProgress::Extracting {
                disc: header.disc_num + 1,
                files: count,
                total,
            });
            eprintln!(
                "  [{count}/{total} files] Extracting {}: {name}",
                disc_path.display()
            );
        }
    }
    ensure!(
        !supplement_only || count == SUPPLEMENT.len(),
        "disc 2 is missing required supplemental files"
    );
    eprintln!("Read {count} source files from {}", disc_path.display());
    Ok(())
}
