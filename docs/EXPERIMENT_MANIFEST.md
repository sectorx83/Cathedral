# Experiment Manifest

## Status

This document describes the proposed `cathedral.toml` format for Phase 1. The schema is not implemented yet and may change before it is versioned as stable.

## Goals

The manifest should be:
- Human-readable and easy to version-control.
- Explicit about runtime and dependency requirements.
- Strictly validated before an experiment is created or run.
- Versioned so future Cathedral releases can evolve safely.
- Portable without embedding machine-specific secrets.

## Proposed example

```toml
manifest_version = 1

[experiment]
name = "hello-lab"
description = "A small reproducible development experiment"

[environment]
working_directory = "."

[dependencies]
# Dependency declarations will be defined by supported package providers.
# Do not assume arbitrary host packages are captured automatically.

[run]
# Execution settings will be expanded with the Phase 2 runtime.
# No workload is launched by Phase 1 manifest parsing.
```

## Validation requirements

The Phase 1 parser should:

1. Require a supported `manifest_version`.
2. Validate experiment names and reject path separators, traversal components, and reserved names.
3. Reject unknown required fields and malformed values.
4. Resolve relative paths against the manifest directory and verify they remain within the permitted project/workspace boundary.
5. Avoid interpreting manifest text as shell commands.
6. Return errors with the field path and a useful explanation.
7. Preserve forward compatibility only for explicitly defined extension namespaces.

## Secrets and host-specific values

Secrets must not be committed in manifests or exported packages by default. Future secret references should identify an external secret provider or environment variable by name, not store the secret value itself.

Machine-specific paths, credentials, and private environment values must be excluded from portable exports unless the user explicitly selects a safe, documented inclusion mechanism.

## Stability

The manifest format should be considered experimental until implementation, schema tests, and migration rules are complete. A stable format requires a documented compatibility policy.
