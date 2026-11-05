# Architecture

## 1. Purpose

Cathedral is an experiment-management and reproducibility platform. Its architecture separates experiment metadata and lifecycle management from platform-specific execution, observation, and packaging.

The central rule is that the shared core owns the domain model and policy decisions, while platform backends own operating-system interactions. A platform backend must report what it can actually enforce.

## 2. Current Phase 1 structure

The implemented Rust workspace contains two crates:

- **cathedral-core:** manifest validation, experiment lifecycle services, SQLite registry, managed workspace operations, host identification, and platform capability contracts.
- **cathedral CLI:** argument parsing and terminal presentation. It delegates domain operations to cathedral-core.

The current implementation manages metadata and workspace directories, validates declared dependency metadata without resolving it, and diagnoses registry/workspace consistency. It does not execute experiments or provide workload isolation.

## 3. Architectural layers

### Shared core

The shared core defines:

- Experiment identifiers and metadata.
- Experiment lifecycle operations.
- Manifest parsing and validation.
- Workspace layout and safe path handling.
- Host platform and architecture detection.
- Platform capability contracts.
- Registry persistence and domain-level errors.

The core does not directly invoke Linux namespaces, Windows Job Objects, or macOS sandbox APIs.

### CLI

The CLI is the Phase 1 interface. It parses arguments, loads configuration, invokes application services, and renders stable human-readable output. CLI commands do not duplicate business rules.

### Registry and workspace manager

SQLite stores local metadata such as experiment IDs, names, paths, timestamps, and manifest versions. Filesystem contents remain in managed workspaces rather than being stored in the database.

The registry uses SQLite's transactional `user_version` marker, migrates the original one-row schema marker without discarding experiment records, validates required columns, and exposes SQLite integrity checks. Experiment names have a unique constraint. Workspace operations validate canonical paths and reject symlinked workspace roots and symlinked experiment directories. Manifest persistence uses a temporary sibling file followed by rename, avoiding partially written manifests. The `cathedral doctor` command verifies registry integrity, each registered workspace and manifest, and detects unregistered workspace directories.

### Runtime and platform backends

The runtime layer is future work. The platform module currently detects the host and defines a backend contract. It does not claim that a native execution backend exists.

Future adapters will implement narrow interfaces for process creation, isolation, resource controls, filesystem setup, and network policy.

A capability model must distinguish:
- Supported and enforced.
- Supported but advisory.
- Unsupported.
- Unavailable due to host configuration or permissions.

The runtime must never silently downgrade an enforced security setting to an advisory one.

### Observatory

The observatory is planned for Phase 2. It will collect runtime events and measurements, including process state, resource use, logs, exit status, and failures. It should identify the source and reliability of each measurement.

### Snapshot and reproduction

The snapshot subsystem is planned for Phase 3. It will record filesystem state and metadata with integrity verification. The reproduction subsystem will validate package contents, dependencies, platform requirements, and compatibility before rebuilding an experiment.

Filesystem checkpointing must remain distinct from VM snapshots and live process-state capture.

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
