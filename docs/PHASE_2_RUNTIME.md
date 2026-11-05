# Phase 2 Runtime Design

## First implementation slice: host preflight

The Phase 2 foundation begins with a read-only runtime preflight in
`cathedral-core::runtime`. The probe reports whether the host exposes:

- The util-linux `unshare` utility.
- User, PID, and mount namespace creation for the current user.
- Network namespace creation for the current user.
- A cgroup v2 hierarchy.
- The cgroup v2 CPU and memory controllers.

On Linux, namespace checks execute a short `/bin/true` process inside the
requested namespaces. They do not modify persistent host configuration.
On Windows and macOS, Linux-specific prerequisites are reported as not
applicable, not as available.

## Capability semantics

A successful preflight is evidence that a narrow host operation succeeded at
the time of the check. It is not proof that arbitrary workloads are safely
isolated, that a cgroup subtree can be delegated, or that resource limits can
be applied. The current probe does not create cgroups, alter namespace policy,
mount filesystems, or launch experiment code.

Cathedral must keep these distinctions visible:

- **Host prerequisite:** an OS feature or utility appears usable.
- **Configured:** Cathedral has successfully prepared required runtime resources.
- **Enforced:** the OS confirms the requested policy is active for the process tree.
- **Observed:** a measurement was collected, with its source and sampling time.

Only the latter two can support claims about actual runtime behavior.

## Next implementation work

1. Define a versioned run configuration and explicit isolation modes.
2. Implement a Linux process supervisor with process-group cleanup and persistent run metadata.
3. Add cgroup v2 delegation checks and CPU/memory enforcement, failing closed when a requested hard limit cannot be installed.
4. Build filesystem and network isolation around explicit mount and namespace policies.
5. Add process, resource, and log collection with timestamps and provenance.
6. Investigate native Windows and macOS backends independently.

## Security boundary

Linux namespaces and cgroups are useful containment mechanisms, but they are
not a universal security boundary against hostile code. Cathedral must not
run untrusted experiments with elevated privileges or expose the host control
plane, registry, credentials, or home directory to experiment write access.
