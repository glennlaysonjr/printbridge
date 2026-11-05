//! Enumerates printers installed on this machine: CUPS (`lpstat`) on macOS/Linux,
//! WMI `Win32_Printer` through PowerShell on Windows.

use anyhow::Result;
use serde::Deserialize;

use crate::models::{Printer, PrinterSource};
#[cfg(not(windows))]
use crate::sys;

pub async fn discover() -> Vec<Printer> {
    match discover_platform().await {
        Ok(printers) => printers,
        Err(err) => {
            tracing::warn!("local printer enumeration failed: {err:#}");
            Vec::new()
        }
    }
}

#[cfg(not(windows))]
async fn discover_platform() -> Result<Vec<Printer>> {
    // "lpstat -p" lists queues with state, "lpstat -d" reports the default destination.
    // Both exit non-zero when no printers are configured, so stdout is used regardless.
    let mut p = sys::command("lpstat");
    p.arg("-p");
    let mut d = sys::command("lpstat");
    d.arg("-d");
    let (p_out, d_out) = tokio::try_join!(sys::run(p), sys::run(d))?;
    Ok(parse_lpstat(&p_out.stdout, &d_out.stdout))
}

/// Parses `lpstat -p` / `lpstat -d` output (run under the C locale).
#[cfg_attr(windows, allow(dead_code))]
fn parse_lpstat(printers_out: &str, default_out: &str) -> Vec<Printer> {
    // "system default destination: NAME" or "no system default destination".
    let default_queue = default_out
        .split_once(':')
        .map(|(_, name)| name.trim())
        .filter(|name| !name.is_empty());

    printers_out
        .lines()
        .filter_map(|line| {
            // "printer NAME is idle.  enabled since ..." / "printer NAME disabled since ..."
            let mut words = line.split_whitespace();
            if words.next() != Some("printer") {
                return None; // continuation lines (alerts, descriptions) are indented
            }
            let name = words.next()?;
            let lower = line.to_ascii_lowercase();
            let state = if lower.contains("disabled") {
                Some("disabled")
            } else if lower.contains("now printing") || lower.contains(" printing") {
                Some("printing")
            } else if lower.contains("idle") {
                Some("idle")
            } else {
                None
            };

            let mut printer = Printer::new(
                format!("local:{name}"),
                name.to_string(),
                PrinterSource::Local,
            );
            printer.protocols = vec!["spooler".into()];
            printer.is_default = default_queue == Some(name);
            printer.state = state.map(str::to_string);
            Some(printer)
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
#[cfg_attr(not(windows), allow(dead_code))]
struct WinPrinter {
    name: Option<String>,
    driver_name: Option<String>,
    location: Option<String>,
    default: Option<bool>,
    printer_status: Option<u16>,
}

#[cfg(windows)]
async fn discover_platform() -> Result<Vec<Printer>> {
    const SCRIPT: &str = "$ErrorActionPreference = 'Stop'; \
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
        $p = @(Get-CimInstance -ClassName Win32_Printer | \
               Select-Object Name, DriverName, Location, Default, PrinterStatus); \
        ConvertTo-Json -InputObject $p -Compress";
    let out = crate::sys::powershell(SCRIPT, &[]).await?;
    parse_windows(&out.stdout)
}

/// Parses `ConvertTo-Json` output of Win32_Printer rows.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_windows(json: &str) -> Result<Vec<Printer>> {
    let json = json.trim();
    if json.is_empty() {
        return Ok(Vec::new());
    }
    // ConvertTo-Json emits a bare object for a single item on some PowerShell versions.
    let rows: Vec<WinPrinter> = if json.starts_with('[') {
        serde_json::from_str(json)?
    } else {
        vec![serde_json::from_str(json)?]
    };

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let name = row.name.filter(|n| !n.is_empty())?;
            let mut printer =
                Printer::new(format!("local:{name}"), name.clone(), PrinterSource::Local);
            printer.protocols = vec!["spooler".into()];
            printer.make_and_model = row.driver_name;
            printer.location = row.location.filter(|l| !l.is_empty());
            printer.is_default = row.default.unwrap_or(false);
            // Win32_Printer.PrinterStatus codes.
            printer.state = row.printer_status.and_then(|s| match s {
                3 => Some("idle".into()),
                4 => Some("printing".into()),
                5 => Some("warmup".into()),
                6 => Some("stopped".into()),
                7 => Some("offline".into()),
                _ => None,
            });
            Some(printer)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lpstat() {
        let p = "printer Office_Laser is idle.  enabled since Tue Nov  4 10:00:00 2026\n\
                 \tReady\n\
                 printer Zebra_ZD420 disabled since Tue Nov  4 09:00:00 2026 -\n\
                 \tPaused\n\
                 printer Receipts now printing Receipts-12.  enabled since ...\n";
        let d = "system default destination: Office_Laser\n";
        let printers = parse_lpstat(p, d);

        assert_eq!(printers.len(), 3);
        assert_eq!(printers[0].id, "local:Office_Laser");
        assert!(printers[0].is_default);
        assert_eq!(printers[0].state.as_deref(), Some("idle"));
        assert_eq!(printers[1].state.as_deref(), Some("disabled"));
        assert!(!printers[1].is_default);
        assert_eq!(printers[2].state.as_deref(), Some("printing"));
    }

    #[test]
    fn lpstat_without_default() {
        let printers = parse_lpstat("printer A is idle.\n", "no system default destination\n");
        assert!(!printers[0].is_default);
    }

    #[test]
    fn parses_windows_single_and_array() {
        let one = r#"{"Name":"HP","DriverName":"HP Universal","Location":"","Default":true,"PrinterStatus":3}"#;
        let printers = parse_windows(one).unwrap();
        assert_eq!(printers.len(), 1);
        assert!(printers[0].is_default);
        assert_eq!(printers[0].state.as_deref(), Some("idle"));
        assert_eq!(printers[0].location, None);

        let many = r#"[{"Name":"A","DriverName":null,"Location":"2F","Default":false,"PrinterStatus":7},{"Name":"B"}]"#;
        let printers = parse_windows(many).unwrap();
        assert_eq!(printers.len(), 2);
        assert_eq!(printers[0].state.as_deref(), Some("offline"));
        assert_eq!(printers[0].location.as_deref(), Some("2F"));
    }
}
