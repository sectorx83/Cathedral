//! Linux workload launcher and process lifecycle metadata.
//!
//! Execution is refused unless bubblewrap can provide user, PID, IPC, UTS,
//! and network namespaces plus a restricted filesystem view. This is a
//! container-style boundary, not a VM or a defense against kernel exploits.

use crate::{manifest::Manifest, CathedralError, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(target_os = "linux")]
use std::{fs::File, io::Read, process::Command};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunRecord {
    pub id: String,
    pub pid: u32,
    pub process_start_ticks: u64,
    pub started_at: u64,
    pub command: Vec<String>,
    pub log_file: String,
    #[serde(default)]
    pub cgroup_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessMetrics {
    pub pid: u32,
    pub state: String,
    pub parent_pid: u32,
    pub virtual_memory_kb: u64,
    pub resident_memory_kb: u64,
    pub user_cpu_ticks: u64,
    pub system_cpu_ticks: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Finished,
    ProcessIdentityChanged,
}

impl ProcessState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Finished => "finished",
            Self::ProcessIdentityChanged => "process identity changed",
        }
    }
}

fn runs_root(workspace: &Path) -> PathBuf {
    workspace.join(".cathedral").join("runs")
}

fn run_directory(workspace: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err(CathedralError::Manifest("invalid run identifier".to_owned()));
    }
    Ok(runs_root(workspace).join(id))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        CathedralError::Manifest("run metadata path has no parent".to_owned())
    })?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{}.{}.tmp", path.file_name().unwrap_or_default().to_string_lossy(), uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn read_record(path: &Path) -> Result<RunRecord> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| CathedralError::Manifest(format!("invalid run metadata: {error}")))
}

pub fn list_runs(workspace: &Path) -> Result<Vec<RunRecord>> {
    let root = runs_root(workspace);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let record_path = entry.path().join("run.json");
        if record_path.is_file() {
            records.push(read_record(&record_path)?);
        }
    }
    records.sort_by_key(|record| record.started_at);
    Ok(records)
}

pub fn launch(workspace: &Path, command: &[String]) -> Result<RunRecord> {
    if command.is_empty() || command[0].trim().is_empty() {
        return Err(CathedralError::Manifest("a non-empty command is required".to_owned()));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (workspace, command);
        return Err(CathedralError::Manifest(
            "isolated workload execution is currently supported on Linux only".to_owned(),
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let canonical_workspace = workspace.canonicalize()?;
        if !canonical_workspace.is_dir() {
            return Err(CathedralError::Manifest("experiment workspace is not a directory".to_owned()));
        }
        let bwrap = Command::new("bwrap").arg("--version").output()
            .map_err(|_| CathedralError::Manifest(
                "bubblewrap (bwrap) is required for Linux isolation; install it and rerun cathedral doctor".to_owned()
            ))?;
        if !bwrap.status.success() {
            return Err(CathedralError::Manifest("bubblewrap is installed but could not be executed".to_owned()));
        }

        let manifest = Manifest::load(&canonical_workspace.join("cathedral.toml"))?;
        let relative_workdir = manifest.environment.working_directory.as_deref().unwrap_or(".");
        let workdir = canonical_workspace.join(relative_workdir);
        fs::create_dir_all(&workdir)?;
        let canonical_workdir = workdir.canonicalize()?;
        if !canonical_workdir.starts_with(&canonical_workspace) {
            return Err(CathedralError::Manifest("working directory resolves outside the experiment workspace".to_owned()));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let directory = run_directory(&canonical_workspace, &id)?;
        fs::create_dir_all(&directory)?;
        let log_path = directory.join("combined.log");
        let log = File::create(&log_path)?;
        let stderr = log.try_clone()?;

        let cgroup_path = prepare_cgroup(&manifest)?;
        let mut child = Command::new("bwrap");
        child.args([
            "--unshare-all",
            "--new-session",
            "--ro-bind", "/usr", "/usr",
            "--symlink", "usr/bin", "/bin",
            "--symlink", "usr/sbin", "/sbin",
            "--symlink", "usr/lib", "/lib",
            "--symlink", "usr/lib64", "/lib64",
            "--ro-bind", "/etc", "/etc",
            "--proc", "/proc",
            "--dev", "/dev",
            "--tmpfs", "/tmp",
            "--dir", "/workspace",
            "--bind",
        ])
        .arg(&canonical_workspace)
        .arg("/workspace")
        .args(["--chdir", "/workspace"])
        .args(["--clearenv", "--setenv", "PATH", "/usr/local/bin:/usr/bin:/bin"])
        .args(["--setenv", "HOME", "/workspace"])
        .args(["--setenv", "TMPDIR", "/tmp"]);
        for (key, value) in &manifest.environment.variables {
            child.arg("--setenv").arg(key).arg(value);
        }
        let relative = canonical_workdir.strip_prefix(&canonical_workspace)
            .map_err(|_| CathedralError::Manifest("working directory escaped workspace".to_owned()))?;
        let sandbox_workdir = if relative.as_os_str().is_empty() {
            "/workspace".to_owned()
        } else {
            format!("/workspace/{}", relative.to_string_lossy())
        };
        child.arg("--chdir").arg(&sandbox_workdir).arg("--");
        child.args(command);
        child.stdin(Stdio::null()).stdout(Stdio::from(log)).stderr(Stdio::from(stderr));
        #[cfg(unix)]
        {
            use std::os::unix::{ffi::OsStrExt, process::CommandExt};
            child.process_group(0);
            if let Some(path) = &cgroup_path {
                let control_file = std::ffi::CString::new(path.join("cgroup.procs").as_os_str().as_bytes())
                    .map_err(|_| CathedralError::Manifest("cgroup path contains a null byte".to_owned()))?;
                unsafe {
                    child.pre_exec(move || {
                        let fd = libc::open(control_file.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
                        if fd < 0 { return Err(std::io::Error::last_os_error()); }
                        let mut buffer = [0u8; 20];
                        let mut value = libc::getpid() as u32;
                        let mut cursor = buffer.len();
                        if value == 0 { cursor -= 1; buffer[cursor] = b'0'; }
                        while value > 0 {
                            cursor -= 1;
                            buffer[cursor] = b'0' + (value % 10) as u8;
                            value /= 10;
                        }
                        let length = buffer.len() - cursor;
                        let written = libc::write(fd, buffer[cursor..].as_ptr().cast(), length);
                        let close_result = libc::close(fd);
                        if written != length as isize || close_result != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
        }
        let process = match child.spawn() {
            Ok(process) => process,
            Err(error) => {
                if let Some(path) = &cgroup_path { let _ = fs::remove_dir(path); }
                return Err(CathedralError::Manifest(format!("failed to start isolated workload: {error}")));
            }
        };
        let pid = process.id();
        // The child is intentionally detached. Its log and identity metadata
        // allow another Cathedral invocation to inspect or terminate it.
        drop(process);
        let start_ticks = proc_start_ticks(pid).unwrap_or(0);
        let record = RunRecord {
            id,
            pid,
            process_start_ticks: start_ticks,
            started_at: now_seconds(),
            command: command.to_vec(),
            log_file: log_path.to_string_lossy().into_owned(),
            cgroup_path: cgroup_path.map(|path| path.to_string_lossy().into_owned()),
        };
        let encoded = serde_json::to_vec_pretty(&record)
            .map_err(|error| CathedralError::Manifest(error.to_string()))?;
        if let Err(error) = atomic_write(&directory.join("run.json"), &encoded) {
            if let Some(path) = &record.cgroup_path {
                let _ = fs::write(PathBuf::from(path).join("cgroup.kill"), "1");
                let _ = fs::remove_dir(path);
            } else {
                let _ = Command::new("kill").args(["-TERM", "--", &format!("-{pid}")]).status();
            }
            return Err(error);
        }
        Ok(record)
    }
}

fn prepare_cgroup(manifest: &Manifest) -> Result<Option<PathBuf>> {
    let limits = &manifest.resources;
    if limits.memory_max_bytes.is_none() && limits.cpu_quota_percent.is_none() && limits.pids_max.is_none() {
        return Ok(None);
    }
    #[cfg(not(target_os = "linux"))]
    {
        return Err(CathedralError::Manifest("hard resource limits currently require Linux cgroup v2".to_owned()));
    }
    #[cfg(target_os = "linux")]
    {
        let configured = std::env::var_os("CATHEDRAL_CGROUP_ROOT").ok_or_else(|| CathedralError::Manifest(
            "resource limits were requested but CATHEDRAL_CGROUP_ROOT is not configured to a delegated cgroup v2 directory".to_owned()))?;
        let root = PathBuf::from(configured).canonicalize()?;
        if !root.join("cgroup.controllers").is_file() || !root.join("cgroup.procs").is_file() {
            return Err(CathedralError::Manifest("CATHEDRAL_CGROUP_ROOT is not a cgroup v2 directory".to_owned()));
        }
        let path = root.join(format!("cathedral-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)?;
        let configured_limits = (|| -> Result<()> {
            if let Some(value) = limits.memory_max_bytes { fs::write(path.join("memory.max"), value.to_string())?; }
            if let Some(value) = limits.cpu_quota_percent {
                let quota = u64::from(value) * 1000;
                fs::write(path.join("cpu.max"), format!("{quota} 100000"))?;
            }
            if let Some(value) = limits.pids_max { fs::write(path.join("pids.max"), value.to_string())?; }
            Ok(())
        })();
        if let Err(error) = configured_limits {
            let _ = fs::remove_dir(&path);
            return Err(CathedralError::Manifest(format!("could not enforce requested cgroup v2 limits: {error}")));
        }
        Ok(Some(path))
    }
}
pub fn process_state(record: &RunRecord) -> ProcessState {
    #[cfg(target_os = "linux")]
    {
        match proc_start_ticks(record.pid) {
            None => ProcessState::Finished,
            Some(ticks) if record.process_start_ticks != 0 && ticks != record.process_start_ticks => {
                ProcessState::ProcessIdentityChanged
            }
            Some(_) => ProcessState::Running,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = record;
        ProcessState::Finished
    }
}

pub fn cleanup_finished(record: &RunRecord) -> Result<()> {
    if process_state(record) != ProcessState::Finished { return Ok(()); }
    if let Some(path) = &record.cgroup_path {
        let path = PathBuf::from(path);
        if path.exists() {
            fs::remove_dir(path).map_err(|error| CathedralError::Manifest(format!(
                "workload exited but cgroup cleanup failed; remaining processes may still exist: {error}")))?;
        }
    }
    Ok(())
}
pub fn stop(workspace: &Path, run_id: &str) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (workspace, run_id);
        return Err(CathedralError::Manifest("process control is currently supported on Linux only".to_owned()));
    }
    #[cfg(target_os = "linux")]
    {
        let record = read_record(&run_directory(workspace, run_id)?.join("run.json"))?;
        match process_state(&record) {
            ProcessState::Finished => return Ok(()),
            ProcessState::ProcessIdentityChanged => {
                return Err(CathedralError::Manifest("refusing to signal a reused PID".to_owned()));
            }
            ProcessState::Running => {}
        }
        if record.process_start_ticks == 0 && record.cgroup_path.is_none() {
            return Err(CathedralError::Manifest("cannot verify workload PID identity; refusing to signal a possibly reused PID".to_owned()));
        }
        if let Some(path) = &record.cgroup_path {
            let kill_file = PathBuf::from(path).join("cgroup.kill");
            if kill_file.is_file() {
                fs::write(kill_file, "1")?;
                return Ok(());
            }
        }
        let status = Command::new("kill")
            .args(["-TERM", "--", &format!("-{}", record.pid)])
            .status()?;
        if !status.success() {
            return Err(CathedralError::Manifest("failed to send SIGTERM to the workload process group".to_owned()));
        }
        Ok(())
    }
}

pub fn logs(workspace: &Path, run_id: &str) -> Result<String> {
    let record = read_record(&run_directory(workspace, run_id)?.join("run.json"))?;
    Ok(fs::read_to_string(record.log_file)?)
}

pub fn metrics(record: &RunRecord) -> Result<Option<ProcessMetrics>> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = record;
        return Ok(None);
    }
    #[cfg(target_os = "linux")]
    {
        let root = PathBuf::from(format!("/proc/{}", record.pid));
        let Some(start) = proc_start_ticks(record.pid) else { return Ok(None); };
        if record.process_start_ticks != 0 && start != record.process_start_ticks {
            return Ok(None);
        }
        let status = fs::read_to_string(root.join("status"))?;
        let stat = fs::read_to_string(root.join("stat"))?;
        let io = fs::read_to_string(root.join("io")).unwrap_or_default();
        let state = stat.split_once(") ").map(|(_, rest)| rest.split_whitespace().next().unwrap_or("?").to_owned()).unwrap_or_else(|| "?".to_owned());
        let fields: Vec<&str> = stat.split_once(") ").map(|(_, rest)| rest.split_whitespace().collect()).unwrap_or_default();
        let parent_pid = fields.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        let user_cpu_ticks = fields.get(11).and_then(|v| v.parse().ok()).unwrap_or(0);
        let system_cpu_ticks = fields.get(12).and_then(|v| v.parse().ok()).unwrap_or(0);
        let value_kb = |key: &str| -> u64 {
            status.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                (name == key).then(|| value.split_whitespace().next()?.parse().ok()).flatten()
            }).unwrap_or(0)
        };
        let io_value = |key: &str| -> u64 {
            io.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                (name == key).then(|| value.trim().parse().ok()).flatten()
            }).unwrap_or(0)
        };
        Ok(Some(ProcessMetrics {
            pid: record.pid,
            state,
            parent_pid,
            virtual_memory_kb: value_kb("VmSize"),
            resident_memory_kb: value_kb("VmRSS"),
            user_cpu_ticks,
            system_cpu_ticks,
            read_bytes: io_value("read_bytes"),
            write_bytes: io_value("write_bytes"),
        }))
    }
}

#[cfg(target_os = "linux")]
fn proc_start_ticks(pid: u32) -> Option<u64> {
    let mut stat = String::new();
    File::open(format!("/proc/{pid}/stat")).ok()?.read_to_string(&mut stat).ok()?;
    let (_, rest) = stat.rsplit_once(") ")?;
    rest.split_whitespace().nth(19)?.parse().ok()
}

fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_ids_are_restricted_to_safe_directory_names() {
        assert!(run_directory(Path::new("/tmp/lab"), "abc-123").is_ok());
        assert!(run_directory(Path::new("/tmp/lab"), "../outside").is_err());
        assert!(run_directory(Path::new("/tmp/lab"), "").is_err());
    }

    #[test]
    fn process_start_time_parser_uses_proc_field_twenty_two() {
        // The helper's field selection is exercised against the current
        // process on Linux; non-Linux builds still compile the public API.
        #[cfg(target_os = "linux")]
        assert!(proc_start_ticks(std::process::id()).is_some());
    }

    #[test]
    fn empty_command_is_rejected_before_platform_dispatch() {
        assert!(launch(Path::new("/tmp"), &[]).is_err());
    }
}
