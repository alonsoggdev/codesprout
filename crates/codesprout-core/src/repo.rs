//! Queries and mutations on a repository: the Rust port of the Electron `src/git.js`.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, bail, Result};

use crate::model::*;
use crate::runner::{self, clean_output, failure, git, out, out_opts, run, OpHandle, Opts};

const DIFF_ARGS: &[&str] = &["--no-color", "--no-ext-diff", "--no-textconv"];
pub const MAX_DIFF_BYTES: usize = 8 * 1024 * 1024;
const FROM_STDIN: &[&str] = &["--pathspec-from-file=-", "--pathspec-file-nul"];

fn caches() -> &'static Mutex<HashMap<String, String>> {
    static C: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached(key: String, f: impl FnOnce() -> Result<String>) -> Result<String> {
    if let Some(v) = caches().lock().unwrap().get(&key) {
        return Ok(v.clone());
    }
    let v = f()?;
    caches().lock().unwrap().insert(key, v.clone());
    Ok(v)
}

/// Lexically normalizes `p` (resolves `.` and `..` without touching the disk).
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Joins a repo-relative path, refusing anything that escapes the repository.
pub fn safe_join(repo: &Path, rel: &str) -> Result<PathBuf> {
    let root = normalize(repo);
    let full = normalize(&root.join(if rel.is_empty() { "." } else { rel }));
    if !full.starts_with(&root) {
        bail!("Ruta fuera del repositorio.");
    }
    Ok(full)
}

fn split_n(s: &str, n: usize) -> Vec<&str> {
    let mut parts = Vec::with_capacity(n + 1);
    let mut rest = s;
    for _ in 0..n {
        match rest.find(' ') {
            Some(i) => {
                parts.push(&rest[..i]);
                rest = &rest[i + 1..];
            }
            None => break,
        }
    }
    parts.push(rest);
    parts
}

fn pathspec_input(paths: &[String]) -> Vec<u8> {
    paths.join("\0").into_bytes()
}

// ---------------------------------------------------------------- queries

pub fn version() -> Result<String> {
    let home = std::env::temp_dir();
    Ok(out(&home, &["--version"])?.trim().to_string())
}

pub fn resolve_repo(dir: &Path) -> Result<PathBuf> {
    if !dir.exists() {
        bail!("La carpeta no existe.");
    }
    let dir = if dir.is_dir() { dir.to_path_buf() } else { dir.parent().unwrap_or(dir).to_path_buf() };
    let r = run(&dir, &["rev-parse", "--show-toplevel"], &Opts::default())?;
    if !r.ok() {
        bail!("La carpeta seleccionada no es un repositorio Git (o es un repositorio \"bare\").");
    }
    let top = r.text().trim().to_string();
    Ok(PathBuf::from(dunce(&top)))
}

/// Git prints forward slashes on Windows; turn them into native separators.
fn dunce(p: &str) -> String {
    if cfg!(windows) {
        p.replace('/', "\\")
    } else {
        p.to_string()
    }
}

pub fn git_dir(repo: &Path) -> Result<PathBuf> {
    let key = format!("gitdir|{}", repo.display());
    let d = cached(key, || Ok(out(repo, &["rev-parse", "--absolute-git-dir"])?.trim().to_string()))?;
    Ok(PathBuf::from(dunce(&d)))
}

pub fn has_head(repo: &Path) -> bool {
    run(repo, &["rev-parse", "-q", "--verify", "HEAD"], &Opts::default()).map(|r| r.ok()).unwrap_or(false)
}

pub fn parse_status(raw: &str) -> Status {
    let mut res = Status::default();
    let toks: Vec<&str> = raw.split('\0').collect();
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        i += 1;
        if t.is_empty() {
            continue;
        }
        if let Some(h) = t.strip_prefix("# ") {
            let (key, val) = h.split_once(' ').unwrap_or((h, ""));
            match key {
                "branch.oid" => res.oid = (val != "(initial)").then(|| val.to_string()),
                "branch.head" => res.branch = (val != "(detached)").then(|| val.to_string()),
                "branch.upstream" => res.upstream = Some(val.to_string()),
                "branch.ab" => {
                    let mut it = val.split(' ');
                    if let (Some(a), Some(b)) = (it.next(), it.next()) {
                        res.ahead = a.trim_start_matches('+').parse().unwrap_or(0);
                        res.behind = b.trim_start_matches('-').parse().unwrap_or(0);
                    }
                }
                _ => {}
            }
            continue;
        }
        let kind = t.as_bytes()[0];
        match kind {
            b'1' | b'2' => {
                let parts = split_n(t, if kind == b'1' { 8 } else { 9 });
                let xy = parts.get(1).copied().unwrap_or("..");
                let file = parts.last().copied().unwrap_or("");
                let orig = if kind == b'2' {
                    let o = toks.get(i).copied();
                    i += 1;
                    o
                } else {
                    None
                };
                let mut chars = xy.chars();
                let x = chars.next().unwrap_or('.');
                let y = chars.next().unwrap_or('.');
                let orig_for = |c: char| if c == 'R' || c == 'C' { orig } else { None };
                if x != '.' {
                    res.staged.push(FileChange::new(file, &x.to_string(), orig_for(x)));
                }
                if y != '.' {
                    res.unstaged.push(FileChange::new(file, &y.to_string(), orig_for(y)));
                }
            }
            b'u' => {
                let parts = split_n(t, 10);
                if parts.len() == 11 {
                    res.conflicted.push(FileChange::new(parts[10], parts[1], None));
                }
            }
            b'?' => res.unstaged.push(FileChange::new(&t[2..], "?", None)),
            _ => {}
        }
    }
    res.staged.sort_by(|a, b| a.path.cmp(&b.path));
    res.unstaged.sort_by(|a, b| a.path.cmp(&b.path));
    res.conflicted.sort_by(|a, b| a.path.cmp(&b.path));
    res
}

pub fn status(repo: &Path) -> Result<Status> {
    let r = git(repo, &["status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all"], &Opts::default())?;
    let dir = git_dir(repo)?;
    let mut res = parse_status(&r.text());
    let ex = |p: &str| dir.join(p).exists();
    res.state = if ex("rebase-merge") || ex("rebase-apply") {
        Some(RepoState::Rebase)
    } else if ex("MERGE_HEAD") {
        Some(RepoState::Merge)
    } else if ex("CHERRY_PICK_HEAD") {
        Some(RepoState::CherryPick)
    } else if ex("REVERT_HEAD") {
        Some(RepoState::Revert)
    } else {
        None
    };
    if matches!(res.state, Some(s) if s != RepoState::Rebase) {
        if let Ok(msg) = fs::read_to_string(dir.join("MERGE_MSG")) {
            res.merge_msg = msg.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>().join("\n").trim().to_string();
        }
    }
    Ok(res)
}

fn parse_track(track: &str) -> (u32, u32, bool) {
    let num = |key: &str| {
        track.find(key).and_then(|i| {
            track[i + key.len()..].split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse().ok())
        })
    };
    (num("ahead ").unwrap_or(0), num("behind ").unwrap_or(0), track == "gone")
}

pub fn refs(repo: &Path) -> Result<Refs> {
    let fmt = [
        "%(refname)", "%(objectname)", "%(*objectname)", "%(upstream:short)", "%(upstream:track,nobracket)",
        "%(HEAD)", "%(upstream:remotename)", "%(upstream:remoteref)",
    ]
    .join("%00");
    let remotes_out = out(repo, &["remote"])?;
    let refs_out = out(repo, &["for-each-ref", &format!("--format={fmt}"), "refs/heads", "refs/remotes", "refs/tags"])?;
    let stash_r = run(repo, &["stash", "list", "--format=%gd%x00%H%x00%gs"], &Opts::default())?;
    let head_r = run(repo, &["rev-parse", "-q", "--verify", "HEAD"], &Opts::default())?;
    let sym_r = run(repo, &["symbolic-ref", "-q", "HEAD"], &Opts::default())?;

    let remotes: Vec<String> = remotes_out.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let mut by_length = remotes.clone();
    by_length.sort_by_key(|r| std::cmp::Reverse(r.len()));
    let mut res = Refs { remotes, ..Default::default() };
    for line in refs_out.lines() {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\0').collect();
        if f.len() < 8 {
            continue;
        }
        let (refname, oid, peeled) = (f[0], f[1], f[2]);
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            let (ahead, behind, gone) = parse_track(f[4]);
            let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
            res.local.push(LocalBranch {
                name: name.into(),
                hash: oid.into(),
                upstream: some(f[3]),
                upstream_remote: some(f[6]),
                upstream_branch: some(f[7].strip_prefix("refs/heads/").unwrap_or(f[7])),
                current: f[5] == "*",
                ahead,
                behind,
                gone,
            });
        } else if let Some(rest) = refname.strip_prefix("refs/remotes/") {
            if rest == "HEAD" || rest.ends_with("/HEAD") {
                continue;
            }
            let rn = by_length
                .iter()
                .find(|r| rest.starts_with(&format!("{r}/")))
                .cloned()
                .unwrap_or_else(|| rest.split('/').next().unwrap_or("").to_string());
            let branch = rest.get(rn.len() + 1..).unwrap_or("").to_string();
            res.remote.push(RemoteBranch { remote: rn, branch, name: rest.into(), hash: oid.into() });
        } else if let Some(name) = refname.strip_prefix("refs/tags/") {
            res.tags.push(Tag { name: name.into(), hash: if peeled.is_empty() { oid.into() } else { peeled.into() } });
        }
    }
    res.tags.sort_by(|a, b| natural_cmp(&b.name, &a.name));
    let stash_out = if stash_r.ok() { stash_r.text() } else { String::new() };
    res.stashes = stash_out
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut p = l.split('\0');
            Stash {
                reference: p.next().unwrap_or("").into(),
                hash: p.next().unwrap_or("").into(),
                message: p.next().unwrap_or("").into(),
            }
        })
        .collect();
    res.head = head_r.ok().then(|| head_r.text().trim().to_string());
    res.current_branch =
        sym_r.ok().then(|| sym_r.text().trim().trim_start_matches("refs/heads/").to_string());
    res.sig = format!(
        "{refs_out}\n|{stash_out}\n|{}\n|{}",
        res.head.clone().unwrap_or_default(),
        res.current_branch.clone().unwrap_or_default()
    );
    Ok(res)
}

/// Compares strings treating runs of digits as numbers (v1.10 > v1.9).
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    nb.push(c);
                    bi.next();
                }
                let o = na.trim_start_matches('0').len().cmp(&nb.trim_start_matches('0').len()).then(na.cmp(&nb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

pub const LOG_FORMAT: &str = "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%s%x1e";

pub fn parse_log(text: &str) -> Vec<Commit> {
    text.split('\x1e')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches(['\r', '\n']);
            if rec.is_empty() {
                return None;
            }
            let f: Vec<&str> = rec.split('\x1f').collect();
            if f.len() < 6 {
                return None;
            }
            Some(Commit {
                hash: f[0].into(),
                parents: f[1].split(' ').filter(|s| !s.is_empty()).map(Into::into).collect(),
                author: f[2].into(),
                email: f[3].into(),
                time: f[4].parse().unwrap_or(0),
                subject: f[5].into(),
            })
        })
        .collect()
}

pub fn log(repo: &Path, max: usize, all: bool) -> Result<Vec<Commit>> {
    let head = has_head(repo);
    let count = max.clamp(1, 1_000_000);
    let mut args: Vec<String> = vec!["log".into(), "--date-order".into(), format!("--max-count={count}"), LOG_FORMAT.into()];
    if all {
        let any = out(repo, &["for-each-ref", "--count=1", "--format=%(refname)", "refs/heads", "refs/remotes", "refs/tags"])?;
        if any.trim().is_empty() && !head {
            return Ok(vec![]);
        }
        args.extend(["--branches".into(), "--remotes".into(), "--tags".into()]);
        if head {
            args.push("HEAD".into());
        }
    } else {
        if !head {
            return Ok(vec![]);
        }
        args.push("HEAD".into());
    }
    args.push("--".into());
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(parse_log(&out(repo, &a)?))
}

pub fn empty_tree(repo: &Path) -> Result<String> {
    cached(format!("empty|{}", repo.display()), || {
        Ok(out_opts(repo, &["hash-object", "-t", "tree", "-w", "--stdin"], &Opts::input(b""))?.trim().to_string())
    })
}

pub fn parse_name_status(text: &str) -> Vec<FileChange> {
    let t: Vec<&str> = text.split('\0').collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let s = t[i];
        if s.is_empty() {
            i += 1;
            continue;
        }
        let code = &s[..1];
        if (code == "R" || code == "C") && i + 2 < t.len() {
            files.push(FileChange::new(t[i + 2], code, Some(t[i + 1])));
            i += 3;
        } else if i + 1 < t.len() {
            files.push(FileChange::new(t[i + 1], code, None));
            i += 2;
        } else {
            break;
        }
    }
    files
}

pub fn commit_details(repo: &Path, hash: &str) -> Result<CommitDetails> {
    let meta = out(
        repo,
        &["show", "-s", "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%cn%x00%ce%x00%ct%x00%G?%x00%GS%x00%GK%x00%B", hash, "--"],
    )?;
    let p: Vec<&str> = meta.split('\0').collect();
    if p.len() < 12 {
        bail!("No se pudo leer el commit {hash}.");
    }
    let parents: Vec<String> = p[1].split_whitespace().map(Into::into).collect();
    let base = match parents.first() {
        Some(b) => b.clone(),
        None => empty_tree(repo)?,
    };
    let mut args = vec!["diff"];
    args.extend(DIFF_ARGS);
    args.extend(["-z", "-M", "--name-status", &base, p[0], "--"]);
    let ns = out(repo, &args)?;
    let sig_code = p[8].chars().next().unwrap_or('N');
    Ok(CommitDetails {
        hash: p[0].into(),
        parents,
        author: p[2].into(),
        email: p[3].into(),
        time: p[4].parse().unwrap_or(0),
        committer: p[5].into(),
        committer_email: p[6].into(),
        commit_time: p[7].parse().unwrap_or(0),
        signature: Some(Signature { code: sig_code, signer: p[9].into(), key: p[10].into() }),
        message: p[11..].join("\0").trim_end().to_string(),
        base,
        files: parse_name_status(&ns),
    })
}

fn diff_result(r: runner::Output) -> DiffText {
    let text = r.text();
    let lossy = text.contains('\u{FFFD}');
    DiffText { text, truncated: r.truncated, lossy }
}

pub fn commit_file_diff(repo: &Path, base: &str, hash: &str, file: &FileChange, o: &DiffOptions) -> Result<DiffText> {
    let extra = o.args();
    let mut args: Vec<&str> = vec!["diff"];
    args.extend(DIFF_ARGS);
    args.push("-M");
    args.extend(extra.iter().map(String::as_str));
    args.extend([base, hash, "--"]);
    let paths = file.paths();
    args.extend(paths.iter().map(String::as_str));
    let r = git(repo, &args, &Opts { max_buffer: MAX_DIFF_BYTES, ..Opts::literal() })?;
    Ok(diff_result(r))
}

/// Diff between two arbitrary revisions (compare view). `file` restricts it to one path.
pub fn range_diff(repo: &Path, from: &str, to: &str, file: Option<&FileChange>, o: &DiffOptions) -> Result<DiffText> {
    let extra = o.args();
    let mut args: Vec<&str> = vec!["diff"];
    args.extend(DIFF_ARGS);
    args.push("-M");
    args.extend(extra.iter().map(String::as_str));
    args.extend([from, to, "--"]);
    let paths = file.map(|f| f.paths()).unwrap_or_default();
    args.extend(paths.iter().map(String::as_str));
    let r = git(repo, &args, &Opts { max_buffer: MAX_DIFF_BYTES, ..Opts::literal() })?;
    Ok(diff_result(r))
}

pub fn range_files(repo: &Path, from: &str, to: &str) -> Result<Vec<FileChange>> {
    let mut args = vec!["diff"];
    args.extend(DIFF_ARGS);
    args.extend(["-z", "-M", "--name-status", from, to, "--"]);
    Ok(parse_name_status(&out(repo, &args)?))
}

pub fn working_diff(repo: &Path, file: &FileChange, staged: bool, o: &DiffOptions) -> Result<DiffText> {
    let extra = o.args();
    let mut args: Vec<&str> = vec!["diff"];
    args.extend(DIFF_ARGS);
    args.push("-M");
    args.extend(extra.iter().map(String::as_str));
    if staged {
        args.push("--cached");
    }
    args.push("--");
    let paths = file.paths();
    args.extend(paths.iter().map(String::as_str));
    let r = git(repo, &args, &Opts { max_buffer: MAX_DIFF_BYTES, ..Opts::literal() })?;
    Ok(diff_result(r))
}

pub fn read_working_file(repo: &Path, rel: &str, max: usize) -> Result<WorkingFile> {
    let full = safe_join(repo, rel)?;
    let meta = fs::metadata(&full)?;
    if meta.is_dir() {
        return Ok(WorkingFile { directory: true, ..Default::default() });
    }
    use std::io::Read;
    let mut f = fs::File::open(&full)?;
    let mut buf = Vec::with_capacity(max.min(meta.len() as usize));
    f.by_ref().take(max as u64).read_to_end(&mut buf)?;
    let binary = buf[..buf.len().min(8000)].contains(&0);
    Ok(WorkingFile {
        directory: false,
        binary,
        size: meta.len(),
        truncated: meta.len() as usize > max,
        text: if binary { String::new() } else { String::from_utf8_lossy(&buf).into_owned() },
    })
}

/// Raw bytes of `rev:path` (for image previews). `rev` may be ":" + path for the index.
pub fn read_blob(repo: &Path, rev: &str, path: &str, max: usize) -> Result<Vec<u8>> {
    let spec = format!("{rev}:{path}");
    let r = git(repo, &["cat-file", "blob", &spec], &Opts { max_buffer: max, ..Default::default() })?;
    Ok(r.stdout)
}

fn config_get(repo: &Path, args: &[&str]) -> String {
    let mut a = vec!["config"];
    a.extend(args);
    run(repo, &a, &Opts::default()).ok().filter(|r| r.ok()).map(|r| r.text().trim().to_string()).unwrap_or_default()
}

pub fn config_value(repo: &Path, key: &str) -> String {
    config_get(repo, &["--get", key])
}

pub fn set_config(repo: &Path, key: &str, value: &str, global: bool) -> Result<()> {
    let scope = if global { "--global" } else { "--local" };
    if value.trim().is_empty() {
        git(repo, &["config", scope, "--unset", key], &Opts { allow_fail: true, ..Default::default() })?;
    } else {
        git(repo, &["config", scope, key, value.trim()], &Opts::default())?;
    }
    Ok(())
}

pub fn identity(repo: &Path) -> Identity {
    Identity {
        name: config_get(repo, &["--get", "user.name"]),
        email: config_get(repo, &["--get", "user.email"]),
        global_name: config_get(repo, &["--global", "--get", "user.name"]),
        global_email: config_get(repo, &["--global", "--get", "user.email"]),
    }
}

pub fn remotes_info(repo: &Path) -> Result<Vec<RemoteInfo>> {
    let text = out(repo, &["remote", "-v"])?;
    let mut list: Vec<RemoteInfo> = Vec::new();
    for line in text.lines() {
        let Some((name, rest)) = line.split_once('\t') else { continue };
        let (url, kind) = match rest.rsplit_once(" (") {
            Some((u, k)) => (u, k.trim_end_matches(')')),
            None => continue,
        };
        match list.iter_mut().find(|r| r.name == name) {
            Some(r) if kind == "fetch" => r.url = url.into(),
            Some(_) => {}
            None => list.push(RemoteInfo { name: name.into(), url: url.into() }),
        }
    }
    Ok(list)
}

// ---------------------------------------------------------------- mutations

pub fn init(dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    git(dir, &["init"], &Opts::default())?;
    resolve_repo(dir)
}

pub fn clone(url: &str, dest: &Path, op: Option<&OpHandle>) -> Result<PathBuf> {
    if url.trim().is_empty() || dest.as_os_str().is_empty() {
        bail!("Indica la URL y la carpeta de destino.");
    }
    if dest.exists() && fs::read_dir(dest)?.next().is_some() {
        bail!("La carpeta de destino ya existe y no está vacía.");
    }
    let parent = dest.parent().ok_or_else(|| anyhow!("Carpeta de destino no válida."))?;
    fs::create_dir_all(parent)?;
    let target = dest.to_string_lossy();
    git(parent, &["clone", "--progress", "--recurse-submodules", "--", url, &target], &Opts::op(op))?;
    resolve_repo(dest)
}

pub fn stage(repo: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let input = pathspec_input(paths);
    let mut args = vec!["add", "-A"];
    args.extend(FROM_STDIN);
    git(repo, &args, &Opts { input: Some(&input), ..Opts::literal() })?;
    Ok(())
}

pub fn stage_all(repo: &Path) -> Result<()> {
    git(repo, &["add", "-A"], &Opts::default())?;
    Ok(())
}

pub fn unstage(repo: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let input = pathspec_input(paths);
    let mut args = if has_head(repo) { vec!["reset", "-q", "HEAD"] } else { vec!["rm", "--cached", "-r", "-q", "--ignore-unmatch"] };
    args.extend(FROM_STDIN);
    git(repo, &args, &Opts { input: Some(&input), ..Opts::literal() })?;
    Ok(())
}

pub fn unstage_all(repo: &Path) -> Result<()> {
    if has_head(repo) {
        git(repo, &["reset", "-q"], &Opts::default())?;
    } else {
        git(repo, &["rm", "--cached", "-r", "-q", "--ignore-unmatch", "."], &Opts::default())?;
    }
    Ok(())
}

/// Discards tracked changes and moves untracked files to the trash.
pub fn discard(repo: &Path, tracked: &[String], untracked: &[String]) -> Result<()> {
    if !tracked.is_empty() {
        let input = pathspec_input(tracked);
        let mut args = vec!["checkout", "-q"];
        args.extend(FROM_STDIN);
        git(repo, &args, &Opts { input: Some(&input), ..Opts::literal() })?;
    }
    for rel in untracked {
        let full = safe_join(repo, rel)?;
        trash::delete(&full).map_err(|e| anyhow!("No se pudo mover a la papelera {rel}: {e}"))?;
    }
    Ok(())
}

pub fn discard_all(repo: &Path, include_untracked: bool) -> Result<()> {
    if has_head(repo) {
        git(repo, &["reset", "--hard", "-q"], &Opts::default())?;
    } else {
        unstage_all(repo)?;
    }
    if include_untracked {
        git(repo, &["clean", "-fd", "-q"], &Opts::default())?;
    }
    Ok(())
}

pub fn apply_patch(repo: &Path, patch: &str, cached: bool, reverse: bool) -> Result<()> {
    let mut args = vec!["apply", "--recount", "--whitespace=nowarn"];
    if cached {
        args.push("--cached");
    }
    if reverse {
        args.push("--reverse");
    }
    args.push("-");
    let mut data = patch.to_string();
    if !data.ends_with('\n') {
        data.push('\n');
    }
    git(repo, &args, &Opts::input(data.as_bytes()))?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CommitFlags {
    pub amend: bool,
    pub sign: bool,
}

pub fn commit(repo: &Path, message: &str, flags: CommitFlags) -> Result<String> {
    if message.trim().is_empty() {
        bail!("El mensaje del commit no puede estar vacío.");
    }
    let mut args = vec!["commit", "-F", "-"];
    if flags.amend {
        args.push("--amend");
    }
    if flags.sign {
        args.push("-S");
    }
    git(repo, &args, &Opts::input(message.as_bytes()))?;
    Ok(out(repo, &["rev-parse", "HEAD"])?.trim().to_string())
}

pub fn last_commit_message(repo: &Path) -> Result<String> {
    if !has_head(repo) {
        return Ok(String::new());
    }
    Ok(out(repo, &["log", "-1", "--format=%B"])?.trim_end().to_string())
}

pub fn unmerged_paths(repo: &Path) -> Vec<String> {
    run(repo, &["diff", "--name-only", "-z", "--diff-filter=U"], &Opts::default())
        .ok()
        .filter(|r| r.ok())
        .map(|r| r.text().split('\0').filter(|s| !s.is_empty()).map(Into::into).collect())
        .unwrap_or_default()
}

/// Commands that may stop with conflicts resolve with `conflicts: true` instead of failing.
pub fn conflict_aware(repo: &Path, args: &[&str], opts: &Opts) -> Result<OpResult> {
    let r = run(repo, args, opts)?;
    if r.cancelled {
        return Err(failure(&r, args));
    }
    let output = clean_output(&format!("{}\n{}", r.text(), r.stderr));
    if r.ok() {
        return Ok(OpResult { conflicts: false, output });
    }
    if !unmerged_paths(repo).is_empty() {
        return Ok(OpResult { conflicts: true, output });
    }
    Err(failure(&r, args))
}

/// Cleans a branch name as it is typed: spaces become `_`, accents are dropped (`canción` →
/// `cancion`) and anything else that is not a letter, digit, `-`, `_`, `.` or `/` is removed,
/// as are repeated `_`, `/` and `.`. `/` is kept for folders like `feature/`.
pub fn branch_name_input(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let ch = match ch {
            c if c.is_whitespace() => '_',
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
            'Á' | 'À' | 'Ä' | 'Â' | 'Ã' | 'Å' => 'A',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'É' | 'È' | 'Ë' | 'Ê' => 'E',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'Í' | 'Ì' | 'Ï' | 'Î' => 'I',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'Ó' | 'Ò' | 'Ö' | 'Ô' | 'Õ' => 'O',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'Ú' | 'Ù' | 'Ü' | 'Û' => 'U',
            'ñ' => 'n',
            'Ñ' => 'N',
            'ç' => 'c',
            'Ç' => 'C',
            c if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/') => c,
            _ => continue,
        };
        if matches!(ch, '_' | '/' | '.') && out.ends_with(ch) {
            continue;
        }
        out.push(ch);
    }
    out
}

/// Final form of a typed branch name: [`branch_name_input`] plus what can only be fixed once the
/// name is complete. Each `/` folder loses leading and trailing `_`, `-` and `.` and a `.lock`
/// ending, and empty folders are dropped. Returns an empty string if nothing valid is left.
pub fn branch_name(s: &str) -> String {
    let trim = |p: &str| p.trim_matches(|c| matches!(c, '_' | '-' | '.')).to_string();
    branch_name_input(s)
        .split('/')
        .map(|part| {
            let mut p = trim(part);
            while let Some(rest) = p.strip_suffix(".lock") {
                p = trim(rest);
            }
            p
        })
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn check_ref_name(repo: &Path, is_branch: bool, name: &str) -> Result<()> {
    let full = if is_branch { name.to_string() } else { format!("refs/tags/{name}") };
    let args: Vec<&str> = if is_branch { vec!["check-ref-format", "--branch", &full] } else { vec!["check-ref-format", &full] };
    let r = run(repo, &args, &Opts::default())?;
    if !r.ok() || name.is_empty() || name.starts_with('-') {
        bail!("\"{name}\" no es un nombre válido.");
    }
    Ok(())
}

pub fn checkout(repo: &Path, reference: &str) -> Result<()> {
    git(repo, &["checkout", reference, "--"], &Opts::default())?;
    Ok(())
}

pub fn checkout_remote(repo: &Path, remote_ref: &str, local_name: &str) -> Result<()> {
    check_ref_name(repo, true, local_name)?;
    git(repo, &["checkout", "-b", local_name, "--track", remote_ref, "--"], &Opts::default())?;
    Ok(())
}

pub fn checkout_detached(repo: &Path, hash: &str) -> Result<()> {
    git(repo, &["checkout", "--detach", hash, "--"], &Opts::default())?;
    Ok(())
}

pub fn create_branch(repo: &Path, name: &str, start: Option<&str>, do_checkout: bool) -> Result<()> {
    check_ref_name(repo, true, name)?;
    let mut args = if do_checkout { vec!["checkout", "-b", name] } else { vec!["branch", name] };
    if let Some(s) = start {
        args.push(s);
    }
    if do_checkout {
        args.push("--");
    }
    git(repo, &args, &Opts::default())?;
    Ok(())
}

pub fn rename_branch(repo: &Path, old: &str, new: &str) -> Result<()> {
    check_ref_name(repo, true, new)?;
    git(repo, &["branch", "-m", old, new], &Opts::default())?;
    Ok(())
}

pub fn delete_branch(repo: &Path, name: &str, force: bool) -> Result<()> {
    git(repo, &["branch", if force { "-D" } else { "-d" }, name], &Opts::default())?;
    Ok(())
}

pub fn merge(repo: &Path, reference: &str, no_ff: bool) -> Result<OpResult> {
    let mut args = vec!["merge", "--no-edit"];
    if no_ff {
        args.push("--no-ff");
    }
    args.push(reference);
    conflict_aware(repo, &args, &Opts::default())
}

pub fn rebase(repo: &Path, onto: &str) -> Result<OpResult> {
    conflict_aware(repo, &["rebase", onto], &Opts::default())
}

pub fn cherry_pick(repo: &Path, hash: &str, is_merge: bool) -> Result<OpResult> {
    let args: Vec<&str> = if is_merge { vec!["cherry-pick", "-m", "1", hash] } else { vec!["cherry-pick", hash] };
    conflict_aware(repo, &args, &Opts::default())
}

pub fn revert(repo: &Path, hash: &str, is_merge: bool) -> Result<OpResult> {
    let args: Vec<&str> =
        if is_merge { vec!["revert", "--no-edit", "-m", "1", hash] } else { vec!["revert", "--no-edit", hash] };
    conflict_aware(repo, &args, &Opts::default())
}

pub fn reset(repo: &Path, target: &str, mode: ResetMode) -> Result<()> {
    git(repo, &["reset", mode.flag(), target, "--"], &Opts::default())?;
    Ok(())
}

pub fn abort_operation(repo: &Path, kind: RepoState) -> Result<()> {
    git(repo, &[kind.command(), "--abort"], &Opts::default())?;
    Ok(())
}

pub fn continue_operation(repo: &Path, kind: RepoState) -> Result<OpResult> {
    if kind == RepoState::Merge {
        return conflict_aware(repo, &["commit", "--no-edit"], &Opts::default());
    }
    conflict_aware(repo, &[kind.command(), "--continue"], &Opts::default())
}

pub fn skip_rebase(repo: &Path) -> Result<OpResult> {
    conflict_aware(repo, &["rebase", "--skip"], &Opts::default())
}

/// `ours = true` keeps the current branch's version.
pub fn resolve_conflict(repo: &Path, file: &str, ours: bool) -> Result<()> {
    git(repo, &["checkout", if ours { "--ours" } else { "--theirs" }, "--", file], &Opts::literal())?;
    stage(repo, &[file.to_string()])
}

pub fn create_tag(repo: &Path, name: &str, target: Option<&str>, message: &str, sign: bool) -> Result<()> {
    check_ref_name(repo, false, name)?;
    let annotated = !message.trim().is_empty() || sign;
    let mut args = if annotated { vec!["tag", if sign { "-s" } else { "-a" }, name, "-F", "-"] } else { vec!["tag", name] };
    if let Some(t) = target {
        args.push(t);
    }
    let msg = if message.trim().is_empty() { name.to_string() } else { message.to_string() };
    let opts = if annotated { Opts::input(msg.as_bytes()) } else { Opts::default() };
    git(repo, &args, &opts)?;
    Ok(())
}

pub fn delete_tag(repo: &Path, name: &str) -> Result<()> {
    git(repo, &["tag", "-d", name], &Opts::default())?;
    Ok(())
}

pub fn fetch(repo: &Path, remote: Option<&str>, prune: bool, op: Option<&OpHandle>) -> Result<()> {
    let mut args = vec!["fetch", "--progress"];
    if prune {
        args.push("--prune");
    }
    args.push(remote.unwrap_or("--all"));
    git(repo, &args, &Opts::op(op))?;
    Ok(())
}

pub fn pull(repo: &Path, remote: Option<&str>, branch: Option<&str>, use_rebase: bool, op: Option<&OpHandle>) -> Result<OpResult> {
    let mut args = vec!["pull", "--progress", if use_rebase { "--rebase" } else { "--no-rebase" }];
    if let Some(r) = remote {
        args.push(r);
        if let Some(b) = branch {
            args.push(b);
        }
    }
    conflict_aware(repo, &args, &Opts::op(op))
}

/// Updates a branch that is not checked out from `remote/branch` without switching to it.
/// Only a fast-forward is possible: a diverged branch is rejected and left untouched.
pub fn fast_forward(repo: &Path, remote: &str, branch: &str, local: &str, op: Option<&OpHandle>) -> Result<()> {
    let spec = format!("refs/heads/{branch}:refs/heads/{local}");
    git(repo, &["fetch", "--progress", remote, &spec], &Opts::op(op)).map_err(|e| {
        if e.to_string().contains("non-fast-forward") {
            anyhow::anyhow!("{local} tiene commits que no están en {remote}/{branch}: cámbiate a ella para hacer pull.")
        } else {
            e
        }
    })?;
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct PushArgs {
    pub remote: String,
    pub branch: String,
    pub remote_branch: Option<String>,
    pub set_upstream: bool,
    pub force: bool,
    pub tags: bool,
}

pub fn push(repo: &Path, a: &PushArgs, op: Option<&OpHandle>) -> Result<()> {
    if a.remote.is_empty() || a.branch.is_empty() {
        bail!("Selecciona el remoto y la rama.");
    }
    let mut args = vec!["push", "--progress"];
    if a.set_upstream {
        args.push("--set-upstream");
    }
    if a.force {
        args.push("--force-with-lease");
    }
    if a.tags {
        args.push("--follow-tags");
    }
    let spec = format!("refs/heads/{}:refs/heads/{}", a.branch, a.remote_branch.as_deref().unwrap_or(&a.branch));
    args.push(&a.remote);
    args.push(&spec);
    git(repo, &args, &Opts::op(op))?;
    Ok(())
}

pub fn delete_remote_branch(repo: &Path, remote: &str, branch: &str, op: Option<&OpHandle>) -> Result<()> {
    let r = format!("refs/heads/{branch}");
    git(repo, &["push", "--progress", remote, "--delete", &r], &Opts::op(op))?;
    Ok(())
}

pub fn push_tag(repo: &Path, remote: &str, name: &str, op: Option<&OpHandle>) -> Result<()> {
    let r = format!("refs/tags/{name}");
    git(repo, &["push", "--progress", remote, &r], &Opts::op(op))?;
    Ok(())
}

pub fn delete_remote_tag(repo: &Path, remote: &str, name: &str, op: Option<&OpHandle>) -> Result<()> {
    let r = format!("refs/tags/{name}");
    git(repo, &["push", "--progress", remote, "--delete", &r], &Opts::op(op))?;
    Ok(())
}

pub fn stash_save(repo: &Path, message: &str, untracked: bool, keep_index: bool) -> Result<()> {
    let mut args = vec!["stash", "push"];
    if untracked {
        args.push("--include-untracked");
    }
    if keep_index {
        args.push("--keep-index");
    }
    if !message.is_empty() {
        args.extend(["-m", message]);
    }
    git(repo, &args, &Opts::default())?;
    Ok(())
}

pub fn stash_apply(repo: &Path, reference: &str, pop: bool) -> Result<OpResult> {
    conflict_aware(repo, &["stash", if pop { "pop" } else { "apply" }, reference], &Opts::default())
}

pub fn stash_drop(repo: &Path, reference: &str) -> Result<()> {
    git(repo, &["stash", "drop", reference], &Opts::default())?;
    Ok(())
}

pub fn add_remote(repo: &Path, name: &str, url: &str) -> Result<()> {
    if name.trim().is_empty() || url.trim().is_empty() {
        bail!("Indica nombre y URL del remoto.");
    }
    git(repo, &["remote", "add", "--", name.trim(), url.trim()], &Opts::default())?;
    Ok(())
}

pub fn remove_remote(repo: &Path, name: &str) -> Result<()> {
    git(repo, &["remote", "remove", name], &Opts::default())?;
    Ok(())
}

pub fn rename_remote(repo: &Path, old: &str, new: &str) -> Result<()> {
    if new.trim().is_empty() {
        bail!("Indica el nuevo nombre del remoto.");
    }
    git(repo, &["remote", "rename", old, new.trim()], &Opts::default())?;
    Ok(())
}

pub fn set_remote_url(repo: &Path, name: &str, url: &str) -> Result<()> {
    if url.trim().is_empty() {
        bail!("Indica la URL del remoto.");
    }
    git(repo, &["remote", "set-url", name, url.trim()], &Opts::default())?;
    Ok(())
}

pub fn set_identity(repo: &Path, name: &str, email: &str, global: bool) -> Result<()> {
    set_config(repo, "user.name", name, global)?;
    set_config(repo, "user.email", email, global)
}

fn escape_ignore(rel: &str) -> String {
    let mut s = String::new();
    for c in rel.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\' | '!' | '#') {
            s.push('\\');
        }
        s.push(c);
    }
    if s.ends_with(' ') {
        s.pop();
        s.push_str("\\ ");
    }
    s
}

fn add_to_gitignore(repo: &Path, pattern: &str) -> Result<()> {
    let file = repo.join(".gitignore");
    let cur = fs::read_to_string(&file).unwrap_or_default();
    if cur.lines().any(|l| l == pattern) {
        return Ok(());
    }
    let nl = if cur.contains("\r\n") { "\r\n" } else { "\n" };
    let prefix = if !cur.is_empty() && !cur.ends_with('\n') { nl } else { "" };
    use std::io::Write;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(&file)?;
    write!(f, "{prefix}{pattern}{nl}")?;
    Ok(())
}

pub fn ignore_paths(repo: &Path, paths: &[String], by_extension: bool) -> Result<()> {
    for rel in paths {
        let ext = Path::new(rel).extension().map(|e| format!(".{}", e.to_string_lossy()));
        let pattern = match ext {
            Some(e) if by_extension => format!("*{}", escape_ignore(&e)),
            _ => format!("/{}", escape_ignore(&rel.replace('\\', "/"))),
        };
        add_to_gitignore(repo, &pattern)?;
    }
    Ok(())
}
