use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How a printer was found. Variant order is the sort order of the printer list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PrinterSource {
    /// Installed on this machine (CUPS / Windows spooler).
    Local,
    /// Discovered on the LAN via mDNS/Bonjour.
    Mdns,
    /// Discovered on the LAN via a TCP port scan (9100/631/515).
    PortScan,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Printer {
    /// Stable identifier the SDK uses to address this printer.
    pub id: String,
    pub name: String,
    pub source: PrinterSource,
    /// IP address or hostname for network printers; None for purely local queues.
    pub host: Option<String>,
    pub port: Option<u16>,
    /// Protocols usable for job submission: "raw", "ipp", "ipps", "lpd", "spooler".
    pub protocols: Vec<String>,
    /// Best URI for submitting a job (e.g. ipp://10.0.0.5:631/ipp/print, socket://10.0.0.5:9100).
    pub uri: Option<String>,
    /// Make and model when the printer advertises it (mDNS TXT "ty"/"product").
    pub make_and_model: Option<String>,
    /// Location string when advertised (mDNS TXT "note").
    pub location: Option<String>,
    /// True for the OS default printer (local source only).
    pub is_default: bool,
    /// Free-form state, e.g. "idle", "printing", "disabled".
    pub state: Option<String>,
    pub discovered_at: DateTime<Utc>,
    /// RAW/JetDirect port when known. Kept apart from `port` because a device that
    /// advertises several protocols is merged into one entry whose `port` is IPP's.
    #[serde(skip)]
    pub raw_port: Option<u16>,
}

impl Printer {
    /// A printer with only the required fields set.
    pub fn new(id: String, name: String, source: PrinterSource) -> Self {
        Self {
            id,
            name,
            source,
            host: None,
            port: None,
            protocols: Vec::new(),
            uri: None,
            make_and_model: None,
            location: None,
            is_default: false,
            state: None,
            discovered_at: Utc::now(),
            raw_port: None,
        }
    }

    pub fn supports(&self, protocol: &str) -> bool {
        self.protocols.iter().any(|p| p == protocol)
    }
}

/// A print job submitted by the browser SDK.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintRequest {
    /// Id of a printer previously returned by /api/printers.
    pub printer_id: String,
    /// Base64-encoded document bytes. Use this OR `text`.
    pub data_base64: Option<String>,
    /// Plain text payload (e.g. ZPL/EPL/ESC-POS commands or plain text pages).
    pub text: Option<String>,
    /// MIME type of the document: application/pdf, application/octet-stream, text/plain...
    #[serde(default = "default_content_type")]
    pub content_type: String,
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default = "default_copies")]
    pub copies: u32,
}

fn default_content_type() -> String {
    "application/octet-stream".into()
}

fn default_title() -> String {
    "PrintBridge job".into()
}

fn default_copies() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintResult {
    pub ok: bool,
    pub printer_id: String,
    pub via: String,
    pub message: Option<String>,
    /// True when the request itself was invalid (HTTP 400) rather than the
    /// printer or transport failing (HTTP 502).
    #[serde(skip)]
    pub rejected: bool,
}

impl PrintResult {
    pub fn ok(printer_id: &str, via: &str, message: impl Into<String>) -> Self {
        Self {
            ok: true,
            printer_id: printer_id.into(),
            via: via.into(),
            message: Some(message.into()),
            rejected: false,
        }
    }

    /// The printer or transport failed.
    pub fn fail(printer_id: &str, via: &str, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            printer_id: printer_id.into(),
            via: via.into(),
            message: Some(message.into()),
            rejected: false,
        }
    }

    /// The request was invalid; nothing was sent to a printer.
    pub fn rejected(printer_id: &str, message: impl Into<String>) -> Self {
        Self {
            rejected: true,
            ..Self::fail(printer_id, "none", message)
        }
    }
}
