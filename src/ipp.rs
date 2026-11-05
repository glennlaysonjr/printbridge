//! Minimal IPP 2.0 client implementing just the Print-Job operation (RFC 8010/8011).
//! Enough to drive any AirPrint / IPP Everywhere printer without a driver.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};

const OP_PRINT_JOB: u16 = 0x0002;

// Delimiter and value tags (RFC 8010 §3.5).
const TAG_OPERATION_ATTRIBUTES: u8 = 0x01;
const TAG_JOB_ATTRIBUTES: u8 = 0x02;
const TAG_END_OF_ATTRIBUTES: u8 = 0x03;
const TAG_INTEGER: u8 = 0x21;
const TAG_NAME_WITHOUT_LANGUAGE: u8 = 0x42;
const TAG_URI: u8 = 0x45;
const TAG_CHARSET: u8 = 0x47;
const TAG_NATURAL_LANGUAGE: u8 = 0x48;
const TAG_MIME_MEDIA_TYPE: u8 = 0x49;

static REQUEST_ID: AtomicU32 = AtomicU32::new(0);

pub struct IppClient {
    http: reqwest::Client,
}

impl IppClient {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            // Network printers ship self-signed certificates almost universally, and the
            // agent only ever talks to devices on the local network, so ipps:// is used for
            // transport encryption without certificate validation.
            .tls_danger_accept_invalid_certs(true)
            .build()
            .context("could not build HTTP client")?;
        Ok(Self { http })
    }

    /// Submits a Print-Job. Ok carries a human-readable status for the result message.
    pub async fn print_job(
        &self,
        printer_uri: &str,
        document: &[u8],
        mime_type: &str,
        job_name: &str,
        copies: u32,
    ) -> Result<String> {
        let body = encode_print_job(printer_uri, document, mime_type, job_name, copies);
        let response = self
            .http
            .post(http_uri(printer_uri))
            .header("Content-Type", "application/ipp")
            .body(body)
            .send()
            .await
            .context("could not reach printer over IPP")?;

        let status = response.status();
        if !status.is_success() {
            bail!("HTTP {} from printer", status.as_u16());
        }
        let payload = response.bytes().await?;
        let ipp_status = decode_status(&payload)?;
        // 0x0000–0x00FF are the "successful" status codes.
        if ipp_status <= 0x00FF {
            Ok(format!("IPP status 0x{ipp_status:04X}"))
        } else {
            bail!("printer rejected job, IPP status 0x{ipp_status:04X}")
        }
    }
}

/// The IPP attribute keeps the ipp:// scheme; the HTTP transport maps it to http(s)
/// (RFC 7472), keeping the default IPP port 631 explicit.
fn http_uri(printer_uri: &str) -> String {
    let (scheme, rest) = match printer_uri.split_once("://") {
        Some((s, r)) if s.eq_ignore_ascii_case("ipps") => ("https", r),
        Some((_, r)) => ("http", r),
        None => ("http", printer_uri),
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    let has_port = authority
        .rsplit_once(':')
        .is_some_and(|(_, p)| !p.contains(']'));
    if has_port {
        format!("{scheme}://{rest}")
    } else {
        format!("{scheme}://{authority}:631{path}")
    }
}

fn decode_status(payload: &[u8]) -> Result<u16> {
    if payload.len() < 4 {
        bail!("malformed IPP response");
    }
    // Bytes 0-1 are the version, 2-3 the status-code.
    Ok(u16::from_be_bytes([payload[2], payload[3]]))
}

fn encode_print_job(
    printer_uri: &str,
    document: &[u8],
    mime_type: &str,
    job_name: &str,
    copies: u32,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(document.len() + 512);
    let attr = |buf: &mut Vec<u8>, tag: u8, name: &str, value: &[u8]| {
        buf.push(tag);
        buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
        buf.extend_from_slice(value);
    };

    buf.extend_from_slice(&[0x02, 0x00]); // version-number 2.0
    buf.extend_from_slice(&OP_PRINT_JOB.to_be_bytes());
    let request_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    buf.extend_from_slice(&request_id.to_be_bytes());

    buf.push(TAG_OPERATION_ATTRIBUTES);
    attr(&mut buf, TAG_CHARSET, "attributes-charset", b"utf-8");
    attr(
        &mut buf,
        TAG_NATURAL_LANGUAGE,
        "attributes-natural-language",
        b"en",
    );
    attr(&mut buf, TAG_URI, "printer-uri", printer_uri.as_bytes());
    attr(
        &mut buf,
        TAG_NAME_WITHOUT_LANGUAGE,
        "requesting-user-name",
        b"printbridge",
    );
    attr(
        &mut buf,
        TAG_NAME_WITHOUT_LANGUAGE,
        "job-name",
        truncate(job_name, 255).as_bytes(),
    );
    attr(
        &mut buf,
        TAG_MIME_MEDIA_TYPE,
        "document-format",
        mime_type.as_bytes(),
    );

    if copies > 1 {
        buf.push(TAG_JOB_ATTRIBUTES);
        let copies = i32::try_from(copies).unwrap_or(i32::MAX);
        attr(&mut buf, TAG_INTEGER, "copies", &copies.to_be_bytes());
    }

    buf.push(TAG_END_OF_ATTRIBUTES);
    buf.extend_from_slice(document);
    buf
}

/// Truncates to at most `max` bytes on a char boundary (IPP name values are ≤255 octets).
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_ipp_uris_to_http() {
        assert_eq!(
            http_uri("ipp://10.0.0.5:631/ipp/print"),
            "http://10.0.0.5:631/ipp/print"
        );
        assert_eq!(
            http_uri("ipps://10.0.0.5:443/ipp/print"),
            "https://10.0.0.5:443/ipp/print"
        );
        assert_eq!(
            http_uri("ipp://printer.local/ipp/print"),
            "http://printer.local:631/ipp/print"
        );
        assert_eq!(
            http_uri("ipp://[fd00::5]/ipp/print"),
            "http://[fd00::5]:631/ipp/print"
        );
        assert_eq!(
            http_uri("ipp://[fd00::5]:8631/x"),
            "http://[fd00::5]:8631/x"
        );
    }

    #[test]
    fn encodes_print_job() {
        let doc = b"%PDF-1.7 hello";
        let bytes = encode_print_job("ipp://h/ipp/print", doc, "application/pdf", "job", 2);

        assert_eq!(&bytes[0..2], &[0x02, 0x00]);
        assert_eq!(u16::from_be_bytes([bytes[2], bytes[3]]), OP_PRINT_JOB);
        assert_eq!(bytes[8], TAG_OPERATION_ATTRIBUTES);
        // First attribute: charset tag, name length 18, "attributes-charset".
        assert_eq!(bytes[9], TAG_CHARSET);
        assert_eq!(u16::from_be_bytes([bytes[10], bytes[11]]), 18);
        assert_eq!(&bytes[12..30], b"attributes-charset");
        // Copies attribute group present, document appended after end tag.
        assert!(bytes.windows(6).any(|w| w == b"copies"));
        assert!(bytes.ends_with(&[&[TAG_END_OF_ATTRIBUTES][..], doc].concat()));
    }

    #[test]
    fn omits_copies_for_single_copy() {
        let bytes = encode_print_job("ipp://h/ipp/print", b"x", "text/plain", "job", 1);
        assert!(!bytes.windows(6).any(|w| w == b"copies"));
    }

    #[test]
    fn decodes_status() {
        assert_eq!(
            decode_status(&[2, 0, 0x04, 0x01, 0, 0, 0, 1]).unwrap(),
            0x0401
        );
        assert!(decode_status(&[2, 0]).is_err());
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate("héllo", 2), "h");
        assert_eq!(truncate("abc", 10), "abc");
    }
}
