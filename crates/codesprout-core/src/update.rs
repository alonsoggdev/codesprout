//! Self-update from GitHub Releases. A release is tagged `vX.Y.Z` and carries the executable
//! for each platform plus a `.sha256` file next to it. Installing swaps the running executable
//! in place: it is renamed to `*.old` (allowed on Windows even while it runs) and the new one
//! takes its name; the leftover is removed on the next start.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const REPO: &str = "alonsoggdev/codesprout";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Name of this platform's executable in a release, if releases ship one.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("codesprout-windows-x64.exe")
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    /// Executable and checksum for this platform; `None` means "download it from the page".
    pub asset: Option<(String, String)>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(timeout)).user_agent(&format!("CodeSprout/{CURRENT}")).build()
}

/// Latest published release (drafts and pre-releases are skipped by GitHub).
pub fn latest() -> Result<Release> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let r: ApiRelease = agent(15)
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| anyhow!("No se pudo consultar las versiones: {e}"))?
        .into_json()?;
    let find = |name: &str| r.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
    let asset = asset_name().and_then(|n| Some((find(n)?, find(&format!("{n}.sha256"))?)));
    Ok(Release {
        version: r.tag_name.trim_start_matches('v').to_string(),
        notes: r.body.unwrap_or_default().trim().to_string(),
        page: r.html_url,
        asset,
    })
}

/// Latest release if it is newer than this build.
pub fn check() -> Result<Option<Release>> {
    let r = latest()?;
    Ok(is_newer(&r.version, CURRENT).then_some(r))
}

fn parse(v: &str) -> Option<(u64, u64, u64)> {
    // pre-release / build suffixes ("2.1.0-beta") are ignored
    let core = v.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next().unwrap_or(Some(0))?, it.next().unwrap_or(Some(0))?))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn old_path(exe: &Path) -> PathBuf {
    exe.with_extension("old")
}

/// Removes the executable left behind by a previous update.
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
    }
}

/// Downloads `release`, checks its SHA-256 and replaces the running executable.
/// `progress(done, total)` is called while downloading. Returns the path to launch.
pub fn install(release: &Release, mut progress: impl FnMut(u64, u64)) -> Result<PathBuf> {
    let Some((url, sha_url)) = &release.asset else {
        bail!("Esta versión no incluye un ejecutable para tu sistema. Descárgala desde la página de la versión.");
    };
    let exe = std::env::current_exe().context("No se encontró el ejecutable actual")?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let dir = exe.parent().ok_or_else(|| anyhow!("Ruta del ejecutable no válida"))?;

    let expected = agent(30)
        .get(sha_url)
        .call()
        .map_err(|e| anyhow!("No se pudo descargar la suma de comprobación: {e}"))?
        .into_string()?;
    let expected = expected.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    if expected.len() != 64 {
        bail!("La suma de comprobación publicada no es válida.");
    }

    let resp = agent(600).get(url).call().map_err(|e| anyhow!("No se pudo descargar la actualización: {e}"))?;
    let total: u64 = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let new = exe.with_extension("new");
    let write = || -> Result<String> {
        let mut file = std::fs::File::create(&new).with_context(|| format!("No se puede escribir en {}", dir.display()))?;
        let mut reader = resp.into_reader();
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])?;
            done += n as u64;
            progress(done, total);
        }
        file.sync_all()?;
        Ok(hex(&hasher.finalize()))
    };
    let actual = match write() {
        Ok(h) => h,
        Err(e) => {
            let _ = std::fs::remove_file(&new);
            return Err(e);
        }
    };
    if actual != expected {
        let _ = std::fs::remove_file(&new);
        bail!("El archivo descargado está dañado (la suma SHA-256 no coincide). Inténtalo de nuevo.");
    }

    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).with_context(|| format!("No se pudo reemplazar {}", exe.display()))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        // put the running version back so the installation is left as it was
        let _ = std::fs::rename(&old, &exe);
        let _ = std::fs::remove_file(&new);
        return Err(anyhow!("No se pudo instalar la nueva versión: {e}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    }
    Ok(exe)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn versions() {
        assert!(is_newer("2.1.0", "2.0.0"));
        assert!(is_newer("v2.0.1", "2.0.0"));
        assert!(is_newer("3", "2.9.9"));
        assert!(is_newer("2.10.0", "2.9.0"));
        assert!(!is_newer("2.0.0", "2.0.0"));
        assert!(!is_newer("1.9.0", "2.0.0"));
        assert!(!is_newer("2.0.0-beta", "2.0.0"));
        assert!(!is_newer("nightly", "2.0.0"));
    }
}
