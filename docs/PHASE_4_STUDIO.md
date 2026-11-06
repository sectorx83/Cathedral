# Phase 4: Cathedral Studio & Ecosystem

**Status: In progress.** The current Phase 4 implementation includes the authenticated local API, a Tauri v2 desktop shell, and a React/TypeScript Studio client. The UI supports experiment management, isolated run controls, process telemetry, logs, read-only workspace browsing, and snapshot create/verify/compare/restore. The desktop shell is compile-checked on Linux, Windows, and macOS in CI; installers have not yet been packaged and manually validated.

## Studio API foundation

The workspace includes the `cathedral-studio-api` binary and library. The Tauri v2 desktop shell lives in `studio/src-tauri` and starts the API in-process on an ephemeral IPv4 loopback port. The shell obtains the one-process bearer token through a Tauri command; the React client auto-connects without persisting that token. In a browser, the same client can still connect manually to a separately launched local API.

Start the API:

```sh
cargo run -p cathedral-studio-api -- --port 0
```

The service initializes the same Cathedral data directory as the CLI, binds only to IPv4 loopback, chooses an available port when `--port 0` is used, and prints one `CATHEDRAL_STUDIO_READY` JSON line containing the address and a fresh per-process bearer token. The token is not persisted. Treat it as a local secret.

Use `--home /path/to/cathedral-data` or `CATHEDRAL_HOME` to select a data directory. Stop the service with Ctrl+C.

Every API route requires:

```http
Authorization: Bearer <token>
```

API version 1 routes:

| Method | Route | Purpose |
|---|---|---|
| GET | `/api/v1/health` | API and host information |
| GET | `/api/v1/experiments` | List experiments |
| POST | `/api/v1/experiments` | Create an experiment |
| GET | `/api/v1/experiments/{name}` | Inspect an experiment |
| DELETE | `/api/v1/experiments/{name}` | Delete an experiment |
| GET | `/api/v1/experiments/{name}/snapshots` | List snapshots |
| GET | `/api/v1/experiments/{name}/snapshots/{id}/verify` | Verify snapshot integrity |
| GET | `/api/v1/experiments/{name}/snapshots/compare?a={id}&b={id}` | Compare two snapshots |
| GET | `/api/v1/experiments/{name}/files?path={relative}` | List a workspace directory |
| GET | `/api/v1/experiments/{name}/files/content?path={relative}` | Preview a UTF-8 text file, up to 256 KiB |
| POST | `/api/v1/experiments/{name}/snapshots` | Create a snapshot |
| POST | `/api/v1/experiments/{name}/snapshots/{id}/restore` | Restore a snapshot |
| GET | `/api/v1/experiments/{name}/runs` | List runs and latest metrics |
| GET | `/api/v1/experiments/{name}/events` | Authenticated server-sent run and telemetry updates |
| POST | `/api/v1/experiments/{name}/runs` | Start an isolated run |
| GET | `/api/v1/experiments/{name}/runs/{id}/logs` | Read the latest 1 MiB of captured logs |
| POST | `/api/v1/experiments/{name}/runs/{id}/input` | Send up to 4 KiB to a running PTY master |
| POST | `/api/v1/experiments/{name}/runs/{id}/terminal/resize` | Set terminal rows and columns (2–200 rows, 2–500 columns) |
| POST | `/api/v1/experiments/{name}/runs/{id}/stop` | Stop a run |

Run creation passes an argument vector to the existing runtime. It does not invoke a shell. On non-Linux hosts, the API reports the same unsupported-runtime error as the CLI rather than falling back to host execution.

## Security boundaries

- The shipped server binds to `127.0.0.1`; it has no configurable non-loopback bind address.
- All routes require a random bearer token generated for that process.
- The token is emitted only in the startup-ready line and is not saved in the registry or workspace.
- CORS is limited to Tauri origins and the standard Vite development origin. CORS is not authentication; the bearer token remains mandatory.
- The API does not provide arbitrary filesystem paths, shell evaluation, or unrestricted host execution. The file browser accepts workspace-relative paths only, rejects symlink traversal, and caps UTF-8 text previews at 256 KiB. Binary and oversized files are not previewed. The read-only file browser accepts only workspace-relative paths, rejects symlink traversal, limits text previews to 256 KiB, and does not preview binary files.
- Linux execution retains the documented Bubblewrap/cgroup limitations. Windows and macOS execution remains unsupported.
- Do not expose this service through a reverse proxy or bind it to a public interface.

## Desktop development

From the `studio` directory, install Node dependencies and run `npm run tauri:dev` to launch the desktop application. Use `npm run tauri:build` to create platform-native bundles. CI compiles the Tauri application on Linux, Windows, and macOS; this compile check is not a substitute for installing and manually exercising each packaged release.

## Extension registry and SDK

`cathedral-plugin-sdk` defines versioned TOML/JSON manifests, semantic-version validation, a versioned Studio API contract, and an explicit permission vocabulary. Studio can register and remove declarative manifests and persist grants. A grant is metadata only in this release: there is no dynamic code loader, and no third-party code is executed.

The registry API is `GET/POST /api/v1/plugins`, `PUT /api/v1/plugins/{id}/grants`, and `DELETE /api/v1/plugins/{id}`. Permissions can only be granted when declared by the manifest. See [Extension development](EXTENSIONS.md).

## Safe starter experiments

The run console includes bounded CPU, memory, workspace-I/O, filesystem-boundary, network-namespace, and child-process probes. Workload stdin/stdout/stderr are attached to a Linux pseudo-terminal; the API retains a PTY master for bounded input and window-size changes while a reader captures output into the run log. These are ordinary argv-based workload examples, not host-wide pressure or network impairment tools. They still depend on the configured runtime and its actual isolation limits.

## Releases

Tag a version as `vX.Y.Z` to build a Linux `.deb`, Windows `.msi`, and macOS `.dmg`. These are unsigned CI artifacts; Windows/macOS users may see operating-system security warnings. Signing and notarization are not configured. The release workflow builds platform packages but does not replace manual install/upgrade testing.

## Remaining Phase 4 work

1. Finish terminal behavior validation, including resize and interactive workloads, on a Linux host where Bubblewrap user/PID/network namespaces are available. The GitHub-hosted Linux runner currently reports that its Bubblewrap smoke command cannot execute the expected `/usr/bin/true` test command, so tests that require the sandbox return early there; a green Rust test job alone does not prove the real sandbox/PTY path ran.
2. Expand capability-gated lab experiments beyond the bounded CPU, memory, disk, filesystem, network-isolation, and child-process probes. Network fault injection remains disabled until it can be confined to a dedicated experiment network namespace with explicit opt-in and cleanup.
3. Exercise installer upgrade behavior and clean-machine installation/removal on Linux, Windows, and macOS. The release workflow now builds each installer and runs platform-specific install/removal smoke checks, but those jobs have not yet been executed successfully and do not yet test an in-place upgrade.

The authenticated API, live event stream, PTY-backed run I/O, terminal resizing endpoint, declarative extension SDK/registry, desktop client, and bounded starter probes are implemented. Phase 4 remains in progress until the platform installer workflow and real Linux sandbox/PTY validation pass, and the remaining advanced lab scope is completed.
