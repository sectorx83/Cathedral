# Architecture

## 1. Purpose

Cathedral is an experiment-management and reproducibility platform. Its architecture separates experiment metadata and lifecycle management from platform-specific execution, observation, and packaging.

The central rule is that the shared core owns the domain model and policy decisions, while platform backends own operating-system interactions. A platform backend must report what it can actually enforce.

## 2. Current workspace structure

The implemented Rust workspace contains two crates:

- **cathedral-core:** manifest validation, experiment lifecycle services, SQLite registry, managed workspace operations, host identification, and platform capability contracts.
- **cathedral CLI:** argument parsing and terminal presentation. It delegates domain operations to cathedral-core.

The shared core and CLI now include a Linux Phase 2 runner and observatory path. Linux workload execution is delegated to Bubblewrap; run metadata and logs live under Cathedral's data directory, outside the writable experiment workspace. Windows and macOS retain Phase 1 management functionality and explicitly reject runtime execution.

## 3. Architectural layers

### Shared core

The shared core defines:

- Experiment identifiers and metadata.
- Experiment lifecycle operations with persisted `ready` and `deleting` states.
- Manifest parsing and validation.
- Workspace layout and safe path handling.
- Host platform and architecture detection.
- Platform capability contracts.
- Registry persistence and domain-level errors.

The core does not directly invoke Linux namespaces, Windows Job Objects, or macOS sandbox APIs.

### CLI

The CLI parses arguments, loads configuration, invokes application services, and renders stable human-readable output. Runtime commands delegate to the core runner and never duplicate platform policy.

### Registry and workspace manager

SQLite stores local metadata such as experiment IDs, names, paths, timestamps, and manifest versions. Filesystem contents remain in managed workspaces rather than being stored in the database.

The registry uses SQLite's transactional `user_version` marker, migrates the original one-row schema marker without discarding experiment records, persists lifecycle state, and allows deletion to resume after an interrupted filesystem cleanup, validates required columns, and exposes SQLite integrity checks. Experiment names have a unique constraint. Workspace operations validate canonical paths and reject symlinked workspace roots and symlinked experiment directories. Manifest persistence uses a temporary sibling file followed by rename, avoiding partially written manifests. The `cathedral doctor` command verifies registry integrity, each registered workspace and manifest, and detects unregistered workspace directories.

### Runtime and platform backends

Linux Phase 2 uses Bubblewrap to create user, PID, IPC, UTS, and network namespaces with a restricted mount view. The experiment workspace is writable; system binaries and selected system configuration are read-only; `/tmp` is private; and the inherited environment is cleared. Network access is disabled by default. The runner stores metadata, logs, exit status, and resource history outside the workspace. Optional memory, CPU, and process-count limits are configured in the manifest and enforced through a delegated cgroup v2 subtree. Requested limits fail closed when controller files cannot be configured. `cathedral doctor` reports Bubblewrap, namespace, and cgroup prerequisites. Windows and macOS report Linux-only prerequisites as not applicable and do not currently implement workload execution.

The platform module continues to define the shared backend contract. Native execution backends must report their actual capabilities and may not silently downgrade enforced settings.

Future adapters will implement narrow interfaces for process creation, isolation, resource controls, filesystem setup, and network policy.

A capability model must distinguish:
- Supported and enforced.
- Supported but advisory.
- Unsupported.
- Unavailable due to host configuration or permissions.

The runtime must never silently downgrade an enforced security setting to an advisory one.

### Observatory

The Phase 2 CLI observatory collects process state, virtual/resident memory, CPU ticks, process disk I/O, shared network-namespace byte counters, cgroup memory/CPU/process counts, combined logs, exit codes, and timeout outcomes. `monitor --watch` samples repeatedly; samples are appended to per-run JSONL history. Streaming dashboards, disk-space quotas, packet-level inspection, and per-process network attribution remain future work.

### Snapshot and reproduction

The Phase 3 snapshot subsystem records filesystem state in versioned metadata with SHA-256 integrity hashes. Checkpoints are stored outside experiment workspaces and can be listed, verified, compared, and restored through a staged directory swap. Portable `.cath` packages contain a versioned header, length-prefixed metadata, and ordered file payloads. Import validates metadata, relative paths, sizes, hashes, and the embedded manifest before creating a new experiment. Reproduction additionally checks operating-system and architecture equality.

The initial format preserves regular files and directories only. It rejects symlinks and special files, and does not preserve ownership, permissions, ACLs, extended attributes, hard links, sparse-file layout, live process memory, open descriptors, namespaces, kernel state, or the host OS. Dependency declarations are preserved but not installed or resolved. Matching OS and architecture is a compatibility preflight, not a guarantee of bit-for-bit reproduction.

### Local API and Studio

A local API is planned for communication between the CLI, background runtime services, and the Tauri desktop application. Axum and WebSockets are the current technology direction.

Studio is planned for Phase 4 and will be a client of the same application services, not a second implementation of Cathedral's domain logic.

## 4. Data flow

The intended flow is:

1. CLI or Studio submits a user operation.
2. Application services validate the request and load experiment metadata.
3. The manifest is parsed and checked against the supported schema version.
4. The registry and workspace manager perform the required state changes.
5. Runtime and platform services are invoked only for operations that require host execution.
6. Events and diagnostics are persisted or streamed to observability clients.
7. Snapshot and reproduction services package and verify experiment state.

Domain errors should be stable and actionable. Low-level OS errors should be preserved as causes and translated at the application boundary.

## 5. Manifest and compatibility

Experiment configuration is versioned. Unknown required fields and unsupported schema versions produce explicit errors. Optional extensions must not change the meaning of existing fields.

Reproduction is conditional on compatible host capabilities, architecture, dependencies, and external inputs. Cathedral should report differences rather than promise byte-for-byte identical behavior across unrelated systems.

## 6. Security boundaries

Experiments may contain untrusted code. The host control plane, registry, and user data must remain outside experiment write access.

The design follows least privilege, explicit permissions, safe path handling, bounded resource use, and clear isolation reporting. Containers and namespaces are useful isolation mechanisms but are not, by themselves, a universal security guarantee against hostile workloads.

See [SECURITY.md](../SECURITY.md) for the threat model and security requirements.

## 7. Quality attributes

- **Correctness:** explicit state transitions, validated inputs, transactional metadata changes.
- **Reliability:** cleanup on failures, recoverable state, bounded retries where appropriate.
- **Portability:** shared domain model and native backends.
- **Observability:** structured events, useful diagnostics, no fabricated metrics.
- **Security:** least privilege, isolation boundaries, explicit capability reporting.
- **Reproducibility:** versioned manifests, dependency metadata, integrity-checked packages.
- **Maintainability:** narrow interfaces, testable components, documented invariants.
