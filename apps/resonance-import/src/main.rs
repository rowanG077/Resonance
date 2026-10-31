use anyhow::{Context, Result, ensure};
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
#[command(
    about = "Import Symphonia disc images into the editable asset library",
    subcommand_negates_reqs = true
)]
struct Args {
    /// Original GameCube disc 1 image (RVZ, ISO or GCM).
    #[arg(value_name = "DISC_1", required = true)]
    disc1: Option<PathBuf>,
    /// Original GameCube disc 2 image; only its nine additional files are read.
    #[arg(value_name = "DISC_2", required = true)]
    disc2: Option<PathBuf>,
    /// Output library to overwrite, also the engine's default asset directory.
    #[arg(long, default_value = "local/all-assets")]
    output: PathBuf,
    #[command(subcommand)]
    command: Option<Action>,
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
    let args = Args::parse();
    let Some(command) = args.command else {
        let disc1 = args.disc1.context("missing disc 1 image")?;
        let disc2 = args.disc2.context("missing disc 2 image")?;
        let scratch = tempfile::tempdir()?;
        eprintln!("Reading disc 1: {}", disc1.display());
        extract(&disc1, &scratch.path().join("disc1"), Some(0))?;
        eprintln!("Reading disc 2 supplement: {}", disc2.display());
        extract(&disc2, &scratch.path().join("disc2"), Some(1))?;
        eprintln!("Cooking asset library: {}", args.output.display());
        resonance_importer::cook_all(scratch.path(), &args.output)?;
        println!("Cooked assets: {}", args.output.display());
        return Ok(());
    };
    match command {
        Action::InspectEffectTemplates => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes)?;
            println!("{}", resonance_importer::inspect_effect_templates(&bytes)?);
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
                resonance_importer::bind_fixture_motion(
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
            resonance_importer::cook_all(&extracted, &output)?;
            println!("Cooked assets: {}", output.display());
        }
        Action::Extract { disc, output } => extract(&disc, &output, None)?,
    }
    Ok(())
}

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

fn extract(disc_path: &Path, output: &Path, expected_disc: Option<u8>) -> Result<()> {
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
