//! The demo machine's static profile: the [`MachineInfo`] the demo reports, and the device
//! ids its snapshots use.
//!
//! Modelled on the reference capture's Mac (an "Apple M5" class laptop: 10 logical CPUs
//! in a "Super" class of 4 and an "Efficiency" class of 6, 24 GB unified memory, a 1 TB
//! "APPLE SSD AP1024Z", 18 network interfaces with Wi-Fi on `en0`, a 10-core GPU and a
//! one-block Neural Engine), with generic names only: no serial numbers, UUIDs or real
//! user names. Fields a real Mac does not report (base speed, L3, memory speed and slots)
//! are `Unavailable` here too, as on the reference's CPU page.
//!
//! [`super::load`] produces the moving numbers for these devices; the ids below tie the
//! two together.

use crate::reading::Reading;
use crate::telemetry::{
    CoreClass, CpuInfo, DiskInfo, DiskKind, GpuInfo, MachineInfo, MemoryInfo, NetInfo, NetKind,
    NpuInfo,
};

use super::rng::hash;

/// Bytes in a gibibyte (the UI's "GB").
pub const GIB: u64 = 1 << 30;
/// Physical memory.
pub const MEMORY_TOTAL: u64 = 24 * GIB;
/// Swap file space macOS has allocated.
pub const SWAP_TOTAL: u64 = 3 * GIB;
/// The share of unified memory the GPU may use.
pub const GPU_MEMORY_BUDGET: u64 = 18 * GIB;
/// Logical CPUs.
pub const LOGICAL_CPUS: usize = 10;
/// Logical CPUs `0..SUPER_CORES` are the "Super" class, the rest "Efficiency".
pub const SUPER_CORES: usize = 4;
/// The system disk's id.
pub const DISK_ID: &str = "disk0";
/// The GPU's id.
pub const GPU_ID: &str = "gpu0";
/// The Neural Engine's id.
pub const NPU_ID: &str = "ane0";
/// The interface carrying the default route.
pub const PRIMARY_NET: &str = "en0";
/// The loopback interface.
pub const LOOPBACK_NET: &str = "lo0";
/// The peer-to-peer wireless interface, which sees occasional chatter.
pub const AWDL_NET: &str = "awdl0";
/// The demo's host name.
pub const MACHINE_NAME: &str = "demo-macbook";
/// The demo's IPv4 address on Wi-Fi.
pub const PRIMARY_IPV4: &str = "192.168.1.42";

/// The temperature sensors, `(id, display name)`.
pub const TEMPERATURE_SENSORS: [(&str, &str); 7] = [
    ("cpu/die-1", "Die 1"),
    ("cpu/die-2", "Die 2"),
    ("cpu/die-3", "Die 3"),
    ("cpu/die-4", "Die 4"),
    ("cpu/hotspot", "CPU hotspot"),
    ("soc/hotspot", "SoC hotspot"),
    ("disk0/temperature", "SSD"),
];
/// The power sensors, `(id, display name)`.
pub const POWER_SENSORS: [(&str, &str); 4] = [
    ("cpu/power", "CPU"),
    ("gpu/power", "GPU"),
    ("ane/power", "Neural Engine"),
    ("system/power", "System total"),
];
/// The clock sensors, `(id, display name)`.
pub const CLOCK_SENSORS: [(&str, &str); 3] = [
    ("cpu/super-clock", "Super cluster"),
    ("cpu/efficiency-clock", "Efficiency cluster"),
    ("gpu/clock", "GPU"),
];
/// The utilisation sensors, `(id, display name)`.
pub const UTILIZATION_SENSORS: [(&str, &str); 2] = [
    ("cpu/super-active", "Super cluster active"),
    ("cpu/efficiency-active", "Efficiency cluster active"),
];

/// How each of the 18 network interfaces behaves in the simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetRole {
    /// Wi-Fi with the default route: bursty traffic.
    Primary,
    /// Loopback: steady local chatter, rx == tx.
    Loopback,
    /// Peer-to-peer wireless: occasional small bursts.
    Awdl,
    /// Up but carrying nothing.
    Idle,
}

/// One interface of the profile: `(id, display name, kind, role)`.
pub const INTERFACES: [(&str, &str, NetKind, NetRole); 18] = [
    ("lo0", "Loopback", NetKind::Loopback, NetRole::Loopback),
    ("gif0", "Generic tunnel", NetKind::Virtual, NetRole::Idle),
    ("stf0", "6to4 tunnel", NetKind::Virtual, NetRole::Idle),
    ("anpi0", "Ethernet adapter 1", NetKind::Other, NetRole::Idle),
    ("anpi1", "Ethernet adapter 2", NetKind::Other, NetRole::Idle),
    ("en0", "Wi-Fi", NetKind::WiFi, NetRole::Primary),
    ("en1", "Thunderbolt 1", NetKind::Ethernet, NetRole::Idle),
    ("en2", "Thunderbolt 2", NetKind::Ethernet, NetRole::Idle),
    ("en3", "Thunderbolt 3", NetKind::Ethernet, NetRole::Idle),
    ("en4", "Ethernet", NetKind::Ethernet, NetRole::Idle),
    (
        "bridge0",
        "Thunderbolt Bridge",
        NetKind::Virtual,
        NetRole::Idle,
    ),
    ("ap1", "Wi-Fi access point", NetKind::WiFi, NetRole::Idle),
    (
        "awdl0",
        "Wireless direct link",
        NetKind::WiFi,
        NetRole::Awdl,
    ),
    ("llw0", "Low-latency WLAN", NetKind::Virtual, NetRole::Idle),
    ("utun0", "Tunnel 0", NetKind::Virtual, NetRole::Idle),
    ("utun1", "Tunnel 1", NetKind::Virtual, NetRole::Idle),
    ("utun2", "Tunnel 2", NetKind::Virtual, NetRole::Idle),
    ("utun3", "Tunnel 3", NetKind::Virtual, NetRole::Idle),
];

/// A locally administered (so never a real vendor's) MAC address derived from the seed.
fn demo_mac(seed: u64, index: usize) -> String {
    let h = hash(seed, 0x3AC, index as i64).to_be_bytes();
    format!(
        "02:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        h[0], h[1], h[2], h[3], h[4]
    )
}

fn network_info(seed: u64) -> Vec<NetInfo> {
    INTERFACES
        .iter()
        .enumerate()
        .map(|(i, &(id, name, kind, role))| {
            let mac = match kind {
                NetKind::Loopback | NetKind::Virtual => Reading::Unsupported,
                _ => Reading::Value(demo_mac(seed, i)),
            };
            let (ipv4, ipv6, link) = match role {
                NetRole::Primary => (
                    vec![PRIMARY_IPV4.to_string()],
                    vec!["fe80::1c2b:3d4e:5f60:7182".to_string()],
                    Reading::Value(1_200_000_000),
                ),
                NetRole::Loopback => (
                    vec!["127.0.0.1".to_string()],
                    vec!["::1".to_string(), "fe80::1".to_string()],
                    Reading::Unsupported,
                ),
                NetRole::Awdl => (
                    Vec::new(),
                    vec!["fe80::a0b1:c2ff:fed3:e4f5".to_string()],
                    Reading::Unavailable,
                ),
                NetRole::Idle => (Vec::new(), Vec::new(), Reading::Unavailable),
            };
            NetInfo {
                id: id.to_string(),
                name: name.to_string(),
                kind,
                mac,
                ipv4,
                ipv6,
                link_speed_bps: link,
            }
        })
        .collect()
}

/// The static facts the demo machine reports.
pub fn machine_info(seed: u64) -> MachineInfo {
    MachineInfo {
        machine_name: Reading::Value(MACHINE_NAME.to_string()),
        model: Reading::Value("MacBook Pro (14-inch)".to_string()),
        os_name: Reading::Value("macOS".to_string()),
        os_version: Reading::Value("26.0".to_string()),
        cpu: CpuInfo {
            brand: Reading::Value("Apple M5".to_string()),
            vendor: Reading::Value("Apple".to_string()),
            physical_cores: Reading::Value(LOGICAL_CPUS),
            logical_cores: LOGICAL_CPUS,
            sockets: Reading::Value(1),
            core_classes: vec![
                CoreClass {
                    name: "Super".to_string(),
                    logical_indices: (0..SUPER_CORES).collect(),
                },
                CoreClass {
                    name: "Efficiency".to_string(),
                    logical_indices: (SUPER_CORES..LOGICAL_CPUS).collect(),
                },
            ],
            // Apple silicon publishes no base clock or L3 to user space.
            base_mhz: Reading::Unavailable,
            l1: Reading::Value(192 * 1024),
            l2: Reading::Value(6 * 1024 * 1024),
            l3: Reading::Unavailable,
            virtualization: Reading::Value("Apple Hypervisor supported".to_string()),
            is_vm: Reading::Value(false),
        },
        memory: MemoryInfo {
            total: Reading::Value(MEMORY_TOTAL),
            kind: Reading::Value("LPDDR5X".to_string()),
            speed_mts: Reading::Unavailable,
            form_factor: Reading::Value("On package".to_string()),
            slots_used: Reading::Unavailable,
            slots_total: Reading::Unavailable,
        },
        disks: vec![DiskInfo {
            id: DISK_ID.to_string(),
            name: "Macintosh HD".to_string(),
            model: Reading::Value("APPLE SSD AP1024Z".to_string()),
            kind: DiskKind::Ssd,
            capacity: Reading::Value(1_000_555_581_440),
            is_system: true,
        }],
        networks: network_info(seed),
        gpus: vec![GpuInfo {
            id: GPU_ID.to_string(),
            name: "Apple M5".to_string(),
            cores: Reading::Value(10),
        }],
        npus: vec![NpuInfo {
            id: NPU_ID.to_string(),
            name: "Apple Neural Engine".to_string(),
            blocks: Reading::Value(1),
        }],
        primary_ip: Reading::Value(PRIMARY_IPV4.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::ProcessorBrand;

    #[test]
    fn profile_matches_the_reference_machine() {
        let info = machine_info(1);
        assert_eq!(info.processor_brand(), ProcessorBrand::AppleSilicon);
        assert_eq!(info.cpu.logical_cores, 10);
        let classes: Vec<(&str, usize)> = info
            .cpu
            .core_classes
            .iter()
            .map(|c| (c.name.as_str(), c.logical_indices.len()))
            .collect();
        assert_eq!(classes, vec![("Super", 4), ("Efficiency", 6)]);
        assert_eq!(info.networks.len(), 18);
        assert_eq!(info.system_disk().map(|d| d.id.as_str()), Some(DISK_ID));
        assert!(info.cpu.base_mhz.is_unavailable() && info.cpu.l3.is_unavailable());
        assert_eq!(info.memory.total, Reading::Value(MEMORY_TOTAL));
    }

    #[test]
    fn macs_are_locally_administered_and_seeded() {
        let a = machine_info(1);
        let b = machine_info(2);
        let mac = |i: &MachineInfo| i.network(PRIMARY_NET).and_then(|n| n.mac.value().cloned());
        let mac_a = mac(&a).unwrap_or_default();
        assert!(mac_a.starts_with("02:"), "{mac_a}");
        assert_ne!(mac(&a), mac(&b));
    }
}
