//! Repositories inside WSL. A repository opened through `\\wsl.localhost\<distro>\...` (or
//! `\\wsl$\...`) is run with the distro's own git, so its configuration, SSH keys and hooks apply
//! and there are no ownership or line-ending surprises. Everything else keeps using Windows git.
//!
//! Git runs through an agent (crate `codesprout-wsl`) that stays open inside the distro: one
//! `wsl.exe` per session instead of one per command (~100 ms each). The app embeds the agent and
//! installs it in `~/.cache/codesprout/` the first time. Without it, each command goes through
//! `wsl.exe` directly, which works but is slower and cannot watch files.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use anyhow::{anyhow, bail, Context, Result};
use codesprout_wsl::{read_frame, write_frame, Event, Hello, Request};
use sha2::{Digest, Sha256};

use crate::runner::{self, Opts, Output};

// ---------------------------------------------------------------- paths

/// A path inside a WSL distro, as seen from Windows.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    /// `\\wsl.localhost\` or `\\wsl$\`, as the path was written.
    pub prefix: String,
    pub distro: String,
    /// The same path inside Linux (`/home/me/repo`).
    pub path: String,
}

/// Recognises `\\wsl.localhost\Ubuntu\home\me`, `\\wsl$\Ubuntu\...` and their `\\?\UNC\` forms.
pub fn parse(p: &str) -> Option<Location> {
    let s = p.replace('/', "\\");
    let s = s.strip_prefix(r"\\?\UNC\").map(|r| format!(r"\\{r}")).unwrap_or(s);
    let lower = s.to_ascii_lowercase();
    let prefix = [r"\\wsl.localhost\", r"\\wsl$\"].into_iter().find(|pre| lower.starts_with(pre))?;
    let rest = &s[prefix.len()..];
    let (distro, path) = rest.split_once('\\').unwrap_or((rest, ""));
    if distro.is_empty() {
        return None;
    }
    let path = path.trim_end_matches('\\');
    Some(Location { prefix: s[..prefix.len()].to_string(), distro: distro.to_string(), path: format!("/{}", path.replace('\\', "/")) })
}

/// [`parse`], only on Windows (elsewhere these paths don't exist).
pub fn locate(p: &Path) -> Option<Location> {
    if cfg!(windows) {
        parse(&p.to_string_lossy())
    } else {
        None
    }
}

/// Linux path → Windows path: Windows drives under `mount` go back to `C:\...`.
pub fn to_windows(prefix: &str, distro: &str, mount: &str, linux: &str) -> String {
    if let Some(rest) = linux.strip_prefix(mount) {
        let mut chars = rest.chars();
        if let (Some(d), next) = (chars.next(), chars.next()) {
            if d.is_ascii_alphabetic() && matches!(next, None | Some('/')) {
                let tail = rest[1..].trim_start_matches('/').replace('/', "\\");
                return format!("{}:\\{tail}", d.to_ascii_uppercase());
            }
        }
    }
    format!("{prefix}{distro}{}", linux.replace('/', "\\"))
}

/// Windows path → Linux path inside `distro`; `None` for paths it cannot reach (another distro, a
/// network share).
pub fn to_linux(distro: &str, mount: &str, win: &str) -> Option<String> {
    if let Some(loc) = parse(win) {
        return loc.distro.eq_ignore_ascii_case(distro).then_some(loc.path);
    }
    let b = win.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || b[2] == b'\\' || b[2] == b'/') {
        let tail = win[2..].trim_start_matches(['\\', '/']).replace('\\', "/");
        return Some(format!("{mount}{}/{tail}", (b[0] as char).to_ascii_lowercase()).trim_end_matches('/').to_string());
    }
    None
}

/// A path printed by git running for `cwd`, as a Windows path.
pub fn native_path(cwd: &Path, git_path: &str) -> PathBuf {
    if let Some(loc) = locate(cwd) {
        if git_path.starts_with('/') {
            return PathBuf::from(to_windows(&loc.prefix, &loc.distro, &mount(&loc.distro), git_path));
        }
    }
    PathBuf::from(if cfg!(windows) { git_path.replace('/', "\\") } else { git_path.to_string() })
}

/// `p` as git sees it when it runs for `cwd` (a Linux path for repositories in WSL).
pub fn arg_path(cwd: &Path, p: &Path) -> String {
    let s = p.to_string_lossy();
    if let Some(loc) = locate(cwd) {
        if let Some(l) = to_linux(&loc.distro, &mount(&loc.distro), &s) {
            return l;
        }
    }
    s.into_owned()
}

/// Installed distros (`wsl -l -q`), without Docker Desktop's internal ones.
pub fn distros() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut cmd = Command::new("wsl.exe");
    cmd.args(["-l", "-q"]).stdin(Stdio::null()).stderr(Stdio::null());
    runner::hide_window(&mut cmd);
    let Ok(o) = cmd.output() else { return Vec::new() };
    // wsl.exe writes UTF-16
    let units: Vec<u16> = o.stdout.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
        .lines()
        .map(|l| l.trim().trim_matches('\0').to_string())
        .filter(|l| !l.is_empty() && !l.to_ascii_lowercase().starts_with("docker-desktop"))
        .collect()
}

/// The default user's home folder in `distro`, as a Windows path. Starts the distro if needed.
pub fn home(distro: &str) -> Result<PathBuf> {
    let s = session(distro)?;
    let prefix = if Path::new(&format!(r"\\wsl.localhost\{distro}\")).exists() { r"\\wsl.localhost\" } else { r"\\wsl$\" };
    Ok(PathBuf::from(to_windows(prefix, distro, &s.info.mount, &s.info.home)))
}

/// The distro has no git: the app offers to install it.
#[derive(Debug)]
pub struct GitMissing {
    pub distro: String,
}

impl std::fmt::Display for GitMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} no tiene Git instalado. Instálalo para trabajar con los repositorios que hay dentro.", self.distro)
    }
}

impl std::error::Error for GitMissing {}

// ---------------------------------------------------------------- sessions

static AGENT: OnceLock<&'static [u8]> = OnceLock::new();

/// Registers the agent binary (static Linux executable) that the app embeds.
pub fn set_agent(bytes: &'static [u8]) {
    let _ = AGENT.set(bytes);
}

struct Session {
    distro: String,
    info: Hello,
    agent: Option<Arc<Agent>>,
}

impl Session {
    fn alive(&self) -> bool {
        self.agent.as_ref().is_none_or(|a| a.alive.load(Ordering::SeqCst))
    }
}

fn sessions() -> &'static Mutex<HashMap<String, Arc<Session>>> {
    static S: OnceLock<Mutex<HashMap<String, Arc<Session>>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn session(distro: &str) -> Result<Arc<Session>> {
    // one start at a time: two threads asking for the same distro must share the agent
    static STARTING: Mutex<()> = Mutex::new(());
    let key = distro.to_ascii_lowercase();
    let cached = || sessions().lock().unwrap().get(&key).filter(|s| s.alive()).cloned();
    if let Some(s) = cached() {
        return Ok(s);
    }
    let _guard = STARTING.lock().unwrap();
    if let Some(s) = cached() {
        return Ok(s);
    }
    let s = Arc::new(start(distro)?);
    sessions().lock().unwrap().insert(key, s.clone());
    Ok(s)
}

fn forget(distro: &str) {
    sessions().lock().unwrap().remove(&distro.to_ascii_lowercase());
}

fn mount(distro: &str) -> String {
    session(distro).map(|s| s.info.mount.clone()).unwrap_or_else(|_| "/mnt/".into())
}

fn wsl(distro: &str) -> Command {
    let mut cmd = Command::new("wsl.exe");
    cmd.args(["-d", distro]);
    runner::hide_window(&mut cmd);
    cmd
}

fn start(distro: &str) -> Result<Session> {
    if let Some(bytes) = AGENT.get() {
        match start_agent(distro, bytes) {
            Ok((agent, info)) => return Ok(Session { distro: distro.into(), info, agent: Some(agent) }),
            Err(e) => eprintln!("CodeSprout: agente de WSL no disponible en {distro}, se usará wsl.exe: {e:#}"),
        }
    }
    Ok(Session { distro: distro.into(), info: probe(distro)?, agent: None })
}

/// What the agent's hello says, asked through `wsl.exe` (mode without agent).
fn probe(distro: &str) -> Result<Hello> {
    // the last line says whether a configured credential helper can run (as usable_helper)
    const SCRIPT: &str = r#"printf '%s\n' "$HOME"; wslpath -u 'C:\' 2>/dev/null || echo /mnt/c/; git --version 2>/dev/null || echo
ep=$(git --exec-path 2>/dev/null)
git config --get-all credential.helper 2>/dev/null | { ok=0; while IFS= read -r h; do p=${h%% *}; case "$h" in
  "") ok=0;; '!'*) ok=1;; /*) [ -f "$p" ] && ok=1;;
  *) { [ -f "$ep/git-credential-$p" ] || command -v "git-credential-$p" >/dev/null; } && ok=1;;
esac; done; echo $ok; }"#;
    let o = wsl(distro).args(["--cd", "~", "--exec", "/bin/sh", "-c", SCRIPT]).stdin(Stdio::null()).output().context("No se pudo ejecutar wsl.exe")?;
    if !o.status.success() {
        bail!("No se pudo iniciar {distro} en WSL: {}", String::from_utf8_lossy(&o.stderr).trim());
    }
    let text = String::from_utf8_lossy(&o.stdout);
    let mut lines = text.lines().map(str::trim);
    let home = lines.next().unwrap_or("/").to_string();
    let mount = lines.next().unwrap_or("/mnt/c/");
    let mount = mount.strip_suffix("c/").unwrap_or("/mnt/").to_string();
    let git = lines.next().filter(|l| !l.is_empty()).map(str::to_string);
    let credential_helper = lines.next() == Some("1");
    Ok(Hello { version: String::new(), home, mount, git, credential_helper })
}

// ---------------------------------------------------------------- agent

type Reply = (Event, Vec<u8>);

struct Agent {
    stdin: Mutex<std::process::ChildStdin>,
    pending: Mutex<HashMap<u64, Sender<Reply>>>,
    watchers: Mutex<HashMap<u64, Box<dyn Fn() + Send>>>,
    next: AtomicU64,
    alive: AtomicBool,
}

impl Agent {
    fn id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst)
    }

    fn send(&self, req: &Request, payload: &[u8]) -> Result<()> {
        let r = write_frame(&mut *self.stdin.lock().unwrap(), req, payload);
        if r.is_err() {
            self.alive.store(false, Ordering::SeqCst);
        }
        r.map_err(|e| anyhow!("Se perdió la conexión con WSL: {e}"))
    }

    fn request(&self, id: u64, req: &Request, payload: &[u8]) -> Result<Receiver<Reply>> {
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        if let Err(e) = self.send(req, payload) {
            self.pending.lock().unwrap().remove(&id);
            return Err(e);
        }
        Ok(rx)
    }
}

/// Starts the agent, installing it first if this version is not in the distro yet.
fn start_agent(distro: &str, bytes: &[u8]) -> Result<(Arc<Agent>, Hello)> {
    let hash: String = Sha256::digest(bytes).iter().take(6).map(|b| format!("{b:02x}")).collect();
    let name = format!("wsl-agent-{hash}");
    let rel = format!(".cache/codesprout/{name}");
    for attempt in 0..2 {
        let mut child = wsl(distro)
            .args(["--cd", "~", "--exec", "/bin/sh", "-c", r#"exec "$HOME/$0""#, &rel])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("No se pudo ejecutar wsl.exe")?;
        let mut stdout = child.stdout.take().expect("stdout");
        if let Ok(Some((Event::Hello(info), _))) = read_frame::<Event>(&mut stdout) {
            let agent = Arc::new(Agent {
                stdin: Mutex::new(child.stdin.take().expect("stdin")),
                pending: Mutex::default(),
                watchers: Mutex::default(),
                next: AtomicU64::new(1),
                alive: AtomicBool::new(true),
            });
            let reader = agent.clone();
            thread::spawn(move || {
                read_loop(&reader, &mut stdout);
                let _ = child.kill();
                let _ = child.wait();
            });
            return Ok((agent, info));
        }
        let _ = child.kill();
        let _ = child.wait();
        if attempt == 0 {
            install_agent(distro, &name, bytes)?;
        }
    }
    bail!("el agente no respondió")
}

fn install_agent(distro: &str, name: &str, bytes: &[u8]) -> Result<()> {
    let run = |args: &[&str]| -> Result<()> {
        let st = wsl(distro).args(["--cd", "~", "--exec"]).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
        if !st.success() {
            bail!("{} falló", args[0]);
        }
        Ok(())
    };
    run(&["mkdir", "-p", ".cache/codesprout"])?;
    // other versions left by earlier updates
    let _ = run(&["find", ".cache/codesprout", "-maxdepth", "1", "-name", "wsl-agent-*", "!", "-name", name, "-delete"]);
    let tmp = format!(".cache/codesprout/{name}.tmp");
    let mut child = wsl(distro).args(["--cd", "~", "--exec", "tee", &tmp]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    child.stdin.take().expect("stdin").write_all(bytes)?;
    if !child.wait()?.success() {
        bail!("no se pudo copiar el agente");
    }
    run(&["chmod", "+x", &tmp])?;
    run(&["mv", "-f", &tmp, &format!(".cache/codesprout/{name}")])
}

fn read_loop(agent: &Agent, stdout: &mut impl std::io::Read) {
    while let Ok(Some((ev, payload))) = read_frame::<Event>(stdout) {
        let id = match &ev {
            Event::Hello(_) => continue,
            Event::Changed { id } => {
                if let Some(f) = agent.watchers.lock().unwrap().get(id) {
                    f();
                }
                continue;
            }
            Event::Progress { id, .. } | Event::Done { id, .. } | Event::Failed { id, .. } | Event::Trashed { id, .. } => *id,
        };
        let finished = !matches!(ev, Event::Progress { .. });
        let mut pending = agent.pending.lock().unwrap();
        if let Some(tx) = pending.get(&id) {
            let _ = tx.send((ev, payload));
        }
        if finished {
            pending.remove(&id);
        }
    }
    // the agent exited: whoever waits gets a disconnected channel
    agent.alive.store(false, Ordering::SeqCst);
    agent.pending.lock().unwrap().clear();
}

// ---------------------------------------------------------------- running git

/// Runs git for a repository inside WSL. Same contract as [`runner::run`].
pub fn run(loc: &Location, args: &[&str], opts: &Opts) -> Result<Output> {
    let s = session(&loc.distro)?;
    if s.info.git.is_none() {
        forget(&loc.distro);
        return Err(GitMissing { distro: s.distro.clone() }.into());
    }
    let mut full: Vec<String> = runner::BASE_ARGS.iter().map(|a| a.to_string()).collect();
    if !s.info.credential_helper {
        // No helper that works in Linux: use Windows' Git Credential Manager (the accounts the
        // user already signed in to), only for this command. The empty value first drops the
        // configured ones, e.g. a `libsecret` that is not installed and only prints errors.
        if let Some(gcm) = crate::credentials::windows_gcm_exe().and_then(|p| to_linux(&s.distro, &s.info.mount, &p.to_string_lossy())) {
            full.extend(["-c".into(), "credential.helper=".into(), "-c".into(), format!("credential.helper={}", shell_escape(&gcm))]);
        }
    }
    full.extend(args.iter().map(|a| a.to_string()));

    let mut env: Vec<(String, String)> = runner::BASE_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    if let Some(askpass) = runner::helper_exe().and_then(|p| to_linux(&s.distro, &s.info.mount, &p.to_string_lossy())) {
        env.push(("GIT_ASKPASS".into(), askpass.clone()));
        env.push(("SSH_ASKPASS".into(), askpass));
        env.push(("SSH_ASKPASS_REQUIRE".into(), "force".into()));
        env.push(("CODESPROUT_ASKPASS".into(), "1".into()));
    }
    env.extend(opts.env.iter().map(|(k, v)| (k.to_string(), v.clone())));

    let result = match &s.agent {
        Some(agent) => run_agent(agent, loc, full, env, opts),
        None => run_direct(&s.distro, loc, &full, env, opts),
    };
    if !s.alive() {
        forget(&loc.distro);
    }
    result
}

/// Backslash-escapes a path for git's shell. Not quotes: git only runs a credential helper as a
/// path when the value starts with `/`.
fn shell_escape(s: &str) -> String {
    s.chars().fold(String::new(), |mut out, c| {
        if !(c.is_ascii_alphanumeric() || "/._-+".contains(c)) {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

fn run_agent(agent: &Arc<Agent>, loc: &Location, args: Vec<String>, mut env: Vec<(String, String)>, opts: &Opts) -> Result<Output> {
    // so the askpass window (a Windows program) gets the flag through WSL interop
    let extra = env.iter().rev().find(|(k, _)| k == "WSLENV").map(|(_, v)| format!(":{v}")).unwrap_or_default();
    env.push(("WSLENV".into(), format!("CODESPROUT_ASKPASS{extra}")));
    let id = agent.id();
    let req = Request::Run { id, cwd: loc.path.clone(), args, env, max_buffer: opts.max_buffer, progress: opts.op.is_some_and(|o| o.has_progress()) };
    let rx = agent.request(id, &req, opts.input.unwrap_or_default())?;
    if let Some(op) = opts.op {
        let a = agent.clone();
        op.set_killer(Box::new(move || {
            let _ = a.send(&Request::Kill { id }, &[]);
        }));
    }
    let result = loop {
        match rx.recv() {
            Ok((Event::Progress { line, .. }, _)) => {
                if let Some(op) = opts.op {
                    op.progress_line(line);
                }
            }
            Ok((Event::Done { code, stderr, truncated, .. }, stdout)) => {
                let cancelled = opts.op.is_some_and(|o| o.is_cancelled());
                break Ok(Output { code, stdout, stderr, truncated, cancelled });
            }
            Ok((Event::Failed { message, .. }, _)) => break Err(anyhow!(message)),
            Ok(_) => continue,
            Err(_) => break Err(anyhow!("Se perdió la conexión con WSL. Vuelve a intentarlo.")),
        }
    };
    if let Some(op) = opts.op {
        op.clear_killer();
    }
    result
}

fn run_direct(distro: &str, loc: &Location, args: &[String], env: Vec<(String, String)>, opts: &Opts) -> Result<Output> {
    let mut cmd = wsl(distro);
    cmd.args(["--cd", &loc.path, "--exec", "git"]).args(args);
    // wsl.exe only passes on the variables listed in WSLENV
    let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
    cmd.env("WSLENV", keys.join(":"));
    for (k, v) in &env {
        cmd.env(k, v);
    }
    runner::collect(cmd, opts)
}

/// Starts git without waiting for it (external diff tools).
pub fn spawn_detached(loc: &Location, args: &[String]) -> Result<()> {
    wsl(&loc.distro)
        .args(["--cd", &loc.path, "--exec", "git"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow!("No se pudo abrir la herramienta: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------- files

/// Moves files of a repository in WSL to the distro's trash.
pub fn trash(repo: &Path, files: &[PathBuf]) -> Result<()> {
    let Some(loc) = locate(repo) else { bail!("No es un repositorio de WSL") };
    let s = session(&loc.distro)?;
    let Some(agent) = &s.agent else {
        // without the agent there is no trash to send them to
        for f in files {
            if f.is_dir() {
                std::fs::remove_dir_all(f)?;
            } else {
                std::fs::remove_file(f)?;
            }
        }
        return Ok(());
    };
    let paths = files.iter().filter_map(|f| to_linux(&s.distro, &s.info.mount, &f.to_string_lossy())).collect();
    let id = agent.id();
    let rx = agent.request(id, &Request::Trash { id, paths }, &[])?;
    match rx.recv() {
        Ok((Event::Trashed { error: None, .. }, _)) => Ok(()),
        Ok((Event::Trashed { error: Some(e), .. }, _)) => bail!("No se pudo mover a la papelera: {e}"),
        _ => bail!("Se perdió la conexión con WSL."),
    }
}

/// Watches a repository in WSL from inside Linux (Windows gets no change notifications there).
/// Dropping the handle stops it.
pub struct WatchHandle {
    agent: Arc<Agent>,
    id: u64,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.agent.watchers.lock().unwrap().remove(&self.id);
        let _ = self.agent.send(&Request::Unwatch { id: self.id }, &[]);
    }
}

/// `None` when files cannot be watched (mode without agent).
pub fn watch(dir: &Path, on_change: impl Fn() + Send + 'static) -> Result<Option<WatchHandle>> {
    let Some(loc) = locate(dir) else { bail!("No es una carpeta de WSL") };
    let s = session(&loc.distro)?;
    let Some(agent) = s.agent.clone() else { return Ok(None) };
    let id = agent.id();
    agent.watchers.lock().unwrap().insert(id, Box::new(on_change));
    agent.send(&Request::Watch { id, path: loc.path }, &[])?;
    Ok(Some(WatchHandle { agent, id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wsl_paths() {
        let l = parse(r"\\wsl.localhost\Ubuntu\home\me\repo").unwrap();
        assert_eq!((l.prefix.as_str(), l.distro.as_str(), l.path.as_str()), (r"\\wsl.localhost\", "Ubuntu", "/home/me/repo"));
        let l = parse(r"\\wsl$\Ubuntu-24.04\home\me\").unwrap();
        assert_eq!((l.prefix.as_str(), l.distro.as_str(), l.path.as_str()), (r"\\wsl$\", "Ubuntu-24.04", "/home/me"));
        assert_eq!(parse(r"\\?\UNC\wsl.localhost\Debian\srv").unwrap().path, "/srv");
        assert_eq!(parse(r"\\WSL.LOCALHOST\Ubuntu").unwrap().path, "/");
        assert_eq!(parse("//wsl.localhost/Ubuntu/tmp/x").unwrap().path, "/tmp/x");
        assert!(parse(r"C:\Users\me").is_none());
        assert!(parse(r"\\server\share\x").is_none());
        assert!(parse(r"\\wsl.localhost\").is_none());
    }

    #[test]
    fn translates_both_ways() {
        let pre = r"\\wsl.localhost\";
        assert_eq!(to_windows(pre, "Ubuntu", "/mnt/", "/home/me/repo"), r"\\wsl.localhost\Ubuntu\home\me\repo");
        assert_eq!(to_windows(pre, "Ubuntu", "/mnt/", "/mnt/c/Users/me"), r"C:\Users\me");
        assert_eq!(to_windows(pre, "Ubuntu", "/mnt/", "/mnt/d"), r"D:\");
        assert_eq!(to_windows(pre, "Ubuntu", "/mnt/", "/mnt/wsl/x"), r"\\wsl.localhost\Ubuntu\mnt\wsl\x");
        assert_eq!(to_windows(pre, "Ubuntu", "/", "/c/x"), r"C:\x");

        assert_eq!(to_linux("Ubuntu", "/mnt/", r"\\wsl.localhost\ubuntu\home\me").as_deref(), Some("/home/me"));
        assert_eq!(to_linux("Ubuntu", "/mnt/", r"C:\Users\me\file.txt").as_deref(), Some("/mnt/c/Users/me/file.txt"));
        assert_eq!(to_linux("Ubuntu", "/mnt/", r"D:\").as_deref(), Some("/mnt/d"));
        assert_eq!(to_linux("Ubuntu", "/mnt/", r"\\wsl.localhost\Debian\home"), None);
        assert_eq!(to_linux("Ubuntu", "/mnt/", r"\\server\share"), None);
        assert_eq!(to_linux("Ubuntu", "/mnt/", "relative/path"), None);
    }

    #[test]
    fn escapes_helper_paths_for_the_shell() {
        assert_eq!(shell_escape("/mnt/c/Program Files/Git/mingw64/bin/git-credential-manager.exe"), r"/mnt/c/Program\ Files/Git/mingw64/bin/git-credential-manager.exe");
        assert_eq!(shell_escape("/tmp/cs helper (x)/helper"), r"/tmp/cs\ helper\ \(x\)/helper");
    }
}
