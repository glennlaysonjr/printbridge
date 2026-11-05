//! Small OS helpers shared by local discovery and spooler printing.

use std::ffi::OsStr;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

/// Upper bound for any helper process (lpstat, lp, powershell).
const PROCESS_TIMEOUT: Duration = Duration::from_secs(60);

/// A command that runs without flashing a console window on Windows and forces the
/// C locale on Unix so CUPS tools print parseable, untranslated output.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.kill_on_drop(true);
    cmd.env("LC_ALL", "C").env("LANG", "C");
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs a command to completion and captures its output.
pub async fn run(mut cmd: Command) -> Result<Output> {
    let out = tokio::time::timeout(PROCESS_TIMEOUT, cmd.output())
        .await
        .context("helper process timed out")?
        .context("could not start helper process")?;
    Ok(Output {
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Runs a PowerShell script. The script is passed with -EncodedCommand (base64 of
/// UTF-16LE), which sidesteps every Windows command-line quoting pitfall. Pass
/// untrusted values through `env` and read them as `$env:NAME` inside the script.
#[cfg_attr(not(windows), allow(dead_code))]
pub async fn powershell(script: &str, env: &[(&str, &str)]) -> Result<Output> {
    use base64::Engine as _;

    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);

    let mut cmd = command("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
        &encoded,
    ]);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = run(cmd).await?;
    if !out.success {
        bail!("PowerShell failed: {}", out.stderr.trim());
    }
    Ok(out)
}
