use std::path::PathBuf;

use anyhow::{Context, Result};
use cathedral_core::{
    create_experiment, delete_experiment, initialize, inspect_experiment, list_experiments,
    manifest::Manifest,
    platform::PlatformInfo,
};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "cathedral", version, about = "The Open Computing Laboratory")]
struct Cli {
    /// Override Cathedral's local data directory.
    #[arg(long, global = true, env = "CATHEDRAL_HOME")]
    home: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize Cathedral's local registry and workspace directory.
    Init,
    /// Create a managed experiment workspace.
    Create {
        name: String,
        /// Load and validate an existing cathedral.toml manifest.
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    /// List registered experiments.
    List,
    /// Display details for one experiment.
    Inspect { name: String },
    /// Delete an experiment and its managed workspace.
    Delete { name: String },
    /// Check local Cathedral storage and registry health.
    Doctor,
}

fn data_dir(override_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        return Ok(path);
    }
    if let Some(path) = std::env::var_os("CATHEDRAL_HOME") {
        return Ok(PathBuf::from(path));
    }
    let base = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .context("could not determine a platform data directory; pass --home")?;
    Ok(base.join("cathedral"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = data_dir(cli.home)?;
    match cli.command {
        Command::Init => {
            initialize(&root).context("failed to initialize Cathedral")?;
            println!("Cathedral initialized at {}", root.display());
        }
        Command::Create { name, manifest } => {
            let parsed = match manifest {
                Some(path) => Manifest::load(&path)
                    .with_context(|| format!("failed to load manifest {}", path.display()))?,
                None => Manifest::minimal(name.clone()),
            };
            let item = create_experiment(&root, &name, parsed)
                .with_context(|| format!("failed to create experiment '{name}'"))?;
            println!("Created experiment '{}'", item.name);
            println!("ID: {}", item.id);
            println!("Workspace: {}", item.workspace_path);
        }
        Command::List => {
            let items = list_experiments(&root)?;
            if items.is_empty() {
                println!("No experiments yet. Create one with: cathedral create <name>");
            } else {
                println!("{:<22} {:<36} CREATED", "NAME", "ID");
                for item in items {
                    println!("{:<22} {:<36} {}", item.name, item.id, item.created_at);
                }
            }
        }
        Command::Inspect { name } => {
            let item = inspect_experiment(&root, &name)?;
            println!("Name: {}", item.name);
            println!("ID: {}", item.id);
            println!("Description: {}", item.description);
            println!("Workspace: {}", item.workspace_path);
            println!("Manifest version: {}", item.manifest_version);
            println!("Created (Unix): {}", item.created_at);
        }
        Command::Delete { name } => {
            delete_experiment(&root, &name)
                .with_context(|| format!("failed to delete experiment '{name}'"))?;
            println!("Deleted experiment '{name}'");
        }
        Command::Doctor => {
            let registry = initialize(&root)?;
            let host = PlatformInfo::detect();
            println!("Cathedral storage: OK");
            println!("Registry: OK");
            println!("Registered experiments: {}", registry.list()?.len());
            println!("Host platform: {}", host.platform.label());
            println!("Host architecture: {}", host.architecture);
            println!("Workload isolation: planned (Phase 2)");
        }
    }
    Ok(())
}
