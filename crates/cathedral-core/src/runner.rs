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
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(target_os = "linux")]
use std::{fs::File, io::Read, process::{Command, Stdio}};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunRecord {
    pub id: String,
    pub pid: u32,
    #[serde(default)]
    pub supervisor_pid: u32,
    #[serde(default)]
    pub supervisor_start_ticks: u64,
    pub process_start_ticks: u64,
    pub started_at: u64,
    pub command: Vec<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    pub log_file: String,
    #[serde(default)]
    pub exit_status_file: String,
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
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub cgroup_memory_current_bytes: Option<u64>,
    pub cgroup_cpu_usage_usec: Option<u64>,
    pub cgroup_process_count: Option<u64>,
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

fn ensure_real_directory(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(CathedralError::UnsafePath(path.to_path_buf()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(create_error) if create_error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(create_error) => return Err(create_error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CathedralError::UnsafePath(path.to_path_buf()));
    }
    Ok(path.canonicalize()?)
}

fn validate_run_root(run_root: &Path) -> Result<PathBuf> {
    let runs_parent = run_root.parent().ok_or_else(|| CathedralError::UnsafePath(run_root.to_path_buf()))?;
    let data_root = runs_parent.parent().ok_or_else(|| CathedralError::UnsafePath(run_root.to_path_buf()))?;
    let canonical_data = data_root.canonicalize()?;
    let runs_metadata = fs::symlink_metadata(runs_parent)?;
    if runs_metadata.file_type().is_symlink() || !runs_metadata.is_dir() {
        return Err(CathedralError::UnsafePath(runs_parent.to_path_buf()));
    }
    let canonical_runs = runs_parent.canonicalize()?;
    if canonical_runs.parent() != Some(canonical_data.as_path()) {
        return Err(CathedralError::UnsafePath(canonical_runs));
    }
    let root_metadata = fs::symlink_metadata(run_root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(CathedralError::UnsafePath(run_root.to_path_buf()));
    }
    let canonical_root = run_root.canonicalize()?;
    if canonical_root.parent() != Some(canonical_runs.as_path()) {
        return Err(CathedralError::UnsafePath(canonical_root));
    }
    Ok(canonical_root)
}

fn ensure_run_root(run_root: &Path) -> Result<PathBuf> {
    let runs_parent = run_root.parent().ok_or_else(|| CathedralError::UnsafePath(run_root.to_path_buf()))?;
    ensure_real_directory(runs_parent)?;
    ensure_real_directory(run_root)?;
    validate_run_root(run_root)
}

fn run_directory(run_root: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err(CathedralError::Manifest("invalid run identifier".to_owned()));
    }
    let root = validate_run_root(run_root)?;
    let path = root.join(id);
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || path.canonicalize()?.parent() != Some(root.as_path()) {
        return Err(CathedralError::UnsafePath(path));
    }
    Ok(path)
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
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CathedralError::UnsafePath(path.to_path_buf()));
    }
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| CathedralError::Manifest(format!("invalid run metadata: {error}")))
}

pub fn list_runs(run_root: &Path) -> Result<Vec<RunRecord>> {
    let root = run_root.to_path_buf();
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(CathedralError::UnsafePath(root));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    }
    let root = validate_run_root(run_root)?;
    let mut records = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let record_path = entry.path().join("run.json");
        if let Ok(file_metadata) = fs::symlink_metadata(&record_path) {
            if file_metadata.file_type().is_symlink() || !file_metadata.is_file() {
                return Err(CathedralError::UnsafePath(record_path));
            }
            let record = read_record(&record_path)?;
            cleanup_finished(&record)?;
            records.push(record);
            }
        }
    }
    records.sort_by_key(|record| record.started_at);
    Ok(records)
}

pub fn launch(workspace: &Path, run_root: &Path, command: &[String]) -> Result<RunRecord> {
    if command.is_empty() || command[0].trim().is_empty() {
        return Err(CathedralError::Manifest("a non-empty command is required".to_owned()));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (workspace, run_root, command);
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
        let mut workdir = canonical_workspace.clone();
        for component in Path::new(relative_workdir).components() {
            match component {
                std::path::Component::CurDir => continue,
                std::path::Component::Normal(part) => {
                    workdir.push(part);
                    match fs::symlink_metadata(&workdir) {
                        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                            return Err(CathedralError::UnsafePath(workdir));
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&workdir)?,
                        Err(error) => return Err(error.into()),
                    }
                }
                _ => return Err(CathedralError::Manifest("working directory contains an unsafe path component".to_owned())),
            }
        }
        let canonical_workdir = workdir.canonicalize()?;
        if !canonical_workdir.starts_with(&canonical_workspace) {
            return Err(CathedralError::Manifest("working directory resolves outside the experiment workspace".to_owned()));
        }

        let canonical_run_root = ensure_run_root(run_root)?;
        let id = uuid::Uuid::new_v4().to_string();
        let directory = canonical_run_root.join(&id);
        fs::create_dir(&directory)?;
        let log_path = directory.join("combined.log");
        let log = File::create(&log_path)?;
        let stderr = log.try_clone()?;

        if manifest.resources.timeout_seconds.is_some() {
            let supported = Command::new("timeout").arg("--help").output().is_ok_and(|output| {
                String::from_utf8_lossy(&output.stdout).contains("--kill-after")
                    || String::from_utf8_lossy(&output.stderr).contains("--kill-after")
            });
            if !supported {
                return Err(CathedralError::Manifest(
                    "this workload requests a timeout, but GNU coreutils timeout with --kill-after is unavailable".to_owned()
                ));
            }
        }
        let cgroup_path = prepare_cgroup(&manifest)?;
        let mut child = Command::new("bwrap");
        child.args([
            "--unshare-all",
            "--new-session",
            "--die-with-parent",
            "--ro-bind", "/usr", "/usr",
            "--symlink", "usr/bin", "/bin",
            "--symlink", "usr/sbin", "/sbin",
            "--symlink", "usr/lib", "/lib",
            "--symlink", "usr/lib64", "/lib64",
            "--dir", "/etc",
            "--ro-bind-try", "/etc/ssl", "/etc/ssl",
            "--ro-bind-try", "/etc/ca-certificates", "/etc/ca-certificates",
            "--ro-bind-try", "/etc/passwd", "/etc/passwd",
            "--ro-bind-try", "/etc/group", "/etc/group",
            "--ro-bind-try", "/etc/nsswitch.conf", "/etc/nsswitch.conf",
            "--ro-bind-try", "/etc/localtime", "/etc/localtime",
            "--ro-bind-try", "/etc/ld.so.cache", "/etc/ld.so.cache",
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
        let bwrap_args = child.get_args().map(|arg| arg.to_os_string()).collect::<Vec<_>>();
        let status_path = directory.join("exit-status");
        let mut supervisor = Command::new("/bin/sh");
        supervisor
            .args([
                "-c",
                "if [ -n \"$CATHEDRAL_TIMEOUT_SECONDS\" ]; then timeout --signal=TERM --kill-after=5s \"$CATHEDRAL_TIMEOUT_SECONDS\" bwrap \"$@\"; else bwrap \"$@\"; fi; status=$?; printf '%s\\n' \"$status\" > \"$CATHEDRAL_RUN_STATUS_FILE\"; exit \"$status\"",
                "cathedral-supervisor",
                "bwrap",
            ])
            .args(bwrap_args)
            .env("CATHEDRAL_RUN_STATUS_FILE", &status_path)
            .env("CATHEDRAL_TIMEOUT_SECONDS", manifest.resources.timeout_seconds.map(|value| value.to_string()).unwrap_or_default())
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr));
        #[cfg(unix)]
        {
            use std::os::unix::{ffi::OsStrExt, process::CommandExt};
            supervisor.process_group(0);
            if let Some(path) = &cgroup_path {
                let control_file = std::ffi::CString::new(path.join("cgroup.procs").as_os_str().as_bytes())
                    .map_err(|_| CathedralError::Manifest("cgroup path contains a null byte".to_owned()))?;
                unsafe {
                    supervisor.pre_exec(move || {
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
        let process = match supervisor.spawn() {
            Ok(process) => process,
            Err(error) => {
                if let Some(path) = &cgroup_path { let _ = fs::remove_dir(path); }
                return Err(CathedralError::Manifest(format!("failed to start isolated workload: {error}")));
            }
        };
        let supervisor_pid = process.id();
        let pid = wait_for_workload_pid(supervisor_pid).unwrap_or(supervisor_pid);
        let supervisor_start_ticks = proc_start_ticks(supervisor_pid).unwrap_or(0);
        if let Some(path) = &cgroup_path {
            if let Err(error) = spawn_cgroup_reaper(path, supervisor_pid, supervisor_start_ticks) {
                let _ = fs::write(path.join("cgroup.kill"), "1");
                let _ = Command::new("kill").args(["-TERM", "--", &format!("-{supervisor_pid}")]).status();
                return Err(error);
            }
        }
        drop(process);
        let start_ticks = proc_start_ticks(pid).unwrap_or(0);
        let record = RunRecord {
            id,
            pid,
            supervisor_pid,
            supervisor_start_ticks,
            process_start_ticks: start_ticks,
            started_at: now_seconds(),
            command: command.to_vec(),
            timeout_seconds: manifest.resources.timeout_seconds,
            log_file: log_path.to_string_lossy().into_owned(),
            exit_status_file: status_path.to_string_lossy().into_owned(),
            cgroup_path: cgroup_path.map(|path| path.to_string_lossy().into_owned()),
        };
        let encoded = serde_json::to_vec_pretty(&record)
            .map_err(|error| CathedralError::Manifest(error.to_string()))?;
        if let Err(error) = atomic_write(&directory.join("run.json"), &encoded) {
            if let Some(path) = &record.cgroup_path {
                let _ = fs::write(PathBuf::from(path).join("cgroup.kill"), "1");
            }
            let _ = Command::new("kill").args(["-TERM", "--", &format!("-{supervisor_pid}")]).status();
            return Err(error);
        }
        Ok(record)
    }
}

#[cfg(target_os = "linux")]
fn spawn_cgroup_reaper(path: &Path, supervisor_pid: u32, start_ticks: u64) -> Result<()> {
    use std::os::unix::process::CommandExt;
    if start_ticks == 0 { return Ok(()); }
    let script = r#"
pid="$1"
expected="$2"
cgroup="$3"
while [ -r "/proc/$pid/stat" ]; do
    stat=$(cat "/proc/$pid/stat" 2>/dev/null) || break
    rest=${stat##*) }
    set -- $rest
    actual=${20}
    [ "$actual" = "$expected" ] || break
    sleep 0.1
done
[ -d "$cgroup" ] || exit 0
count=0
while [ -r "$cgroup/cgroup.procs" ] && [ -n "$(cat "$cgroup/cgroup.procs")" ] && [ "$count" -lt 50 ]; do
    if [ -w "$cgroup/cgroup.kill" ]; then printf '1' > "$cgroup/cgroup.kill" 2>/dev/null || true; fi
    sleep 0.1
    count=$((count + 1))
done
if [ -r "$cgroup/cgroup.procs" ] && [ -z "$(cat "$cgroup/cgroup.procs")" ]; then
    rmdir "$cgroup" 2>/dev/null || true
fi
"#;
    Command::new("/bin/sh")
        .args(["-c", script, "cathedral-cgroup-reaper"])
        .arg(supervisor_pid.to_string())
        .arg(start_ticks.to_string())
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
        .map_err(|error| CathedralError::Manifest(format!(
            "workload started, but automatic cgroup cleanup helper could not start: {error}"
        )))
}
fn prepare_cgroup(manifest: &Manifest) -> Result<Option<PathBuf>> {
    let limits = &manifest.resources;
    let has_limits = limits.memory_max_bytes.is_some()
        || limits.cpu_quota_percent.is_some()
        || limits.pids_max.is_some();
    let configured_root = std::env::var_os("CATHEDRAL_CGROUP_ROOT");
    if !has_limits && configured_root.is_none() {
        return Ok(None);
    }
    #[cfg(not(target_os = "linux"))]
    {
        return Err(CathedralError::Manifest("hard resource limits currently require Linux cgroup v2".to_owned()));
    }
    #[cfg(target_os = "linux")]
    {
        let configured = configured_root.ok_or_else(|| CathedralError::Manifest(
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
        let supervisor_pid = if record.supervisor_pid == 0 { record.pid } else { record.supervisor_pid };
        if let Some(ticks) = proc_start_ticks(supervisor_pid) {
            if record.supervisor_start_ticks != 0 && ticks != record.supervisor_start_ticks {
                return ProcessState::ProcessIdentityChanged;
            }
            if matches!(proc_state(supervisor_pid), Some('Z' | 'X')) {
                if let Some(path) = &record.cgroup_path {
                    let members = fs::read_to_string(PathBuf::from(path).join("cgroup.procs")).unwrap_or_default();
                    return if members.trim().is_empty() { ProcessState::Finished } else { ProcessState::Running };
                }
                return ProcessState::Finished;
            }
            return ProcessState::Running;
        }
        if let Some(path) = &record.cgroup_path {
            let members = fs::read_to_string(PathBuf::from(path).join("cgroup.procs")).unwrap_or_default();
            if !members.trim().is_empty() { return ProcessState::Running; }
        }
        let group_exists = unsafe {
            libc::kill(-(supervisor_pid as i32), 0) == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        };
        if group_exists { ProcessState::Running } else { ProcessState::Finished }
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
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CathedralError::UnsafePath(path));
        }
        let members = match fs::read_to_string(path.join("cgroup.procs")) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if members.trim().is_empty() {
            match fs::remove_dir(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(CathedralError::Manifest(format!(
                    "workload exited but cgroup cleanup failed; remaining processes may still exist: {error}"
                ))),
            }
        }
    }
    Ok(())
}
pub fn stop(run_root: &Path, run_id: &str) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (run_root, run_id);
        return Err(CathedralError::Manifest("process control is currently supported on Linux only".to_owned()));
    }
    #[cfg(target_os = "linux")]
    {
        let record = read_record(&run_directory(run_root, run_id)?.join("run.json"))?;
        cleanup_finished(&record)?;
        match process_state(&record) {
            ProcessState::Finished => return Ok(()),
            ProcessState::ProcessIdentityChanged => {
                return Err(CathedralError::Manifest("refusing to signal a reused PID".to_owned()));
            }
            ProcessState::Running => {}
        }
        if record.supervisor_start_ticks == 0 && record.cgroup_path.is_none() {
            return Err(CathedralError::Manifest("cannot verify workload PID identity; refusing to signal a possibly reused PID".to_owned()));
        }
        if let Some(path) = &record.cgroup_path {
            let cgroup = PathBuf::from(path);
            let metadata = fs::symlink_metadata(&cgroup)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(CathedralError::UnsafePath(cgroup));
            }
            let kill_file = cgroup.join("cgroup.kill");
            if !kill_file.is_file() {
                return Err(CathedralError::Manifest(
                    "this cgroup v2 kernel lacks cgroup.kill; refusing an incomplete process-tree stop".to_owned()
                ));
            }
            fs::write(&kill_file, "1")?;
            if !record.exit_status_file.is_empty() {
                let _ = fs::write(&record.exit_status_file, "signal:SIGKILL\n");
            }
            for _ in 0..500 {
                let members = fs::read_to_string(cgroup.join("cgroup.procs"))?;
                if members.trim().is_empty() {
                    fs::remove_dir(&cgroup).map_err(|error| CathedralError::Manifest(format!(
                        "workload stopped but its cgroup could not be removed: {error}"
                    )))?;
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            return Err(CathedralError::Manifest(
                "cgroup.kill was requested but processes remained after five seconds".to_owned()
            ));
        }
        let supervisor_pid = if record.supervisor_pid == 0 { record.pid } else { record.supervisor_pid };
        let status = Command::new("kill")
            .args(["-TERM", "--", &format!("-{supervisor_pid}")])
            .status()?;
        if !status.success() {
            return Err(CathedralError::Manifest("failed to send SIGTERM to the workload process group".to_owned()));
        }
        if !record.exit_status_file.is_empty() {
            let _ = fs::write(&record.exit_status_file, "signal:SIGTERM\n");
        }
        for _ in 0..100 {
            match process_state(&record) {
                ProcessState::Finished => return Ok(()),
                ProcessState::ProcessIdentityChanged => {
                    return Err(CathedralError::Manifest("workload PID identity changed while stopping".to_owned()));
                }
                ProcessState::Running => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
        let status = Command::new("kill")
            .args(["-KILL", "--", &format!("-{supervisor_pid}")])
            .status()?;
        if !status.success() {
            return Err(CathedralError::Manifest("workload did not stop after SIGTERM and SIGKILL failed".to_owned()));
        }
        for _ in 0..100 {
            match process_state(&record) {
                ProcessState::Finished => return Ok(()),
                ProcessState::ProcessIdentityChanged => {
                    return Err(CathedralError::Manifest("workload PID identity changed while stopping".to_owned()));
                }
                ProcessState::Running => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
        Err(CathedralError::Manifest("workload process group remained after SIGKILL".to_owned()))
    }
}

pub fn exit_status(record: &RunRecord) -> Option<i32> {
    if record.exit_status_file.is_empty() { return None; }
    fs::read_to_string(&record.exit_status_file).ok()?.trim().parse().ok()
}

pub fn termination_signal(record: &RunRecord) -> Option<String> {
    if record.exit_status_file.is_empty() { return None; }
    let value = fs::read_to_string(&record.exit_status_file).ok()?;
    value.trim().strip_prefix("signal:").map(str::to_owned)
}

pub fn timed_out(record: &RunRecord) -> bool {
    record.timeout_seconds.is_some() && exit_status(record).is_some_and(|code| code == 124 || code == 137)
}
pub fn logs(run_root: &Path, run_id: &str) -> Result<String> {
    let directory = run_directory(run_root, run_id)?;
    let _record = read_record(&directory.join("run.json"))?;
    let log_path = directory.join("combined.log");
    let metadata = fs::symlink_metadata(&log_path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CathedralError::UnsafePath(log_path));
    }
    Ok(fs::read_to_string(log_path)?)
}

pub fn processes(record: &RunRecord) -> Result<Vec<ProcessMetrics>> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = record;
        return Err(CathedralError::Manifest("process inspection is currently supported on Linux only".to_owned()));
    }
    #[cfg(target_os = "linux")]
    {
        if record.cgroup_path.is_none() && process_state(record) == ProcessState::ProcessIdentityChanged {
            return Ok(Vec::new());
        }
        let pids: Vec<u32> = if let Some(path) = &record.cgroup_path {
            match fs::read_to_string(PathBuf::from(path).join("cgroup.procs")) {
                Ok(contents) => contents.lines().filter_map(|line| line.trim().parse().ok()).collect(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(error.into()),
            }
        } else {
            let root_pid = if record.supervisor_pid == 0 { record.pid } else { record.supervisor_pid };
            process_tree_pids(root_pid)?
        };
        let mut result = Vec::new();
        for pid in pids {
            if matches!(proc_state(pid), Some('Z' | 'X')) { continue; }
            let mut process_record = record.clone();
            process_record.pid = pid;
            process_record.process_start_ticks = proc_start_ticks(pid).unwrap_or(0);
            process_record.cgroup_path = None;
            if let Some(snapshot) = metrics(&process_record)? { result.push(snapshot); }
        }
        result.sort_by_key(|item| item.pid);
        Ok(result)
    }
}
#[cfg(target_os = "linux")]
fn parse_network_device_bytes(contents: &str) -> (u64, u64) {
    contents.lines().filter_map(|line| {
        let (_, counters) = line.split_once(':')?;
        let fields: Vec<_> = counters.split_whitespace().collect();
        if fields.len() < 9 { return None; }
        Some((fields[0].parse::<u64>().ok()?, fields[8].parse::<u64>().ok()?))
    }).fold((0u64, 0u64), |(rx, tx), (next_rx, next_tx)| {
        (rx.saturating_add(next_rx), tx.saturating_add(next_tx))
    })
}

#[cfg(target_os = "linux")]
fn proc_network_device_bytes(pid: u32) -> (u64, u64) {
    fs::read_to_string(format!("/proc/{pid}/net/dev"))
        .map(|contents| parse_network_device_bytes(&contents))
        .unwrap_or_default()
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
        let (network_rx_bytes, network_tx_bytes) = proc_network_device_bytes(record.pid);
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
        let cgroup_memory_current_bytes = record.cgroup_path.as_ref().and_then(|path| {
            fs::read_to_string(PathBuf::from(path).join("memory.current")).ok()?.trim().parse().ok()
        });
        let cgroup_cpu_usage_usec = record.cgroup_path.as_ref().and_then(|path| {
            fs::read_to_string(PathBuf::from(path).join("cpu.stat")).ok()?.lines().find_map(|line| {
                let mut fields = line.split_whitespace();
                (fields.next()? == "usage_usec").then(|| fields.next()?.parse().ok()).flatten()
            })
        });
        let cgroup_process_count = record.cgroup_path.as_ref().and_then(|path| {
            fs::read_to_string(PathBuf::from(path).join("cgroup.procs")).ok().map(|value| {
                value.lines().filter(|line| !line.trim().is_empty()).count() as u64
            })
        });
        let snapshot = ProcessMetrics {
            pid: record.pid,
            state,
            parent_pid,
            virtual_memory_kb: value_kb("VmSize"),
            resident_memory_kb: value_kb("VmRSS"),
            user_cpu_ticks,
            system_cpu_ticks,
            read_bytes: io_value("read_bytes"),
            write_bytes: io_value("write_bytes"),
            network_rx_bytes,
            network_tx_bytes,
            cgroup_memory_current_bytes,
            cgroup_cpu_usage_usec,
            cgroup_process_count,
        };
        let history_path = PathBuf::from(&record.log_file).with_file_name("metrics.jsonl");
        let mut history = OpenOptions::new().create(true).append(true).open(history_path)?;
        let history_line = serde_json::json!({
            "timestamp": now_seconds(),
            "pid": snapshot.pid,
            "state": snapshot.state,
            "virtual_memory_kb": snapshot.virtual_memory_kb,
            "resident_memory_kb": snapshot.resident_memory_kb,
            "user_cpu_ticks": snapshot.user_cpu_ticks,
            "system_cpu_ticks": snapshot.system_cpu_ticks,
            "read_bytes": snapshot.read_bytes,
            "write_bytes": snapshot.write_bytes,
            "network_rx_bytes": snapshot.network_rx_bytes,
            "network_tx_bytes": snapshot.network_tx_bytes,
            "cgroup_memory_current_bytes": snapshot.cgroup_memory_current_bytes,
            "cgroup_cpu_usage_usec": snapshot.cgroup_cpu_usage_usec,
            "cgroup_process_count": snapshot.cgroup_process_count
        });
        writeln!(history, "{history_line}")?;
        Ok(Some(snapshot))
    }
}

#[cfg(target_os = "linux")]
fn process_tree_pids(root_pid: u32) -> Result<Vec<u32>> {
    let mut found = std::collections::HashSet::from([root_pid]);
    loop {
        let mut changed = false;
        for entry in fs::read_dir("/proc")? {
            let entry = entry?;
            let Some(pid) = entry.file_name().to_str().and_then(|value| value.parse::<u32>().ok()) else { continue; };
            if found.contains(&pid) { continue; }
            if proc_parent_pid(pid).is_some_and(|parent| found.contains(&parent)) { found.insert(pid); changed = true; }
        }
        if !changed { break; }
    }
    let mut pids: Vec<_> = found.into_iter().collect();
    pids.sort_unstable();
    Ok(pids)
}

#[cfg(target_os = "linux")]
fn proc_parent_pid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.rsplit_once(") ")?;
    rest.split_whitespace().nth(1)?.parse().ok()
}
#[cfg(target_os = "linux")]
fn wait_for_workload_pid(supervisor_pid: u32) -> Option<u32> {
    for _ in 0..100 {
        let children = fs::read_to_string(format!("/proc/{supervisor_pid}/task/{supervisor_pid}/children")).ok()?;
        if let Some(bwrap_pid) = children.split_whitespace().next().and_then(|value| value.parse::<u32>().ok()) {
            let nested = fs::read_to_string(format!("/proc/{bwrap_pid}/task/{bwrap_pid}/children")).unwrap_or_default();
            if let Some(pid) = nested.split_whitespace().next().and_then(|value| value.parse().ok()) { return Some(pid); }
            if proc_start_ticks(bwrap_pid).is_none() { return Some(bwrap_pid); }
        }
        if proc_start_ticks(supervisor_pid).is_none() { return None; }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}
#[cfg(target_os = "linux")]
fn proc_state(pid: u32) -> Option<char> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.rsplit_once(") ")?;
    rest.chars().next()
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
    #[cfg(target_os = "linux")]
    fn bubblewrap_works() -> bool {
        Command::new("bwrap").args([
            "--unshare-all", "--new-session", "--ro-bind", "/usr", "/usr",
            "--proc", "/proc", "--dev", "/dev", "--", "/usr/bin/true"
        ]).status().is_ok_and(|status| status.success())
    }

    #[test]
    fn run_ids_are_restricted_to_safe_directory_names() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("runs");
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("abc-123")).unwrap();
        assert!(run_directory(&root, "abc-123").is_ok());
        assert!(run_directory(&root, "../outside").is_err());
        assert!(run_directory(&root, "").is_err());
    }

    #[test]
    fn rejects_symlinked_run_metadata_roots() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let temp = tempfile::tempdir().unwrap();
            let outside = temp.path().join("outside");
            fs::create_dir(&outside).unwrap();
            let link = temp.path().join("linked-runs");
            symlink(&outside, &link).unwrap();
            assert!(list_runs(&link).is_err());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detached_cgroup_reaper_removes_empty_cgroup_after_supervisor_exit() {
        let temp = tempfile::tempdir().unwrap();
        let cgroup = temp.path().join("delegated-run");
        fs::create_dir(&cgroup).unwrap();
        fs::write(cgroup.join("cgroup.procs"), "").unwrap();
        let mut child = Command::new("/bin/sleep").arg("0.2").spawn().unwrap();
        let start_ticks = proc_start_ticks(child.id()).unwrap();
        spawn_cgroup_reaper(&cgroup, child.id(), start_ticks).unwrap();
        child.wait().unwrap();
        for _ in 0..150 {
            if !cgroup.exists() { break; }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!cgroup.exists(), "detached reaper left an empty cgroup behind");
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn parses_network_device_byte_counters_and_sums_interfaces() {
        let sample = r#"Inter-| Receive | Transmit
 face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed
  lo: 100 1 0 0 0 0 0 0 250 2 0 0 0 0 0 0
 eth0: 300 3 0 0 0 0 0 0 450 4 0 0 0 0 0 0
"#;
        assert_eq!(parse_network_device_bytes(sample), (400, 700));
    }
    #[test]
    fn process_start_time_parser_uses_proc_field_twenty_two() {
        // The helper's field selection is exercised against the current
        // process on Linux; non-Linux builds still compile the public API.
        #[cfg(target_os = "linux")]
        assert!(proc_start_ticks(std::process::id()).is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn launches_and_captures_a_bubblewrap_workload_when_available() {
        let available = Command::new("bwrap").args([
            "--unshare-all", "--new-session", "--ro-bind", "/usr", "/usr",
            "--symlink", "usr/bin", "/bin", "--dir", "/etc",
            "--ro-bind-try", "/etc/passwd", "/etc/passwd",
            "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
            "--", "/usr/bin/true"
        ]).status().is_ok_and(|status| status.success());
        if !available { return; }
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path();
        Manifest::minimal("runtime-test").write_to(&workspace.join("cathedral.toml")).unwrap();
        let run_root = temp.path().join("data").join("runs").join("runtime-test");
        fs::create_dir_all(run_root.parent().unwrap()).unwrap();
        let record = launch(workspace, &run_root, &["/usr/bin/printf".to_owned(), "cathedral-runtime-ok".to_owned()]).unwrap();
        let mut output = String::new();
        for _ in 0..100 {
            output = logs(&run_root, &record.id).unwrap();
            if output.contains("cathedral-runtime-ok") && exit_status(&record).is_some() { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(output.contains("cathedral-runtime-ok"), "workload output was not captured: {output}");
        assert_eq!(exit_status(&record), Some(0));
        assert!(record.pid > 0);

        let failing = launch(workspace, &run_root, &["/usr/bin/false".to_owned()]).unwrap();
        for _ in 0..100 {
            if exit_status(&failing).is_some() { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(exit_status(&failing), Some(1));

        let sleeping = launch(workspace, &run_root, &["/usr/bin/sleep".to_owned(), "30".to_owned()]).unwrap();
        assert_eq!(process_state(&sleeping), ProcessState::Running);
        stop(&run_root, &sleeping.id).unwrap();
        for _ in 0..100 {
            if process_state(&sleeping) == ProcessState::Finished { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(process_state(&sleeping), ProcessState::Finished);

        let mut timed_manifest = Manifest::minimal("runtime-test");
        timed_manifest.resources.timeout_seconds = Some(1);
        timed_manifest.write_to(&workspace.join("cathedral.toml")).unwrap();
        let timed = launch(workspace, &run_root, &["/usr/bin/sleep".to_owned(), "30".to_owned()]).unwrap();
        for _ in 0..300 {
            if exit_status(&timed).is_some() { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(timed_out(&timed));
        assert_eq!(exit_status(&timed), Some(124));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_exposes_only_workspace_and_private_tmp() {
        if !bubblewrap_works() { return; }
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        Manifest::minimal("filesystem-test").write_to(&workspace.join("cathedral.toml")).unwrap();
        let run_root = temp.path().join("data").join("runs").join("filesystem-test");
        fs::create_dir_all(run_root.parent().unwrap()).unwrap();
        let command = [
            "/usr/bin/sh".to_owned(),
            "-c".to_owned(),
            "test ! -e /etc/shadow && test ! -e /root && test -w /workspace && test -w /tmp && printf isolated > /workspace/sandbox-proof".to_owned(),
        ];
        let record = launch(&workspace, &run_root, &command).unwrap();
        for _ in 0..200 {
            if exit_status(&record).is_some() { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(exit_status(&record), Some(0));
        assert_eq!(fs::read_to_string(workspace.join("sandbox-proof")).unwrap(), "isolated");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn refuses_symlinked_working_directory() {
        use std::os::unix::fs::symlink;
        if !Command::new("bwrap").arg("--version").output().is_ok_and(|out| out.status.success()) {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let outside = temp.path().join("outside");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&outside).unwrap();
        symlink(&outside, workspace.join("linked")).unwrap();
        let mut manifest = Manifest::minimal("symlink-test");
        manifest.environment.working_directory = Some("linked".to_owned());
        manifest.write_to(&workspace.join("cathedral.toml")).unwrap();
        let run_root = temp.path().join("data").join("runs").join("symlink-test");
        fs::create_dir_all(run_root.parent().unwrap()).unwrap();
        assert!(launch(&workspace, &run_root, &["/usr/bin/true".to_owned()]).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stop_terminates_a_running_workload() {
        if !Command::new("bwrap").arg("--version").output().is_ok_and(|out| out.status.success()) {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        Manifest::minimal("stop-test").write_to(&workspace.join("cathedral.toml")).unwrap();
        let run_root = temp.path().join("data").join("runs").join("stop-test");
        fs::create_dir_all(run_root.parent().unwrap()).unwrap();
        let record = launch(&workspace, &run_root, &["/usr/bin/sleep".to_owned(), "30".to_owned()]).unwrap();
        assert_eq!(process_state(&record), ProcessState::Running);
        stop(&run_root, &record.id).unwrap();
        assert_eq!(process_state(&record), ProcessState::Finished);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn enforces_manifest_timeout_and_records_timeout_exit() {
        if !Command::new("bwrap").arg("--version").output().is_ok_and(|out| out.status.success())
            || !Command::new("timeout").arg("--help").output().is_ok_and(|out| out.status.success()) {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let mut manifest = Manifest::minimal("timeout-test");
        manifest.resources.timeout_seconds = Some(1);
        manifest.write_to(&workspace.join("cathedral.toml")).unwrap();
        let run_root = temp.path().join("data").join("runs").join("timeout-test");
        fs::create_dir_all(run_root.parent().unwrap()).unwrap();
        let record = launch(&workspace, &run_root, &["/usr/bin/sleep".to_owned(), "30".to_owned()]).unwrap();
        for _ in 0..300 {
            if process_state(&record) == ProcessState::Finished { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(process_state(&record), ProcessState::Finished);
        assert!(timed_out(&record));
        cleanup_finished(&record).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn refuses_to_stop_a_reused_process_identifier() {
        let record = RunRecord {
            id: "test-run".to_owned(),
            pid: std::process::id(),
            supervisor_pid: 0,
            supervisor_start_ticks: 1,
            process_start_ticks: 0,
            started_at: 0,
            command: vec!["test".to_owned()],
            timeout_seconds: None,
            log_file: String::new(),
            exit_status_file: String::new(),
            cgroup_path: None,
        };
        assert_eq!(process_state(&record), ProcessState::ProcessIdentityChanged);
    }
    #[test]
    fn empty_command_is_rejected_before_platform_dispatch() {
        assert!(launch(Path::new("/tmp"), Path::new("/tmp/data/runs/test"), &[]).is_err());
    }
}
