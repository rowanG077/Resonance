use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use symphonia_script_compiler::{ScriptKind, SourceResolver, check, native_reference, script_kind};
use symphonia_script_tools::{SourceTree, StandardSources};

#[derive(Parser)]
#[command(about = "Check and format SymphoniaScript using Resonance's native API")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum Host {
    Field,
    World,
    Model,
}

impl Host {
    fn declarations(self) -> Vec<symphonia_script::authored::NativeDeclaration> {
        match self {
            Self::Field => resonance_events::authored::native_declarations(),
            Self::World => resonance_game::overworld::scripts::native_declarations(),
            Self::Model => resonance_model_behavior::native_declarations(),
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Compile modules and their imports without starting the game.
    Check {
        /// Select a runtime host; field/model entries otherwise use their declared mode.
        #[arg(long)]
        host: Option<Host>,
        /// Resolve std modules from this cooked asset directory.
        #[arg(long)]
        assets: Option<PathBuf>,
        root: String,
        #[arg(required = true)]
        modules: Vec<String>,
    },
    /// Format sources, preserving comments; defaults to all modules in ROOT.
    Fmt {
        #[arg(long)]
        check: bool,
        root: String,
        modules: Vec<String>,
    },
    /// Print the selected host's actual native declarations.
    Api { host: Host },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Check {
            host,
            assets,
            root,
            modules,
        } => {
            let project = if assets.is_some() {
                SourceTree::load_project(root)?
            } else {
                SourceTree::load(root)?
            };
            let standard = assets
                .map(|assets| SourceTree::load(assets.join("scripts")))
                .transpose()?;
            let sources = StandardSources {
                project: &project,
                standard: standard.as_ref().unwrap_or(&project),
            };
            for module in &modules {
                let source = sources
                    .source(module)
                    .with_context(|| format!("module not found: {module}"))?;
                let kind = script_kind(module, source)?;
                let selected = host.or(match kind {
                    ScriptKind::Field => Some(Host::Field),
                    ScriptKind::Model => Some(Host::Model),
                    ScriptKind::Library => None,
                });
                anyhow::ensure!(
                    !matches!(
                        (kind, selected),
                        (ScriptKind::Field, Some(Host::Model))
                            | (ScriptKind::Model, Some(Host::Field | Host::World))
                    ),
                    "{kind} script is incompatible with the selected host"
                );
                let natives = match selected {
                    Some(host) => host.declarations(),
                    None => Vec::new(),
                };
                check(module, &sources, &natives)
                    .with_context(|| if kind == ScriptKind::Library && selected.is_none() {
                        format!("checking library '{module}' without host natives; for host services, check a field/model entry importing this library")
                    } else {
                        format!("checking {kind} script '{module}'")
                    })?;
            }
            println!("checked {} module(s)", modules.len());
        }
        Command::Fmt {
            check,
            root,
            modules,
        } => {
            let mut command = vec!["fmt".into()];
            if check {
                command.push("--check".into());
            }
            command.push(root);
            command.extend(modules);
            symphonia_script_tools::run(command, &[], &mut std::io::stdout().lock())?;
        }
        Command::Api { host } => {
            let natives = host.declarations();
            print!("{}", native_reference(&natives));
        }
    }
    Ok(())
}
