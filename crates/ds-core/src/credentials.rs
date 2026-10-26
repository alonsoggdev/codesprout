//! Sign-in through Git Credential Manager (GCM): `git credential fill` makes GCM open the
//! provider's browser / SSO flow when there is no stored credential, and the resulting token
//! is the same one git uses for HTTPS and the app uses for hosting APIs.

use std::path::Path;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use crate::runner::{git, run, Opts};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Provider {
    GitHub,
    GitLab,
    Bitbucket,
    AzureDevOps,
}

impl Provider {
    pub const ALL: [Provider; 4] = [Provider::GitHub, Provider::GitLab, Provider::Bitbucket, Provider::AzureDevOps];

    pub fn label(self) -> &'static str {
        match self {
            Provider::GitHub => "GitHub",
            Provider::GitLab => "GitLab",
            Provider::Bitbucket => "Bitbucket",
            Provider::AzureDevOps => "Azure DevOps",
        }
    }

    pub fn default_host(self) -> &'static str {
        match self {
            Provider::GitHub => "github.com",
            Provider::GitLab => "gitlab.com",
            Provider::Bitbucket => "bitbucket.org",
            Provider::AzureDevOps => "dev.azure.com",
        }
    }

    /// Guesses the provider from a host name (self-hosted GitLab usually contains "gitlab").
    pub fn from_host(host: &str) -> Option<Provider> {
        let h = host.to_ascii_lowercase();
        if h == "github.com" || h.ends_with(".github.com") || h.contains("github") {
            Some(Provider::GitHub)
        } else if h.contains("gitlab") {
            Some(Provider::GitLab)
        } else if h == "bitbucket.org" || h.contains("bitbucket") {
            Some(Provider::Bitbucket)
        } else if h == "dev.azure.com" || h.ends_with("visualstudio.com") {
            Some(Provider::AzureDevOps)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub provider: Provider,
    pub host: String,
    pub username: String,
}

#[derive(Clone, Debug)]
pub struct Credential {
    pub username: String,
    pub password: String,
}

fn home() -> std::path::PathBuf {
    std::env::temp_dir()
}

/// Version of Git Credential Manager, if installed.
pub fn gcm_version() -> Option<String> {
    let r = run(&home(), &["credential-manager", "--version"], &Opts::default()).ok()?;
    // "2.5.0+d349307…": keep the version, drop the build hash
    r.ok().then(|| r.text().trim().split('+').next().unwrap_or("").to_string())
}

/// Configured credential helpers (all scopes), in order.
pub fn helpers() -> Vec<String> {
    run(&home(), &["config", "--get-all", "credential.helper"], &Opts::default())
        .ok()
        .filter(|r| r.ok())
        .map(|r| r.text().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

pub fn gcm_configured() -> bool {
    helpers().iter().any(|h| h.contains("manager"))
}

/// Makes GCM the global credential helper.
pub fn configure_gcm() -> Result<()> {
    git(&home(), &["config", "--global", "credential.helper", "manager"], &Opts::default())?;
    Ok(())
}

fn request(host: &str, username: Option<&str>) -> String {
    let mut s = format!("protocol=https\nhost={host}\n");
    if let Some(u) = username {
        s.push_str(&format!("username={u}\n"));
    }
    s.push('\n');
    s
}

fn parse_credential(text: &str) -> Option<Credential> {
    let mut user = None;
    let mut pass = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("username=") {
            user = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("password=") {
            pass = Some(v.to_string());
        }
    }
    Some(Credential { username: user.unwrap_or_default(), password: pass? })
}

/// Stored credential for `host`, without ever prompting.
pub fn stored(host: &str) -> Option<Credential> {
    let input = request(host, None);
    let opts = Opts::input(input.as_bytes())
        .with_env("GCM_INTERACTIVE", "never")
        .with_env("GIT_ASKPASS", "")
        .with_env("SSH_ASKPASS", "")
        .with_env("DESTINY_STONE_ASKPASS", "");
    let r = run(&home(), &["credential", "fill"], &opts).ok()?;
    if !r.ok() {
        return None;
    }
    parse_credential(&r.text())
}

/// Signs in to `host`. GCM opens the browser (OAuth / SSO) when needed; the credential is then
/// stored so git and the app can use it.
pub fn login(provider: Provider, host: &str) -> Result<Account> {
    if !gcm_configured() {
        bail!("Git Credential Manager no está configurado como gestor de credenciales.");
    }
    let input = request(host, None);
    let opts = Opts::input(input.as_bytes()).with_env("GCM_INTERACTIVE", "always");
    let r = run(&home(), &["credential", "fill"], &opts)?;
    if !r.ok() {
        let msg = crate::runner::clean_output(&r.stderr);
        bail!(if msg.is_empty() { "No se completó el inicio de sesión.".to_string() } else { msg });
    }
    let text = r.text();
    let cred = parse_credential(&text).ok_or_else(|| anyhow!("El gestor de credenciales no devolvió ninguna credencial."))?;
    // Tell the helper the credential is good so it is persisted.
    let approve = format!("protocol=https\nhost={host}\nusername={}\npassword={}\n\n", cred.username, cred.password);
    git(&home(), &["credential", "approve"], &Opts::input(approve.as_bytes()))?;
    Ok(Account { provider, host: host.to_string(), username: cred.username })
}

/// Removes the stored credential for the account.
pub fn logout(account: &Account) -> Result<()> {
    let input = request(&account.host, Some(&account.username).filter(|u| !u.is_empty()).map(|s| s.as_str()));
    git(&home(), &["credential", "reject"], &Opts::input(input.as_bytes()))?;
    Ok(())
}

/// Host part of a remote URL (https, ssh://, or scp-like `git@host:path`).
pub fn url_host(url: &str) -> Option<String> {
    let u = url.trim();
    if let Some(rest) = u.split_once("://").map(|x| x.1) {
        let authority = rest.split('/').next()?;
        let host = authority.rsplit('@').next()?;
        return Some(host.split(':').next()?.to_string());
    }
    // scp-like: [user@]host:path
    let (left, _) = u.split_once(':')?;
    Some(left.rsplit('@').next()?.to_string())
}

pub fn is_ssh_url(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("ssh://") || (!u.contains("://") && u.contains('@') && u.contains(':'))
}

/// Remotes of the repository whose host has no stored credential (HTTPS only).
pub fn hosts_needing_login(repo: &Path) -> Vec<String> {
    let mut hosts: Vec<String> = crate::repo::remotes_info(repo)
        .unwrap_or_default()
        .iter()
        .filter(|r| !is_ssh_url(&r.url) && r.url.starts_with("https://"))
        .filter_map(|r| url_host(&r.url))
        .collect();
    hosts.sort();
    hosts.dedup();
    hosts.into_iter().filter(|h| stored(h).is_none()).collect()
}
