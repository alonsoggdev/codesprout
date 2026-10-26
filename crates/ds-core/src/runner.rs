//! Runs git directly (no shell) with an argument vector, so paths, branch names and
//! messages are never interpreted by a shell.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use anyhow::{anyhow, bail, Result};

const BASE_ARGS: &[&str] = &[
    "--no-optional-locks",
    "-c",
    "core.quotepath=false",
    "-c",
    "color.ui=never",
    "-c",
    "diff.noprefix=false",
    "-c",
    "diff.mnemonicPrefix=false",
    "-c",
    "log.showSignature=false",
    "-c",
    "core.pager=cat",
];

pub const LITERAL: (&str, &str) = ("GIT_LITERAL_PATHSPECS", "1");

/// Helper executable used as GIT_ASKPASS / SSH_ASKPASS / GIT_SEQUENCE_EDITOR.
static HELPER: OnceLock<PathBuf> = OnceLock::new();

/// Registers the executable that answers credential prompts (`--askpass`) and edits
/// rebase todo lists (`--sequence-editor`). Normally the application binary itself.
pub fn set_helper_exe(path: PathBuf) {
    let _ = HELPER.set(path);
}

pub fn helper_exe() -> Option<&'static Path> {
    HELPER.get().map(|p| p.as_path())
}

/// Quotes a path for git's shell-interpreted settings (GIT_SEQUENCE_EDITOR, exec lines).
pub fn sh_quote(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Cancellation and progress for long-running (network) operations.
#[derive(Clone, Default)]
pub struct OpHandle {
    cancelled: Arc<AtomicBool>,
    pid: Arc<Mutex<Option<u32>>>,
    progress: Option<Sender<String>>,
}

impl OpHandle {
    pub fn new(progress: Option<Sender<String>>) -> Self {
        Self { progress, ..Default::default() }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(pid) = *self.pid.lock().unwrap() {
            kill_tree(pid);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/pid", &pid.to_string(), "/T", "/F"]);
        hide_window(&mut cmd);
        let _ = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).spawn();
    }
}

#[cfg(windows)]
pub fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_window(_cmd: &mut Command) {}

#[derive(Default)]
pub struct Opts<'a> {
    pub input: Option<&'a [u8]>,
    pub op: Option<&'a OpHandle>,
    pub env: Vec<(&'a str, String)>,
    pub max_buffer: usize,
    pub allow_fail: bool,
}

impl<'a> Opts<'a> {
    pub fn input(data: &'a [u8]) -> Self {
        Self { input: Some(data), ..Default::default() }
    }
    pub fn literal() -> Self {
        Self { env: vec![(LITERAL.0, LITERAL.1.into())], ..Default::default() }
    }
    pub fn op(op: Option<&'a OpHandle>) -> Self {
        Self { op, ..Default::default() }
    }
    pub fn with_env(mut self, key: &'a str, value: impl Into<String>) -> Self {
        self.env.push((key, value.into()));
        self
    }
}

pub struct Output {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub truncated: bool,
    pub cancelled: bool,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0) && !self.cancelled
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

/// Spawns `git` and collects its output. Never fails because of the exit code.
pub fn run(cwd: &Path, args: &[&str], opts: &Opts) -> Result<Output> {
    if !cwd.exists() {
        bail!("La carpeta no existe: {}", cwd.display());
    }
    let mut cmd = Command::new("git");
    cmd.args(BASE_ARGS)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no");
    if let Some(helper) = helper_exe() {
        let askpass = helper.to_string_lossy().into_owned();
        cmd.env("GIT_ASKPASS", &askpass)
            .env("SSH_ASKPASS", &askpass)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DESTINY_STONE_ASKPASS", "1");
    }
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }
    hide_window(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow!("No se encontró Git. Instálalo (https://git-scm.com) y asegúrate de que está en el PATH.")
        } else {
            anyhow!(e)
        }
    })?;
    if let Some(op) = opts.op {
        *op.pid.lock().unwrap() = Some(child.id());
        if op.is_cancelled() {
            kill_tree(child.id());
        }
    }

    let mut stdin = child.stdin.take().expect("stdin");
    let input = opts.input.map(|b| b.to_vec());
    let writer = thread::spawn(move || {
        if let Some(data) = input {
            let _ = stdin.write_all(&data);
        }
        drop(stdin);
    });

    let mut stderr = child.stderr.take().expect("stderr");
    let progress = opts.op.and_then(|o| o.progress.clone());
    let err_reader = thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    all.extend_from_slice(&buf[..n]);
                    if let Some(tx) = &progress {
                        for line in String::from_utf8_lossy(&buf[..n]).split(['\r', '\n']) {
                            let t = line.trim();
                            if !t.is_empty() {
                                let _ = tx.send(t.to_string());
                            }
                        }
                    }
                }
            }
        }
        String::from_utf8_lossy(&all).into_owned()
    });

    let mut stdout = child.stdout.take().expect("stdout");
    let max = opts.max_buffer;
    let mut out = Vec::new();
    let mut truncated = false;
    let mut buf = [0u8; 65536];
    loop {
        match stdout.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if max > 0 && out.len() + n > max {
                    let room = max.saturating_sub(out.len());
                    out.extend_from_slice(&buf[..room]);
                    truncated = true;
                } else if !truncated {
                    out.extend_from_slice(&buf[..n]);
                }
            }
        }
    }
    let status = child.wait()?;
    let _ = writer.join();
    let stderr = err_reader.join().unwrap_or_default();
    let cancelled = opts.op.map(|o| o.is_cancelled()).unwrap_or(false);
    if let Some(op) = opts.op {
        *op.pid.lock().unwrap() = None;
    }
    Ok(Output { code: status.code(), stdout: out, stderr, truncated, cancelled })
}

fn is_progress_line(l: &str) -> bool {
    const P: &[&str] = &[
        "Enumerating", "Counting", "Compressing", "Writing", "Receiving", "Resolving", "Total",
        "Delta compression", "Unpacking", "Updating files", "Checking out files", "Filtering content",
    ];
    let l = l.strip_prefix("remote: ").unwrap_or(l);
    P.iter().any(|p| l.starts_with(p))
}

pub fn clean_output(text: &str) -> String {
    text.split(['\r', '\n'])
        .map(|l| l.trim_end())
        .filter(|l| !l.trim().is_empty() && !is_progress_line(l))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

pub fn failure(r: &Output, args: &[&str]) -> anyhow::Error {
    if r.cancelled {
        return anyhow!("Operación cancelada.");
    }
    let mut msg = clean_output(&r.stderr);
    if msg.is_empty() {
        msg = clean_output(&r.text());
    }
    if msg.is_empty() {
        msg = format!("git {} terminó con código {:?}", args.first().unwrap_or(&""), r.code);
    }
    anyhow!(msg)
}

/// Runs git and fails on a non-zero exit code (unless `allow_fail`).
pub fn git(cwd: &Path, args: &[&str], opts: &Opts) -> Result<Output> {
    let r = run(cwd, args, opts)?;
    if r.cancelled || (r.code != Some(0) && !opts.allow_fail) {
        return Err(failure(&r, args));
    }
    Ok(r)
}

pub fn out(cwd: &Path, args: &[&str]) -> Result<String> {
    Ok(git(cwd, args, &Opts::default())?.text())
}

pub fn out_opts(cwd: &Path, args: &[&str], opts: &Opts) -> Result<String> {
    Ok(git(cwd, args, opts)?.text())
}
