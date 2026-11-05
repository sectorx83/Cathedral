# Contributing to Cathedral

Thank you for considering a contribution.

Cathedral is an open-source computing laboratory focused on safe experiment management, observability, and reproducibility. Contributions should improve correctness and clarity before expanding the feature surface.

## Before contributing

- Read [README.md](README.md) for the project scope.
- Read [ROADMAP.md](ROADMAP.md) to understand the current phase.
- Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) before changing component boundaries.
- Read [SECURITY.md](SECURITY.md) before touching runtime, filesystem, process, network, or plugin code.

## Engineering expectations

- Do not introduce placeholder behavior that looks like a working feature.
- Keep platform-specific code behind explicit interfaces.
- Never silently weaken a requested security policy.
- Validate all external input.
- Add tests for new behavior and failure paths.
- Keep error messages actionable and logs structured.
- Update documentation when behavior or guarantees change.
- Preserve existing working functionality unless the change explicitly requires otherwise.

## Current project workflow

The current repository workflow is direct commits to the `main` branch. Keep changes focused and review the resulting diff and test output before committing.

## Documentation contributions

Documentation should distinguish current behavior from planned behavior. Use labels such as "planned", "implemented", and "verified" where ambiguity could mislead users. Avoid unsupported security, portability, or reproducibility claims.

## Reporting bugs

A useful bug report includes:
- Cathedral commit or release.
- Operating system and version.
- Relevant configuration and manifest.
- Exact steps to reproduce.
- Expected and actual behavior.
- Logs or diagnostic output with secrets removed.

For security vulnerabilities, follow [SECURITY.md](SECURITY.md) rather than opening a public issue with exploit details.
