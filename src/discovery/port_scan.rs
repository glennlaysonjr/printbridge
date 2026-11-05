//! Fallback discovery for printers that don't advertise over mDNS: sweeps the local
//! IPv4 /24 subnet(s) for the three classic printing ports. Connect-only probes —
//! nothing is written to the socket, so it never disturbs a device.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::models::{Printer, PrinterSource};

/// Probed in this order; the first open port identifies the device.
const PRINTER_PORTS: [(u16, &str); 3] = [
    (9100, "raw"), // JetDirect / RAW
    (631, "ipp"),  // IPP
    (515, "lpd"),  // LPD
];

/// Caps concurrent connects so a scan doesn't flood small home routers.
const MAX_CONCURRENT_PROBES: usize = 64;

pub async fn discover(probe_timeout: Duration) -> Vec<Printer> {
    let subnets = local_subnets();
    if subnets.is_empty() {
        return Vec::new();
    }

    let gate = Arc::new(Semaphore::new(MAX_CONCURRENT_PROBES));
    let mut probes = JoinSet::new();
    for (network, own_ip) in subnets {
        for host in 1..=254u8 {
            let ip = Ipv4Addr::new(network[0], network[1], network[2], host);
            if ip == own_ip {
                continue;
            }
            let gate = gate.clone();
            probes.spawn(async move {
                let _permit = gate.acquire_owned().await.ok()?;
                probe_host(ip, probe_timeout).await
            });
        }
    }

    let mut found = Vec::new();
    while let Some(result) = probes.join_next().await {
        if let Ok(Some(printer)) = result {
            found.push(printer);
        }
    }
    found
}

async fn probe_host(ip: Ipv4Addr, timeout: Duration) -> Option<Printer> {
    for (port, protocol) in PRINTER_PORTS {
        let addr = SocketAddr::from((ip, port));
        let Ok(Ok(_stream)) = tokio::time::timeout(timeout, TcpStream::connect(addr)).await else {
            continue; // closed port / unreachable host — expected for most of the sweep
        };

        let uri = match protocol {
            "raw" => format!("socket://{ip}:{port}"),
            "ipp" => format!("ipp://{ip}:{port}/ipp/print"),
            _ => format!("lpd://{ip}:{port}"),
        };
        let mut printer = Printer::new(
            format!("scan:{ip}:{port}"),
            format!("{ip} ({protocol})"),
            PrinterSource::PortScan,
        );
        printer.host = Some(ip.to_string());
        printer.port = Some(port);
        printer.protocols = vec![protocol.to_string()];
        printer.uri = Some(uri);
        printer.raw_port = (protocol == "raw").then_some(port);
        return Some(printer);
    }
    None
}

/// Returns (first three octets, own address) for every /24-or-smaller IPv4 network
/// this machine sits on. Wider networks are skipped: sweeping a /16 (65k hosts) is
/// not something a discovery agent should do implicitly.
fn local_subnets() -> Vec<([u8; 3], Ipv4Addr)> {
    let interfaces = match if_addrs::get_if_addrs() {
        Ok(i) => i,
        Err(err) => {
            tracing::warn!("could not enumerate network interfaces: {err}");
            return Vec::new();
        }
    };

    let mut subnets: Vec<([u8; 3], Ipv4Addr)> = Vec::new();
    for iface in interfaces {
        if iface.is_loopback() || iface.is_link_local() || iface.is_p2p() {
            continue;
        }
        let if_addrs::IfAddr::V4(v4) = &iface.addr else {
            continue;
        };
        if v4.prefixlen < 24 {
            continue;
        }
        let o = v4.ip.octets();
        let network = [o[0], o[1], o[2]];
        if !subnets.iter().any(|(n, _)| *n == network) {
            subnets.push((network, v4.ip));
        }
    }
    subnets
}
