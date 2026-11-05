# Cathedral

[![Cathedral CI](https://github.com/sectorx83/Cathedral-/actions/workflows/ci.yml/badge.svg)](https://github.com/sectorx83/Cathedral-/actions/workflows/ci.yml)

### The Open Computing Laboratory

Cathedral is an open-source developer platform for creating isolated software experiments, observing how they behave, diagnosing failures, and reproducing environments on other compatible machines.

Its goal is simple: make **"it works on my machine"** a problem that can be investigated and reproduced instead of a mystery.

Cathedral is designed for developers, students, researchers, and engineering teams who need a controlled place to test software and understand its behavior.

> **Project status:** Phase 1 foundation is implemented and undergoing cross-platform CI verification. Runtime isolation, live observability, snapshots, reproduction, and desktop Studio are not implemented yet.

## What Cathedral is

Cathedral is an experiment laboratory, not a hosting platform or an operating system. An experiment is a declared workspace with configuration, dependencies, execution settings, observable activity, and (eventually) portable checkpoints.

The intended workflow is:

1. Define an experiment and its requirements.
2. Create a managed workspace.
3. Run it with platform-appropriate isolation and resource controls.
4. Observe processes, resource usage, logs, and failures.
5. Capture a checkpoint and export the experiment.
6. Recreate it on another compatible machine and inspect any differences.

## Current Phase 1 implementation

The initial Rust workspace currently provides:

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

Phase 1 now includes transactional registry initialization with legacy schema migration, persisted lifecycle state, resumable deletion, SQLite integrity checks, atomic manifest persistence, validated dependency metadata, managed-path consistency checks in `cathedral doctor`, structured tracing, and expanded lifecycle failure tests. Runtime execution and isolation remain Phase 2 work.

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

Manifest values are configuration data. Declared dependency requirements are recorded as metadata only. Phase 1 does not resolve or install dependencies, execute commands, or launch workloads.

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

## Planned CLI beyond Phase 1

The following commands are planned for later phases and are **not available yet**:

```text
cathedral run my-experiment
cathedral monitor my-experiment
cathedral logs my-experiment
cathedral processes my-experiment
cathedral stop my-experiment

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

Linux isolation is expected to build on namespaces, cgroups v2, and filesystem layering. Windows and macOS will use their own native isolation mechanisms. These are design choices under evaluation and will be validated during implementation.

## Security expectations

Experiments may contain untrusted code. The current Phase 1 implementation does not execute workloads and is not a sandbox. Containers alone will not be described as a complete security boundary for arbitrary hostile workloads.

## License

Cathedral is distributed under the MIT License. See [LICENSE](LICENSE).
