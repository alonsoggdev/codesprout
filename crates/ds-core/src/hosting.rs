//! GitHub, GitLab and Bitbucket APIs: repositories to clone, pull requests, creating PRs.
//! Authenticates with the credential Git Credential Manager stored at sign-in.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use crate::credentials::{self, url_host, Provider};

#[derive(Clone, Debug, Default)]
pub struct HostedRepo {
    pub full_name: String,
    pub description: String,
    pub clone_url: String,
    pub ssh_url: String,
    pub private: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub source: String,
    pub target: String,
    pub url: String,
    pub draft: bool,
}

/// A repository on a hosting service, identified from a remote URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteRef {
    pub provider: Provider,
    pub host: String,
    /// owner/name (GitLab: full namespace path)
    pub path: String,
}

impl RemoteRef {
    pub fn parse(url: &str) -> Option<RemoteRef> {
        let host = url_host(url)?;
        let provider = Provider::from_host(&host)?;
        let u = url.trim();
        let path = if let Some(rest) = u.split_once("://").map(|x| x.1) {
            rest.splitn(2, '/').nth(1)?.to_string()
        } else {
            u.split_once(':')?.1.to_string()
        };
        let mut path = path.trim_end_matches('/').trim_end_matches(".git").to_string();
        if provider == Provider::AzureDevOps {
            path = path.replace("/_git/", "/");
        }
        Some(RemoteRef { provider, host, path })
    }

    pub fn web_url(&self) -> String {
        match self.provider {
            Provider::AzureDevOps => {
                let parts: Vec<&str> = self.path.split('/').collect();
                if parts.len() >= 3 {
                    format!("https://{}/{}/{}/_git/{}", self.host, parts[0], parts[1], parts[2])
                } else {
                    format!("https://{}/{}", self.host, self.path)
                }
            }
            _ => format!("https://{}/{}", self.host, self.path),
        }
    }

    /// Web page to open a pull request from `source` into `target`.
    pub fn new_pr_url(&self, source: &str, target: &str) -> String {
        let s = encode(source);
        let t = encode(target);
        match self.provider {
            Provider::GitHub => format!("{}/compare/{t}...{s}?expand=1", self.web_url()),
            Provider::GitLab => format!(
                "{}/-/merge_requests/new?merge_request[source_branch]={s}&merge_request[target_branch]={t}",
                self.web_url()
            ),
            Provider::Bitbucket => format!("{}/pull-requests/new?source={s}&dest={t}", self.web_url()),
            Provider::AzureDevOps => format!("{}/pullrequestcreate?sourceRef={s}&targetRef={t}", self.web_url()),
        }
    }
}

pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).user_agent("DestinyStone/2.0").build()
}

struct Api {
    provider: Provider,
    base: String,
    token: String,
    user: String,
}

impl Api {
    fn new(provider: Provider, host: &str) -> Result<Api> {
        let cred = credentials::stored(host)
            .ok_or_else(|| anyhow!("No has iniciado sesión en {host}. Usa «Cuentas» para iniciar sesión."))?;
        let base = match provider {
            Provider::GitHub if host == "github.com" => "https://api.github.com".to_string(),
            Provider::GitHub => format!("https://{host}/api/v3"),
            Provider::GitLab => format!("https://{host}/api/v4"),
            Provider::Bitbucket => "https://api.bitbucket.org/2.0".to_string(),
            Provider::AzureDevOps => bail!("La integración con Azure DevOps solo abre las páginas web por ahora."),
        };
        Ok(Api { provider, base, token: cred.password, user: cred.username })
    }

    fn request(&self, method: &str, url: &str) -> ureq::Request {
        let req = agent().request(method, url);
        let req = match self.provider {
            Provider::GitHub => req.set("Accept", "application/vnd.github+json").set("X-GitHub-Api-Version", "2022-11-28"),
            _ => req.set("Accept", "application/json"),
        };
        req.set("Authorization", &format!("Bearer {}", self.token))
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let url = if path.starts_with("http") { path.to_string() } else { format!("{}{}", self.base, path) };
        let send = |req: ureq::Request| match &body {
            Some(b) => req.send_json(b.clone()),
            None => req.call(),
        };
        let mut res = send(self.request(method, &url));
        // Bitbucket app passwords need Basic auth instead of a bearer token.
        if self.provider == Provider::Bitbucket {
            if let Err(ureq::Error::Status(401, _)) = res {
                let basic = basic_auth(&self.user, &self.token);
                res = send(agent().request(method, &url).set("Accept", "application/json").set("Authorization", &basic));
            }
        }
        match res {
            Ok(r) => Ok(r.into_json::<Value>().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                let msg = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| {
                        v["message"].as_str().map(String::from).or_else(|| v["error"]["message"].as_str().map(String::from))
                    })
                    .unwrap_or(text);
                bail!("{} respondió {code}: {}", self.provider.label(), msg.trim())
            }
            Err(e) => bail!("No se pudo conectar con {}: {e}", self.provider.label()),
        }
    }
}

fn basic_auth(user: &str, pass: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let data = format!("{user}:{pass}").into_bytes();
    let mut s = String::from("Basic ");
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

/// Repositories of the signed-in user (most recently updated first).
pub fn user_repos(provider: Provider, host: &str) -> Result<Vec<HostedRepo>> {
    let api = Api::new(provider, host)?;
    let mut list = Vec::new();
    match provider {
        Provider::GitHub => {
            for page in 1..=5 {
                let v = api.call(
                    "GET",
                    &format!("/user/repos?per_page=100&sort=updated&affiliation=owner,collaborator,organization_member&page={page}"),
                    None,
                )?;
                let arr = v.as_array().cloned().unwrap_or_default();
                for r in &arr {
                    list.push(HostedRepo {
                        full_name: str_of(&r["full_name"]),
                        description: str_of(&r["description"]),
                        clone_url: str_of(&r["clone_url"]),
                        ssh_url: str_of(&r["ssh_url"]),
                        private: r["private"].as_bool().unwrap_or(false),
                    });
                }
                if arr.len() < 100 {
                    break;
                }
            }
        }
        Provider::GitLab => {
            for page in 1..=5 {
                let v = api.call(
                    "GET",
                    &format!("/projects?membership=true&order_by=last_activity_at&per_page=100&page={page}"),
                    None,
                )?;
                let arr = v.as_array().cloned().unwrap_or_default();
                for r in &arr {
                    list.push(HostedRepo {
                        full_name: str_of(&r["path_with_namespace"]),
                        description: str_of(&r["description"]),
                        clone_url: str_of(&r["http_url_to_repo"]),
                        ssh_url: str_of(&r["ssh_url_to_repo"]),
                        private: r["visibility"].as_str() != Some("public"),
                    });
                }
                if arr.len() < 100 {
                    break;
                }
            }
        }
        Provider::Bitbucket => {
            let mut next = Some("/repositories?role=member&pagelen=100&sort=-updated_on".to_string());
            let mut pages = 0;
            while let Some(url) = next.take() {
                let v = api.call("GET", &url, None)?;
                for r in v["values"].as_array().cloned().unwrap_or_default() {
                    let clone = |name: &str| {
                        r["links"]["clone"]
                            .as_array()
                            .and_then(|a| a.iter().find(|c| c["name"] == name))
                            .map(|c| str_of(&c["href"]))
                            .unwrap_or_default()
                    };
                    list.push(HostedRepo {
                        full_name: str_of(&r["full_name"]),
                        description: str_of(&r["description"]),
                        clone_url: clone("https"),
                        ssh_url: clone("ssh"),
                        private: r["is_private"].as_bool().unwrap_or(false),
                    });
                }
                pages += 1;
                next = v["next"].as_str().filter(|_| pages < 5).map(String::from);
            }
        }
        Provider::AzureDevOps => unreachable!(),
    }
    Ok(list)
}

pub fn pull_requests(r: &RemoteRef) -> Result<Vec<PullRequest>> {
    let api = Api::new(r.provider, &r.host)?;
    let mut list = Vec::new();
    match r.provider {
        Provider::GitHub => {
            let v = api.call("GET", &format!("/repos/{}/pulls?state=open&per_page=100", r.path), None)?;
            for p in v.as_array().cloned().unwrap_or_default() {
                list.push(PullRequest {
                    number: p["number"].as_u64().unwrap_or(0),
                    title: str_of(&p["title"]),
                    author: str_of(&p["user"]["login"]),
                    source: str_of(&p["head"]["ref"]),
                    target: str_of(&p["base"]["ref"]),
                    url: str_of(&p["html_url"]),
                    draft: p["draft"].as_bool().unwrap_or(false),
                });
            }
        }
        Provider::GitLab => {
            let v = api.call(
                "GET",
                &format!("/projects/{}/merge_requests?state=opened&per_page=100", encode(&r.path)),
                None,
            )?;
            for p in v.as_array().cloned().unwrap_or_default() {
                list.push(PullRequest {
                    number: p["iid"].as_u64().unwrap_or(0),
                    title: str_of(&p["title"]),
                    author: str_of(&p["author"]["username"]),
                    source: str_of(&p["source_branch"]),
                    target: str_of(&p["target_branch"]),
                    url: str_of(&p["web_url"]),
                    draft: p["draft"].as_bool().unwrap_or(false),
                });
            }
        }
        Provider::Bitbucket => {
            let v = api.call("GET", &format!("/repositories/{}/pullrequests?state=OPEN&pagelen=50", r.path), None)?;
            for p in v["values"].as_array().cloned().unwrap_or_default() {
                list.push(PullRequest {
                    number: p["id"].as_u64().unwrap_or(0),
                    title: str_of(&p["title"]),
                    author: str_of(&p["author"]["display_name"]),
                    source: str_of(&p["source"]["branch"]["name"]),
                    target: str_of(&p["destination"]["branch"]["name"]),
                    url: str_of(&p["links"]["html"]["href"]),
                    draft: p["draft"].as_bool().unwrap_or(false),
                });
            }
        }
        Provider::AzureDevOps => bail!("Abre las pull requests de Azure DevOps en el navegador."),
    }
    Ok(list)
}

/// Creates a pull request and returns its web URL.
pub fn create_pull_request(
    r: &RemoteRef,
    title: &str,
    body: &str,
    source: &str,
    target: &str,
    draft: bool,
) -> Result<String> {
    if title.trim().is_empty() {
        bail!("Escribe un título.");
    }
    let api = Api::new(r.provider, &r.host)?;
    let v = match r.provider {
        Provider::GitHub => api.call(
            "POST",
            &format!("/repos/{}/pulls", r.path),
            Some(json!({ "title": title, "body": body, "head": source, "base": target, "draft": draft })),
        )?,
        Provider::GitLab => {
            let t = if draft { format!("Draft: {title}") } else { title.to_string() };
            api.call(
                "POST",
                &format!("/projects/{}/merge_requests", encode(&r.path)),
                Some(json!({ "title": t, "description": body, "source_branch": source, "target_branch": target })),
            )?
        }
        Provider::Bitbucket => api.call(
            "POST",
            &format!("/repositories/{}/pullrequests", r.path),
            Some(json!({
                "title": title,
                "description": body,
                "draft": draft,
                "source": { "branch": { "name": source } },
                "destination": { "branch": { "name": target } }
            })),
        )?,
        Provider::AzureDevOps => bail!("Crea la pull request de Azure DevOps en el navegador."),
    };
    let url = match r.provider {
        Provider::GitHub => str_of(&v["html_url"]),
        Provider::GitLab => str_of(&v["web_url"]),
        _ => str_of(&v["links"]["html"]["href"]),
    };
    Ok(url)
}
