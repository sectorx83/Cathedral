# Platform Support Strategy

## Current status

Phase 1 host detection and experiment management compile on Linux, Windows, and macOS. Phase 2 implements the first workload backend on Linux using Bubblewrap namespaces and a restricted filesystem view. Optional hard memory, CPU, and process-count limits use a delegated cgroup v2 subtree. Windows and macOS do not yet have workload backends; their runtime commands fail explicitly.

Linux is the first runtime implementation target. Windows and macOS remain first-class architectural targets, but their functionality must be delivered through native backends and tested independently.

## Shared contract

The shared core defines platform-neutral requests and results. Platform adapters report capabilities at runtime. A capability response should include:

- Capability identifier.
- Availability status.
- Whether enforcement is guaranteed by the backend or only advisory.
- Required host configuration and permissions.
- Relevant limitations and diagnostic details.

The CLI and future Studio must display these differences clearly. They must not imply equivalent isolation simply because the same configuration field exists on each platform.

## Linux

Implemented first backend:

- Bubblewrap user, PID, IPC, UTS, and network namespaces.
- Read-only `/usr` and `/etc`, writable experiment workspace, private `/tmp`, and a cleared inherited environment.
- Optional cgroup v2 `memory.max`, `cpu.max`, and `pids.max` controls; requested limits fail closed when delegation is unavailable.
- Process identity tracking, process-group/cgroup termination, combined logs, and procfs/cgroup resource samples.
- Runtime prerequisite checks through `cathedral doctor`.

Availability depends on kernel configuration, Bubblewrap installation, distribution mount layout, unprivileged user namespace policy, and delegated cgroup permissions. Cathedral checks namespace and Bubblewrap prerequisites before launch. Resource limits require the requested controller files to be writable in the configured delegated cgroup subtree.

## Windows

Planned investigation:

- Job Objects for process grouping and resource controls.
- Windows-specific filesystem and process isolation mechanisms.
- AppContainer or other suitable restricted execution facilities where applicable.
- Virtualization-based isolation for stronger boundaries when available and justified.

Job Objects alone do not provide a complete filesystem or security sandbox. The backend must disclose which protections are actually active.

## macOS

Planned investigation:

- Supported sandboxing facilities and entitlements.
- Virtualization.framework or another suitable VM-backed mechanism where required.
- Resource and process observation through supported system interfaces.

The backend must account for platform policy, entitlements, and differences between sandboxed processes and virtual machines.

## Compatibility policy

- A package records its required architecture, runtime features, dependencies, and manifest version.
- Import validates the package before modifying an existing experiment.
- Reproduction checks required capabilities before execution.
- Unsupported required features cause a clear compatibility failure.
- Optional features may be omitted only when the manifest explicitly allows it.
- Host-specific differences are included in the reproduction report.

## Support claims

A platform is not considered supported merely because the code compiles for it. Support requires automated tests, runtime verification on that operating system, documented prerequisites, and an honest statement of limitations.
