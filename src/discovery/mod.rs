//! Printer discovery sources. Each returns an empty list on failure rather than an
//! error: one broken source (no CUPS, multicast blocked...) must not hide the others.

pub mod local;
pub mod mdns;
pub mod port_scan;
