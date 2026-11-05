use clap::Parser;

/// Local print agent: exposes every printer reachable from this machine to web
/// pages through a loopback-only HTTP + WebSocket API.
///
/// Every flag can also be set through the environment variable shown next to it,
/// which is the convenient route under launchd, systemd, or a Windows scheduled task.
#[derive(Debug, Clone, Parser)]
#[command(name = "printbridge", version, about, long_about = None)]
pub struct Config {
    /// TCP port to listen on (always bound to 127.0.0.1).
    #[arg(long, env = "PRINTBRIDGE_PORT", default_value_t = 9631)]
    pub port: u16,

    /// Web origins allowed to call the agent, comma-separated. "*" allows any site,
    /// which is fine for development but lets ANY website the user visits print.
    /// Production: e.g. "https://app.example.com,https://admin.example.com".
    #[arg(
        long,
        env = "PRINTBRIDGE_ALLOWED_ORIGINS",
        value_delimiter = ',',
        default_value = "*"
    )]
    pub allowed_origins: Vec<String>,

    /// How long each discovery pass listens for mDNS/Bonjour answers, in seconds.
    #[arg(long, env = "PRINTBRIDGE_MDNS_SCAN_SECONDS", default_value_t = 4)]
    pub mdns_scan_seconds: u64,

    /// Per-host TCP connect timeout for the subnet port sweep, in milliseconds.
    #[arg(long, env = "PRINTBRIDGE_PORT_PROBE_TIMEOUT_MS", default_value_t = 350)]
    pub port_probe_timeout_ms: u64,

    /// Disable the /24 port sweep and rely on mDNS + local queues only.
    #[arg(long, env = "PRINTBRIDGE_DISABLE_PORT_SCAN")]
    pub disable_port_scan: bool,

    /// GET /api/printers rescans when the cached list is older than this, in seconds.
    #[arg(long, env = "PRINTBRIDGE_CACHE_SECONDS", default_value_t = 60)]
    pub cache_seconds: u64,
}

impl Config {
    pub fn allows_any_origin(&self) -> bool {
        self.allowed_origins.iter().any(|o| o.trim() == "*")
    }

    pub fn origin_allowed(&self, origin: &str) -> bool {
        self.allows_any_origin()
            || self
                .allowed_origins
                .iter()
                .any(|o| o.trim().trim_end_matches('/').eq_ignore_ascii_case(origin))
    }
}
