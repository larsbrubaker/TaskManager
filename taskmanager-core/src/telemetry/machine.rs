//! `MachineInfo`: the static facts about the machine (PLAN §4.2, §5.2, §5.9).
//!
//! Read once from the [`TelemetryProvider`](crate::platform::TelemetryProvider) when the
//! [`Sampler`](super::Sampler) starts and again on request (a disk or interface was
//! added). Snapshots refer to devices by the `id`s listed here. Anything the platform may
//! not be able to tell us is a [`Reading`].
//!
//! Also derives the [`ProcessorBrand`] (which names the Processor page "Apple Silicon" or
//! "Processor") from the CPU vendor and brand text, so the core never needs `cfg`.

use crate::page::ProcessorBrand;
use crate::reading::Reading;

/// Static facts about the machine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MachineInfo {
    /// Host name.
    pub machine_name: Reading<String>,
    /// Hardware model ("MacBook Pro (14-inch, 2024)", DMI product name, …).
    pub model: Reading<String>,
    /// "macOS", "Windows 11 Pro", "Ubuntu", …
    pub os_name: Reading<String>,
    pub os_version: Reading<String>,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetInfo>,
    pub gpus: Vec<GpuInfo>,
    pub npus: Vec<NpuInfo>,
    /// The address of the interface carrying the default route.
    pub primary_ip: Reading<String>,
}

impl MachineInfo {
    /// The processor brand for page naming.
    pub fn processor_brand(&self) -> ProcessorBrand {
        self.cpu.processor_brand()
    }

    /// The disk with this id.
    pub fn disk(&self, id: &str) -> Option<&DiskInfo> {
        self.disks.iter().find(|d| d.id == id)
    }

    /// The network interface with this id.
    pub fn network(&self, id: &str) -> Option<&NetInfo> {
        self.networks.iter().find(|n| n.id == id)
    }

    /// The disk holding the OS, if known.
    pub fn system_disk(&self) -> Option<&DiskInfo> {
        self.disks.iter().find(|d| d.is_system)
    }
}

/// The processor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuInfo {
    /// Marketing name ("Apple M3 Pro", "AMD Ryzen 9 7950X").
    pub brand: Reading<String>,
    /// Vendor id ("Apple", "GenuineIntel", "AuthenticAMD").
    pub vendor: Reading<String>,
    pub physical_cores: Reading<usize>,
    /// Logical processors; `Snapshot::cpu.per_cpu` has this many entries.
    pub logical_cores: usize,
    pub sockets: Reading<usize>,
    /// Core classes (Performance / Efficiency, …); empty when the CPU has a single class
    /// or the platform cannot tell.
    pub core_classes: Vec<CoreClass>,
    pub base_mhz: Reading<f32>,
    /// Cache sizes in bytes (total across the package).
    pub l1: Reading<u64>,
    pub l2: Reading<u64>,
    pub l3: Reading<u64>,
    /// "Enabled", "Disabled", "VT-x", …
    pub virtualization: Reading<String>,
    /// Whether this OS is running in a virtual machine.
    pub is_vm: Reading<bool>,
}

impl CpuInfo {
    /// The processor brand for page naming (generic when neither text is known).
    pub fn processor_brand(&self) -> ProcessorBrand {
        let vendor = self.vendor.value().map_or("", String::as_str);
        let brand = self.brand.value().map_or("", String::as_str);
        processor_brand(vendor, brand)
    }
}

/// A group of logical CPUs of one kind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoreClass {
    /// "Performance", "Efficiency" or "Super".
    pub name: String,
    /// Indices into `Snapshot::cpu.per_cpu`.
    pub logical_indices: Vec<usize>,
}

/// Installed memory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryInfo {
    /// Bytes of physical memory.
    pub total: Reading<u64>,
    /// "LPDDR5", "DDR4", …
    pub kind: Reading<String>,
    /// Transfer rate in MT/s.
    pub speed_mts: Reading<u32>,
    /// "SODIMM", "DIMM", "On package", …
    pub form_factor: Reading<String>,
    pub slots_used: Reading<u32>,
    pub slots_total: Reading<u32>,
}

/// What kind of storage a disk is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DiskKind {
    Ssd,
    Hdd,
    #[default]
    Unknown,
}

/// One physical disk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiskInfo {
    /// Stable id ("disk0", "nvme0n1", "PhysicalDrive0"); `DiskSample::id` matches.
    pub id: String,
    /// Display name ("Macintosh HD", "Disk 0 (C:)").
    pub name: String,
    /// Device model ("APPLE SSD AP1024Z").
    pub model: Reading<String>,
    pub kind: DiskKind,
    /// Bytes.
    pub capacity: Reading<u64>,
    /// Whether the OS boots from this disk.
    pub is_system: bool,
}

/// What kind of link a network interface is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NetKind {
    Ethernet,
    WiFi,
    Loopback,
    Virtual,
    #[default]
    Other,
}

/// One network interface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetInfo {
    /// Stable id ("en0", "eth0", the adapter GUID); `NetSample::id` matches.
    pub id: String,
    /// Display name ("Wi-Fi", "Ethernet").
    pub name: String,
    pub kind: NetKind,
    /// Hardware address, "aa:bb:cc:dd:ee:ff".
    pub mac: Reading<String>,
    /// Assigned IPv4 addresses (empty: none assigned).
    pub ipv4: Vec<String>,
    /// Assigned IPv6 addresses (empty: none assigned).
    pub ipv6: Vec<String>,
    /// Negotiated link speed in bits per second.
    pub link_speed_bps: Reading<u64>,
}

/// One GPU.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuInfo {
    /// Stable id; `GpuSample::id` matches.
    pub id: String,
    pub name: String,
    /// GPU cores (Apple) / compute units.
    pub cores: Reading<u32>,
}

/// One NPU (neural engine).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NpuInfo {
    /// Stable id; `NpuSample::id` matches.
    pub id: String,
    pub name: String,
    /// Independently powered blocks.
    pub blocks: Reading<u32>,
}

/// Derive the processor brand from the CPU vendor id and marketing name.
///
/// Apple silicon reports vendor "Apple" (macOS) or a brand like "Apple M2 Max"; Intel
/// Macs, PCs and VMs report something else. Matching is case-insensitive and ignores
/// surrounding whitespace.
pub fn processor_brand(vendor: &str, brand: &str) -> ProcessorBrand {
    if vendor.trim().eq_ignore_ascii_case("apple") {
        return ProcessorBrand::AppleSilicon;
    }
    let brand = brand.trim().to_ascii_lowercase();
    let mut words = brand.split_whitespace();
    let is_apple_m = words.next() == Some("apple")
        && words.next().is_some_and(|chip| {
            let mut chars = chip.chars();
            chars.next() == Some('m') && chars.next().is_some_and(|c| c.is_ascii_digit())
        });
    if is_apple_m {
        ProcessorBrand::AppleSilicon
    } else {
        ProcessorBrand::Generic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_vendor_is_apple_silicon() {
        assert_eq!(
            processor_brand("Apple", "Apple M3 Pro"),
            ProcessorBrand::AppleSilicon
        );
        assert_eq!(processor_brand(" apple ", ""), ProcessorBrand::AppleSilicon);
    }

    #[test]
    fn apple_m_brand_is_apple_silicon_without_vendor() {
        assert_eq!(
            processor_brand("", "Apple M1"),
            ProcessorBrand::AppleSilicon
        );
        assert_eq!(
            processor_brand("", "  APPLE m4 Max "),
            ProcessorBrand::AppleSilicon
        );
    }

    #[test]
    fn everything_else_is_generic() {
        assert_eq!(
            processor_brand("GenuineIntel", "Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz"),
            ProcessorBrand::Generic
        );
        assert_eq!(
            processor_brand("AuthenticAMD", "AMD Ryzen 9 7950X 16-Core Processor"),
            ProcessorBrand::Generic
        );
        assert_eq!(processor_brand("", ""), ProcessorBrand::Generic);
        // "Apple" alone, or a word that merely starts with m, is not an M-series chip.
        assert_eq!(processor_brand("", "Apple"), ProcessorBrand::Generic);
        assert_eq!(processor_brand("", "Apple Mobile"), ProcessorBrand::Generic);
        assert_eq!(processor_brand("", "Pineapple M1"), ProcessorBrand::Generic);
    }

    #[test]
    fn machine_info_delegates_brand_and_finds_devices() {
        let info = MachineInfo {
            cpu: CpuInfo {
                vendor: Reading::Value("Apple".into()),
                brand: Reading::Unavailable,
                logical_cores: 8,
                ..CpuInfo::default()
            },
            disks: vec![
                DiskInfo {
                    id: "disk1".into(),
                    ..DiskInfo::default()
                },
                DiskInfo {
                    id: "disk0".into(),
                    is_system: true,
                    kind: DiskKind::Ssd,
                    ..DiskInfo::default()
                },
            ],
            networks: vec![NetInfo {
                id: "en0".into(),
                kind: NetKind::WiFi,
                ..NetInfo::default()
            }],
            ..MachineInfo::default()
        };
        assert_eq!(info.processor_brand(), ProcessorBrand::AppleSilicon);
        assert_eq!(info.system_disk().map(|d| d.id.as_str()), Some("disk0"));
        assert_eq!(info.disk("disk1").map(|d| d.kind), Some(DiskKind::Unknown));
        assert_eq!(info.network("en0").map(|n| n.kind), Some(NetKind::WiFi));
        assert!(info.network("eth0").is_none());
    }

    #[test]
    fn brand_text_alone_is_enough() {
        let cpu = CpuInfo {
            brand: Reading::Value("Apple M4".into()),
            ..CpuInfo::default()
        };
        assert_eq!(cpu.processor_brand(), ProcessorBrand::AppleSilicon);
    }

    #[test]
    fn defaults_claim_nothing() {
        let info = MachineInfo::default();
        assert!(info.machine_name.is_unavailable());
        assert!(info.cpu.is_vm.is_unavailable());
        assert!(info.cpu.brand.is_unavailable() && info.cpu.vendor.is_unavailable());
        assert!(info.memory.total.is_unavailable());
        assert!(info.primary_ip.is_unavailable());
        assert_eq!(info.processor_brand(), ProcessorBrand::Generic);
    }
}
