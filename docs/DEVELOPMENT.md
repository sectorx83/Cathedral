# Development Guide

## Project status

The Phase 1 Rust workspace and initial CLI are now present. The project is still early-stage; runtime isolation, observability, snapshots, reproduction, and Studio are not implemented.

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

The GitHub Actions workflow runs formatting, Clippy, and tests on Linux, Windows, and macOS.

## Current CLI

```sh
cargo run -p cathedral -- --help
cargo run -p cathedral -- init
cargo run -p cathedral -- create demo-lab
cargo run -p cathedral -- list
cargo run -p cathedral -- inspect demo-lab
cargo run -p cathedral -- doctor
cargo run -p cathedral -- delete demo-lab
```

Use `--home /path/to/data` or `CATHEDRAL_HOME` to select a non-default data directory.

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
2. Registry tests using temporary SQLite databases.
3. Workspace tests using temporary directories and adversarial path cases.
4. Integration tests for complete CLI workflows.
5. Platform tests for capability detection and runtime enforcement.
6. Reproduction tests for corrupted, incomplete, and incompatible packages.

## Change workflow

The project workflow uses direct commits to the main branch. Keep changes focused and descriptive. Before committing, inspect the diff, run the relevant checks, and update documentation for any behavior or guarantee that changed.

Do not mark a phase complete while required tests are failing or while a critical limitation is undocumented.
