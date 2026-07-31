//! mDNS browse for AirPlay receivers (`_airplay._tcp`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use airplay_lib::{AirPlayError, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use tracing::{debug, info, warn};

/// Service type queried by the discovery client (with local domain).
pub const AIRPLAY_SERVICE_TYPE: &str = "_airplay._tcp.local.";

/// Discovered AirPlay receiver advertisement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AirPlayService {
    /// Instance name (e.g. display name of the receiver).
    pub name: String,
    /// Preferred host address (IPv4 preferred) as a string.
    pub host: String,
    /// Control / RTSP port.
    pub port: u16,
    /// Decoded TXT key/value pairs.
    pub txt: HashMap<String, String>,
}

/// Browse for `_airplay._tcp` services until `timeout` elapses.
///
/// Returns whatever was resolved (may be empty when offline or when mDNS is
/// blocked). Soft-fails to an empty list when the daemon cannot start.
pub async fn browse_airplay(timeout: Duration) -> Result<Vec<AirPlayService>> {
    tokio::task::spawn_blocking(move || browse_airplay_blocking(timeout))
        .await
        .map_err(|e| AirPlayError::Bonjour(format!("browse task join: {e}")))?
}

/// Blocking mDNS browse used by [`browse_airplay`].
pub fn browse_airplay_blocking(timeout: Duration) -> Result<Vec<AirPlayService>> {
    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            warn!(
                "browse_airplay: failed to create mDNS daemon ({e}); \
                 returning empty (port 5353 may need privileges / firewall)"
            );
            return Ok(Vec::new());
        }
    };

    let receiver = match daemon.browse(AIRPLAY_SERVICE_TYPE) {
        Ok(r) => r,
        Err(e) => {
            warn!("browse_airplay: browse failed: {e}");
            let _ = daemon.shutdown();
            return Ok(Vec::new());
        }
    };

    let deadline = Instant::now() + timeout;
    // Keyed by fullname so re-resolves overwrite rather than duplicate.
    let mut found: HashMap<String, AirPlayService> = HashMap::new();

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                if let Some(svc) = resolved_to_service(&info) {
                    debug!(
                        name = %svc.name,
                        host = %svc.host,
                        port = svc.port,
                        "browse_airplay: resolved"
                    );
                    found.insert(info.fullname.clone(), svc);
                }
            }
            Ok(ServiceEvent::SearchStopped(_)) => break,
            Ok(other) => {
                debug!(?other, "browse_airplay: event");
            }
            Err(_) => break, // timeout or channel closed
        }
    }

    // Best-effort stop browse + shutdown.
    let _ = daemon.stop_browse(AIRPLAY_SERVICE_TYPE);
    let _ = daemon.shutdown();

    let mut services: Vec<AirPlayService> = found.into_values().collect();
    services.sort_by(|a, b| a.name.cmp(&b.name).then(a.host.cmp(&b.host)));
    info!(count = services.len(), "browse_airplay: done");
    Ok(services)
}

fn resolved_to_service(info: &mdns_sd::ResolvedService) -> Option<AirPlayService> {
    if !info.is_valid() {
        return None;
    }

    let host = prefer_host_addr(info)?;
    let name = instance_name(&info.fullname, &info.ty_domain);

    let mut txt = HashMap::new();
    for prop in info.get_properties().iter() {
        txt.insert(prop.key().to_string(), prop.val_str().to_string());
    }

    Some(AirPlayService {
        name,
        host,
        port: info.port,
        txt,
    })
}

/// Prefer first IPv4, then any IP, else hostname string.
fn prefer_host_addr(info: &mdns_sd::ResolvedService) -> Option<String> {
    let v4 = info.get_addresses_v4();
    if let Some(ip) = v4.iter().next() {
        return Some(ip.to_string());
    }
    if let Some(addr) = info.addresses.iter().next() {
        return Some(addr.to_ip_addr().to_string());
    }
    if !info.host.is_empty() {
        return Some(info.host.trim_end_matches('.').to_string());
    }
    None
}

/// Extract instance name from fullname / ty_domain.
fn instance_name(fullname: &str, ty_domain: &str) -> String {
    // fullname is typically "{instance}.{ty_domain}"
    if let Some(stripped) = fullname.strip_suffix(ty_domain) {
        return stripped.trim_end_matches('.').to_string();
    }
    let with_dot = format!(".{ty_domain}");
    if let Some(stripped) = fullname.strip_suffix(&with_dot) {
        return stripped.to_string();
    }
    fullname.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_name_strips_type() {
        assert_eq!(
            instance_name("MyTV._airplay._tcp.local.", "_airplay._tcp.local."),
            "MyTV"
        );
        assert_eq!(
            instance_name("Living Room._airplay._tcp.local.", "_airplay._tcp.local."),
            "Living Room"
        );
    }

    /// Smoke: short browse must return Ok (possibly empty) without panicking.
    #[tokio::test]
    #[ignore = "mDNS browse needs network / UDP 5353; run manually"]
    async fn browse_smoke() {
        let services = browse_airplay(Duration::from_secs(2))
            .await
            .expect("browse should soft-succeed");
        // Offline CI: empty is fine.
        eprintln!("found {} service(s)", services.len());
        for s in &services {
            eprintln!("  {} @ {}:{}", s.name, s.host, s.port);
        }
    }
}
