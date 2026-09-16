//! CPU, memory and OS facts about the host.
//!
//! Per-OS, because there is no portable way to ask and a portable guess is
//! exactly the metadata lie the schema refuses to publish. Hosts this build
//! cannot interrogate fail loudly with [`ProbeError::UnsupportedHost`].

use super::ProbeError;
use crate::schema::{Hardware, HostPlatform};

/// Reads the CPU model, core counts and installed memory.
pub fn probe_hardware() -> Result<Hardware, ProbeError> {
    imp::hardware()
}

/// Reads the OS name, version, architecture and the filesystem of `path`.
pub fn probe_platform(filesystem: String) -> Result<HostPlatform, ProbeError> {
    Ok(HostPlatform {
        os: std::env::consts::OS.to_owned(),
        os_version: imp::os_version()?,
        arch: std::env::consts::ARCH.to_owned(),
        filesystem,
    })
}

#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux")),
    allow(dead_code, reason = "only the per-OS probes call it")
)]
fn parse_u64(field: &'static str, text: &str) -> Result<u64, ProbeError> {
    text.trim()
        .parse::<u64>()
        .map_err(|error| ProbeError::missing(field, format!("`{text}` is not a number: {error}")))
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{parse_u64, Hardware, ProbeError};
    use crate::env::exec::output;

    pub fn hardware() -> Result<Hardware, ProbeError> {
        let cpu_model = output("sysctl", &["-n", "machdep.cpu.brand_string"])?;
        if cpu_model.is_empty() {
            return Err(ProbeError::missing(
                "hardware.cpu_model",
                "machdep.cpu.brand_string is empty",
            ));
        }
        Ok(Hardware {
            cpu_model,
            physical_cores: parse_u64(
                "hardware.physical_cores",
                &output("sysctl", &["-n", "hw.physicalcpu"])?,
            )?,
            logical_cores: parse_u64(
                "hardware.logical_cores",
                &output("sysctl", &["-n", "hw.logicalcpu"])?,
            )?,
            memory_bytes: parse_u64(
                "hardware.memory_bytes",
                &output("sysctl", &["-n", "hw.memsize"])?,
            )?,
        })
    }

    pub fn os_version() -> Result<String, ProbeError> {
        let product = output("sw_vers", &["-productVersion"])?;
        let build = output("sw_vers", &["-buildVersion"])?;
        if product.is_empty() {
            return Err(ProbeError::missing(
                "platform.os_version",
                "sw_vers is mute",
            ));
        }
        Ok(format!("{product} ({build})"))
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{parse_u64, Hardware, ProbeError};
    use std::fs;

    pub fn hardware() -> Result<Hardware, ProbeError> {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").map_err(|error| {
            ProbeError::missing("hardware.cpu_model", format!("/proc/cpuinfo: {error}"))
        })?;
        let cpu_model = field(&cpuinfo, "model name")
            .or_else(|| field(&cpuinfo, "Model"))
            .ok_or_else(|| {
                ProbeError::missing("hardware.cpu_model", "no model line in /proc/cpuinfo")
            })?;

        let logical_cores = cpuinfo
            .lines()
            .filter(|line| line.starts_with("processor"))
            .count() as u64;
        if logical_cores == 0 {
            return Err(ProbeError::missing(
                "hardware.logical_cores",
                "no processor lines in /proc/cpuinfo",
            ));
        }
        let cores_per_socket = field(&cpuinfo, "cpu cores")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(logical_cores);
        let siblings = field(&cpuinfo, "siblings")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(logical_cores);
        let threads_per_core = (siblings / cores_per_socket.max(1)).max(1);
        let physical_cores = (logical_cores / threads_per_core).max(1);

        let meminfo = fs::read_to_string("/proc/meminfo").map_err(|error| {
            ProbeError::missing("hardware.memory_bytes", format!("/proc/meminfo: {error}"))
        })?;
        let mem_kib = field(&meminfo, "MemTotal")
            .map(|value| value.trim_end_matches(" kB").to_owned())
            .ok_or_else(|| {
                ProbeError::missing("hardware.memory_bytes", "no MemTotal in /proc/meminfo")
            })?;
        let memory_bytes = parse_u64("hardware.memory_bytes", &mem_kib)? * 1024;

        Ok(Hardware {
            cpu_model,
            physical_cores,
            logical_cores,
            memory_bytes,
        })
    }

    pub fn os_version() -> Result<String, ProbeError> {
        let release = fs::read_to_string("/proc/sys/kernel/osrelease").map_err(|error| {
            ProbeError::missing(
                "platform.os_version",
                format!("/proc/sys/kernel/osrelease: {error}"),
            )
        })?;
        let release = release.trim().to_owned();
        if release.is_empty() {
            return Err(ProbeError::missing(
                "platform.os_version",
                "empty kernel release",
            ));
        }
        Ok(release)
    }

    fn field(text: &str, key: &str) -> Option<String> {
        text.lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_once(':'))
            .map(|(_, value)| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod imp {
    use super::{Hardware, ProbeError};

    pub fn hardware() -> Result<Hardware, ProbeError> {
        Err(ProbeError::UnsupportedHost {
            os: std::env::consts::OS,
        })
    }

    pub fn os_version() -> Result<String, ProbeError> {
        Err(ProbeError::UnsupportedHost {
            os: std::env::consts::OS,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn the_host_reports_real_hardware() {
        let hardware = probe_hardware().expect("macOS and Linux hosts are probeable");
        assert!(!hardware.cpu_model.is_empty());
        assert!(hardware.physical_cores >= 1);
        assert!(hardware.logical_cores >= hardware.physical_cores);
        assert!(hardware.memory_bytes > 64 * 1024 * 1024);
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn the_host_reports_an_os_version() {
        let platform = probe_platform("apfs".to_owned()).expect("probeable host");
        assert!(!platform.os_version.is_empty());
        assert_eq!(platform.arch, std::env::consts::ARCH);
        assert_eq!(platform.filesystem, "apfs");
    }

    #[test]
    fn unparseable_numbers_name_their_field() {
        let error = parse_u64("hardware.memory_bytes", "lots").expect_err("not a number");
        assert!(error.to_string().contains("hardware.memory_bytes"));
    }
}
