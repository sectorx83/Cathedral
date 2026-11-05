//! Host identification and platform capability contracts.
//!
//! Phase 1 detects the host only. This module does not claim that an execution
//! or isolation backend exists on any operating system.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPlatform {
    Linux,
    Windows,
    MacOs,
    Other,
}

impl HostPlatform {
    pub const fn current() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Self::MacOs
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            Self::Other
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityState {
    Implemented,
    Planned,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformInfo {
    pub platform: HostPlatform,
    pub architecture: &'static str,
    pub operating_system: &'static str,
}

impl PlatformInfo {
    pub fn detect() -> Self {
        Self {
            platform: HostPlatform::current(),
            architecture: std::env::consts::ARCH,
            operating_system: std::env::consts::OS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub managed_workspaces: CapabilityState,
    pub workload_isolation: CapabilityState,
    pub resource_enforcement: CapabilityState,
    pub live_observability: CapabilityState,
}

/// Contract implemented by future native runtime backends.
///
/// The Phase 1 core intentionally provides no implementation of this trait.
/// A backend must report its actual capabilities rather than inheriting
/// optimistic defaults.
pub trait PlatformBackend {
    fn info(&self) -> PlatformInfo;
    fn capabilities(&self) -> PlatformCapabilities;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_detection_returns_platform_and_architecture() {
        let info = PlatformInfo::detect();
        assert!(!info.architecture.is_empty());
        assert!(!info.operating_system.is_empty());
        assert_eq!(info.platform, HostPlatform::current());
    }

    #[test]
    fn phase_one_capabilities_do_not_claim_runtime_support() {
        let capabilities = PlatformCapabilities {
            managed_workspaces: CapabilityState::Implemented,
            workload_isolation: CapabilityState::Planned,
            resource_enforcement: CapabilityState::Planned,
            live_observability: CapabilityState::Planned,
        };
        assert_eq!(capabilities.workload_isolation, CapabilityState::Planned);
    }
}
