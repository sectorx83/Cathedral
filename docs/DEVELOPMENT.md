# Development Guide

## Project status

Cathedral is currently documentation-first. The Rust workspace and executable features are planned but not yet present. This guide records the intended development workflow and will be updated as the actual toolchain and commands are introduced.

## Planned toolchain

- Rust stable toolchain and Cargo for the core and CLI.
- SQLite for local metadata.
- Tauri, React, and TypeScript for the Phase 4 desktop application.
- Platform-specific CI runners for Linux, Windows, and macOS.

Exact minimum tool versions will be pinned when the workspace is created. Avoid documenting unverified commands as working setup steps.

## Development principles

- Keep domain logic independent of operating-system APIs.
- Put native operations behind small platform interfaces.
- Validate all external input, including manifests and imported packages.
- Use structured errors and logs.
- Make filesystem operations safe against traversal and symlink surprises.
- Add tests for both successful behavior and failure recovery.
- Do not claim a security property without a test or platform-specific evidence.
- Keep documentation aligned with the implementation.

## Expected quality checks

As the Rust workspace is introduced, CI should include:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace`

Platform-specific tests should run on native runners. Tests requiring elevated privileges or kernel features must be isolated and clearly marked rather than silently skipped.

## Suggested test layers

1. Unit tests for domain types, validation, and state transitions.
2. Registry tests using temporary SQLite databases.
3. Workspace tests using temporary directories and adversarial path cases.
4. Integration tests for complete CLI workflows.
5. Platform tests for capability detection and runtime enforcement.
6. Reproduction tests for corrupted, incomplete, and incompatible packages.

## Change workflow

The project workflow uses direct commits to `main`. Keep commits focused and descriptive. Before committing, inspect the diff, run the relevant checks, and update documentation for any behavior or guarantee that changed.

Do not mark a phase complete while required tests are failing or while a critical limitation is undocumented.
