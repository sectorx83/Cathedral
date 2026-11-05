# Cathedral

[![Cathedral CI](https://github.com/sectorx83/Cathedral-/actions/workflows/ci.yml/badge.svg)](https://github.com/sectorx83/Cathedral-/actions/workflows/ci.yml)

### The Open Computing Laboratory

Cathedral is an open-source developer platform being built to manage isolated software experiments, observe how they behave, diagnose failures, and reproduce environments on other compatible machines.

Its goal is simple: make **"it works on my machine"** a problem that can be investigated and reproduced instead of a mystery.

Cathedral is designed for developers, students, researchers, and engineering teams who need a controlled place to test software and understand its behavior.

> **Project status:** Phase 1 is complete. Phase 2 is complete for the Linux beta backend: Bubblewrap isolation, optional cgroup v2 enforcement, process lifecycle controls, logs, exit/timeout diagnostics, and resource/network-namespace telemetry are implemented. Windows and macOS execution backends remain unsupported. Phase 3 and Phase 4 remain future work.

## What Cathedral is

Cathedral is an experiment laboratory, not a hosting platform or an operating system. An experiment is a declared workspace with configuration, dependencies, execution settings, observable activity, and (eventually) portable checkpoints.

The intended workflow is:

1. Define an experiment and its requirements.
2. Create a managed workspace.
3. Run it with platform-appropriate isolation and resource controls.
4. Observe processes, resource usage, logs, and failures.
5. Capture a checkpoint and export the experiment.
6. Recreate it on another compatible machine and inspect any differences.

## Implemented foundation

The completed Phase 1 Rust foundation provides:

- A CLI named `cathedral`.
- SQLite-backed local experiment metadata.
- Experiment create, list, inspect, and delete operations with persisted `ready` / `deleting` lifecycle states and resumable deletion.
- Version-1 TOML manifest parsing, environment validation, and declared dependency metadata (recorded only; not resolved or installed).
- Managed experiment workspace directories.
- Managed workspace path validation and symlink checks for validation and deletion.
- Atomic manifest writes and rollback-safe experiment creation.
- Registry schema validation, legacy marker migration, and SQLite integrity checks.
- A diagnostic `cathedral doctor` command that checks registry integrity, registered workspace/manifest consistency, and orphaned workspace directories.
- Structured tracing controlled by `RUST_LOG`.
- Host data-directory selection with a `CATHEDRAL_HOME` override.
- A cross-platform CI matrix for Linux, Windows, and macOS.

## Phase 2 Linux runtime

On Linux, Cathedral can launch a command through Bubblewrap with user, PID, IPC, UTS, and network namespaces. The sandbox exposes the experiment workspace as writable, system binaries and configuration as read-only, and a temporary filesystem at `/tmp`. The host environment is cleared; only a small default environment and manifest-declared variables are passed in.

The CLI provides:

```sh
cathedral run hello-lab -- /usr/bin/python3 app.py
cathedral monitor hello-lab
cathedral monitor hello-lab --watch --interval-seconds 2
cathedral processes hello-lab
cathedral logs hello-lab
cathedral stop hello-lab <run-id>
```

Install Bubblewrap and ensure unprivileged user namespaces are permitted. `cathedral doctor` checks the host prerequisites. Workloads have no network access by default.

Optional hard resource limits are declared in `cathedral.toml` and enforced through a user-delegated cgroup v2 subtree. The runtime records process exit codes, disk I/O, shared network-namespace byte counters, and appends samples to `metrics.jsonl` while `monitor --watch` or `processes` is used. Set `CATHEDRAL_CGROUP_ROOT` to that delegated directory. Cathedral refuses to start when a requested limit cannot be installed. The limits are not silently downgraded.

Linux namespaces and cgroups are container-style containment, not a virtual machine or a security guarantee against kernel vulnerabilities. Do not use this early runtime for hostile code requiring a hardened boundary. Windows and macOS currently provide experiment management only, not workload isolation.

## Build from source

Install the stable Rust toolchain, then run:

```sh
cargo build --workspace
cargo test --workspace
cargo run -p cathedral -- --help
```

Initialize Cathedral and create an experiment:

```sh
cargo run -p cathedral -- init
cargo run -p cathedral -- create hello-lab
cargo run -p cathedral -- list
cargo run -p cathedral -- inspect hello-lab
RUST_LOG=cathedral_core=debug,cathedral=debug cargo run -p cathedral -- doctor
cargo run -p cathedral -- delete hello-lab
```

To use a custom data directory, pass `--home /path/to/cathedral-data` before the subcommand or set `CATHEDRAL_HOME`.

## Experiment manifest

A manifest can be supplied when creating an experiment:

```toml
manifest_version = 1

[experiment]
name = "hello-lab"
description = "A small development experiment"

[environment]
working_directory = "."

[environment.variables]
RUST_LOG = "info"

[dependencies.packages]
serde = "^1.0"
```

```sh
cargo run -p cathedral -- create hello-lab --manifest ./cathedral.toml
```

Manifest values are configuration data. Declared dependency requirements are recorded as metadata only and are not resolved or installed. Resource limits are optional and require a delegated cgroup v2 root on Linux.

## Design principles

- **Reproducibility first:** record the inputs and configuration needed to recreate an experiment.
- **Isolation with honest guarantees:** use the strongest practical platform mechanisms and clearly document their limits.
- **Observable by default:** make process behavior, resource consumption, and failures inspectable.
- **Cross-platform architecture:** share the core model while using native backends where operating systems differ.
- **Portable, explicit configuration:** keep experiment definitions readable, versionable, and validated.
- **Open and extensible:** build a documented foundation that can be tested, audited, and extended.

## Platform strategy

Cathedral is intended for Linux, Windows, and macOS. Linux is the first implementation target. Cross-platform support is an architectural requirement, not a claim that all platforms already have equivalent functionality.

The shared core defines common concepts and capability interfaces. Platform backends will implement those interfaces using native facilities. Cathedral must report unavailable or weaker capabilities instead of silently pretending that behavior is identical.

## Roadmap

The project is organized into four phases:

| Phase | Focus | Intended outcome |
|---|---|---|
| 1 | Foundation & Core Engine | Reliable CLI, experiment lifecycle, manifest validation, registry, workspace management, and platform interfaces |
| 2 | Isolation & Observatory | Managed execution, resource controls, process inspection, logs, and runtime diagnostics |
| 3 | Snapshot & Reproduction Engine | Checkpoints, integrity checks, export/import, and reproducible environment reports |
| 4 | Cathedral Studio & Ecosystem | Desktop studio, advanced lab tools, plugin SDK, and platform releases |

See [ROADMAP.md](ROADMAP.md) for scope and completion criteria.

## Planned CLI beyond Phase 2

Snapshots and reproduction are **not available yet**:

```text
cathedral snapshot my-experiment
cathedral snapshots my-experiment
cathedral restore my-experiment <snapshot>
cathedral export my-experiment
cathedral import experiment.cath
cathedral reproduce experiment.cath
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md): system boundaries and technical design.
- [Roadmap](ROADMAP.md): the four approved phases and their acceptance criteria.
- [Platform support](docs/PLATFORM_SUPPORT.md): cross-platform strategy and capability reporting.
- [Experiment manifest](docs/EXPERIMENT_MANIFEST.md): version-1 `cathedral.toml` format.
- [Development setup](docs/DEVELOPMENT.md): development workflow and quality gates.
- [CI verification record](docs/CI_STATUS.md): latest source revision verified across Linux, Windows, and macOS.
- [Security model](SECURITY.md): threat model, isolation boundaries, and security commitments.
- [Contributing](CONTRIBUTING.md): contribution workflow and engineering standards.

## Technology direction

The core and CLI use Rust. SQLite stores local metadata. A Tauri + React + TypeScript desktop application is planned for the final phase. A local API is expected to use Axum with WebSockets where streaming updates are needed.

The Linux runtime currently uses Bubblewrap namespaces and a restricted mount view, with optional cgroup v2 resource controls. Windows and macOS native isolation backends remain future work.

## Security expectations

Experiments may contain untrusted code. The Linux runtime is an early container-style isolation layer, not a VM or a hardened security boundary for arbitrary hostile workloads. Do not run untrusted code that requires strong containment.

## License

Cathedral is distributed under the MIT License. See [LICENSE](LICENSE).
