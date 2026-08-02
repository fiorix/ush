//! Self-update check and `ush upgrade` implementation.
//!
//! Three cooperating pieces:
//!
//! 1. **Background probe.** After a normal command, `ush` spawns a detached
//!    subprocess ([`maybe_spawn_background_check`]) that fetches release
//!    metadata and writes a state file under the XDG data directory. The
//!    subprocess uses `setsid` so it outlives the parent.
//! 2. **Banner.** On subsequent runs, [`maybe_print_banner_from_env`] reads
//!    the state file and prints a one-line hint to stderr if a newer version
//!    is available and stderr is a TTY.
//! 3. **Upgrade.** [`run_upgrade`] downloads the matching release tarball,
//!    verifies its SHA-256, extracts the `ush` binary, and atomically renames
//!    it over `/proc/self/exe`.
//!
//! Distribution package builds set `USH_PACKAGED` via `build.rs`. When that
//! value is not `source`, the probe, banner, and upgrade command are disabled
//! so `ush` does not overwrite files managed by dpkg/rpm/pacman/etc.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;

/// Default metadata URL. Override with `USH_UPDATE_METADATA_URL`.
const DEFAULT_METADATA_URL: &str = "https://ush.sucks/dl/cli/latest.json";

/// Environment variable that disables the update probe/banner.
const ENV_DISABLE: &str = "USH_UPDATE_CHECK";

/// Environment variable that overrides the metadata URL.
const ENV_METADATA_URL: &str = "USH_UPDATE_METADATA_URL";

/// Environment variable that pins the version to install/upgrade to.
const ENV_VERSION_OVERRIDE: &str = "USH_UPDATE_VERSION";

/// State file name under the XDG data directory.
const STATE_FILE: &str = "update-check.json";

/// Prefix for in-flight download temp files.
const UPGRADE_TEMP_PREFIX: &str = ".ush.upgrade.";

/// Minimum age before a stale upgrade temp is swept (seconds).
const STALE_TEMP_AGE_SECS: u64 = 600;

/// Connect timeout for background probe (seconds).
const BG_CONNECT_TIMEOUT: u64 = 3;

/// Body timeout for background probe (seconds).
const BG_BODY_TIMEOUT: u64 = 5;

/// Upgrade download connect timeout (seconds).
const UPGRADE_CONNECT_TIMEOUT: u64 = 10;

/// Upgrade download body timeout (seconds).
const UPGRADE_BODY_TIMEOUT: u64 = 60;

/// Safety cap on the downloaded tarball size (64 MiB).
const MAX_TARBALL_SIZE: u64 = 64 * 1024 * 1024;

/// Safety cap on metadata JSON responses (1 MiB).
const MAX_METADATA_SIZE: u64 = 1024 * 1024;

/// Check interval for the background probe (seconds); default 24 hours.
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Build provenance, injected by `build.rs` from the `USH_PACKAGED` env var.
fn build_packaged() -> &'static str {
    option_env!("USH_PACKAGED").unwrap_or("source")
}

/// Whether this binary came from a distribution package.
pub fn is_packaged_build() -> bool {
    build_packaged() != "source"
}

/// Guidance printed when `ush upgrade` runs on a packaged build.
pub fn packaged_build_notice() -> Option<String> {
    if !is_packaged_build() {
        return None;
    }
    let channel = build_packaged();
    let pm_upgrade = match channel {
        "deb" => "sudo apt update && sudo apt upgrade ush",
        "rpm" => "sudo dnf upgrade ush",
        "aur" => "sudo pacman -Syu ush",
        "nix" => "nix profile upgrade ush",
        "brew" => "brew upgrade ush",
        _ => "your package manager",
    };
    Some(format!(
        "ush was installed from a distribution package and is managed by your package manager.\n\
         Self-upgrade is disabled so it does not conflict with the packaged files.\n\n\
         Upgrade with:\n    {pm_upgrade}\n\n\
         Or remove the distro package and reinstall the standalone build:\n    \
         curl -fsSL https://ush.sucks/install.sh | bash"
    ))
}

/// Release metadata fetched from the website.
#[derive(Debug, Deserialize)]
struct Metadata {
    version: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    target: String,
    url: String,
    sha256: String,
}

/// State recorded after a background check.
#[derive(Debug, Serialize, Deserialize, Default)]
struct State {
    checked_at: u64,
    checked_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_version: Option<String>,
}

/// Options for [`run_upgrade`].
pub struct UpgradeOptions {
    /// Skip the confirmation prompt.
    pub assume_yes: bool,
    /// Only check and report; do not download or replace.
    pub check_only: bool,
    /// Pin to a specific version instead of querying metadata.
    pub version_override: Option<String>,
    /// Verbose progress messages to stderr.
    pub verbose: bool,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn env_disabled() -> bool {
    matches!(env::var(ENV_DISABLE), Ok(v) if v == "0")
}

fn metadata_url() -> String {
    env::var(ENV_METADATA_URL).unwrap_or_else(|_| DEFAULT_METADATA_URL.to_string())
}

fn data_dir() -> Result<PathBuf> {
    let base = match env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let home = env::var("HOME").context("HOME is not set")?;
            PathBuf::from(home).join(".local/share")
        }
    };
    Ok(base.join("ush"))
}

fn state_path() -> Result<PathBuf> {
    Ok(data_dir()?.join(STATE_FILE))
}

fn read_state(path: &Path) -> Option<State> {
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn write_state(path: &Path, state: &State) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let body = serde_json::to_vec_pretty(state).context("failed to serialize state")?;
    atomic_write(path, &body, 0o644).context("failed to write state file")
}

fn atomic_write(path: &Path, data: &[u8], mode: u32) -> Result<()> {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut file = File::create(&tmp)
        .with_context(|| format!("failed to create {}", tmp.display()))?;
    file.write_all(data)
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    file.flush()?;
    drop(file);
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename {} to {}", tmp.display(), path.display()))
}

fn http_agent(connect_timeout: u64, body_timeout: u64) -> Result<ureq::Agent> {
    let config = ureq::Agent::config_builder()
        .user_agent(concat!("ush/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(connect_timeout)))
        .timeout_resolve(Some(Duration::from_secs(connect_timeout)))
        .timeout_recv_response(Some(Duration::from_secs(connect_timeout * 2)))
        .timeout_recv_body(Some(Duration::from_secs(body_timeout)));
    Ok(config.build().into())
}

fn read_body_to_string(response: ureq::http::Response<ureq::Body>, limit: u64) -> Result<String> {
    let mut reader = response.into_body().into_reader().take(limit);
    let mut body = String::new();
    reader
        .read_to_string(&mut body)
        .context("failed to read HTTP response body")?;
    Ok(body)
}

fn fetch_metadata(agent: &ureq::Agent, url: &str) -> Result<Metadata> {
    if !url.starts_with("https://")
        && !url.starts_with("file://")
        && env::var("USH_UPDATE_INSECURE").ok().as_deref() != Some("1")
    {
        bail!("metadata URL must use https:// scheme (or set USH_UPDATE_INSECURE=1 for http/file)");
    }
    let response = agent
        .get(url)
        .call()
        .with_context(|| format!("metadata request failed: {url}"))?;
    let body = read_body_to_string(response, MAX_METADATA_SIZE)?;
    serde_json::from_str(&body).context("metadata is not valid JSON")
}

/// Map the host platform to the Rust target triple used for release artifacts.
pub fn detect_target() -> Result<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        (os, arch) => bail!("unsupported platform: {os}/{arch}"),
    }
}

fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let mut parts = s.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch_raw = parts.next().unwrap_or("0");
    let patch_digits: String = patch_raw.chars().take_while(|c| c.is_ascii_digit()).collect();
    let patch: u32 = patch_digits.parse().ok()?;
    Some((major, minor, patch))
}

fn semver_newer(latest: &str, current: &str) -> bool {
    match (parse_semver(latest), parse_semver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Drop-guard that unlinks a path on drop unless disarmed.
struct TempGuard(Option<PathBuf>);

impl TempGuard {
    fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(&path);
        }
    }
}

fn resolve_self_exe() -> Result<PathBuf> {
    fs::canonicalize("/proc/self/exe")
        .or_else(|_| env::current_exe())
        .context("failed to resolve current executable path")
}

fn cleanup_stale_upgrade_temps(binary_dir: &Path) -> Result<usize> {
    let Ok(entries) = fs::read_dir(binary_dir) else {
        return Ok(0);
    };
    let my_pid = std::process::id();
    let now = SystemTime::now();
    Ok(entries
        .flatten()
        .filter(|entry| is_stale_upgrade_temp(entry.path().as_path(), my_pid, now))
        .filter(|entry| fs::remove_file(entry.path()).is_ok())
        .count())
}

fn is_stale_upgrade_temp(path: &Path, my_pid: u32, now: SystemTime) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let Some(pid_str) = name.strip_prefix(UPGRADE_TEMP_PREFIX) else {
        return false;
    };
    if pid_str.is_empty() || !pid_str.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Ok(pid) = pid_str.parse::<u32>() else {
        return false;
    };
    if pid == my_pid {
        return false;
    }
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        return false;
    }
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    let Ok(age) = now.duration_since(mtime) else {
        return false;
    };
    age.as_secs() >= STALE_TEMP_AGE_SECS
}

/// Spawn the detached `__update-check` subprocess when due.
pub fn maybe_spawn_background_check(verbose: bool) {
    if env_disabled() || is_packaged_build() {
        return;
    }
    let interval_secs = match env::var("USH_UPDATE_INTERVAL_SECS") {
        Ok(v) => v.parse().unwrap_or(CHECK_INTERVAL_SECS),
        Err(_) => CHECK_INTERVAL_SECS,
    };
    if let Ok(path) = state_path() {
        if let Some(state) = read_state(&path) {
            if now_unix().saturating_sub(state.checked_at) < interval_secs {
                return;
            }
        }
    }
    let exe = match env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };
    let mut cmd = Command::new(&exe);
    cmd.arg("__update-check")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    match cmd.spawn() {
        Ok(_) => {
            if verbose {
                eprintln!("update-check: background probe spawned");
            }
        }
        Err(e) => {
            if verbose {
                eprintln!("update-check: failed to spawn probe: {e}");
            }
        }
    }
}

/// Entry point for the hidden `__update-check` subcommand.
pub fn run_background_check() -> Result<()> {
    if is_packaged_build() {
        return Ok(());
    }
    let checked_version = env!("CARGO_PKG_VERSION").to_string();
    let now = now_unix();
    let state = match http_agent(BG_CONNECT_TIMEOUT, BG_BODY_TIMEOUT)
        .and_then(|agent| fetch_metadata(&agent, &metadata_url()))
    {
        Ok(metadata) => State {
            checked_at: now,
            checked_version,
            latest_version: Some(metadata.version.trim_start_matches('v').to_string()),
        },
        Err(_) => State {
            checked_at: now,
            checked_version,
            ..State::default()
        },
    };
    if let Ok(path) = state_path() {
        let _ = write_state(&path, &state);
    }
    if let Ok(exe) = resolve_self_exe() {
        if let Some(dir) = exe.parent() {
            let _ = cleanup_stale_upgrade_temps(dir);
        }
    }
    Ok(())
}

fn stderr_is_tty() -> bool {
    unsafe { libc::isatty(libc::STDERR_FILENO) != 0 }
}

/// Print the "update available" banner to stderr if applicable.
pub fn maybe_print_banner_from_env() {
    if !stderr_is_tty() || env_disabled() || is_packaged_build() {
        return;
    }
    let path = match state_path() {
        Ok(p) => p,
        Err(_) => return,
    };
    let state = match read_state(&path) {
        Some(s) => s,
        None => return,
    };
    let latest = match state.latest_version.as_deref() {
        Some(v) => v,
        None => return,
    };
    let current = env!("CARGO_PKG_VERSION");
    if !semver_newer(latest, current) {
        return;
    }
    eprintln!(
        "\nush: update available: {latest} (you have {current}). \
         Run 'ush upgrade' to update."
    );
}

/// Execute `ush upgrade`.
pub fn run_upgrade(opts: UpgradeOptions) -> Result<()> {
    if is_packaged_build() {
        if let Some(notice) = packaged_build_notice() {
            println!("{notice}");
        }
        return Ok(());
    }

    if env::consts::OS != "linux" && env::consts::OS != "macos" {
        bail!("self-upgrade is only supported on Linux and macOS");
    }

    let current = env!("CARGO_PKG_VERSION").to_string();
    let target_version = if let Some(v) = opts.version_override {
        let v = v.trim().trim_start_matches('v').to_string();
        if v.is_empty() {
            bail!("--version cannot be empty");
        }
        v
    } else if let Ok(v) = env::var(ENV_VERSION_OVERRIDE) {
        let v = v.trim().trim_start_matches('v').to_string();
        if v.is_empty() {
            bail!("{ENV_VERSION_OVERRIDE} cannot be empty");
        }
        v
    } else {
        let agent = http_agent(UPGRADE_CONNECT_TIMEOUT, UPGRADE_BODY_TIMEOUT)?;
        let metadata = fetch_metadata(&agent, &metadata_url())?;
        metadata.version.trim_start_matches('v').to_string()
    };

    if target_version == current {
        println!("ush is already at version {current}");
        return Ok(());
    }

    if opts.check_only {
        if semver_newer(&target_version, &current) {
            println!("update available: {target_version} (current: {current})");
        } else {
            println!("target version {target_version} is not newer than current {current}");
        }
        return Ok(());
    }

    let target_triple = detect_target()?;
    let exe_path = resolve_self_exe()?;
    let binary_dir = exe_path
        .parent()
        .context("current executable has no parent directory")?
        .to_path_buf();

    let agent = http_agent(UPGRADE_CONNECT_TIMEOUT, UPGRADE_BODY_TIMEOUT)?;
    let metadata = fetch_metadata(&agent, &metadata_url())?;
    let asset = metadata
        .assets
        .iter()
        .find(|a| a.target == target_triple)
        .with_context(|| format!("no release asset for target {target_triple}"))?;
    let download_url = &asset.url;
    let expected_sha256 = &asset.sha256;

    if !download_url.starts_with("https://")
        && env::var("USH_UPDATE_INSECURE").ok().as_deref() != Some("1")
    {
        bail!("download URL must use https:// scheme: {download_url}");
    }

    // Writability check.
    {
        let probe = binary_dir.join(format!(".ush.upgrade-probe.{}", std::process::id()));
        fs::File::create(&probe).with_context(|| {
            format!("binary directory is not writable: {}", binary_dir.display())
        })?;
        let _ = fs::remove_file(&probe);
    }

    if let Ok(n) = cleanup_stale_upgrade_temps(&binary_dir) {
        if n > 0 && opts.verbose {
            eprintln!("cleaned up {n} stale upgrade temp file(s)");
        }
    }

    println!(
        "ush: upgrading from {current} to {target_version} ({target_triple}) at {}",
        exe_path.display()
    );

    let interactive = stderr_is_tty();
    if !opts.assume_yes {
        if !interactive {
            bail!("use -y to confirm upgrade in non-interactive mode");
        }
        print!("proceed? [Y/n] ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();
        if !input.is_empty() && input != "y" && input != "yes" {
            bail!("aborted");
        }
    }

    let tarball_temp = binary_dir.join(format!("{UPGRADE_TEMP_PREFIX}{}", std::process::id()));
    let guard = TempGuard::new(tarball_temp.clone());

    if opts.verbose {
        eprintln!("downloading {download_url}");
    }
    let response = agent
        .get(download_url)
        .call()
        .with_context(|| format!("failed to download {download_url}"))?;
    let mut reader = response.into_body().into_reader();
    let mut file = File::create(&tarball_temp)
        .with_context(|| format!("failed to create {}", tarball_temp.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut total: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .with_context(|| format!("failed to read from {download_url}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .with_context(|| format!("failed to write to {}", tarball_temp.display()))?;
        hasher.update(&buf[..n]);
        total += n as u64;
        if total > MAX_TARBALL_SIZE {
            bail!("downloaded tarball exceeds safety cap of {MAX_TARBALL_SIZE} bytes");
        }
    }
    file.flush()?;
    drop(file);

    let actual_sha256 = format!("{:x}", hasher.finalize());
    if opts.verbose {
        eprintln!("downloaded {total} bytes, sha256={actual_sha256}");
    }
    if actual_sha256 != expected_sha256.to_ascii_lowercase() {
        bail!(
            "SHA256 mismatch: expected {expected_sha256}, got {actual_sha256}"
        );
    }

    // Extract the `ush` binary from the tarball to a temp binary path.
    let binary_temp = binary_dir.join(format!(".ush.binary.{}", std::process::id()));
    let mut binary_guard = TempGuard::new(binary_temp.clone());
    {
        let tar_gz = File::open(&tarball_temp)
            .with_context(|| format!("failed to open {}", tarball_temp.display()))?;
        let tar = GzDecoder::new(tar_gz);
        let mut archive = Archive::new(tar);
        let mut found = false;
        for entry in archive.entries().context("failed to read tarball entries")? {
            let mut entry = entry.context("failed to read tarball entry")?;
            let path = entry.path().context("failed to read entry path")?;
            if path.file_name().and_then(|n| n.to_str()) == Some("ush") {
                entry
                    .unpack(&binary_temp)
                    .with_context(|| format!("failed to extract to {}", binary_temp.display()))?;
                found = true;
                break;
            }
        }
        if !found {
            bail!("tarball does not contain a 'ush' binary");
        }
    }

    fs::set_permissions(&binary_temp, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("failed to chmod {}", binary_temp.display()))?;

    fs::rename(&binary_temp, &exe_path).with_context(|| {
        format!(
            "failed to replace {} with {}",
            exe_path.display(),
            binary_temp.display()
        )
    })?;
    binary_guard.disarm();

    // The tarball temp is no longer needed.
    drop(guard);

    // Suppress the banner until the next probe.
    if let Ok(path) = state_path() {
        let _ = write_state(
            &path,
            &State {
                checked_at: now_unix(),
                checked_version: target_version.clone(),
                ..State::default()
            },
        );
    }

    println!("ush: upgraded to {target_version}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_target_supported() {
        // This only checks that the function returns Ok on supported hosts.
        let target = detect_target();
        assert!(
            target.is_ok() || cfg!(not(any(target_os = "linux", target_os = "macos"))),
            "detect_target should succeed on Linux/macOS"
        );
    }

    #[test]
    fn test_semver_newer() {
        assert!(semver_newer("2.1.0", "2.0.0"));
        assert!(semver_newer("2.0.1", "2.0.0"));
        assert!(!semver_newer("2.0.0", "2.0.0"));
        assert!(!semver_newer("1.9.9", "2.0.0"));
        assert!(semver_newer("v2.1.0", "v2.0.0"));
    }

}
