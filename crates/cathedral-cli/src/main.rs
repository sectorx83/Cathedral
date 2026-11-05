use std::{collections::HashSet, path::{Path, PathBuf}};

use anyhow::{bail, Context, Result};
use cathedral_core::{
    create_experiment, delete_experiment, initialize, inspect_experiment, list_experiments,
    manifest::Manifest, platform::PlatformInfo, workspace::Workspace,
};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

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
    /// Check local Cathedral storage, registry, and experiment consistency.
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
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_target(true)
        .init();

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
            registry.integrity_check()?;
            let items = registry.list()?;
            let workspace = Workspace::new(&root)?;
            let mut issues = Vec::new();
            let mut known_workspaces = HashSet::new();

            for item in &items {
                let path = Path::new(&item.workspace_path);
                match workspace.validate(path) {
                    Ok(valid_path) => {
                        known_workspaces.insert(valid_path.clone());
                        match Manifest::load(&valid_path.join("cathedral.toml")) {
                        Ok(manifest)
                            if manifest.experiment.name == item.name
                                && manifest.experiment.description == item.description
                                && manifest.manifest_version == item.manifest_version => {}
                        Ok(_) => issues.push(format!(
                            "experiment '{}' has manifest metadata that does not match the registry",
                            item.name
                        )),
                        Err(error) => issues.push(format!(
                            "experiment '{}' has an unreadable or invalid manifest: {error}",
                            item.name
                        )),
                        }
                    },
                    Err(error) => issues.push(format!(
                        "experiment '{}' has an invalid workspace: {error}",
                        item.name
                    )),
                }
            }

            for entry in std::fs::read_dir(root.join("experiments"))? {
                let entry = entry?;
                let path = entry.path();
                let metadata = std::fs::symlink_metadata(&path)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    issues.push(format!(
                        "managed workspace entry '{}' is not a real directory",
                        path.display()
                    ));
                    continue;
                }
                let canonical = path.canonicalize()?;
                if !known_workspaces.contains(&canonical) {
                    issues.push(format!(
                        "unregistered workspace '{}' was found",
                        path.display()
                    ));
                }
            }

            let host = PlatformInfo::detect();
            println!("Cathedral storage: OK");
            println!(
                "Registry integrity: OK (schema {})",
                registry.schema_version()?
            );
            println!("Registered experiments: {}", items.len());
            println!("Experiment workspaces checked: {}", items.len());
            println!("Host platform: {}", host.platform.label());
            println!("Host architecture: {}", host.architecture);
            println!("Workload isolation: planned (Phase 2)");

            if !issues.is_empty() {
                for issue in &issues {
                    eprintln!("Problem: {issue}");
                }
                bail!("doctor found {} consistency problem(s)", issues.len());
            }
            println!("Experiment consistency: OK");
        }
    }
    Ok(())
}
