# Troubleshooting

Start with the agent's log. It reports every discovery pass and every print failure:

```
INFO printbridge::registry: discovery pass: 2 local, 3 mDNS, 1 port-scan -> 5 unique
WARN printbridge::print: IPP submit to Office failed (...); trying RAW fallback
```

Run it in a terminal with `RUST_LOG=debug printbridge` for more detail. When it was
installed with the scripts, the logs are at `~/Library/Logs/PrintBridge/printbridge.log` (macOS)
or in `journalctl --user -u printbridge` (Linux).

## The page says the agent isn't reachable

| Check | How |
|---|---|
| Is it running? | `curl http://127.0.0.1:9631/api/health` |
| Same port? | `--port` on the agent must match `new PrintBridge({ port })` |
| Origin allowed? | The browser console shows a CORS error. Add the page's exact origin (scheme + host + port) to `--allowed-origins`. `localhost` and `127.0.0.1` are different origins. |
| Port already in use? | The agent exits with "could not listen on 127.0.0.1:9631 — is PrintBridge already running?" |
| HTTPS page in Firefox/Safari? | These may block `http://127.0.0.1` from secure pages. Test in Chrome/Edge, or serve the agent behind a locally trusted TLS proxy. |
| Request returns 403 "only accepts localhost requests" | You called it through a hostname other than `localhost` / `127.0.0.1` / `[::1]` (DNS-rebinding guard). |

## A network printer isn't listed

- **macOS:** allow *Local Network* access for PrintBridge (*System Settings → Privacy &
  Security → Local Network*). Without it, mDNS and the sweep quietly find nothing.
- **Windows:** allow `printbridge.exe` through the firewall on *Private* networks, and check the
  network profile is *Private*, not *Public*.
- **Linux:** allow inbound UDP 5353: `sudo ufw allow 5353/udp`.
- **Different subnet or VLAN?** mDNS doesn't cross routers, and the sweep only covers your own /24.
  Add the printer as a local queue in the OS, and it shows up as `source: "local"`.
- **Larger network than /24** (e.g. /22, /16)? The sweep skips it on purpose. Rely on mDNS, or
  add the printer to the OS.
- **Slow printer?** Raise `--mdns-scan-seconds` (e.g. 8) and `--port-probe-timeout-ms` (e.g. 800).
- Check what the OS sees: `dns-sd -B _ipp._tcp` (macOS), `avahi-browse -rt _ipp._tcp` (Linux).

## A local printer isn't listed

- macOS/Linux: run `lpstat -p`. If it says "No destinations added", the printer isn't installed in
  CUPS. On Linux, install `cups-client` if `lpstat` is missing.
- Windows: run `Get-CimInstance Win32_Printer | Select Name` in PowerShell.

## The job "succeeds" but nothing prints

`200 OK` means the printer or spooler *accepted* the job. Check what it did next:

- **Garbage or blank pages from a label/receipt printer.** The payload was rendered as text
  instead of sent raw. Use `contentType: 'application/octet-stream'`.
- **ZPL printed as literal text on an office printer.** That printer doesn't speak ZPL. Send
  PDF instead.
- **PDF to a network printer does nothing.** Some older printers don't take PDF over IPP or 9100.
  Install the printer as a local queue so its driver renders the PDF.
- **PDF to a Windows local queue fails** with "no installed PDF application supports printing".
  Install Adobe Acrobat Reader or SumatraPDF and set it as the default PDF app.
- **Stuck in the queue?** Check `lpstat -o` (CUPS) or the Windows print queue.

## "Unknown printer id"

Ids are rebuilt on every discovery pass. A network printer's id contains its IP, so it changes
if DHCP gives the printer a new address. Call `getPrinters({ refresh: true })` and pick
again. To keep addresses stable, give network printers a DHCP reservation.

## IPP fails, RAW fallback fails

The error message contains both failures. A common cause is a printer that advertises IPP
but has it turned off, with port 9100 closed too. Turn on IPP or "RAW/JetDirect printing" in the
printer's web admin page.
