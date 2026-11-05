# Security Policy

## Status

Cathedral is under active early-stage design. Its isolation and execution systems are not implemented yet. **Do not use the current repository as a sandbox for untrusted code.** No security boundary is being provided by the documentation or planned architecture alone.

## Phase 1 filesystem handling

Phase 1 rejects parent traversal, symlinked workspace roots, symlinked experiment entries, and workspace paths that are not direct children of Cathedral's managed root. It also checks registered manifests and reports orphaned workspaces through `cathedral doctor`. These checks protect against common mistakes and accidental path substitution, but they are not race-free against a hostile process running with the same user privileges. Phase 1 does not use OS-level directory-handle confinement and does not provide a sandbox or workload isolation.

## Threat model

Cathedral is intended to manage experiments that may execute software supplied by the user or obtained from external sources. Such software may be buggy, compromised, or deliberately hostile.

The design must protect:
- The host operating system and user data.
- Other experiments and their workspaces.
- Cathedral's registry, configuration, and control plane.
- Credentials and secrets not explicitly provided to an experiment.
- Resource availability on the host.

Potential threats include filesystem traversal, symlink attacks, privilege escalation, process escape, resource exhaustion, malicious network activity, corrupted imports, dependency compromise, and unsafe plugin behavior.

## Security principles

1. **Least privilege:** do not run workloads with elevated privileges unless a narrowly justified operation requires it.
2. **Isolation is explicit:** report the exact mechanisms active for each experiment.
3. **No silent downgrade:** if a required control cannot be enforced, refuse execution or require a clear user-approved weaker policy.
4. **Host data stays outside:** experiment processes must not receive unrestricted access to user files, credentials, or the Cathedral control plane.
5. **Bound resources:** apply and verify resource limits where the platform supports enforcement.
6. **Validate boundaries:** validate paths, manifests, archive entries, IPC requests, and plugin permissions.
7. **Safe cleanup:** handle crashes, termination, and interrupted operations without leaving uncontrolled processes or mounts.
8. **Honest claims:** distinguish enforced controls, advisory measurements, unsupported controls, and unverified assumptions.

## Isolation caveats

Linux namespaces and cgroups improve isolation and resource control, but containers are not automatically equivalent to virtual machines and are not a universal defense against kernel vulnerabilities or hostile workloads.

Windows Job Objects provide useful process and resource management but do not alone form a complete sandbox.

macOS sandboxing and virtualization have platform-specific constraints and must be evaluated separately.

For workloads requiring a stronger boundary, Cathedral may need a microVM or full VM backend. Such a backend is future work and must not be implied to exist before it is implemented and tested.

## Reporting a vulnerability

Please do not publish details of an exploitable vulnerability before maintainers have had a reasonable opportunity to investigate and coordinate a fix. Use GitHub's private vulnerability reporting feature if it is enabled for this repository. Otherwise contact the maintainers through a private channel associated with the project.

Include affected versions or commits, platform and configuration, reproduction steps, expected impact, and any suggested mitigation. Do not include real credentials or other secrets in reports.

## Security release policy

Security fixes should include regression tests where practical, a clear impact statement, affected versions, mitigations, and release notes. Security claims must be reviewed against the actual implementation on each supported platform.
