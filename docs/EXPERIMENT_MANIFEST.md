# Experiment Manifest

## Status

Manifest version 1 is implemented in the Phase 1 core. The schema remains experimental until compatibility and migration rules are finalized.

## Goals

The manifest is human-readable, versioned, strictly validated, and safe to store in source control. It describes experiment metadata and environment configuration. It does not contain executable shell snippets.

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
```

The `experiment.name` field is required. Names must be 1-48 characters, start with a lowercase ASCII letter, end with a lowercase letter or digit, and contain only lowercase ASCII letters, digits, and hyphens.

The description is optional and limited to 4096 bytes. The environment section is optional. Its working directory, when supplied, must be a relative path without parent-directory traversal. Environment variable names must follow the portable ASCII identifier convention.

## CLI usage

Create an experiment with a generated minimal manifest:

```sh
cathedral create hello-lab
```

Create an experiment from a manifest:

```sh
cathedral create hello-lab --manifest ./cathedral.toml
```

The manifest name must match the name passed to the CLI. Cathedral copies the validated manifest into the new managed workspace as `cathedral.toml`.

## Validation behavior

The current parser:

1. Requires manifest version 1.
2. Rejects unknown fields.
3. Validates experiment names.
4. Limits description length.
5. Rejects absolute and parent-traversal working-directory paths.
6. Validates environment variable names.
7. Returns a field-specific parsing or validation error where available.

Manifest values are treated as data. Phase 1 does not execute commands, install dependencies, or launch workloads.

## Secrets and host-specific values

Do not commit secrets in manifests or exported packages. Future secret references should identify an external secret provider or environment variable by name, not store the secret value itself.

Machine-specific paths and private environment values must be excluded from portable exports unless the user explicitly selects a safe, documented inclusion mechanism.

## Stability

The manifest format remains experimental until schema tests, migration rules, and compatibility guarantees are complete. Changes to a future stable format must be versioned and documented.
