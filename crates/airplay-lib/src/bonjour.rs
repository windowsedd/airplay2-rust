//! Bonjour / mDNS advertisement for AirPlay (`_airplay._tcp`) and RAOP (`_raop._tcp`).
//!
//! TXT records match the Java `AirPlayBonjour` implementation.
//!
//! # Platform notes
//!
//! Registration uses the [`mdns-sd`](https://crates.io/crates/mdns-sd) crate (works on Windows,
//! macOS, and Linux). Binding UDP port 5353 may require elevated privileges or firewall
//! allowances on some systems; `start` soft-fails (logs + returns `Ok`) when the daemon or
//! interface enumeration cannot advertise, so callers are never panicked for missing NICs.

use std::collections::HashMap;
use std::net::IpAddr;

use mdns_sd::{ServiceDaemon, ServiceInfo};
use rand_core::{OsRng, RngCore};
use tracing::{info, warn};

use crate::error::{AirPlayError, Result};

/// Fixed AirPlay device public key advertised in TXT (matches Java reference).
pub const AIRPLAY_PK: &str =
    "f3769a660475d27b4f6040381d784645e13e21c53e6d2da6a8c3d757086fc336";

/// Features bitfield advertised for both AirPlay and RAOP.
pub const AIRPLAY_FEATURES: &str = "0x5A7FFFF7,0x1E";

/// Source / server version string.
pub const AIRPLAY_SRCVERS: &str = "220.68";

const SERVICE_AIRPLAY: &str = "_airplay._tcp.local.";
const SERVICE_RAOP: &str = "_raop._tcp.local.";

/// Pure helper: format MAC bytes as uppercase colon-separated hex (`AA:BB:CC:DD:EE:FF`).
pub fn format_mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// RAOP instance name: `{macWithoutColons}@{serverName}`.
pub fn raop_service_name(mac: &str, server_name: &str) -> String {
    let mac_plain: String = mac.chars().filter(|c| *c != ':').collect();
    format!("{mac_plain}@{server_name}")
}

/// AirPlay (`_airplay._tcp`) TXT record map (exact Java keys/values).
pub fn airplay_txt_records(device_id: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("deviceid".into(), device_id.to_string());
    m.insert("features".into(), AIRPLAY_FEATURES.into());
    m.insert("srcvers".into(), AIRPLAY_SRCVERS.into());
    m.insert("flags".into(), "0x44".into());
    m.insert("vv".into(), "2".into());
    m.insert("model".into(), "AppleTV3,2C".into());
    m.insert("rhd".into(), "5.6.0.0".into());
    m.insert("pw".into(), "false".into());
    m.insert("pk".into(), AIRPLAY_PK.into());
    m.insert("rmodel".into(), "PC1.0".into());
    m.insert("rrv".into(), "1.01".into());
    m.insert("rsv".into(), "1.00".into());
    m.insert("pcversion".into(), "1715".into());
    m
}

/// RAOP (`_raop._tcp`) TXT record map (exact Java keys/values).
pub fn raop_txt_records() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("ch".into(), "2".into());
    m.insert("cn".into(), "1,3".into());
    m.insert("da".into(), "true".into());
    m.insert("et".into(), "0,3,5".into());
    m.insert("ek".into(), "1".into());
    m.insert("ft".into(), AIRPLAY_FEATURES.into());
    m.insert("am".into(), "AppleTV3,2C".into());
    m.insert("md".into(), "0,1,2".into());
    m.insert("sr".into(), "44100".into());
    m.insert("ss".into(), "16".into());
    m.insert("sv".into(), "false".into());
    m.insert("sm".into(), "false".into());
    m.insert("tp".into(), "UDP".into());
    m.insert("txtvers".into(), "1".into());
    m.insert("sf".into(), "0x44".into());
    m.insert("vs".into(), AIRPLAY_SRCVERS.into());
    m.insert("vn".into(), "65537".into());
    m.insert("pk".into(), AIRPLAY_PK.into());
    m
}

/// mDNS advertisement handle for AirPlay + RAOP services.
pub struct AirPlayBonjour {
    server_name: String,
    /// Device MAC / `deviceid` (`XX:XX:XX:XX:XX:XX`).
    device_id: String,
    daemon: Option<ServiceDaemon>,
    /// Full service names for unregister (e.g. `Name._airplay._tcp.local.`).
    registered: Vec<String>,
}

impl AirPlayBonjour {
    /// Create a new advertiser. Generates a random local device MAC for TXT `deviceid`.
    pub fn new(server_name: impl Into<String>) -> Self {
        let mut mac = [0u8; 6];
        OsRng.fill_bytes(&mut mac);
        // Locally administered unicast MAC (bit1 of first octet set, bit0 clear).
        mac[0] = (mac[0] | 0x02) & 0xFE;
        Self {
            server_name: server_name.into(),
            device_id: format_mac(&mac),
            daemon: None,
            registered: Vec::new(),
        }
    }

    /// Server display name used as AirPlay instance name.
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Device id advertised as AirPlay TXT `deviceid`.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Register `_airplay._tcp` and `_raop._tcp` on `air_tunes_port`.
    ///
    /// Soft-fails (logs + `Ok(())`) when interfaces are missing or the mDNS daemon cannot
    /// start/register — does not panic.
    pub fn start(&mut self, air_tunes_port: u16) -> Result<()> {
        if self.daemon.is_some() {
            return Err(AirPlayError::InvalidState(
                "AirPlayBonjour already started".into(),
            ));
        }

        let addrs = local_ip_addrs();
        if addrs.is_empty() {
            warn!(
                "AirPlayBonjour: no non-loopback interfaces found; skipping mDNS advertise \
                 (full mDNS may need privileges / firewall on this platform)"
            );
            return Ok(());
        }

        let daemon = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                warn!(
                    "AirPlayBonjour: failed to create mDNS daemon ({e}); \
                     advertisement disabled (port 5353 may need privileges)"
                );
                return Ok(());
            }
        };

        let host_name = dns_host_name(&self.server_name);
        let airplay_props = airplay_txt_records(&self.device_id);
        let raop_props = raop_txt_records();
        let raop_name = raop_service_name(&self.device_id, &self.server_name);

        let airplay_info = match ServiceInfo::new(
            SERVICE_AIRPLAY,
            &self.server_name,
            &host_name,
            &addrs[..],
            air_tunes_port,
            airplay_props,
        ) {
            Ok(info) => info.enable_addr_auto(),
            Err(e) => {
                warn!("AirPlayBonjour: build _airplay ServiceInfo failed: {e}");
                let _ = daemon.shutdown();
                return Ok(());
            }
        };

        let raop_info = match ServiceInfo::new(
            SERVICE_RAOP,
            &raop_name,
            &host_name,
            &addrs[..],
            air_tunes_port,
            raop_props,
        ) {
            Ok(info) => info.enable_addr_auto(),
            Err(e) => {
                warn!("AirPlayBonjour: build _raop ServiceInfo failed: {e}");
                let _ = daemon.shutdown();
                return Ok(());
            }
        };

        let airplay_fullname = airplay_info.get_fullname().to_string();
        let raop_fullname = raop_info.get_fullname().to_string();

        if let Err(e) = daemon.register(airplay_info) {
            warn!("AirPlayBonjour: register _airplay failed: {e}");
            let _ = daemon.shutdown();
            return Ok(());
        }
        if let Err(e) = daemon.register(raop_info) {
            warn!("AirPlayBonjour: register _raop failed: {e}");
            let _ = daemon.unregister(&airplay_fullname);
            let _ = daemon.shutdown();
            return Ok(());
        }

        info!(
            server = %self.server_name,
            device_id = %self.device_id,
            port = air_tunes_port,
            "AirPlayBonjour: advertising _airplay._tcp and _raop._tcp"
        );

        self.registered = vec![airplay_fullname, raop_fullname];
        self.daemon = Some(daemon);
        Ok(())
    }

    /// Unregister services and shut down the mDNS daemon (idempotent).
    pub fn stop(&mut self) {
        if let Some(daemon) = self.daemon.take() {
            for fullname in self.registered.drain(..) {
                if let Err(e) = daemon.unregister(&fullname) {
                    warn!("AirPlayBonjour: unregister {fullname} failed: {e}");
                }
            }
            if let Err(e) = daemon.shutdown() {
                warn!("AirPlayBonjour: daemon shutdown failed: {e}");
            } else {
                info!("AirPlayBonjour: stopped");
            }
        } else {
            self.registered.clear();
        }
    }
}

impl Drop for AirPlayBonjour {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Collect non-loopback IPv4/IPv6 addresses for A/AAAA records.
fn local_ip_addrs() -> Vec<IpAddr> {
    match if_addrs::get_if_addrs() {
        Ok(ifaces) => ifaces
            .into_iter()
            .filter(|ifa| !ifa.is_loopback())
            .map(|ifa| ifa.ip())
            .collect(),
        Err(e) => {
            warn!("AirPlayBonjour: if-addrs enumeration failed: {e}");
            Vec::new()
        }
    }
}

/// Build a DNS-safe hostname label ending with `.local.`.
fn dns_host_name(server_name: &str) -> String {
    let label: String = server_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let label = if label.is_empty() {
        "airplay".to_string()
    } else {
        label
    };
    format!("{label}.local.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_mac_uppercase_with_colons() {
        assert_eq!(
            format_mac(&[0x48, 0x5d, 0x60, 0x7c, 0xee, 0x22]),
            "48:5D:60:7C:EE:22"
        );
        assert_eq!(format_mac(&[0x00, 0x0a, 0xff]), "00:0A:FF");
        assert_eq!(format_mac(&[]), "");
    }

    #[test]
    fn raop_service_name_strips_colons() {
        assert_eq!(
            raop_service_name("48:5D:60:7C:EE:22", "MyTV"),
            "485D607CEE22@MyTV"
        );
    }

    #[test]
    fn airplay_txt_exact_java_props() {
        let device_id = "48:5D:60:7C:EE:22";
        let m = airplay_txt_records(device_id);
        assert_eq!(m.get("deviceid").map(String::as_str), Some(device_id));
        assert_eq!(m.get("features").map(String::as_str), Some("0x5A7FFFF7,0x1E"));
        assert_eq!(m.get("srcvers").map(String::as_str), Some("220.68"));
        assert_eq!(m.get("flags").map(String::as_str), Some("0x44"));
        assert_eq!(m.get("vv").map(String::as_str), Some("2"));
        assert_eq!(m.get("model").map(String::as_str), Some("AppleTV3,2C"));
        assert_eq!(m.get("rhd").map(String::as_str), Some("5.6.0.0"));
        assert_eq!(m.get("pw").map(String::as_str), Some("false"));
        assert_eq!(m.get("pk").map(String::as_str), Some(AIRPLAY_PK));
        assert_eq!(m.get("rmodel").map(String::as_str), Some("PC1.0"));
        assert_eq!(m.get("rrv").map(String::as_str), Some("1.01"));
        assert_eq!(m.get("rsv").map(String::as_str), Some("1.00"));
        assert_eq!(m.get("pcversion").map(String::as_str), Some("1715"));
        assert_eq!(m.len(), 13);
    }

    #[test]
    fn raop_txt_exact_java_props() {
        let m = raop_txt_records();
        assert_eq!(m.get("ch").map(String::as_str), Some("2"));
        assert_eq!(m.get("cn").map(String::as_str), Some("1,3"));
        assert_eq!(m.get("da").map(String::as_str), Some("true"));
        assert_eq!(m.get("et").map(String::as_str), Some("0,3,5"));
        assert_eq!(m.get("ek").map(String::as_str), Some("1"));
        assert_eq!(m.get("ft").map(String::as_str), Some("0x5A7FFFF7,0x1E"));
        assert_eq!(m.get("am").map(String::as_str), Some("AppleTV3,2C"));
        assert_eq!(m.get("md").map(String::as_str), Some("0,1,2"));
        assert_eq!(m.get("sr").map(String::as_str), Some("44100"));
        assert_eq!(m.get("ss").map(String::as_str), Some("16"));
        assert_eq!(m.get("sv").map(String::as_str), Some("false"));
        assert_eq!(m.get("sm").map(String::as_str), Some("false"));
        assert_eq!(m.get("tp").map(String::as_str), Some("UDP"));
        assert_eq!(m.get("txtvers").map(String::as_str), Some("1"));
        assert_eq!(m.get("sf").map(String::as_str), Some("0x44"));
        assert_eq!(m.get("vs").map(String::as_str), Some("220.68"));
        assert_eq!(m.get("vn").map(String::as_str), Some("65537"));
        assert_eq!(m.get("pk").map(String::as_str), Some(AIRPLAY_PK));
        assert_eq!(m.len(), 18);
    }

    #[test]
    fn dns_host_name_sanitizes() {
        assert_eq!(dns_host_name("My TV"), "My-TV.local.");
        assert_eq!(dns_host_name("ok"), "ok.local.");
    }

    #[test]
    fn new_generates_device_id() {
        let b = AirPlayBonjour::new("TestServer");
        assert_eq!(b.server_name(), "TestServer");
        // XX:XX:XX:XX:XX:XX
        assert_eq!(b.device_id().matches(':').count(), 5);
        assert_eq!(b.device_id().len(), 17);
    }

    /// Best-effort smoke: start/stop must not panic (may soft-skip on CI without mDNS).
    #[test]
    #[ignore = "binds mDNS (UDP 5353); run manually when network advertise is allowed"]
    fn start_stop_smoke() {
        let mut b = AirPlayBonjour::new("RustAirPlaySmoke");
        b.start(7000).expect("start should soft-succeed");
        b.stop();
        // Second stop is idempotent.
        b.stop();
    }
}
