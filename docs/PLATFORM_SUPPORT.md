# Platform Support Strategy

## Current status

Cathedral is at the documentation and planning stage. No operating system currently has an implemented or verified runtime backend.

Linux is the first implementation target. Windows and macOS remain first-class architectural targets, but their functionality must be delivered through native backends and tested independently.

## Shared contract

The shared core defines platform-neutral requests and results. Platform adapters report capabilities at runtime. A capability response should include:

- Capability identifier.
- Availability status.
- Whether enforcement is guaranteed by the backend or only advisory.
- Required host configuration and permissions.
- Relevant limitations and diagnostic details.

The UI and CLI must display these differences clearly. They must not imply equivalent isolation simply because the same configuration field exists on each platform.

## Linux

Planned first backend:

- Namespaces for process, mount, user, and network isolation where supported and correctly configured.
- cgroups v2 for resource accounting and enforcement.
- Filesystem isolation using appropriate mount and layering mechanisms.
- Explicit privilege and kernel-feature checks.
- Cleanup and recovery for interrupted workloads.

Availability depends on kernel configuration, distribution, delegated cgroup permissions, and user namespace policy. Cathedral must test these prerequisites before launching a workload.

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
