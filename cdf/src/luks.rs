//! Detached-header LUKS2 helpers. `K_luks` is always fed through stdin and the
//! header only ever lives in an ephemeral temp file.

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use uuid::Uuid;

use crate::format::Profile;
use crate::keys::LuksKey;

fn cryptsetup_command() -> Command {
    let mut cmd = Command::new("cryptsetup");
    if std::env::var("CDF_CRYPTSETUP_DEBUG").as_deref() == Ok("1") {
        cmd.arg("--debug");
    }
    cmd
}

fn cryptsetup_with_key(mut cmd: Command, key: &[u8]) -> Result<()> {
    let prog = format!("{cmd:?}");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().with_context(|| format!("spawn {prog}"))?;
    {
        let mut stdin = child.stdin.take().context("cryptsetup stdin")?;
        stdin.write_all(key)?;
    }
    let out = child
        .wait_with_output()
        .with_context(|| format!("wait {prog}"))?;
    if !out.status.success() {
        bail!(
            "command failed ({status}): {prog}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim(),
            status = out.status,
        );
    }
    Ok(())
}

/// `luksFormat` `data_dev` with a detached header; returns the header bytes.
pub fn format_detached(
    data_dev: &Path,
    k_luks: &LuksKey,
    volume_uuid: &Uuid,
    profile: Profile,
) -> Result<Vec<u8>> {
    let dir = tempfile::tempdir().context("tempdir for LUKS header")?;
    let header = dir.path().join("luks2.header");
    {
        let f = std::fs::File::create(&header)?;
        // 4 MiB detached header; base64 ≈ 5.3 MiB inside P_meta (32 MiB).
        f.set_len(4 * 1024 * 1024)?;
    }

    let mut cmd = cryptsetup_command();
    cmd.args([
        "luksFormat",
        "--type",
        "luks2",
        "--batch-mode",
        "--cipher",
        "aes-xts-plain64",
        "--key-size",
        "512",
        "--sector-size",
        "4096",
        "--pbkdf",
        "pbkdf2",
        "--pbkdf-force-iterations",
        "1000",
        "--subsystem",
        "cdf-v1",
        "--label",
        &volume_uuid.to_string(),
        "--header",
    ]);
    cmd.arg(&header);
    if matches!(profile, Profile::CryptIntegrity) {
        cmd.args(["--integrity", "hmac-sha256", "--integrity-no-wipe"]);
    }
    cmd.args(["--key-file", "-"]);
    cmd.arg(data_dev);
    cryptsetup_with_key(cmd, &k_luks.0)?;

    let bytes = std::fs::read(&header).context("read detached LUKS2 header")?;
    if !bytes.windows(4).any(|w| w == b"LUKS") {
        bail!("detached header missing LUKS magic");
    }
    Ok(bytes)
}

/// Open `data_dev` as `/dev/mapper/<mapper>`. After open succeeds the mapping
/// no longer needs the header file.
pub fn open_detached(data_dev: &Path, header: &[u8], k_luks: &LuksKey, mapper: &str) -> Result<()> {
    let mut hdr = tempfile::NamedTempFile::new().context("temp LUKS header")?;
    hdr.write_all(header)?;
    hdr.flush()?;

    let mut cmd = cryptsetup_command();
    cmd.args(["open", "--type", "luks2", "--header"])
        .arg(hdr.path())
        .args(["--key-file", "-"])
        .arg(data_dev)
        .arg(mapper);
    cryptsetup_with_key(cmd, &k_luks.0)
}

pub fn close(mapper: &str) -> Result<()> {
    crate::device::run_checked(Command::new("cryptsetup").args(["close", mapper]))
}
