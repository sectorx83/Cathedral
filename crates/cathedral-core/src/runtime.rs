//! Host-side runtime prerequisite inspection.
//!
//! These checks are diagnostic only. They do not start workloads, create a
//! sandbox, or prove that a future runtime configuration will be enforceable.

#[cfg(target_os = "linux")]
use std::{fs, process::Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeStatus {
    Available,
    Unavailable,
    NotApplicable,
}

impl ProbeStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::NotApplicable => "not applicable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeProbe {
    pub unshare_binary: ProbeStatus,
    pub user_pid_mount_namespaces: ProbeStatus,
    pub network_namespace: ProbeStatus,
    pub cgroup_v2: ProbeStatus,
    pub cpu_controller: ProbeStatus,
    pub memory_controller: ProbeStatus,
}

impl RuntimeProbe {
    /// Inspect host prerequisites without changing persistent host configuration.
    pub fn inspect() -> Self {
        #[cfg(target_os = "linux")]
        {
            inspect_linux()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self {
                unshare_binary: ProbeStatus::NotApplicable,
                user_pid_mount_namespaces: ProbeStatus::NotApplicable,
                network_namespace: ProbeStatus::NotApplicable,
                cgroup_v2: ProbeStatus::NotApplicable,
                cpu_controller: ProbeStatus::NotApplicable,
                memory_controller: ProbeStatus::NotApplicable,
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn inspect_linux() -> RuntimeProbe {
    let unshare_installed = Command::new("unshare")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    let (user_pid_mount_namespaces, network_namespace) = if unshare_installed {
        (
            probe_unshare(&[
                "--user",
                "--map-root-user",
                "--pid",
                "--fork",
                "--mount-proc",
                "/bin/true",
            ]),
            probe_unshare(&["--user", "--map-root-user", "--net", "--fork", "/bin/true"]),
        )
    } else {
        (ProbeStatus::Unavailable, ProbeStatus::Unavailable)
    };
    let controllers = fs::read_to_string("/sys/fs/cgroup/cgroup.controllers").ok();
    let cgroup_v2 = if controllers.is_some() {
        ProbeStatus::Available
    } else {
        ProbeStatus::Unavailable
    };
    RuntimeProbe {
        unshare_binary: if unshare_installed {
            ProbeStatus::Available
        } else {
            ProbeStatus::Unavailable
        },
        user_pid_mount_namespaces,
        network_namespace,
        cgroup_v2,
        cpu_controller: controller_status(controllers.as_deref(), "cpu"),
        memory_controller: controller_status(controllers.as_deref(), "memory"),
    }
}

#[cfg(target_os = "linux")]
fn probe_unshare(arguments: &[&str]) -> ProbeStatus {
    match Command::new("unshare").args(arguments).output() {
        Ok(output) if output.status.success() => ProbeStatus::Available,
        Ok(_) | Err(_) => ProbeStatus::Unavailable,
    }
}

#[cfg(any(target_os = "linux", test))]
fn controller_status(contents: Option<&str>, controller: &str) -> ProbeStatus {
    match contents {
        Some(value) if value.split_whitespace().any(|item| item == controller) => {
            ProbeStatus::Available
        }
        Some(_) => ProbeStatus::Unavailable,
        None => ProbeStatus::NotApplicable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_detection_requires_an_exact_token() {
        assert_eq!(
            controller_status(Some("cpu memory io"), "cpu"),
            ProbeStatus::Available
        );
        assert_eq!(
            controller_status(Some("cpuset memory"), "cpu"),
            ProbeStatus::Unavailable
        );
    }

    #[test]
    fn missing_cgroup_file_is_not_misreported_as_available() {
        assert_eq!(
            controller_status(None, "memory"),
            ProbeStatus::NotApplicable
        );
    }

    #[test]
    fn probe_result_has_a_status_for_every_capability() {
        let result = RuntimeProbe::inspect();
        assert!(!result.unshare_binary.label().is_empty());
        assert!(!result.user_pid_mount_namespaces.label().is_empty());
        assert!(!result.network_namespace.label().is_empty());
        assert!(!result.cgroup_v2.label().is_empty());
        assert!(!result.cpu_controller.label().is_empty());
        assert!(!result.memory_controller.label().is_empty());
    }
}
