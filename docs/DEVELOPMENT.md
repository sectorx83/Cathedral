# Development Guide

## Project status

Phase 1 Foundation & Core Engine is complete. Its Rust workspace, CLI, registry, manifest validation, workspace manager, and diagnostics passed the Linux, Windows, and macOS CI matrix. Phase 2 adds a Linux Bubblewrap runner, optional cgroup v2 hard limits, process lifecycle commands, captured logs, and procfs/cgroup resource samples. Windows/macOS execution backends, snapshots, reproduction, and Studio remain unimplemented. See [CI_STATUS.md](CI_STATUS.md) for the verified source commit and run.

## Toolchain

- Rust stable toolchain and Cargo.
- SQLite is bundled through rusqlite for the local registry.
- Tauri, React, and TypeScript will be introduced during Phase 4.

The repository includes a rust-toolchain.toml file that selects stable Rust with rustfmt and Clippy.

## Build and test

From the repository root:

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The GitHub Actions workflow runs formatting, Clippy, and tests on Linux, Windows, and macOS. Core lifecycle diagnostics use structured tracing; set `RUST_LOG=cathedral_core=debug,cathedral=debug` to inspect debug events.

## Current CLI

```sh
cargo run -p cathedral -- --help
cargo run -p cathedral -- init
cargo run -p cathedral -- create demo-lab
cargo run -p cathedral -- list
cargo run -p cathedral -- inspect demo-lab
cargo run -p cathedral -- doctor
cargo run -p cathedral -- run demo-lab -- /usr/bin/printf hello
cargo run -p cathedral -- monitor demo-lab
cargo run -p cathedral -- processes demo-lab
cargo run -p cathedral -- logs demo-lab
RUST_LOG=cathedral_core=debug,cathedral=debug cargo run -p cathedral -- doctor
cargo run -p cathedral -- delete demo-lab
```

Use `--home /path/to/data` or `CATHEDRAL_HOME` to select a non-default data directory. Linux workload execution requires Bubblewrap and working unprivileged user namespaces. Set `CATHEDRAL_CGROUP_ROOT` to a delegated cgroup v2 directory to enable resource controls and aggregate process metrics. Resource limits in a manifest fail closed if the delegated controllers are unavailable.

## Engineering principles

- Keep domain logic independent of operating-system APIs.
- Put native operations behind small platform interfaces.
- Validate all external input, including manifests and imported packages.
- Use structured errors and actionable diagnostics.
- Make filesystem operations safe against traversal and symlink surprises.
- Add tests for both successful behavior and failure recovery.
- Do not claim a security property without a test or platform-specific evidence.
- Keep documentation aligned with the implementation.

## Test layers

1. Unit tests for domain types, validation, and state transitions.
2. Registry tests using temporary SQLite databases, including schema migration, unsupported versions, duplicate legacy markers, persisted lifecycle transitions, and integrity checks.
3. Workspace tests using temporary directories and adversarial path cases.
4. CLI integration tests for complete lifecycle workflows, duplicate creation, invalid names, doctor consistency checks, and data-directory overrides. Core lifecycle tests cover resumable deletion after interruption.
5. Platform tests for capability detection and runtime enforcement. Linux integration tests exercise Bubblewrap when the CI host permits user namespaces.
6. Reproduction tests for corrupted, incomplete, and incompatible packages.

## Change workflow

The project workflow uses direct commits to the main branch. Keep changes focused and descriptive. Before committing, inspect the diff, run the relevant checks, and update documentation for any behavior or guarantee that changed.

Do not mark a phase complete while required tests are failing or while a critical limitation is undocumented.
