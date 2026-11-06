# Cathedral Roadmap

This document defines the project's four-phase implementation plan. The phase boundaries are deliberate: establish a trustworthy core before adding execution, then make execution observable, then make experiments reproducible, and finally build the desktop and extension ecosystem.

A phase is complete only when its acceptance criteria are met, automated checks pass, documentation reflects actual behavior, and known limitations are recorded. A command or feature appearing in this roadmap is not evidence that it has been implemented.

## Phase 1: Foundation & Core Engine

**Objective:** Build a reliable, cross-platform foundation for managing experiment workspaces.

**Status: Complete.** The Phase 1 implementation passed CI run [#65](https://github.com/sectorx83/Cathedral-/actions/runs/42030567551) on Linux, Windows, and macOS. Formatting, Clippy with warnings denied, and the complete workspace test suite passed.

### Scope

- Establish a Rust workspace and modular crate boundaries.
- Implement the Cathedral CLI and consistent command/error conventions.
- Define experiment identity, metadata, lifecycle states, and workspace layout.
- Implement create, delete, list, and inspect operations.
- Define and validate the `cathedral.toml` experiment manifest.
- Add a local SQLite registry with schema versioning and safe migrations.
- Manage workspace paths safely and prevent path traversal.
- Add structured logging and actionable diagnostics.
- Detect the host operating system and expose platform capability interfaces.
- Define configuration validation and dependency metadata boundaries.
- Add unit and integration tests, including filesystem and registry failure cases.

### Intended CLI surface

- `cathedral init`
- `cathedral create <name>`
- `cathedral list`
- `cathedral inspect <name>`
- `cathedral delete <name>`
- `cathedral doctor`

### Acceptance criteria

- A developer can initialize Cathedral, create an experiment, list it, inspect it, and delete it.
- Experiment metadata survives process restarts.
- Invalid manifests produce clear, actionable validation errors.
- Workspace operations are constrained to Cathedral-managed paths.
- Platform detection works without claiming unsupported capabilities.
- Unit and integration tests pass on the supported Phase 1 CI targets.
- Setup, architecture, and CLI documentation match the implemented behavior.

**Not included:** running workloads, claiming sandbox isolation, live observability, snapshots, export/import, or the desktop application.

## Phase 2: Isolation & Observatory

**Status: Complete.** Phase 2 was accepted as complete before Phase 3 began. Linux execution uses Bubblewrap namespaces and a restricted filesystem view, network access is denied by default, and optional hard memory, CPU, and process-count limits use delegated cgroup v2. Linux execution uses Bubblewrap namespaces and a restricted filesystem view, network access is denied by default, and optional hard memory, CPU, and process-count limits use delegated cgroup v2. The CLI supports run, monitor (including watch mode), process inspection, logs, and stop, with procfs/cgroup resource samples, exit diagnostics, timeout enforcement, network-namespace byte counters, and JSONL history. Windows and macOS runtime backends are explicitly unsupported and remain future work; desktop streaming observability belongs to Phase 4.

**Objective:** Execute experiments under explicit, inspectable resource and isolation policies.

### Scope

- Define a runtime lifecycle and process supervision model.
- Implement the first Linux runtime using namespaces and cgroups v2 where available.
- Apply filesystem isolation and configurable CPU and memory limits.
- Add process lifecycle management, termination, and cleanup.
- Deny network access by default through a network namespace and report the current limitation: configurable allowlists and host-network mode are not implemented.
- Investigate a Windows backend using Job Objects and suitable isolation facilities.
- Investigate a macOS backend using supported sandboxing or virtualization facilities.
- Add observability for CPU, memory, disk activity, processes, logs, and resource history.
- Capture exit codes, termination signals, and timeout outcomes; automatically clean up completed delegated cgroups and verify stop completion.

### Intended CLI surface

- `cathedral run <name>`
- `cathedral monitor <name>`
- `cathedral logs <name>`
- `cathedral processes <name>`
- `cathedral stop <name>`

### Acceptance criteria

- A workload can be started, monitored, and stopped through Cathedral on the first supported platform.
- Enforced limits are distinguished from advisory measurements.
- Runtime cleanup occurs after normal exit, failure, and stop; delegated cgroups are reaped automatically when the supervisor exits.
- Unsupported isolation features fail closed or require an explicit, documented weaker mode.
- Process, resource, and log data can be inspected without relying on the workload's cooperation.
- Platform-specific guarantees and limitations are documented and tested.

## Phase 3: Snapshot & Reproduction Engine

**Status: Implementation in progress; cross-platform CI verification pending.** The current revision implements filesystem checkpoints with SHA-256 manifests, snapshot listing/verification/comparison, staged restoration, portable `.cath` export/import, and strict OS/architecture checks for reproduction. Dependency declarations are preserved but not installed or resolved.

**Objective:** Package experiment inputs and checkpoints so compatible machines can recreate an experiment and report differences.

### Scope

- Design snapshot metadata, content integrity, and versioning.
- Implement filesystem checkpoints and restoration.
- Compare snapshots and identify changed files or metadata.
- Define environment manifests and lockfiles.
- Track dependency versions and relevant host/runtime requirements.
- Export and import portable experiment packages.
- Validate compatibility before attempting reproduction.
- Generate reproduction reports describing verified, missing, and differing inputs.
- Test interrupted exports, corrupted archives, incompatible versions, and failed restores.

### Intended CLI surface

- `cathedral snapshot <name>`
- `cathedral snapshots <name>`
- `cathedral restore <name> <snapshot>`
- `cathedral export <name>`
- `cathedral import <package>`
- `cathedral reproduce <package>`

### Acceptance criteria

- A checkpoint can be created, integrity-checked, listed, compared, and restored.
- An experiment can be exported and imported without silently dropping required metadata.
- Reproduction checks report compatibility and missing dependencies before execution.
- A second compatible machine can recreate the declared environment within documented limits.
- The reproduction report distinguishes verified facts from assumptions and host-specific differences.

**Important boundary:** filesystem snapshots are not full virtual-machine snapshots. Cathedral will not claim to restore kernel memory, arbitrary process state, or an entire machine unless a future implementation explicitly provides and tests those capabilities.

## Phase 4: Cathedral Studio & Ecosystem

**Status: In progress.** The first increment adds a loopback-only, bearer-token-authenticated Axum API over the existing experiment core, runtime, and snapshot engine. The Tauri desktop shell, React/TypeScript UI, streaming dashboard, extension system, and release packaging are not implemented yet. See [Phase 4 Studio](docs/PHASE_4_STUDIO.md).

**Objective:** Deliver a polished desktop experience and a safe extension model.

### Scope

- Establish a local authenticated API shared by the desktop client and CLI.
- Build a Tauri desktop application with React and TypeScript.
- Provide an experiment dashboard and lifecycle controls.
- Add an integrated terminal, live resource graphs, filesystem browser, logs, and snapshot timeline.
- Add experiment comparison and advanced lab tools.
- Explore network fault simulation and controlled resource-pressure testing.
- Define a plugin architecture, SDK, extension permissions, and install/removal lifecycle.
- Publish user, developer, and extension documentation.
- Prepare Linux, Windows, and macOS releases with platform-specific capability disclosures.

### Acceptance criteria

- Users can manage and inspect experiments from the desktop application.
- Studio state and CLI state remain consistent through the supported local API.
- Long-running operations provide clear progress, cancellation, and failure reporting.
- Extensions are permission-scoped and cannot silently gain unrestricted host access.
- Release artifacts are tested on each declared supported platform.
- Installation, upgrade, recovery, and removal procedures are documented.

## Cross-phase engineering gates

Every phase must follow these rules:

1. Commit directly to `main`; do not create feature branches or pull requests for this project workflow.
2. Do not represent placeholders, stubs, or simulated output as completed functionality.
3. Run formatting, linting, unit tests, and relevant integration tests after meaningful changes.
4. Fix regressions introduced by the work before calling it complete.
5. Treat experiment workloads as potentially untrusted.
6. Keep the shared core platform-neutral and isolate OS-specific code behind explicit interfaces.
7. Update documentation when behavior, guarantees, or commands change.
8. Preserve existing working behavior unless a deliberate, documented change is required.
9. Report completed work, verification results, remaining work, and known limitations honestly.

## Scope control

The four phases are the approved project plan. New ideas should be evaluated against the current phase and deferred when they do not unblock its acceptance criteria. Security, correctness, reproducibility, and maintainability take priority over adding surface area.
