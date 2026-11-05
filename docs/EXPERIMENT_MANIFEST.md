# Experiment Manifest

## Status

Manifest version 1 is implemented in the Phase 1 core. The schema remains experimental until compatibility and migration rules are finalized.

## Goals

The manifest is human-readable, versioned, strictly validated, and safe to store in source control. It describes experiment metadata, environment configuration, and declared dependency requirements. It does not contain executable shell snippets.

## Version 1 format

```toml
manifest_version = 1

[experiment]
name = "hello-lab"
description = "A small development experiment"

[environment]
working_directory = "."

[environment.variables]
RUST_LOG = "info"

[resources]
memory_max_bytes = 536870912
cpu_quota_percent = 100
pids_max = 128
timeout_seconds = 3600

[dependencies.packages]
serde = "^1.0"
```

The `experiment.name` field is required. Names must be 1-48 characters, start with a lowercase ASCII letter, end with a lowercase letter or digit, and contain only lowercase ASCII letters, digits, and hyphens.

The description is optional and limited to 4096 bytes. The environment section is optional. Its working directory, when supplied, must be a portable relative path without parent-directory traversal, empty path segments, or platform-specific separators. Environment variable names must follow the portable ASCII identifier convention.

The optional `[resources]` table declares Linux cgroup v2 hard limits. `memory_max_bytes` sets `memory.max`; `cpu_quota_percent` sets `cpu.max` as a percentage of one logical CPU (100 = one CPU); and `pids_max` sets `pids.max`. `timeout_seconds` limits a run's duration using GNU coreutils `timeout`, from 1 second to 7 days. Values must be positive, and CPU quota must be between 1 and 1000. These limits require `CATHEDRAL_CGROUP_ROOT` to point to a delegated cgroup v2 subtree with the requested controllers enabled. Cathedral refuses to launch if a requested limit cannot be applied. The optional `dependencies.packages` table records package names and their declared version requirement strings. Package names are limited to portable ASCII letters, digits, hyphens, underscores, and periods. Requirement strings must be non-empty and at most 256 bytes. These strings are opaque metadata in Phase 1: Cathedral does not resolve, download, verify, or install packages yet.

## CLI usage

Create an experiment with a generated minimal manifest:

```sh
cathedral create hello-lab
```

Create an experiment from a manifest:

```sh
cathedral create hello-lab --manifest ./cathedral.toml
```

The manifest name must match the name passed to the CLI. Cathedral writes the validated manifest through a temporary sibling file and an atomic rename into the new managed workspace as `cathedral.toml`.

## Validation behavior

The current parser:

1. Requires manifest version 1.
2. Rejects unknown fields.
3. Validates experiment names.
4. Limits description length.
5. Rejects absolute, traversal, and non-portable working-directory paths.
6. Validates environment variable names.
7. Validates dependency package identifiers and non-empty requirement strings.
8. Returns a field-specific parsing or validation error where available.

Manifest values are treated as data. Declared dependencies remain metadata only and are not resolved or installed. Linux runtime commands are supplied explicitly to `cathedral run`; no shell snippets are embedded in the manifest.

## Secrets and host-specific values

Do not commit secrets in manifests or exported packages. Future secret references should identify an external secret provider or environment variable by name, not store the secret value itself.

Machine-specific paths and private environment values must be excluded from portable exports unless the user explicitly selects a safe, documented inclusion mechanism.

## Stability

The manifest format remains experimental until schema tests, migration rules, and compatibility guarantees are complete. Changes to a future stable format must be versioned and documented.
