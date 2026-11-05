//! Aggregates all discovery sources into one deduplicated, cached printer list and
//! notifies subscribers (the WebSocket endpoint) when the list changes.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, broadcast};

use crate::config::Config;
use crate::discovery::{local, mdns, port_scan};
use crate::models::Printer;

/// Preference for the URI a merged network printer is submitted through.
/// Plain IPP first: it works with every IPP printer, while many older devices
/// speak only TLS versions modern clients refuse.
const PROTOCOL_PREFERENCE: [&str; 4] = ["ipp", "ipps", "raw", "lpd"];

pub struct PrinterRegistry {
    config: Arc<Config>,
    printers: RwLock<HashMap<String, Printer>>,
    last_scan: RwLock<Option<Instant>>,
    scan_lock: Mutex<()>,
    changes: broadcast::Sender<Vec<Printer>>,
}

impl PrinterRegistry {
    pub fn new(config: Arc<Config>) -> Self {
        let (changes, _) = broadcast::channel(16);
        Self {
            config,
            printers: RwLock::new(HashMap::new()),
            last_scan: RwLock::new(None),
            scan_lock: Mutex::new(()),
            changes,
        }
    }

    /// Fires with the full list after every discovery pass.
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<Printer>> {
        self.changes.subscribe()
    }

    /// The cached list, sorted by source then name.
    pub fn current(&self) -> Vec<Printer> {
        let mut list: Vec<Printer> = self.printers.read().unwrap().values().cloned().collect();
        list.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        list
    }

    pub fn find(&self, id: &str) -> Option<Printer> {
        self.printers.read().unwrap().get(id).cloned()
    }

    /// Runs a full discovery pass. Concurrent callers share one scan.
    pub async fn refresh(&self) -> Vec<Printer> {
        let Ok(_guard) = self.scan_lock.try_lock() else {
            // A scan is already running — wait for it and return its result.
            let _ = self.scan_lock.lock().await;
            return self.current();
        };

        let cfg = &self.config;
        let scan = async {
            if cfg.disable_port_scan {
                Vec::new()
            } else {
                port_scan::discover(Duration::from_millis(cfg.port_probe_timeout_ms)).await
            }
        };
        let (local, mdns, scanned) = tokio::join!(
            local::discover(),
            mdns::discover(Duration::from_secs(cfg.mdns_scan_seconds)),
            scan,
        );

        let counts = (local.len(), mdns.len(), scanned.len());
        let merged = merge(local, mdns, scanned);
        tracing::info!(
            "discovery pass: {} local, {} mDNS, {} port-scan -> {} unique",
            counts.0,
            counts.1,
            counts.2,
            merged.len()
        );

        *self.printers.write().unwrap() = merged.into_iter().map(|p| (p.id.clone(), p)).collect();
        *self.last_scan.write().unwrap() = Some(Instant::now());

        let list = self.current();
        let _ = self.changes.send(list.clone()); // Err just means nobody is subscribed
        list
    }

    /// Returns the cache, refreshing first if it is stale or `force` is set.
    pub async fn get(&self, force: bool) -> Vec<Printer> {
        let max_age = Duration::from_secs(self.config.cache_seconds);
        let stale = self
            .last_scan
            .read()
            .unwrap()
            .is_none_or(|t| t.elapsed() > max_age);
        if force || stale {
            self.refresh().await
        } else {
            self.current()
        }
    }
}

/// Dedupe policy: one entry per network host, preferring the richest source
/// (mDNS beats port-scan — it knows names, models, and IPP paths). Local queues
/// are always kept; they may spool to a network device but address it differently.
fn merge(local: Vec<Printer>, mdns: Vec<Printer>, scanned: Vec<Printer>) -> Vec<Printer> {
    let rank = |p: &Printer| {
        p.protocols
            .first()
            .and_then(|proto| PROTOCOL_PREFERENCE.iter().position(|x| x == proto))
            .unwrap_or(usize::MAX)
    };

    // Same device advertising several protocols: start from its most preferred
    // service (its id and URI win) and fold the other protocols into it.
    let mut mdns = mdns;
    mdns.sort_by_key(rank);

    let mut by_host: HashMap<String, Printer> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for p in mdns.into_iter().chain(scanned) {
        let Some(host) = p.host.clone() else { continue };
        match by_host.get_mut(&host) {
            Some(existing) if existing.source == p.source => {
                for proto in p.protocols {
                    if !existing.protocols.contains(&proto) {
                        existing.protocols.push(proto);
                    }
                }
                if existing.make_and_model.is_none() {
                    existing.make_and_model = p.make_and_model;
                }
                if existing.location.is_none() {
                    existing.location = p.location;
                }
                if existing.raw_port.is_none() {
                    existing.raw_port = p.raw_port;
                }
            }
            Some(_) => {} // port-scan hit for a host mDNS already described
            None => {
                order.push(host.clone());
                by_host.insert(host, p);
            }
        }
    }

    let mut result = local;
    result.extend(order.into_iter().filter_map(|h| by_host.remove(&h)));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PrinterSource;

    fn net(source: PrinterSource, host: &str, port: u16, proto: &str) -> Printer {
        let mut p = Printer::new(format!("{source:?}:{host}:{port}"), host.into(), source);
        p.host = Some(host.into());
        p.port = Some(port);
        p.protocols = vec![proto.into()];
        p.raw_port = (proto == "raw").then_some(port);
        p
    }

    #[test]
    fn merges_protocols_per_host_and_prefers_mdns() {
        let local = vec![Printer::new(
            "local:A".into(),
            "A".into(),
            PrinterSource::Local,
        )];
        let mdns = vec![
            net(PrinterSource::Mdns, "10.0.0.5", 9100, "raw"),
            net(PrinterSource::Mdns, "10.0.0.5", 631, "ipp"),
        ];
        let scanned = vec![
            net(PrinterSource::PortScan, "10.0.0.5", 9100, "raw"),
            net(PrinterSource::PortScan, "10.0.0.9", 9100, "raw"),
        ];

        let merged = merge(local, mdns, scanned);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].source, PrinterSource::Local);

        let five = merged
            .iter()
            .find(|p| p.host.as_deref() == Some("10.0.0.5"))
            .unwrap();
        assert_eq!(five.source, PrinterSource::Mdns);
        assert_eq!(five.protocols, vec!["ipp", "raw"]); // ipp preferred → its id/uri kept
        assert_eq!(five.port, Some(631));
        assert_eq!(five.raw_port, Some(9100)); // RAW fallback must not reuse IPP's port

        let nine = merged
            .iter()
            .find(|p| p.host.as_deref() == Some("10.0.0.9"))
            .unwrap();
        assert_eq!(nine.source, PrinterSource::PortScan);
    }
}
