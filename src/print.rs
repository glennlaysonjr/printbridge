//! Routes a print job to the best transport a printer supports: IPP for driverless
//! network printing, RAW 9100 for label/receipt printers and PCL/PostScript devices,
//! and the OS spooler for locally installed queues.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine as _;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::ipp::IppClient;
use crate::models::{PrintRequest, PrintResult, Printer, PrinterSource};
use crate::registry::PrinterRegistry;

const RAW_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RAW_WRITE_TIMEOUT: Duration = Duration::from_secs(120);
/// Guards against runaway loops from a bad `copies` value.
const MAX_COPIES: u32 = 999;

pub struct PrintService {
    registry: Arc<PrinterRegistry>,
    ipp: IppClient,
}

impl PrintService {
    pub fn new(registry: Arc<PrinterRegistry>) -> Result<Self> {
        Ok(Self {
            registry,
            ipp: IppClient::new()?,
        })
    }

    pub async fn print(&self, request: PrintRequest) -> PrintResult {
        let Some(printer) = self.registry.find(&request.printer_id) else {
            return PrintResult::rejected(
                &request.printer_id,
                "Unknown printer id — call getPrinters() first; ids change between discovery passes.",
            );
        };

        let data = match (&request.data_base64, &request.text) {
            (Some(b64), _) => match base64::engine::general_purpose::STANDARD.decode(b64.trim()) {
                Ok(bytes) => bytes,
                Err(_) => {
                    return PrintResult::rejected(&printer.id, "dataBase64 is not valid base64");
                }
            },
            (None, Some(text)) => text.clone().into_bytes(),
            (None, None) => {
                return PrintResult::rejected(&printer.id, "Provide dataBase64 or text");
            }
        };
        if data.is_empty() {
            return PrintResult::rejected(&printer.id, "Empty document");
        }
        let copies = request.copies.clamp(1, MAX_COPIES);

        match self.dispatch(&printer, &data, &request, copies).await {
            Ok(result) => result,
            Err(err) => {
                tracing::error!("print to {} failed: {err:#}", printer.name);
                PrintResult::fail(&printer.id, "none", format!("{err:#}"))
            }
        }
    }

    async fn dispatch(
        &self,
        printer: &Printer,
        data: &[u8],
        request: &PrintRequest,
        copies: u32,
    ) -> Result<PrintResult> {
        if printer.source == PrinterSource::Local {
            return print_via_spooler(printer, data, request, copies).await;
        }

        let mut ipp_error = None;
        if printer.supports("ipp") || printer.supports("ipps") {
            let uri = match &printer.uri {
                Some(uri) if uri.starts_with("ipp") => uri.clone(),
                _ => format!(
                    "ipp://{}:{}/ipp/print",
                    host_for_uri(printer.host.as_deref().unwrap_or_default()),
                    631
                ),
            };
            match self
                .ipp
                .print_job(&uri, data, &request.content_type, &request.title, copies)
                .await
            {
                Ok(msg) => return Ok(PrintResult::ok(&printer.id, "ipp", msg)),
                Err(err) => {
                    tracing::warn!(
                        "IPP submit to {} failed ({err:#}); trying RAW fallback",
                        printer.name
                    );
                    ipp_error = Some(format!("{err:#}"));
                }
            }
        }

        if printer.host.is_some() {
            let raw = print_via_raw_socket(printer, data, copies).await;
            return match ipp_error {
                // Report both attempts: the IPP error is usually the informative one.
                Some(ipp) => {
                    raw.with_context(|| format!("IPP failed ({ipp}); RAW fallback failed"))
                }
                None => raw,
            };
        }

        Ok(PrintResult::fail(
            &printer.id,
            "none",
            format!(
                "No usable transport for protocols [{}]",
                printer.protocols.join(",")
            ),
        ))
    }
}

fn host_for_uri(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

async fn print_via_raw_socket(printer: &Printer, data: &[u8], copies: u32) -> Result<PrintResult> {
    let host = printer.host.as_deref().context("printer has no host")?;
    let port = printer.raw_port.unwrap_or(9100);

    let mut stream = tokio::time::timeout(RAW_CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .with_context(|| format!("timed out connecting to {host}:{port}"))?
        .with_context(|| format!("could not connect to {host}:{port}"))?;

    tokio::time::timeout(RAW_WRITE_TIMEOUT, async {
        for _ in 0..copies {
            stream.write_all(data).await?;
        }
        stream.flush().await?;
        stream.shutdown().await
    })
    .await
    .context("timed out sending to printer")?
    .context("error sending to printer")?;

    Ok(PrintResult::ok(
        &printer.id,
        "raw",
        format!("Sent {} bytes to {host}:{port}", data.len()),
    ))
}

async fn print_via_spooler(
    printer: &Printer,
    data: &[u8],
    request: &PrintRequest,
    copies: u32,
) -> Result<PrintResult> {
    let queue = printer
        .id
        .strip_prefix("local:")
        .context("not a local printer id")?
        .to_string();

    let path = write_temp_file(data, &request.content_type).await?;
    let result = spool(&queue, &path, data, request, copies).await;
    match &result {
        // The Windows PDF route hands the file to another application that opens it
        // asynchronously, so it must outlive this call; clean it up later instead.
        Ok(Spooled::Handoff) => {
            let path = path.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(300)).await;
                let _ = tokio::fs::remove_file(path).await;
            });
        }
        _ => {
            let _ = tokio::fs::remove_file(&path).await;
        }
    }

    match result? {
        Spooled::Queued(msg) => Ok(PrintResult::ok(&printer.id, "spooler", msg)),
        Spooled::Handoff => Ok(PrintResult::ok(
            &printer.id,
            "spooler",
            format!("Handed to the default PDF application for '{queue}'"),
        )),
    }
}

enum Spooled {
    Queued(String),
    /// File was handed off to another process that still needs it.
    #[cfg_attr(not(windows), allow(dead_code))]
    Handoff,
}

#[cfg(not(windows))]
async fn spool(
    queue: &str,
    path: &Path,
    _data: &[u8],
    request: &PrintRequest,
    copies: u32,
) -> Result<Spooled> {
    // Arguments are passed as an argv array — no shell, so queue names and titles
    // can't inject anything.
    let mut cmd = crate::sys::command("lp");
    cmd.arg("-d")
        .arg(queue)
        .arg("-n")
        .arg(copies.to_string())
        .arg("-t")
        .arg(&request.title);
    if is_raw_payload(&request.content_type) {
        // Opaque bytes (ZPL, ESC/POS, PCL...) go to the device untouched.
        cmd.arg("-o").arg("raw");
    }
    cmd.arg("--").arg(path);

    let out = crate::sys::run(cmd).await?;
    if !out.success {
        anyhow::bail!("lp failed: {}", out.stderr.trim());
    }
    let detail = out.stdout.trim();
    Ok(Spooled::Queued(if detail.is_empty() {
        format!("Queued on '{queue}'")
    } else {
        format!("Queued on '{queue}': {detail}")
    }))
}

#[cfg(windows)]
async fn spool(
    queue: &str,
    path: &Path,
    data: &[u8],
    request: &PrintRequest,
    copies: u32,
) -> Result<Spooled> {
    let mime = request.content_type.to_ascii_lowercase();
    let path_str = path.to_string_lossy().into_owned();

    if mime.starts_with("text/") {
        // Out-Printer renders text through the queue's driver.
        let script = "$ErrorActionPreference = 'Stop'; \
            $t = Get-Content -LiteralPath $env:PB_FILE -Raw -Encoding UTF8; \
            for ($i = 0; $i -lt [int]$env:PB_COPIES; $i++) { $t | Out-Printer -Name $env:PB_PRINTER }";
        let copies = copies.to_string();
        crate::sys::powershell(
            script,
            &[
                ("PB_FILE", &path_str),
                ("PB_PRINTER", queue),
                ("PB_COPIES", &copies),
            ],
        )
        .await?;
        return Ok(Spooled::Queued(format!("Queued on '{queue}'")));
    }

    if mime == "application/pdf" {
        // Windows has no built-in PDF rasterizer for the spooler; the registered PDF
        // application's "PrintTo" verb does the rendering (Acrobat, SumatraPDF, ...).
        let script = "$ErrorActionPreference = 'Stop'; \
            for ($i = 0; $i -lt [int]$env:PB_COPIES; $i++) { \
              Start-Process -FilePath $env:PB_FILE -Verb PrintTo \
                -ArgumentList ('\"' + $env:PB_PRINTER + '\"') -WindowStyle Hidden }";
        let copies = copies.to_string();
        crate::sys::powershell(
            script,
            &[
                ("PB_FILE", &path_str),
                ("PB_PRINTER", queue),
                ("PB_COPIES", &copies),
            ],
        )
        .await
        .context("no installed PDF application supports printing (PrintTo verb)")?;
        return Ok(Spooled::Handoff);
    }

    // Everything else is passed to the queue as RAW bytes (ZPL, ESC/POS, PCL, PS...).
    let queue_owned = queue.to_string();
    let title = request.title.clone();
    let data = data.to_vec();
    tokio::task::spawn_blocking(move || winspool::write_raw(&queue_owned, &title, &data, copies))
        .await
        .context("spooler task panicked")??;
    Ok(Spooled::Queued(format!("Queued RAW on '{queue}'")))
}

/// Payloads that must reach the device byte-for-byte rather than be rendered.
#[cfg_attr(windows, allow(dead_code))]
fn is_raw_payload(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    ct == "application/octet-stream"
        || ct == "application/vnd.cups-raw"
        || ct.contains("zpl")
        || ct.contains("escpos")
}

async fn write_temp_file(data: &[u8], content_type: &str) -> Result<PathBuf> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = format!(
        "printbridge-{}-{}-{}{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
        extension_for(content_type)
    );
    let path = std::env::temp_dir().join(unique);
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .with_context(|| format!("could not create {}", path.display()))?;
    file.write_all(data).await?;
    file.flush().await?;
    Ok(path)
}

fn extension_for(mime: &str) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "application/pdf" => ".pdf",
        "text/plain" => ".txt",
        "application/postscript" => ".ps",
        _ => ".bin",
    }
}

#[cfg(windows)]
mod winspool {
    //! Sends bytes to a Windows print queue with the RAW datatype, bypassing the
    //! driver's rendering — what label and receipt printers expect.

    use anyhow::{Result, bail};
    use windows_sys::Win32::Graphics::Printing::{
        ClosePrinter, DOC_INFO_1W, EndDocPrinter, EndPagePrinter, OpenPrinterW, PRINTER_HANDLE,
        StartDocPrinterW, StartPagePrinter, WritePrinter,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error(what: &str) -> anyhow::Error {
        anyhow::anyhow!("{what} failed: {}", std::io::Error::last_os_error())
    }

    struct Handle(PRINTER_HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful OpenPrinterW and is closed once.
            unsafe { ClosePrinter(self.0) };
        }
    }

    pub fn write_raw(queue: &str, title: &str, data: &[u8], copies: u32) -> Result<()> {
        let name = wide(queue);
        let mut doc_name = wide(title);
        let mut datatype = wide("RAW");

        let mut raw = PRINTER_HANDLE::default();
        // SAFETY: `name` is NUL-terminated and outlives the call; `raw` is a valid out-pointer.
        if unsafe { OpenPrinterW(name.as_ptr(), &mut raw, std::ptr::null()) } == 0 {
            return Err(last_error("OpenPrinter"));
        }
        let handle = Handle(raw);

        let info = DOC_INFO_1W {
            pDocName: doc_name.as_mut_ptr(),
            pOutputFile: std::ptr::null_mut(),
            pDatatype: datatype.as_mut_ptr(),
        };
        // SAFETY: `info` points at NUL-terminated buffers that outlive the call.
        if unsafe { StartDocPrinterW(handle.0, 1, &info) } == 0 {
            return Err(last_error("StartDocPrinter"));
        }

        let result = (|| {
            // SAFETY: a document is open on this handle.
            if unsafe { StartPagePrinter(handle.0) } == 0 {
                return Err(last_error("StartPagePrinter"));
            }
            for _ in 0..copies {
                for chunk in data.chunks(64 * 1024) {
                    let mut offset = 0;
                    while offset < chunk.len() {
                        let rest = &chunk[offset..];
                        let mut written = 0u32;
                        // SAFETY: `rest` is a live slice of `rest.len()` bytes (≤ 64 KiB).
                        let ok = unsafe {
                            WritePrinter(
                                handle.0,
                                rest.as_ptr().cast(),
                                rest.len() as u32,
                                &mut written,
                            )
                        };
                        if ok == 0 {
                            return Err(last_error("WritePrinter"));
                        }
                        if written == 0 {
                            bail!("WritePrinter made no progress");
                        }
                        offset += written as usize;
                    }
                }
            }
            // SAFETY: a page is open on this handle.
            unsafe { EndPagePrinter(handle.0) };
            Ok(())
        })();

        // SAFETY: the document was started above.
        unsafe { EndDocPrinter(handle.0) };
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_raw_payloads() {
        assert!(is_raw_payload("application/octet-stream"));
        assert!(is_raw_payload("application/x-zpl"));
        assert!(!is_raw_payload("application/pdf"));
        assert!(!is_raw_payload("text/plain"));
    }

    #[test]
    fn brackets_ipv6_hosts() {
        assert_eq!(host_for_uri("10.0.0.5"), "10.0.0.5");
        assert_eq!(host_for_uri("fd00::5"), "[fd00::5]");
    }
}
