use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use nod::read::{DiscOptions, DiscReader};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "resonance-import",
    version,
    about = "Cook the editable asset library, optionally extracting both disc images first"
)]
struct Args {
    /// Original GameCube disc 1 image (RVZ, ISO or GCM).
    #[arg(value_name = "DISC_1", requires = "disc2")]
    disc1: Option<PathBuf>,
    /// Original GameCube disc 2 image; only its nine additional files are read.
    #[arg(value_name = "DISC_2", requires = "disc1")]
    disc2: Option<PathBuf>,
    /// Output library to overwrite, also the engine's default asset directory.
    #[arg(long, default_value = "local/all-assets")]
    output: PathBuf,
    /// Automatic mods directory, matching the game's --mods-directory option.
    #[arg(long, global = true, default_value = "local/mods")]
    mods_directory: PathBuf,
    #[command(subcommand)]
    command: Option<Action>,
}

#[derive(Subcommand)]
enum Action {
    /// Build an additive arte mod from authored sources and a cooked reference pose.
    PrepareArteMod {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Inspect startup-resolved documents from granular assets and ordered mods.
    Resolve {
        #[arg(long)]
        assets: PathBuf,
        #[arg(long = "mod")]
        mods: Vec<PathBuf>,
        /// One logical document; omit to print all assembled documents.
        #[arg(long)]
        document: Option<String>,
    },
    /// Split a logical document into independently owned mod source files.
    SplitDocument {
        #[arg(long)]
        input: PathBuf,
        /// Logical runtime path, such as game/items.json.
        #[arg(long)]
        document: String,
        #[arg(long)]
        output: PathBuf,
    },
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
        #[arg(long, default_value = EXTRACTED_DISCS)]
        extracted: PathBuf,
        #[arg(long, default_value = "local/all-assets")]
        output: PathBuf,
    },
    /// Extract one original GameCube disc into a source directory.
    Extract {
        #[arg(long)]
        disc: PathBuf,
        /// Destination for this disc (default: local/extracted-discs/disc1 or disc2).
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

const EXTRACTED_DISCS: &str = "local/extracted-discs";

fn main() -> Result<()> {
    let args = Args::parse();
    let Some(command) = args.command else {
        let extracted = Path::new(EXTRACTED_DISCS);
        if let (Some(disc1), Some(disc2)) = (&args.disc1, &args.disc2) {
            if extracted.exists() {
                let extraction = extracted.canonicalize()?;
                for disc in [disc1, disc2] {
                    let source = disc
                        .canonicalize()
                        .with_context(|| format!("open disc image {}", disc.display()))?;
                    ensure!(
                        !source.starts_with(&extraction),
                        "disc image {} must be outside extraction directory {}",
                        disc.display(),
                        extracted.display()
                    );
                }
                fs::remove_dir_all(extracted)?;
            }
            eprintln!("Reading disc 1: {}", disc1.display());
            extract(disc1, None, Some(0))?;
            eprintln!("Reading disc 2 supplement: {}", disc2.display());
            extract(disc2, None, Some(1))?;
        }
        eprintln!("Cooking asset library: {}", args.output.display());
        cook(extracted, &args.output, &args.mods_directory)?;
        return Ok(());
    };
    match command {
        Action::PrepareArteMod {
            source,
            assets,
            output,
        } => {
            resonance_importer::prepare_arte_mod(&source, &assets, &output)?;
        }
        Action::Resolve {
            assets,
            mods,
            document,
        } => {
            let library = resonance_content::library::Library::open_paths(&assets, &mods)?;
            let value = if let Some(path) = document {
                library
                    .snapshot()
                    .json::<serde_json::Value>(&path, resonance_content::preparation::JSON_LIMIT)?
            } else {
                let documents = library
                    .resolved_documents()
                    .map(|(path, bytes)| {
                        Ok((
                            path.to_owned(),
                            serde_json::from_slice::<serde_json::Value>(&bytes)?,
                        ))
                    })
                    .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
                serde_json::json!({"documents":documents})
            };
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        Action::SplitDocument {
            input,
            document,
            output,
        } => {
            let value = serde_json::from_slice(&fs::read(input)?)?;
            resonance_importer::publish_document(&output, &document, &value)?;
        }
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
            cook(&extracted, &output, &args.mods_directory)?;
        }
        Action::Extract { disc, output } => extract(&disc, output.as_deref(), None)?,
    }
    Ok(())
}

fn cook(extracted: &Path, output: &Path, mods: &Path) -> Result<()> {
    resonance_importer::cook_all(extracted, output)?;
    install_ghost_wolf(output, mods)?;
    println!("Cooked assets: {}", output.display());
    Ok(())
}

fn install_ghost_wolf(assets: &Path, mods: &Path) -> Result<()> {
    let output = mods.join("000100-ghost-wolf");
    eprintln!("Installing Ghost Wolf: {}", output.display());
    resonance_importer::install_ghost_wolf(assets, &output)
        .context("install bundled Ghost Wolf mod")?;
    println!("Installed Ghost Wolf: {}", output.display());
    Ok(())
}

fn extract(disc_path: &Path, output: Option<&Path>, expected_disc: Option<u8>) -> Result<()> {
    let disc = DiscReader::new(disc_path, &DiscOptions::default())
        .with_context(|| format!("open disc image {}", disc_path.display()))?;
    let output = output.map(Path::to_path_buf).unwrap_or_else(|| {
        Path::new(EXTRACTED_DISCS).join(format!("disc{}", u16::from(disc.header().disc_num) + 1))
    });
    resonance_importer::extract_disc(disc_path, &output, expected_disc, &|_| {})
}
