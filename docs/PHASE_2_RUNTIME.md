# Phase 2 Runtime: Linux Implementation

## Execution model

The first supported backend is Linux. `cathedral run <experiment> -- <command> [args...]`
starts the command through Bubblewrap. It uses user, PID, IPC, UTS, and network
namespaces, a new session, a read-only system binary/configuration view, a
writable experiment workspace, and a private temporary filesystem. The inherited
host environment is cleared; `PATH`, `HOME`, `TMPDIR`, and explicitly declared
manifest variables are passed to the workload.

Network access is disabled by default. There is no network allowlist or host
network mode in this release.

Bubblewrap must be installed and the host must permit the namespace operations.
`cathedral doctor` reports the relevant prerequisites. Launch fails if
Bubblewrap is missing or cannot start the sandbox.

## Resource policy

Optional limits live in the manifest:

```toml
[resources]
memory_max_bytes = 536870912
cpu_quota_percent = 100
pids_max = 128
timeout_seconds = 3600
```

The values map to cgroup v2 `memory.max`, `cpu.max`, and `pids.max`. `timeout_seconds` uses GNU coreutils `timeout` to send `SIGTERM` at the deadline and `SIGKILL` after a five-second grace period. Valid durations are 1 second through 7 days.
Set `CATHEDRAL_CGROUP_ROOT` to a cgroup v2 subtree delegated to the current
user, with the relevant controllers enabled for child cgroups. For systemd
setups, the owning user service/scope must be configured with delegation
(`Delegate=yes`) and the `cpu`, `memory`, and/or `pids` controllers enabled in
that subtree. Do not point this variable at the global `/sys/fs/cgroup` root;
use a subtree actually delegated to the account running Cathedral. Cathedral
creates one child cgroup per run and attaches the Bubblewrap process before
it executes. If a requested controller file cannot be configured, launch
fails instead of silently dropping the limit.

When a delegated cgroup root is configured, runs without explicit limits are
also placed in per-run cgroups so aggregate process count, memory, and CPU
accounting can be read. Without a delegated cgroup root, process-level metrics
are sampled from procfs.

## Lifecycle and observability

- `run` stores a run ID, process identity, command, and combined output log.
- `monitor` reports process state, virtual/resident memory, CPU ticks, disk
  I/O counters, and cgroup aggregate metrics when available. Use `--watch`
  for repeated sampling and `--interval-seconds` to set the sample interval.
- `processes` lists recorded runs and their current resource samples.
- `logs` reads the combined stdout/stderr log for a selected or latest run.
- `stop` sends termination to the run's cgroup when available, otherwise to
  the workload process group.
- Samples requested through `monitor` or `processes` are appended to
  `runs/<experiment-id>/<run-id>/metrics.jsonl` under Cathedral's data directory, outside the writable experiment workspace.

The current CLI samples on demand. It does not provide a streaming dashboard,
automatic periodic history collection, disk-space quotas, or network traffic
accounting. Timeout policies are configured per experiment in the manifest; adaptive or per-command timeout profiles are not included.

## Platform status

Windows and macOS continue to support the Phase 1 experiment-management
commands, but do not yet have workload execution backends. Runtime commands
return an explicit unsupported-platform error on those systems.

## Security boundary

This is container-style isolation, not a virtual machine. Namespaces and
cgroups do not protect against kernel vulnerabilities, all host-kernel attack
surfaces, or every misconfiguration. The runner is early-stage and has not
been independently audited. Do not use it for hostile workloads requiring
a strong security boundary. Consider a microVM/full VM backend for that use.
