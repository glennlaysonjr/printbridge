//! Discovers network printers advertising over mDNS/Bonjour — the mechanism AirPrint
//! and virtually every modern network printer uses to announce itself on the LAN.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent};

use crate::models::{Printer, PrinterSource};

/// Service types printers advertise, with the protocol each one maps to.
const SERVICE_TYPES: [(&str, &str); 4] = [
    ("_ipp._tcp.local.", "ipp"),            // IPP (AirPrint et al.)
    ("_ipps._tcp.local.", "ipps"),          // IPP over TLS
    ("_pdl-datastream._tcp.local.", "raw"), // RAW / JetDirect port 9100
    ("_printer._tcp.local.", "lpd"),        // LPD
];

pub async fn discover(scan_time: Duration) -> Vec<Printer> {
    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(err) => {
            tracing::warn!("mDNS daemon failed to start: {err}");
            return Vec::new();
        }
    };

    let mut browsers = Vec::new();
    for (service_type, protocol) in SERVICE_TYPES {
        match daemon.browse(service_type) {
            Ok(rx) => browsers.push(tokio::spawn(async move {
                let mut found = Vec::new();
                let deadline = tokio::time::Instant::now() + scan_time;
                while let Ok(Ok(event)) = tokio::time::timeout_at(deadline, rx.recv_async()).await {
                    if let ServiceEvent::ServiceResolved(service) = event
                        && let Some(printer) = to_printer(&service, protocol)
                    {
                        found.push(printer);
                    }
                }
                found
            })),
            Err(err) => tracing::warn!("mDNS browse for {service_type} failed: {err}"),
        }
    }

    // The same service is often resolved once per network interface; keep one.
    let mut printers: HashMap<String, Printer> = HashMap::new();
    for browser in browsers {
        for printer in browser.await.unwrap_or_default() {
            printers.entry(printer.id.clone()).or_insert(printer);
        }
    }

    if let Ok(status) = daemon.shutdown() {
        let _ = status.recv_async().await;
    }
    printers.into_values().collect()
}

fn to_printer(service: &ResolvedService, protocol: &str) -> Option<Printer> {
    let ip = pick_address(service)?;
    let port = service.get_port();
    let txt = |key: &str| {
        service
            .get_property_val_str(key)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };

    // IPv6 literals need brackets inside URIs.
    let host_in_uri = match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };
    let resource_path = txt("rp").unwrap_or_else(|| "ipp/print".into());
    let resource_path = resource_path.trim_start_matches('/');
    let uri = match protocol {
        "ipp" => format!("ipp://{host_in_uri}:{port}/{resource_path}"),
        "ipps" => format!("ipps://{host_in_uri}:{port}/{resource_path}"),
        "raw" => format!("socket://{host_in_uri}:{port}"),
        _ => format!("lpd://{host_in_uri}:{port}"),
    };

    // "Office Printer._ipp._tcp.local." → "Office Printer"
    let name = service
        .get_fullname()
        .strip_suffix(service.ty_domain.as_str())
        .unwrap_or(service.get_fullname())
        .trim_end_matches('.')
        .replace("\\032", " ")
        .replace('\\', "");

    let mut printer = Printer::new(
        format!("mdns:{ip}:{port}:{protocol}"),
        if name.is_empty() {
            ip.to_string()
        } else {
            name
        },
        PrinterSource::Mdns,
    );
    printer.host = Some(ip.to_string());
    printer.port = Some(port);
    printer.protocols = vec![protocol.to_string()];
    printer.uri = Some(uri);
    printer.raw_port = (protocol == "raw").then_some(port);
    printer.make_and_model = txt("ty")
        .or_else(|| txt("product").map(|p| p.trim_matches(|c| c == '(' || c == ')').to_string()));
    printer.location = txt("note");
    printer.state = txt("printer-state");
    Some(printer)
}

/// Prefers IPv4; falls back to a routable IPv6 address (link-local ones need a
/// scope id that doesn't survive being turned into a URI).
fn pick_address(service: &ResolvedService) -> Option<IpAddr> {
    let addrs: Vec<IpAddr> = service
        .get_addresses()
        .iter()
        .map(|a| a.to_ip_addr())
        .collect();
    let mut v4: Vec<IpAddr> = addrs.iter().copied().filter(IpAddr::is_ipv4).collect();
    v4.sort();
    v4.into_iter().next().or_else(|| {
        addrs.into_iter().find(|a| match a {
            IpAddr::V6(v6) => !v6.is_unicast_link_local() && !v6.is_loopback(),
            IpAddr::V4(_) => false,
        })
    })
}
