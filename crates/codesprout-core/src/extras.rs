//! Submodules, Git LFS, worktrees, patches, archives, commit templates, external tools
//! and user-defined custom actions.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Result};

use crate::model::OpResult;
use crate::repo::{config_value, conflict_aware, safe_join};
use crate::runner::{self, git, out, run, OpHandle, Opts};

// ---------------------------------------------------------------- submodules

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmoduleState {
    UpToDate,
    NotInitialized,
    /// Checked-out commit differs from the one recorded in the superproject.
    Modified,
    Conflict,
}

impl SubmoduleState {
    pub fn label(self) -> &'static str {
        match self {
            SubmoduleState::UpToDate => "Al día",
            SubmoduleState::NotInitialized => "Sin inicializar",
            SubmoduleState::Modified => "Commit distinto al registrado",
            SubmoduleState::Conflict => "En conflicto",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Submodule {
    pub path: String,
    pub hash: String,
    pub state: SubmoduleState,
    pub describe: String,
    pub url: String,
}

pub fn submodules(repo: &Path) -> Result<Vec<Submodule>> {
    if !repo.join(".gitmodules").exists() {
        return Ok(vec![]);
    }
    let text = out(repo, &["submodule", "status"])?;
    let mut list = Vec::new();
    for line in text.lines() {
        if line.len() < 42 {
            continue;
        }
        let state = match &line[..1] {
            "-" => SubmoduleState::NotInitialized,
            "+" => SubmoduleState::Modified,
            "U" => SubmoduleState::Conflict,
            _ => SubmoduleState::UpToDate,
        };
        let rest = &line[1..];
        let (hash, rest) = rest.split_once(' ').unwrap_or((rest, ""));
        let (path, describe) = match rest.rfind(" (") {
            Some(i) if rest.ends_with(')') => (&rest[..i], &rest[i + 2..rest.len() - 1]),
            _ => (rest, ""),
        };
        let url = config_value(repo, &format!("submodule.{path}.url"));
        list.push(Submodule { path: path.into(), hash: hash.into(), state, describe: describe.into(), url });
    }
    Ok(list)
}

/// `git submodule update --init --recursive` for one path (or all when `path` is None).
pub fn submodule_update(repo: &Path, path: Option<&str>, op: Option<&OpHandle>) -> Result<()> {
    let mut args = vec!["submodule", "update", "--init", "--recursive", "--progress"];
    if let Some(p) = path {
        args.extend(["--", p]);
    }
    git(repo, &args, &Opts::op(op))?;
    Ok(())
}

pub fn submodule_sync(repo: &Path) -> Result<()> {
    git(repo, &["submodule", "sync", "--recursive"], &Opts::default())?;
    Ok(())
}

pub fn submodule_add(repo: &Path, url: &str, path: &str, op: Option<&OpHandle>) -> Result<()> {
    if url.trim().is_empty() || path.trim().is_empty() {
        bail!("Indica la URL y la carpeta del submódulo.");
    }
    git(repo, &["submodule", "add", "--progress", "--", url.trim(), path.trim()], &Opts::op(op))?;
    Ok(())
}

pub fn submodule_deinit(repo: &Path, path: &str) -> Result<()> {
    git(repo, &["submodule", "deinit", "-f", "--", path], &Opts::default())?;
    Ok(())
}

// ---------------------------------------------------------------- LFS

pub fn lfs_version() -> Option<String> {
    let r = run(&std::env::temp_dir(), &["lfs", "version"], &Opts::default()).ok()?;
    r.ok().then(|| r.text().trim().to_string())
}

#[derive(Clone, Debug)]
pub struct LfsFile {
    pub path: String,
    pub oid: String,
    /// Whether the real content is present (false: only the pointer is checked out).
    pub downloaded: bool,
}

#[derive(Clone, Debug)]
pub struct LfsLock {
    pub id: String,
    pub path: String,
    pub owner: String,
}

#[derive(Clone, Debug, Default)]
pub struct LfsInfo {
    pub version: Option<String>,
    pub patterns: Vec<String>,
    pub files: Vec<LfsFile>,
}

pub fn lfs_info(repo: &Path) -> Result<LfsInfo> {
    let version = lfs_version();
    if version.is_none() {
        return Ok(LfsInfo::default());
    }
    let mut info = LfsInfo { version, ..Default::default() };
    if let Ok(r) = run(repo, &["lfs", "track"], &Opts::default()) {
        for line in r.text().lines() {
            let l = line.trim();
            if let Some(p) = l.split(" (").next().filter(|_| line.starts_with("    ")) {
                info.patterns.push(p.trim().to_string());
            }
        }
    }
    if let Ok(r) = run(repo, &["lfs", "ls-files", "--long"], &Opts::default()) {
        for line in r.text().lines() {
            // <oid> <*|-> <path>
            let mut it = line.splitn(3, ' ');
            if let (Some(oid), Some(mark), Some(path)) = (it.next(), it.next(), it.next()) {
                info.files.push(LfsFile { oid: oid.into(), downloaded: mark == "*", path: path.into() });
            }
        }
    }
    Ok(info)
}

pub fn lfs_install(repo: &Path) -> Result<()> {
    git(repo, &["lfs", "install"], &Opts::default())?;
    Ok(())
}

pub fn lfs_track(repo: &Path, pattern: &str) -> Result<()> {
    if pattern.trim().is_empty() {
        bail!("Indica un patrón, por ejemplo *.psd");
    }
    git(repo, &["lfs", "track", "--", pattern.trim()], &Opts::default())?;
    Ok(())
}

pub fn lfs_untrack(repo: &Path, pattern: &str) -> Result<()> {
    git(repo, &["lfs", "untrack", "--", pattern], &Opts::default())?;
    Ok(())
}

pub fn lfs_pull(repo: &Path, op: Option<&OpHandle>) -> Result<()> {
    git(repo, &["lfs", "pull"], &Opts::op(op))?;
    Ok(())
}

pub fn lfs_locks(repo: &Path) -> Result<Vec<LfsLock>> {
    let text = out(repo, &["lfs", "locks", "--json"])?;
    let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap_or(serde_json::Value::Null);
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .map(|l| LfsLock {
                    id: l["id"].as_str().unwrap_or("").into(),
                    path: l["path"].as_str().unwrap_or("").into(),
                    owner: l["owner"]["name"].as_str().unwrap_or("").into(),
                })
                .collect()
        })
        .unwrap_or_default())
}

pub fn lfs_lock(repo: &Path, path: &str) -> Result<()> {
    git(repo, &["lfs", "lock", "--", path], &Opts::default())?;
    Ok(())
}

pub fn lfs_unlock(repo: &Path, path: &str, force: bool) -> Result<()> {
    let mut args = vec!["lfs", "unlock"];
    if force {
        args.push("--force");
    }
    args.extend(["--", path]);
    git(repo, &args, &Opts::default())?;
    Ok(())
}

// ---------------------------------------------------------------- worktrees

#[derive(Clone, Debug, Default)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub main: bool,
    pub locked: bool,
    pub prunable: bool,
}

pub fn worktrees(repo: &Path) -> Result<Vec<Worktree>> {
    let text = out(repo, &["worktree", "list", "--porcelain"])?;
    let mut list: Vec<Worktree> = Vec::new();
    for block in text.split("\n\n") {
        let mut w = Worktree::default();
        for line in block.lines() {
            let (k, v) = line.split_once(' ').unwrap_or((line, ""));
            match k {
                "worktree" => w.path = PathBuf::from(if cfg!(windows) { v.replace('/', "\\") } else { v.to_string() }),
                "HEAD" => w.head = v.into(),
                "branch" => w.branch = Some(v.trim_start_matches("refs/heads/").into()),
                "locked" => w.locked = true,
                "prunable" => w.prunable = true,
                _ => {}
            }
        }
        if !w.path.as_os_str().is_empty() {
            w.main = list.is_empty();
            list.push(w);
        }
    }
    Ok(list)
}

pub fn worktree_add(repo: &Path, path: &Path, branch: &str, new_branch: bool) -> Result<()> {
    let p = path.to_string_lossy();
    let args: Vec<&str> = if new_branch {
        vec!["worktree", "add", "-b", branch, &p]
    } else {
        vec!["worktree", "add", &p, branch]
    };
    git(repo, &args, &Opts::default())?;
    Ok(())
}

pub fn worktree_remove(repo: &Path, path: &Path, force: bool) -> Result<()> {
    let p = path.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&p);
    git(repo, &args, &Opts::default())?;
    Ok(())
}

pub fn worktree_prune(repo: &Path) -> Result<()> {
    git(repo, &["worktree", "prune"], &Opts::default())?;
    Ok(())
}

// ---------------------------------------------------------------- patches & archives

/// Writes `git format-patch` files for the given commits (oldest first) into `dir`.
pub fn create_patches(repo: &Path, hashes: &[String], dir: &Path) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let d = dir.to_string_lossy();
    let mut files = Vec::new();
    for (i, h) in hashes.iter().enumerate() {
        let n = format!("--start-number={}", i + 1);
        let text = out(repo, &["format-patch", "-1", &n, "-o", &d, h])?;
        files.extend(text.lines().filter(|l| !l.trim().is_empty()).map(|l| PathBuf::from(l.trim())));
    }
    Ok(files)
}

/// Saves the diff of a commit (or of `from..to`) as a single patch file.
pub fn save_diff_patch(repo: &Path, from: &str, to: &str, file: &Path) -> Result<()> {
    let r = git(repo, &["diff", "--binary", "--no-color", "--no-ext-diff", from, to, "--"], &Opts::default())?;
    std::fs::write(file, r.stdout)?;
    Ok(())
}

/// Applies a patch file: mailbox patches (format-patch) with `git am`, plain diffs with `git apply`.
pub fn apply_patch_file(repo: &Path, file: &Path, three_way: bool) -> Result<OpResult> {
    let content = std::fs::read(file)?;
    let is_mbox = content.starts_with(b"From ");
    let p = file.to_string_lossy().into_owned();
    if is_mbox {
        let mut args = vec!["am"];
        if three_way {
            args.push("-3");
        }
        args.push(&p);
        conflict_aware(repo, &args, &Opts::default())
    } else {
        let mut args = vec!["apply", "--index"];
        if three_way {
            args.push("-3");
        }
        args.push(&p);
        conflict_aware(repo, &args, &Opts::default())
    }
}

pub fn archive(repo: &Path, rev: &str, file: &Path) -> Result<()> {
    let fmt = match file.extension().and_then(|e| e.to_str()) {
        Some("zip") => "zip",
        Some("tar") => "tar",
        Some("gz") | Some("tgz") => "tar.gz",
        _ => "zip",
    };
    let f = file.to_string_lossy();
    let format = format!("--format={fmt}");
    git(repo, &["archive", &format, "-o", &f, rev], &Opts::default())?;
    Ok(())
}

// ---------------------------------------------------------------- commit template & signing

/// Contents of `commit.template`, if configured.
pub fn commit_template(repo: &Path) -> Option<String> {
    let path = config_value(repo, "commit.template");
    if path.is_empty() {
        return None;
    }
    let p = if let Some(rest) = path.strip_prefix("~/") {
        dirs_home()?.join(rest)
    } else {
        let p = PathBuf::from(&path);
        if p.is_absolute() { p } else { repo.join(p) }
    };
    let text = std::fs::read_to_string(p).ok()?;
    let cleaned: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    Some(cleaned.join("\n").trim_end().to_string())
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Whether commits are signed by default (`commit.gpgsign`).
pub fn signs_by_default(repo: &Path) -> bool {
    config_value(repo, "commit.gpgsign") == "true"
}

pub fn signing_key(repo: &Path) -> String {
    config_value(repo, "user.signingkey")
}

// ---------------------------------------------------------------- external tools

/// Opens the configured `diff.tool` on a file (working tree, staged, or a commit range).
pub fn difftool(repo: &Path, path: &str, staged: bool, range: Option<(&str, &str)>) -> Result<()> {
    let mut args: Vec<String> = vec!["difftool".into(), "-y".into(), "--no-prompt".into()];
    if staged {
        args.push("--cached".into());
    }
    if let Some((a, b)) = range {
        args.push(a.into());
        args.push(b.into());
    }
    args.push("--".into());
    args.push(path.into());
    spawn_git_detached(repo, &args)
}

/// Opens the configured `merge.tool` on a conflicted file.
pub fn mergetool(repo: &Path, path: &str) -> Result<OpResult> {
    let r = run(repo, &["mergetool", "-y", "--no-prompt", "--", path], &Opts::default())?;
    if !r.ok() {
        return Err(runner::failure(&r, &["mergetool"]));
    }
    Ok(OpResult { conflicts: false, output: runner::clean_output(&r.stderr) })
}

fn spawn_git_detached(repo: &Path, args: &[String]) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(repo).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    runner::hide_window(&mut cmd);
    cmd.spawn().map_err(|e| anyhow!("No se pudo abrir la herramienta: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------- custom actions

/// Context passed to a custom action. Placeholders: `$REPO`, `$SHA`, `$FILE`, `$BRANCH`.
#[derive(Clone, Debug, Default)]
pub struct ActionContext {
    pub repo: PathBuf,
    pub sha: Option<String>,
    pub file: Option<String>,
    pub branch: Option<String>,
}

fn expand(arg: &str, ctx: &ActionContext) -> String {
    let file_abs = ctx
        .file
        .as_ref()
        .and_then(|f| safe_join(&ctx.repo, f).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    arg.replace("$REPO", &ctx.repo.to_string_lossy())
        .replace("$SHA", ctx.sha.as_deref().unwrap_or(""))
        .replace("$FILE", &file_abs)
        .replace("$BRANCH", ctx.branch.as_deref().unwrap_or(""))
}

/// Splits a command line into arguments, honouring double quotes.
pub fn split_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut has = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        args.push(cur);
    }
    args
}

/// Runs a custom action without a shell and returns its combined output.
pub fn run_custom_action(program: &str, args: &str, ctx: &ActionContext) -> Result<String> {
    let program = expand(program.trim(), ctx);
    if program.is_empty() {
        bail!("La acción no tiene programa.");
    }
    let args: Vec<String> = split_args(args).iter().map(|a| expand(a, ctx)).collect();
    let mut cmd = Command::new(&program);
    cmd.args(&args).current_dir(&ctx.repo).stdin(Stdio::null());
    runner::hide_window(&mut cmd);
    let o = cmd.output().map_err(|e| anyhow!("No se pudo ejecutar «{program}»: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    if !o.status.success() {
        bail!("«{program}» terminó con error:\n{}", text.trim());
    }
    Ok(text.trim().to_string())
}
