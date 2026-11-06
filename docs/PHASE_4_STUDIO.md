# Phase 4: Cathedral Studio & Ecosystem

**Status: In progress.** The first Phase 4 increment establishes a local authenticated API so a desktop client can use the same experiment registry, runtime, and snapshot engine as the CLI. The desktop shell and interactive UI are not yet implemented.

## Studio API foundation

The workspace includes the `cathedral-studio-api` binary and library.

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
| POST | `/api/v1/experiments/{name}/snapshots` | Create a snapshot |
| POST | `/api/v1/experiments/{name}/snapshots/{id}/restore` | Restore a snapshot |
| GET | `/api/v1/experiments/{name}/runs` | List runs and latest metrics |
| POST | `/api/v1/experiments/{name}/runs` | Start an isolated run |
| GET | `/api/v1/experiments/{name}/runs/{id}/logs` | Read the latest 1 MiB of captured logs |
| POST | `/api/v1/experiments/{name}/runs/{id}/stop` | Stop a run |

Run creation passes an argument vector to the existing runtime. It does not invoke a shell. On non-Linux hosts, the API reports the same unsupported-runtime error as the CLI rather than falling back to host execution.

## Security boundaries

- The shipped server binds to `127.0.0.1`; it has no configurable non-loopback bind address.
- All routes require a random bearer token generated for that process.
- The token is emitted only in the startup-ready line and is not saved in the registry or workspace.
- CORS is limited to Tauri origins and the standard Vite development origin. CORS is not authentication; the bearer token remains mandatory.
- The API does not provide arbitrary filesystem paths, shell evaluation, or unrestricted host execution.
- Linux execution retains the documented Bubblewrap/cgroup limitations. Windows and macOS execution remains unsupported.
- Do not expose this service through a reverse proxy or bind it to a public interface.

## Next Phase 4 work

1. Wrap the existing React/TypeScript Studio UI in a Tauri desktop shell.
2. Add an experiment dashboard, lifecycle controls, filesystem browser, snapshot timeline, and log viewer.
3. Add streaming observability and graph history.
4. Add permission-scoped extension interfaces and SDK.
5. Package and verify platform-specific releases.

The API foundation is not the desktop application and does not complete Phase 4.
