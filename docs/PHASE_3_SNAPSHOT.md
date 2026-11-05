# Phase 3: Snapshot & Reproduction

Cathedral stores filesystem checkpoints outside experiment workspaces at `snapshots/<experiment-id>/<snapshot-id>`. Each checkpoint includes a copy of the workspace, a generated `cathedral.lock` file, and a versioned JSON manifest containing experiment metadata, the original manifest, source OS and architecture, timestamps, relative paths, file sizes, and SHA-256 digests. The lockfile binds the canonical manifest hash to the host OS/architecture, runtime backend requirement, and declared dependency versions. It is regenerated for each checkpoint and included in exported packages.

## Commands

```sh
cathedral snapshot hello-lab
cathedral snapshots hello-lab
cathedral verify hello-lab <snapshot-id>
cathedral compare hello-lab <snapshot-a> <snapshot-b>
cathedral restore hello-lab <snapshot-id>
cathedral export hello-lab [output.cath]
cathedral import experiment.cath
cathedral reproduce experiment.cath
```

The portable `.cath` format contains a versioned magic header, a length-prefixed JSON manifest, and file contents in manifest order. Import validates metadata, path safety, file sizes and SHA-256 hashes, trailing data, and the embedded `cathedral.toml` before registering a new experiment. An existing experiment with the same name is never overwritten. Reproduction additionally requires matching operating-system and architecture identifiers.

## Guarantees and limitations

Snapshots preserve regular files and directories. Symbolic links and special files are rejected. Restores are staged beside the workspace and use a directory swap with rollback if activation fails. Snapshot comparison reports added, changed, and removed paths.

This format does not preserve ownership, permissions, ACLs, extended attributes, hard links, sparse-file layout, live process memory, open descriptors, namespaces, kernel state, or the host OS. Dependency versions are locked as declared metadata but are not resolved, downloaded, or installed. `cathedral.lock` does not replace package-manager lockfiles such as `Cargo.lock`, `package-lock.json`, or `poetry.lock`. Matching OS and architecture is a compatibility preflight, not a promise of bit-for-bit reproduction. Packages are integrity-checked but not signed or authenticated; only import packages from trusted sources. Phase 2 execution remains Linux-only.
