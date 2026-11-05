# Security Policy

## Status

Cathedral has an early Linux workload runtime based on Bubblewrap namespaces and a restricted filesystem view, with optional cgroup v2 resource controls. It has not undergone a security audit and is not a VM or a hardened boundary against hostile code. **Do not use it to execute untrusted workloads that require strong containment.** Windows and macOS workload isolation is not implemented.

## Phase 1 filesystem handling

Phase 1 rejects parent traversal, symlinked workspace roots, symlinked experiment entries, and workspace paths that are not direct children of Cathedral's managed root. It also checks registered manifests and reports orphaned workspaces through `cathedral doctor`. These checks protect against common mistakes and accidental path substitution, but they are not race-free against a hostile process running with the same user privileges. These Phase 1 filesystem checks do not provide a sandbox. The Linux Phase 2 runner adds a separate OS-level containment layer described below.

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

## Implemented Linux runtime boundary

The Linux runner uses Bubblewrap to create user, PID, IPC, UTS, and network namespaces. It presents `/usr` and `/etc` read-only, mounts the experiment workspace writable, mounts `/tmp` as temporary storage, and clears the inherited environment before setting a small allowlist plus manifest variables. Network access is disabled by default. Optional memory, CPU, and process-count ceilings are applied using cgroup v2 only when a delegated cgroup root is configured and all requested controller files accept the settings. A failed requested limit prevents launch.

The runner records process identity, combined stdout/stderr, and resource samples. Process and cgroup cleanup is best-effort and reports failures. A kernel vulnerability, a Bubblewrap or kernel configuration defect, or an incorrectly delegated cgroup can still undermine containment. The implementation has not been independently security audited.

Windows and macOS do not yet have workload backends. Their CLI runtime commands return an unsupported-platform error rather than silently running workloads without isolation.

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


Run records, status files, logs, and resource history are stored under Cathedral's data directory, outside the writable experiment workspace. The sandbox does not receive a bind mount of the registry or run-control directory.
