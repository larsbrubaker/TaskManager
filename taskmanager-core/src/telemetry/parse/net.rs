//! Network interface classification: the [`NetKind`] of an interface from its name (every
//! OS), and the macOS `networksetup -listallhardwareports` listing, which maps BSD names
//! (`en0`) to hardware port names (`Wi-Fi`, `Thunderbolt Bridge`).
//!
//! The native platform lists interfaces through sysinfo and asks [`net_kind_from_name`];
//! on macOS, where `en0` may be Wi-Fi or Ethernet, it runs `networksetup` once per
//! `machine_info` and prefers [`HardwarePort::kind`]. Pure, so tested on every OS.

use crate::telemetry::NetKind;

/// Guess an interface's kind from its name (`lo0`, `wlp2s0`, `utun3`, `eth0`, Windows
/// friendly names such as `Wi-Fi` or `vEthernet (WSL)`). `Other` when unsure.
pub fn net_kind_from_name(name: &str) -> NetKind {
    let lower = name.trim().to_ascii_lowercase();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| lower.starts_with(p));
    let contains = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));

    let is_lo_name = lower == "lo"
        || lower
            .strip_prefix("lo")
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
    if is_lo_name || contains(&["loopback"]) {
        return NetKind::Loopback;
    }
    if contains(&[
        "virtual",
        "vethernet",
        "hyper-v",
        "vmware",
        "virtualbox",
        "pseudo",
        "vpn",
        "tunnel",
        "isatap",
        "teredo",
        "tap-",
        "wsl",
    ]) || starts(&[
        "utun",
        "tun",
        "tap",
        "awdl",
        "llw",
        "bridge",
        "gif",
        "stf",
        "anpi",
        "vmnet",
        "veth",
        "docker",
        "virbr",
        "br-",
        "wg",
        "tailscale",
        "zt",
        "vboxnet",
        "ipsec",
        "ppp",
        "ap1",
    ]) {
        return NetKind::Virtual;
    }
    if starts(&["wl"]) || contains(&["wi-fi", "wifi", "wireless", "wlan", "802.11"]) {
        return NetKind::WiFi;
    }
    if starts(&["eth", "en", "em"]) || contains(&["ethernet"]) {
        return NetKind::Ethernet;
    }
    NetKind::Other
}

/// One entry of `networksetup -listallhardwareports`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardwarePort {
    /// "Wi-Fi", "Ethernet Adapter (en4)", "Thunderbolt Bridge".
    pub port: String,
    /// BSD name, "en0".
    pub device: String,
    /// "aa:bb:cc:dd:ee:ff", when listed.
    pub mac: Option<String>,
}

impl HardwarePort {
    /// The interface kind its port name implies.
    pub fn kind(&self) -> NetKind {
        let port = self.port.to_ascii_lowercase();
        if port.contains("wi-fi") || port.contains("airport") {
            NetKind::WiFi
        } else if port.contains("bridge") || port.contains("vpn") {
            NetKind::Virtual
        } else if port.contains("ethernet") || port.contains("lan") {
            NetKind::Ethernet
        } else {
            NetKind::Other
        }
    }
}

/// Parse `networksetup -listallhardwareports`. Entries without a `Device:` line are
/// skipped; the trailing VLAN section is ignored.
pub fn parse_hardware_ports(text: &str) -> Vec<HardwarePort> {
    let mut ports = Vec::new();
    let mut current: Option<HardwarePort> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(port) = line.strip_prefix("Hardware Port:") {
            ports.extend(current.take().filter(|p| !p.device.is_empty()));
            current = Some(HardwarePort {
                port: port.trim().to_string(),
                device: String::new(),
                mac: None,
            });
        } else if let Some(device) = line.strip_prefix("Device:") {
            if let Some(port) = current.as_mut() {
                port.device = device.trim().to_string();
            }
        } else if let Some(mac) = line.strip_prefix("Ethernet Address:") {
            let mac = mac.trim();
            if let Some(port) = current.as_mut() {
                port.mac = (!mac.is_empty() && mac != "N/A").then(|| mac.to_string());
            }
        } else if line.starts_with("VLAN Configurations") {
            break;
        }
    }
    ports.extend(current.filter(|p| !p.device.is_empty()));
    ports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_classify_across_platforms() {
        for (name, kind) in [
            ("lo", NetKind::Loopback),
            ("lo0", NetKind::Loopback),
            ("Loopback Pseudo-Interface 1", NetKind::Loopback),
            ("wlp2s0", NetKind::WiFi),
            ("wlan0", NetKind::WiFi),
            ("Wi-Fi", NetKind::WiFi),
            ("eth0", NetKind::Ethernet),
            ("enp3s0", NetKind::Ethernet),
            ("en0", NetKind::Ethernet),
            ("Ethernet 2", NetKind::Ethernet),
            ("utun3", NetKind::Virtual),
            ("awdl0", NetKind::Virtual),
            ("bridge0", NetKind::Virtual),
            ("docker0", NetKind::Virtual),
            ("vEthernet (WSL)", NetKind::Virtual),
            ("veth1a2b", NetKind::Virtual),
            ("long", NetKind::Other),
            ("Bluetooth Network Connection", NetKind::Other),
        ] {
            assert_eq!(net_kind_from_name(name), kind, "{name}");
        }
    }

    const PORTS: &str = "
Hardware Port: Ethernet Adapter (en4)
Device: en4
Ethernet Address: 7e:11:22:33:44:55

Hardware Port: Wi-Fi
Device: en0
Ethernet Address: a0:b1:c2:d3:e4:f5

Hardware Port: Thunderbolt Bridge
Device: bridge0
Ethernet Address: N/A

Hardware Port: Orphan

VLAN Configurations
===================
Hardware Port: Fake
Device: vlan0
";

    #[test]
    fn hardware_ports_map_devices_to_ports() {
        let ports = parse_hardware_ports(PORTS);
        assert_eq!(ports.len(), 3);
        assert_eq!(ports[0].device, "en4");
        assert_eq!(ports[0].kind(), NetKind::Ethernet);
        assert_eq!(ports[1].port, "Wi-Fi");
        assert_eq!(ports[1].mac.as_deref(), Some("a0:b1:c2:d3:e4:f5"));
        assert_eq!(ports[1].kind(), NetKind::WiFi);
        assert_eq!(ports[2].mac, None);
        assert_eq!(ports[2].kind(), NetKind::Virtual);
        assert!(parse_hardware_ports("").is_empty());
    }
}
