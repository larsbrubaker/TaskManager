//! [`MachineInfo`] for the native platform: sysinfo's static facts (host and OS names, CPU
//! brand / vendor / core counts, installed memory, the volume and interface lists, the
//! hardware model via `Product::name`), then the OS module's extras (core classes, cache
//! sizes, interface names and kinds) and the primary IP.
//!
//! Called by `NativeProvider::machine_info` when the sampler starts and on refresh.

use sysinfo::{Disks, Networks, Product, System};
use taskmanager_core::telemetry::{CpuInfo, MachineInfo, MemoryInfo};
use taskmanager_core::Reading;

use super::devices;
use super::os;

/// A non-empty, trimmed string, or `Unavailable`.
fn text(value: Option<String>) -> Reading<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .into()
}

/// Read the machine's static facts. `sys` must have its CPU list (read once, at start-up)
/// and memory refreshed.
pub fn read_machine_info(sys: &System, disks: &Disks, networks: &Networks) -> MachineInfo {
    let first_cpu = sys.cpus().first();
    let mut info = MachineInfo {
        machine_name: text(System::host_name()),
        model: text(Product::name()),
        os_name: text(System::name()),
        os_version: text(System::os_version()),
        cpu: CpuInfo {
            brand: text(first_cpu.map(|c| c.brand().to_string())),
            vendor: text(first_cpu.map(|c| c.vendor_id().to_string())),
            physical_cores: System::physical_core_count().into(),
            logical_cores: sys.cpus().len(),
            ..CpuInfo::default()
        },
        memory: MemoryInfo {
            total: match sys.total_memory() {
                0 => Reading::Unavailable,
                bytes => Reading::Value(bytes),
            },
            ..MemoryInfo::default()
        },
        disks: devices::disk_infos(disks),
        networks: devices::net_infos(networks),
        ..MachineInfo::default()
    };
    os::machine_extras(&mut info);
    info.primary_ip = devices::primary_ip(&info.networks);
    info
}
