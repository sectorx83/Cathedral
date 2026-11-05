# Cathedral
### The Open Computing Laboratory

Cathedral is an open-source developer platform for creating isolated software experiments, observing how they behave, diagnosing failures, and reproducing environments on other compatible machines.

Its goal is simple: make **"it works on my machine"** a problem that can be investigated and reproduced instead of a mystery.

Cathedral is designed for developers, students, researchers, and engineering teams who need a controlled place to test software and understand its behavior.

> **Project status:** Early planning and documentation. The four-phase roadmap is defined, but the runtime, isolation backends, observatory, snapshot engine, and desktop studio are not implemented yet. Do not treat planned capabilities as available features.

## What Cathedral is

Cathedral is an experiment laboratory, not a hosting platform or an operating system. An experiment is a declared workspace with configuration, dependencies, execution settings, observable activity, and (eventually) portable checkpoints.

The intended workflow is:

1. Define an experiment and its requirements.
2. Create a managed workspace.
3. Run it with platform-appropriate isolation and resource controls.
4. Observe processes, resource usage, logs, and failures.
5. Capture a checkpoint and export the experiment.
6. Recreate it on another compatible machine and inspect any differences.

## Design principles

- **Reproducibility first:** record the inputs and configuration needed to recreate an experiment.
- **Isolation with honest guarantees:** use the strongest practical platform mechanisms and clearly document their limits.
- **Observable by default:** make process behavior, resource consumption, and failures inspectable.
- **Cross-platform architecture:** share the core model while using native backends where operating systems differ.
- **Portable, explicit configuration:** keep experiment definitions readable, versionable, and validated.
- **Open and extensible:** build a documented foundation that can be tested, audited, and extended.

## Platform strategy

Cathedral is intended for Linux, Windows, and macOS. Linux is the first implementation target. Cross-platform support is an architectural requirement, not a claim that all platforms already have equivalent functionality.

The shared core will define common concepts and capability interfaces. Platform backends will implement those interfaces using native facilities. Cathedral must report unavailable or weaker capabilities instead of silently pretending that behavior is identical.

## Roadmap

The project is organized into four phases:

| Phase | Focus | Intended outcome |
|---|---|---|
| 1 | Foundation & Core Engine | Reliable CLI, experiment lifecycle, manifest validation, registry, workspace management, and platform interfaces |
| 2 | Isolation & Observatory | Managed execution, resource controls, process inspection, logs, and runtime diagnostics |
| 3 | Snapshot & Reproduction Engine | Checkpoints, integrity checks, export/import, and reproducible environment reports |
| 4 | Cathedral Studio & Ecosystem | Desktop studio, advanced lab tools, plugin SDK, and platform releases |

See [ROADMAP.md](ROADMAP.md) for scope and completion criteria.

## Planned CLI

The following commands describe the intended interface. They are **planned**, not currently usable:

```text
cathedral init
cathedral create my-experiment
cathedral list
cathedral inspect my-experiment
cathedral delete my-experiment
cathedral doctor

cathedral run my-experiment
cathedral monitor my-experiment
cathedral logs my-experiment
cathedral processes my-experiment
cathedral stop my-experiment

cathedral snapshot my-experiment
cathedral snapshots my-experiment
cathedral restore my-experiment <snapshot>
cathedral export my-experiment
cathedral import experiment.cath
cathedral reproduce experiment.cath
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md): system boundaries and technical design.
- [Roadmap](ROADMAP.md): the four approved phases and their acceptance criteria.
- [Platform support](docs/PLATFORM_SUPPORT.md): cross-platform strategy and capability reporting.
- [Experiment manifest](docs/EXPERIMENT_MANIFEST.md): proposed `cathedral.toml` format and validation rules.
- [Development setup](docs/DEVELOPMENT.md): development workflow and quality gates.
- [Security model](SECURITY.md): threat model, isolation boundaries, and security commitments.
- [Contributing](CONTRIBUTING.md): contribution workflow and engineering standards.

## Technology direction

The current architectural direction is a Rust workspace for the core and CLI, SQLite for local metadata, and a Tauri + React + TypeScript desktop application in the final phase. The local API is expected to use Axum with WebSockets where streaming updates are needed.

Linux isolation is expected to build on namespaces, cgroups v2, and filesystem layering. Windows and macOS will use their own native isolation mechanisms. These are design choices under evaluation and will be validated during implementation.

## Project status and expectations

Cathedral is in its documentation and foundation-planning stage. No isolation, monitoring, snapshot, reproduction, or desktop functionality should be assumed to exist until it is implemented, tested, and documented.

Security-sensitive features will require explicit capability checks, least privilege, and tests. Containers alone will not be described as a complete security boundary for arbitrary hostile workloads.

## License

Cathedral is distributed under the MIT License. See [LICENSE](LICENSE).
