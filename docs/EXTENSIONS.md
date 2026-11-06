# Cathedral Extension Development

Cathedral extensions use a versioned, declarative manifest. The first SDK release defines metadata and permission contracts; it does not load or execute extension code.

## Manifest

```toml
schema_version = 1
id = "sample-inspector"
name = "Sample Inspector"
version = "1.0.0"
api_version = 1
description = "Reads experiment metadata"

permissions = ["read_experiments", "read_run_logs"]
```

- `schema_version` must be `1`.
- `id` is 2-64 lowercase ASCII letters/digits with single hyphens, starting with a letter.
- `version` must be semantic versioning.
- `api_version` must match the Studio API version.
- Unknown manifest fields and duplicate permissions are rejected.

## Permission vocabulary

| Permission | Intended access |
| --- | --- |
| `read_experiments` | Read experiment metadata |
| `read_run_logs` | Read captured process output |
| `read_workspace_files` | Read workspace files through the bounded browser |
| `create_experiments` | Create experiment metadata and workspaces |
| `start_runs` | Start a configured isolated workload |
| `stop_runs` | Stop a workload |
| `create_snapshots` | Create filesystem checkpoints |
| `restore_snapshots` | Restore a filesystem checkpoint |

A plugin can only receive permissions that it declared. Grants are stored locally and can be revoked independently. Removing a plugin removes its manifest and grants.

## Local registry API

All routes require the Studio bearer token:

- `GET /api/v1/plugins`: list registered manifests and grants.
- `POST /api/v1/plugins`: register a manifest using `{"manifest": {...}}`.
- `PUT /api/v1/plugins/{id}/grants`: replace the grant set using `{"permissions": [...]}`.
- `DELETE /api/v1/plugins/{id}`: remove the registered manifest.

Registry metadata is stored under Cathedral's data directory in `plugins/`. The API rejects invalid identifiers, symlinked registry entries, duplicate grants, undeclared grants, and malformed manifests.

## Security boundary and current limitation

A permission manifest is not a sandbox. The current Studio version does not execute extension code, load dynamic libraries, or grant third-party code access to the host. The SDK is a contract for a future host that must enforce each permission at every API boundary. Do not treat a registered manifest as an installed executable extension.
