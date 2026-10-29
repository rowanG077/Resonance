use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use nod::{
    common::PartitionKind,
    read::{DiscOptions, DiscReader, PartitionOptions},
};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Parser)]
#[command(about = "Extract a Symphonia disc or cook the editable asset library")]
struct Args {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Decode native effect declarations for an external observation.
    InspectEffectTemplates,
    /// Bind one observed package motion to its receiving party body.
    BindFixtureMotion {
        #[arg(long)]
        extracted: PathBuf,
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        package_clip: u16,
        #[arg(long)]
        character: u8,
        #[arg(long)]
        costume: u8,
        #[arg(long)]
        output: PathBuf,
    },
    /// Convert the extracted GameCube source into the editable asset library.
    CookAll {
        #[arg(long, default_value = "local/extracted")]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Extract one original GameCube disc into a source directory.
    Extract {
        #[arg(long)]
        disc: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    match Args::parse().command {
        Action::InspectEffectTemplates => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes)?;
            println!(
                "{}",
                resonance_importer_vnext::inspect_effect_templates(&bytes)?
            );
        }
        Action::BindFixtureMotion {
            extracted,
            package,
            package_clip,
            character,
            costume,
            output,
        } => {
            println!(
                "{}",
                resonance_importer_vnext::bind_fixture_motion(
                    &extracted,
                    &package,
                    package_clip,
                    character,
                    costume,
                    &output
                )?
            );
        }
        Action::CookAll { extracted, output } => {
            resonance_importer_vnext::cook_all(&extracted, &output)?;
            println!("Cooked assets: {}", output.display());
        }
        Action::Extract { disc, output } => extract(&disc, &output)?,
    }
    Ok(())
}

fn extract(disc_path: &Path, output: &Path) -> Result<()> {
    let disc = DiscReader::new(disc_path, &DiscOptions::default())?;
    let header = disc.header();
    ensure!(
        header.game_id_str() == "GQSEAF" && header.disc_version == 0 && header.is_gamecube(),
        "expected GameCube GQSEAF revision 0"
    );
    let mut partition =
        disc.open_partition_kind(PartitionKind::Data, &PartitionOptions::default())?;
    let meta = partition.meta()?;
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
    let fst = meta.fst().map_err(anyhow::Error::msg)?;
    let mut count = 0;
    for (_, node, name) in fst.iter() {
        if !node.is_file() {
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
        let target = output.join("files").join(name);
        fs::create_dir_all(target.parent().expect("file has a parent"))?;
        fs::write(target, bytes)?;
        count += 1;
    }
    println!("Extracted {count} files into {}", output.display());
    Ok(())
}
