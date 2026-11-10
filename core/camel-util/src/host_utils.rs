//! Host name and network address helpers. Port of `org.apache.camel.util.HostUtils` and
//! `org.apache.camel.util.InetAddressUtil`.

use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, ToSocketAddrs};

/// Returns the IPv4 addresses per network interface (sorted by interface name), only for
/// interfaces that are up and not loopback.
pub fn get_network_interface_addresses() -> BTreeMap<String, Vec<IpAddr>> {
    let mut map: BTreeMap<String, Vec<IpAddr>> = BTreeMap::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            let ip = iface.ip();
            // we only care about usable non-loopback IPv4 interfaces
            if iface.is_loopback() || !ip.is_ipv4() {
                continue;
            }
            let list = map.entry(iface.name.clone()).or_default();
            if !list.contains(&ip) {
                list.push(ip);
            }
        }
    }
    map
}

/// Returns the non-loopback IPv4 addresses of all interfaces.
pub fn get_addresses() -> Vec<IpAddr> {
    let mut all = Vec::new();
    for addresses in get_network_interface_addresses().into_values() {
        for a in addresses {
            if !all.contains(&a) {
                all.push(a);
            }
        }
    }
    all
}

/// Returns the host name of the local machine (Java `InetAddressUtil.getLocalHostName`).
///
/// # Errors
///
/// Returns an error if the host name cannot be determined.
pub fn get_local_host_name() -> io::Result<String> {
    let name = gethostname::gethostname().to_string_lossy().into_owned();
    if name.is_empty() {
        Err(io::Error::other("unknown host name"))
    } else {
        Ok(name)
    }
}

/// Returns the host name of the local machine, or `None` if not possible to resolve (Java
/// `InetAddressUtil.getLocalHostNameSafe`).
pub fn get_local_host_name_safe() -> Option<String> {
    get_local_host_name().ok()
}

/// Returns the local IP: the address the local host name resolves to if that is bound to a
/// non-loopback interface, else the first non-loopback address, else the loopback address.
pub fn get_local_ip() -> String {
    let addresses = get_addresses();
    if let Ok(host) = get_local_host_name() {
        if let Ok(resolved) = (host.as_str(), 0).to_socket_addrs() {
            for addr in resolved {
                if addresses.contains(&addr.ip()) {
                    return addr.ip().to_string();
                }
            }
        }
    }
    addresses
        .first()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_name() {
        assert!(get_local_host_name_safe().is_some());
        assert!(!get_local_ip().is_empty());
        for (name, addrs) in get_network_interface_addresses() {
            assert!(!name.is_empty());
            assert!(addrs.iter().all(|a| a.is_ipv4() && !a.is_loopback()));
        }
        assert!(get_addresses().iter().all(|a| !a.is_loopback()));
    }
}
