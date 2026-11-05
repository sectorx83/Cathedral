use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use cathedral_core::{
    create_experiment, delete_experiment, initialize, inspect_experiment, list_experiments,
    manifest::Manifest, platform::PlatformInfo, runner, workspace::Workspace,
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
    /// Start a command inside the Linux isolated experiment environment.
    Run {
        name: String,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Show the latest workload state and resource measurements.
    Monitor {
        name: String,
        #[arg(long)]
        watch: bool,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
        interval_seconds: u64,
    },
    /// Display captured workload output. Defaults to the latest run.
    Logs { name: String, run_id: Option<String> },
    /// List workload processes and their current resource measurements.
    Processes { name: String },
    /// Stop a workload process group by run identifier.
    Stop { name: String, run_id: String },
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
                println!("{:<22} {:<36} {:<10} CREATED", "NAME", "ID", "STATE");
                for item in items {
                    println!(
                        "{:<22} {:<36} {:<10} {}",
                        item.name,
                        item.id,
                        item.state.as_str(),
                        item.created_at
                    );
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
            println!("State: {}", item.state.as_str());
            println!("Created (Unix): {}", item.created_at);
        }
        Command::Delete { name } => {
            delete_experiment(&root, &name)
                .with_context(|| format!("failed to delete experiment '{name}'"))?;
            println!("Deleted experiment '{name}'");
        }
        Command::Run { name, command } => {
            let item = inspect_experiment(&root, &name)?;
            let _workspace = Workspace::new(&root)?.validate(Path::new(&item.workspace_path))?;
            let run_root = root.canonicalize()?.join("runs").join(&item.id);
            let record = runner::launch(&workspace, &run_root, &command)?;
            println!("Started isolated run {}", record.id);
            println!("PID: {}", record.pid);
            println!("Command: {}", record.command.join(" "));
            println!("Logs: {}", record.log_file);
        }
        Command::Monitor { name, watch, interval_seconds } => {
            let item = inspect_experiment(&root, &name)?;
            let _workspace = Workspace::new(&root)?.validate(Path::new(&item.workspace_path))?;
            let run_root = root.canonicalize()?.join("runs").join(&item.id);
            let records = runner::list_runs(&run_root)?;
            let record = records.last().ok_or_else(|| anyhow::anyhow!("experiment has no recorded runs"))?;
            loop {
                runner::cleanup_finished(record)?;
                println!("Run: {} | PID: {} | State: {}", record.id, record.pid, runner::process_state(record).label());
                if let Some(metrics) = runner::metrics(record)? {
                    println!("Process: {} | Parent PID: {}", metrics.state, metrics.parent_pid);
                    println!("Memory: {} KiB RSS / {} KiB virtual", metrics.resident_memory_kb, metrics.virtual_memory_kb);
                    println!("CPU: {} user ticks / {} system ticks", metrics.user_cpu_ticks, metrics.system_cpu_ticks);
                    println!("Disk I/O: {} bytes read / {} bytes written", metrics.read_bytes, metrics.write_bytes);
                    if let Some(value) = metrics.cgroup_memory_current_bytes { println!("Cgroup memory: {} bytes", value); }
                    if let Some(value) = metrics.cgroup_cpu_usage_usec { println!("Cgroup CPU time: {} microseconds", value); }
                    if let Some(value) = metrics.cgroup_process_count { println!("Cgroup processes: {}", value); }
                } else {
                    println!("Process metrics: unavailable (workload exited)");
                }
                if !watch || runner::process_state(record) != runner::ProcessState::Running { break; }
                std::thread::sleep(std::time::Duration::from_secs(interval_seconds));
            }
        }
        Command::Logs { name, run_id } => {
            let item = inspect_experiment(&root, &name)?;
            let _workspace = Workspace::new(&root)?.validate(Path::new(&item.workspace_path))?;
            let run_root = root.canonicalize()?.join("runs").join(&item.id);
            let records = runner::list_runs(&run_root)?;
            let id = run_id.or_else(|| records.last().map(|record| record.id.clone()))
                .ok_or_else(|| anyhow::anyhow!("experiment has no recorded runs"))?;
            print!("{}", runner::logs(&run_root, &id)?);
        }
        Command::Processes { name } => {
            let item = inspect_experiment(&root, &name)?;
            let workspace = Workspace::new(&root)?.validate(Path::new(&item.workspace_path))?;
            let run_root = root.canonicalize()?.join("runs").join(&item.id);
            let records = runner::list_runs(&run_root)?;
            if records.is_empty() {
                println!("No workload runs recorded.");
            } else {
                println!("{:<36} {:<10} {:<12} {:>12} {:>12}", "RUN ID", "PID", "STATE", "RSS (KiB)", "READ (B)");
                for record in records {
                    runner::cleanup_finished(&record)?;
                    let state = runner::process_state(&record);
                    let processes = runner::processes(&record)?;
                    if processes.is_empty() {
                        println!("{:<36} {:<10} {:<12} {:>12} {:>12}", record.id, record.pid, state.label(), 0, 0);
                    } else {
                        for process in processes {
                            println!("{:<36} {:<10} {:<12} {:>12} {:>12}", record.id, process.pid, process.state, process.resident_memory_kb, process.read_bytes);
                        }
                    }
                }
            }
        }
        Command::Stop { name, run_id } => {
            let item = inspect_experiment(&root, &name)?;
            let workspace = Workspace::new(&root)?.validate(Path::new(&item.workspace_path))?;
            let run_root = root.canonicalize()?.join("runs").join(&item.id);
            runner::stop(&run_root, &run_id)?;
            println!("Termination signal sent to run {run_id}");
        }
        Command::Doctor => {
            let registry = initialize(&root)?;
            registry.integrity_check()?;
            let items = registry.list()?;
            let workspace = Workspace::new(&root)?;
            let mut issues = Vec::new();
            let mut known_workspaces = HashSet::new();

            for item in &items {
                if item.state.as_str() == "deleting" {
                    issues.push(format!(
                        "experiment '{}' has an interrupted deletion; rerun cathedral delete {}",
                        item.name, item.name
                    ));
                }
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
                    }
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
            let runtime = cathedral_core::runtime::RuntimeProbe::inspect();
            println!("Runtime host prerequisites:");
            println!("  bubblewrap: {}", runtime.bubblewrap.label());
            println!("  unshare utility: {}", runtime.unshare_binary.label());
            println!(
                "  user/PID/mount namespaces: {}",
                runtime.user_pid_mount_namespaces.label()
            );
            println!("  network namespace: {}", runtime.network_namespace.label());
            println!("  cgroup v2: {}", runtime.cgroup_v2.label());
            println!(
                "  cgroup CPU controller: {}",
                runtime.cpu_controller.label()
            );
            println!(
                "  cgroup memory controller: {}",
                runtime.memory_controller.label()
            );
            let namespace_ready = runtime.bubblewrap == cathedral_core::runtime::ProbeStatus::Available
                && runtime.user_pid_mount_namespaces == cathedral_core::runtime::ProbeStatus::Available
                && runtime.network_namespace == cathedral_core::runtime::ProbeStatus::Available;
            println!("Linux isolation prerequisites: {}", if namespace_ready { "available" } else { "incomplete" });
            let cgroup_root = std::env::var_os("CATHEDRAL_CGROUP_ROOT").map(PathBuf::from);
            let cgroup_root_status = cgroup_root.as_ref().map(|path| {
                if path.join("cgroup.controllers").is_file() && path.join("cgroup.procs").is_file() {
                    "configured (delegation is verified at run time)"
                } else {
                    "invalid or not a cgroup v2 directory"
                }
            }).unwrap_or("not configured");
            println!("Delegated cgroup root: {cgroup_root_status}");
            println!("Windows/macOS workload isolation: unsupported in this release");

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
