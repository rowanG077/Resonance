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
    Model,
    Battle,
}

#[derive(Subcommand)]
enum Command {
    /// Compile modules and their imports without starting the game.
    Check {
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
                let natives = match kind {
                    ScriptKind::Field => resonance_events::authored::native_declarations(),
                    ScriptKind::Model => resonance_model_behavior::native_declarations(),
                    ScriptKind::Battle => resonance_battle::native_declarations(),
                    ScriptKind::Library => Vec::new(),
                };
                check(module, &sources, &natives)
                    .with_context(|| if kind == ScriptKind::Library {
                        format!("checking library '{module}' without host natives; for host services, check a field/model/battle entry importing this library")
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
            let natives = match host {
                Host::Field => resonance_events::authored::native_declarations(),
                Host::Model => resonance_model_behavior::native_declarations(),
                Host::Battle => resonance_battle::native_declarations(),
            };
            print!("{}", native_reference(&natives));
        }
    }
    Ok(())
}
