//! Disks, network interfaces and temperature sensors through sysinfo, for both the static
//! [`MachineInfo`] lists and the per-tick [`Snapshot`] samples.
//!
//! - Disks are sysinfo's mounted volumes, identified by mount point (`/`, `C:\`). sysinfo
//!   has no physical-disk view or device model, so `DiskInfo::model` is `Unavailable` and
//!   active time / response time stay `Unavailable`; M4 adds the per-OS physical disk
//!   layer (IOKit, `/proc/diskstats`, `PhysicalDriveN`). Read / write rates come from
//!   sysinfo's cumulative I/O counters per volume.
//! - Interfaces are identified by their OS name (`en0`, `eth0`, the Windows friendly name);
//!   rates come from cumulative byte counters. The OS module then renames / re-classifies
//!   them (`machine_extras`).
//! - Every sysinfo component with a temperature becomes a `SensorKind::Temperature` sensor,
//!   id `temp/<label>` (de-duplicated with `#2`, `#3`, … when labels repeat).
//!
//! [`Snapshot`]: taskmanager_core::Snapshot

use std::collections::HashMap;
use std::net::{IpAddr, UdpSocket};
use std::time::Duration;

use sysinfo::{Components, Disk, DiskRefreshKind, Disks, Networks};
use taskmanager_core::platform::parse::net::net_kind_from_name;
use taskmanager_core::telemetry::{
    DiskInfo, DiskKind, DiskSample, NetInfo, NetKind, NetSample, SensorKind, SensorSample,
};
use taskmanager_core::Reading;

use super::os;
use super::rates::CounterTracker;

/// The stable id of a volume: its mount point.
pub fn disk_id(disk: &Disk) -> String {
    disk.mount_point().to_string_lossy().into_owned()
}

/// Whether a sysinfo volume is worth listing: it has capacity, and it is not a second
/// mount of a volume already listed (same device name and capacity, as with macOS's
/// read-only system volume and its data volume sharing one APFS container).
///
/// TODO(M4): this is a volume view; replace it with physical disks (IOKit on macOS,
/// `/proc/diskstats` on Linux, `PhysicalDriveN` on Windows) so APFS containers, RAID
/// members and multi-partition drives map to one device each.
fn listed_disks(disks: &Disks) -> Vec<&Disk> {
    let mut seen: Vec<(String, u64)> = Vec::new();
    let mut out = Vec::new();
    for disk in disks.list() {
        if disk.total_space() == 0 {
            continue;
        }
        let ident = (
            disk.name().to_string_lossy().into_owned(),
            disk.total_space(),
        );
        if seen.contains(&ident) {
            continue;
        }
        seen.push(ident);
        out.push(disk);
    }
    out
}

/// The static description of each listed volume.
pub fn disk_infos(disks: &Disks) -> Vec<DiskInfo> {
    listed_disks(disks)
        .into_iter()
        .map(|disk| {
            let name = disk.name().to_string_lossy().trim().to_string();
            DiskInfo {
                id: disk_id(disk),
                name: if name.is_empty() { disk_id(disk) } else { name },
                model: Reading::Unavailable,
                kind: match disk.kind() {
                    sysinfo::DiskKind::SSD => DiskKind::Ssd,
                    sysinfo::DiskKind::HDD => DiskKind::Hdd,
                    sysinfo::DiskKind::Unknown(_) => DiskKind::Unknown,
                },
                capacity: Reading::Value(disk.total_space()),
                is_system: os::is_system_mount(disk.mount_point()),
            }
        })
        .collect()
}

/// The static description of each interface.
pub fn net_infos(networks: &Networks) -> Vec<NetInfo> {
    let mut infos: Vec<NetInfo> = networks
        .list()
        .iter()
        .map(|(name, data)| {
            let mac = data.mac_address();
            let mut ipv4 = Vec::new();
            let mut ipv6 = Vec::new();
            for net in data.ip_networks() {
                match net.addr {
                    IpAddr::V4(addr) => ipv4.push(addr.to_string()),
                    IpAddr::V6(addr) => ipv6.push(addr.to_string()),
                }
            }
            NetInfo {
                id: name.clone(),
                name: name.clone(),
                kind: net_kind_from_name(name),
                mac: if mac.is_unspecified() {
                    Reading::Unavailable
                } else {
                    Reading::Value(mac.to_string())
                },
                ipv4,
                ipv6,
                link_speed_bps: Reading::Unavailable,
            }
        })
        .collect();
    infos.sort_by(|a, b| a.id.cmp(&b.id));
    infos
}

/// The address of the interface carrying the default route: the local address the OS
/// picks for a UDP "connection" to a documentation address (RFC 5737; `connect` on UDP
/// sends nothing). Falls back to the first non-loopback IPv4 address.
pub fn primary_ip(networks: &[NetInfo]) -> Reading<String> {
    let routed = UdpSocket::bind(("0.0.0.0", 0))
        .and_then(|socket| {
            socket.connect(("198.51.100.1", 9))?;
            socket.local_addr()
        })
        .ok()
        .map(|addr| addr.ip())
        .filter(|ip| !ip.is_unspecified() && !ip.is_loopback());
    if let Some(ip) = routed {
        return Reading::Value(ip.to_string());
    }
    networks
        .iter()
        .filter(|n| n.kind != NetKind::Loopback)
        .find_map(|n| n.ipv4.first().cloned())
        .into()
}

/// Per-tick disk, network and sensor sampling with the previous counters it needs.
#[derive(Debug, Default)]
pub struct DeviceSampler {
    disk_read: CounterTracker<String>,
    disk_written: CounterTracker<String>,
    net_rx: CounterTracker<String>,
    net_tx: CounterTracker<String>,
}

impl DeviceSampler {
    /// Refresh the volumes' I/O counters and report their rates.
    pub fn disks(&mut self, disks: &mut Disks, elapsed: Option<Duration>) -> Vec<DiskSample> {
        disks.refresh_specifics(true, DiskRefreshKind::nothing().with_io_usage());
        self.disk_read.begin_tick();
        self.disk_written.begin_tick();
        listed_disks(disks)
            .into_iter()
            .map(|disk| {
                let id = disk_id(disk);
                let usage = disk.usage();
                DiskSample {
                    read_bps: self
                        .disk_read
                        .rate(id.clone(), usage.total_read_bytes, elapsed),
                    write_bps: self.disk_written.rate(
                        id.clone(),
                        usage.total_written_bytes,
                        elapsed,
                    ),
                    total_read: Reading::Value(usage.total_read_bytes),
                    total_written: Reading::Value(usage.total_written_bytes),
                    id,
                    ..DiskSample::default()
                }
            })
            .collect()
    }

    /// Refresh the interfaces' byte counters and report their rates.
    pub fn networks(
        &mut self,
        networks: &mut Networks,
        elapsed: Option<Duration>,
    ) -> Vec<NetSample> {
        networks.refresh(true);
        self.net_rx.begin_tick();
        self.net_tx.begin_tick();
        let mut samples: Vec<NetSample> = networks
            .list()
            .iter()
            .map(|(name, data)| NetSample {
                id: name.clone(),
                rx_bps: self
                    .net_rx
                    .rate(name.clone(), data.total_received(), elapsed),
                tx_bps: self
                    .net_tx
                    .rate(name.clone(), data.total_transmitted(), elapsed),
                total_rx: Reading::Value(data.total_received()),
                total_tx: Reading::Value(data.total_transmitted()),
            })
            .collect();
        samples.sort_by(|a, b| a.id.cmp(&b.id));
        samples
    }
}

/// Refresh the hardware sensors and report every temperature.
pub fn sensors(components: &mut Components) -> Vec<SensorSample> {
    components.refresh(true);
    let mut seen: HashMap<String, usize> = HashMap::new();
    components
        .list()
        .iter()
        .map(|component| {
            let label = component.label().trim();
            let label = if label.is_empty() { "sensor" } else { label };
            let n = seen.entry(label.to_string()).or_insert(0);
            *n += 1;
            let id = if *n == 1 {
                format!("temp/{label}")
            } else {
                format!("temp/{label} #{n}")
            };
            SensorSample {
                id,
                kind: SensorKind::Temperature,
                value: component
                    .temperature()
                    .filter(|t| t.is_finite())
                    .map_or(Reading::Unavailable, Reading::Value),
            }
        })
        .collect()
}
