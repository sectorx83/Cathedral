# Architecture

## 1. Purpose

Cathedral is an experiment-management and reproducibility platform. Its architecture separates experiment metadata and lifecycle management from platform-specific execution, observation, and packaging.

The central rule is that the shared core owns the domain model and policy decisions, while platform backends own operating-system interactions. A platform backend must report what it can actually enforce.

## 2. Architectural layers

### Shared core

The shared core defines:

- Experiment identifiers and metadata.
- Experiment lifecycle states and valid transitions.
- Manifest parsing and validation.
- Workspace layout and safe path handling.
- Configuration and capability models.
- Registry interfaces and domain-level errors.
- Versioned data formats used by other components.

The core should not directly invoke Linux namespaces, Windows Job Objects, or macOS sandbox APIs.

### CLI

The CLI is the primary Phase 1 interface. It parses arguments, loads configuration, invokes application services, and renders stable human-readable output. Machine-readable output can be added where it has a concrete use case.

CLI commands should not duplicate business rules. They should call the same application services used by the future local API and desktop application.

### Registry and workspace manager

SQLite stores local metadata such as experiment IDs, names, paths, timestamps, manifest versions, and lifecycle status. Filesystem contents remain in managed workspaces rather than being stored in the database.

The registry must use schema migrations and transactions. Workspace operations must validate canonical paths, avoid following unsafe path components, and handle partial failures without corrupting registry state.

### Runtime and platform backends

The runtime layer owns workload launch, supervision, resource policy, and cleanup. Platform backends implement narrow interfaces for process creation, isolation, resource controls, filesystem setup, and network policy.

A capability model must distinguish:
- Supported and enforced.
- Supported but advisory.
- Unsupported.
- Unavailable due to host configuration or permissions.

The runtime must never silently downgrade an enforced security setting to an advisory one.

### Observatory

The observatory collects runtime events and measurements, including process state, resource use, logs, exit status, and failures. It should identify the source and reliability of each measurement.

The observatory is diagnostic infrastructure, not a security monitor that can guarantee detection of all hostile behavior.

### Snapshot and reproduction

The snapshot subsystem records filesystem state and metadata with integrity verification. The reproduction subsystem validates package contents, dependencies, platform requirements, and compatibility before rebuilding an experiment.

Filesystem checkpointing must remain distinct from VM snapshots and live process-state capture.

### Local API and Studio

A local API is planned for communication between the CLI, background runtime services, and the Tauri desktop application. Axum and WebSockets are the current technology direction.

The API must bind only to an appropriately protected local interface by default, authenticate local clients where necessary, validate all requests, and avoid exposing privileged operations to arbitrary remote callers.

Studio is a client of the same application services, not a second implementation of Cathedral's domain logic.

## 3. Proposed repository structure

The exact crate boundaries may evolve as implementation reveals real dependencies. Avoid empty crates created only to make the tree look complete.

```text
cathedral/
├── crates/
│   ├── cathedral-core/          # domain types, validation, shared contracts
│   ├── cathedral-cli/           # command-line interface
│   ├── cathedral-registry/      # SQLite persistence
│   ├── cathedral-workspace/     # managed workspace operations
│   ├── cathedral-runtime/       # runtime orchestration
│   ├── cathedral-platform/      # platform capability contracts
│   ├── cathedral-observatory/   # measurements and diagnostics
│   ├── cathedral-snapshot/      # checkpoints and integrity
│   └── cathedral-reproduce/     # export/import and reproduction
├── studio/                      # Tauri + React + TypeScript (Phase 4)
├── docs/
├── examples/
├── tests/
├── Cargo.toml
├── README.md
├── ROADMAP.md
├── SECURITY.md
├── CONTRIBUTING.md
└── LICENSE
```

Phase 1 should create only the crates needed for real functionality. Runtime, observatory, snapshot, and reproduction crates should be introduced when their implementation begins.

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

Experiment configuration is versioned. Unknown required fields and unsupported schema versions must produce explicit errors. Optional extensions must not change the meaning of existing fields.

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
