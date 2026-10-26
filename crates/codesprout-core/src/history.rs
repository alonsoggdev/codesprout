//! File history, blame, server-side search and reflog.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::model::Commit;
use crate::repo::{parse_log, LOG_FORMAT};
use crate::runner::{git, out, out_opts, Opts};

/// One commit in the history of a file, with the path the file had in that commit.
#[derive(Clone, Debug, Default)]
pub struct FileRevision {
    pub commit: Commit,
    pub path: String,
    pub status: String,
}

/// `git log --follow` for one file, keeping track of renames.
pub fn file_history(repo: &Path, path: &str, max: usize) -> Result<Vec<FileRevision>> {
    let count = format!("--max-count={}", max.clamp(1, 100_000));
    let text = out_opts(
        repo,
        &["log", "--follow", "-M", &count, "--name-status", "-z", LOG_FORMAT, "--", path],
        &Opts::literal(),
    )?;
    // Each record: header \x1e, then NUL-separated name-status entries until the next header.
    let mut revs = Vec::new();
    let mut pending: Option<Commit> = None;
    let mut fields: Vec<String> = Vec::new();
    let flush = |pending: &mut Option<Commit>, fields: &mut Vec<String>, revs: &mut Vec<FileRevision>| {
        if let Some(c) = pending.take() {
            let f: Vec<&str> = fields.iter().map(|s| s.trim_matches(['\n', '\r'])).filter(|s| !s.is_empty()).collect();
            let (status, path) = match f.first().map(|s| &s[..1]) {
                Some("R") | Some("C") if f.len() >= 3 => (f[0][..1].to_string(), f[2].to_string()),
                Some(_) if f.len() >= 2 => (f[0][..1].to_string(), f[1].to_string()),
                _ => (String::new(), String::new()),
            };
            revs.push(FileRevision { commit: c, path, status });
        }
        fields.clear();
    };
    for chunk in text.split('\x1e') {
        // The chunk begins with the name-status of the previous commit, then the next header.
        let mut parts: Vec<&str> = chunk.split('\0').collect();
        let header = parts.pop().unwrap_or("");
        fields.extend(parts.iter().map(|s| s.to_string()));
        flush(&mut pending, &mut fields, &mut revs);
        let header = header.trim_start_matches(['\r', '\n']);
        if !header.is_empty() {
            pending = parse_log(&format!("{header}\x1e")).into_iter().next();
        }
    }
    flush(&mut pending, &mut fields, &mut revs);
    // Fill paths that git left empty (no name-status for merges) with the newest known path.
    let mut last = path.to_string();
    for r in &mut revs {
        if r.path.is_empty() {
            r.path = last.clone();
        } else {
            last = r.path.clone();
        }
    }
    Ok(revs)
}

#[derive(Clone, Debug, Default)]
pub struct BlameCommit {
    pub author: String,
    pub email: String,
    pub time: i64,
    pub summary: String,
}

#[derive(Clone, Debug, Default)]
pub struct BlameLine {
    pub hash: String,
    pub line: u32,
    pub content: String,
}

#[derive(Clone, Debug, Default)]
pub struct Blame {
    pub lines: Vec<BlameLine>,
    pub commits: HashMap<String, BlameCommit>,
}

pub fn parse_blame(text: &str) -> Blame {
    let mut b = Blame::default();
    let mut current = String::new();
    let mut final_line = 0u32;
    for line in text.lines() {
        if let Some(content) = line.strip_prefix('\t') {
            b.lines.push(BlameLine { hash: current.clone(), line: final_line, content: content.to_string() });
            continue;
        }
        let mut it = line.split(' ');
        let first = it.next().unwrap_or("");
        if first.len() == 40 && first.bytes().all(|c| c.is_ascii_hexdigit()) {
            current = first.to_string();
            let _orig = it.next();
            final_line = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            b.commits.entry(current.clone()).or_default();
            continue;
        }
        let rest = line.split_once(' ').map(|x| x.1).unwrap_or("");
        let c = b.commits.entry(current.clone()).or_default();
        match first {
            "author" => c.author = rest.into(),
            "author-mail" => c.email = rest.trim_matches(['<', '>']).into(),
            "author-time" => c.time = rest.parse().unwrap_or(0),
            "summary" => c.summary = rest.into(),
            _ => {}
        }
    }
    b
}

/// Blame of `path` at `rev` (working tree version when `rev` is None).
pub fn blame(repo: &Path, path: &str, rev: Option<&str>) -> Result<Blame> {
    let mut args = vec!["blame", "--porcelain", "-M", "-C"];
    if let Some(r) = rev {
        args.push(r);
    }
    args.extend(["--", path]);
    Ok(parse_blame(&out_opts(repo, &args, &Opts::literal())?))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Message,
    Author,
    /// Commits that add or remove the text (pickaxe, `-S`).
    Content,
    /// Commits whose diff matches a regular expression (`-G`).
    Regex,
    /// Commits that touch a path.
    Path,
}

impl SearchMode {
    pub const ALL: [SearchMode; 5] =
        [SearchMode::Message, SearchMode::Author, SearchMode::Content, SearchMode::Regex, SearchMode::Path];
    pub fn label(self) -> &'static str {
        match self {
            SearchMode::Message => "Mensaje",
            SearchMode::Author => "Autor",
            SearchMode::Content => "Contenido (añadido/quitado)",
            SearchMode::Regex => "Diff (expresión regular)",
            SearchMode::Path => "Ruta de archivo",
        }
    }
}

pub fn search(repo: &Path, query: &str, mode: SearchMode, all: bool, max: usize) -> Result<Vec<Commit>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let count = format!("--max-count={}", max.clamp(1, 100_000));
    let mut args: Vec<String> = vec!["log".into(), "--date-order".into(), count, LOG_FORMAT.into()];
    match mode {
        SearchMode::Message => args.extend(["-i".into(), format!("--grep={q}")]),
        SearchMode::Author => args.extend(["-i".into(), format!("--author={q}")]),
        SearchMode::Content => args.push(format!("-S{q}")),
        SearchMode::Regex => args.push(format!("-G{q}")),
        SearchMode::Path => {}
    }
    if all {
        args.extend(["--branches".into(), "--remotes".into(), "--tags".into()]);
    }
    if crate::repo::has_head(repo) {
        args.push("HEAD".into());
    } else if !all {
        return Ok(vec![]);
    }
    args.push("--".into());
    if mode == SearchMode::Path {
        args.push(q.into());
    }
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let opts = if mode == SearchMode::Path { Opts::default() } else { Opts::literal() };
    Ok(parse_log(&git(repo, &a, &opts)?.text()))
}

#[derive(Clone, Debug, Default)]
pub struct ReflogEntry {
    pub hash: String,
    /// HEAD@{n}
    pub selector: String,
    /// What happened: "checkout: moving from a to b", "commit: …", "reset: …"
    pub action: String,
    pub time: i64,
    pub subject: String,
}

pub fn reflog(repo: &Path, reference: &str, max: usize) -> Result<Vec<ReflogEntry>> {
    if !crate::repo::has_head(repo) {
        return Ok(vec![]);
    }
    let count = format!("--max-count={}", max.clamp(1, 100_000));
    let text = out(repo, &["log", "-g", &count, "--format=%H%x1f%gd%x1f%gs%x1f%ct%x1f%s%x1e", reference, "--"])?;
    Ok(text
        .split('\x1e')
        .filter_map(|r| {
            let r = r.trim_start_matches(['\r', '\n']);
            let f: Vec<&str> = r.split('\x1f').collect();
            (f.len() >= 5).then(|| ReflogEntry {
                hash: f[0].into(),
                selector: f[1].into(),
                action: f[2].into(),
                time: f[3].parse().unwrap_or(0),
                subject: f[4].into(),
            })
        })
        .collect())
}
