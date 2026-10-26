//! Unified-diff parsing, partial patches (hunk / line staging) and intra-line highlighting.

use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub context: String,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct FileDiff {
    pub header: Vec<String>,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
    pub combined: bool,
    pub is_new: bool,
    pub is_deleted: bool,
    pub renamed_from: Option<String>,
    pub mode_change: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Add,
    Del,
    Ctx,
    Meta,
}

#[derive(Clone, Copy, Debug)]
pub struct NumberedLine {
    pub kind: LineKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
}

fn parse_hunk_header(line: &str) -> Option<Hunk> {
    // @@ -a[,b] +c[,d] @@ context
    let rest = line.strip_prefix("@@ -")?;
    let end = rest.find(" @@")?;
    let ranges = &rest[..end];
    let context = rest[end + 3..].trim().to_string();
    let (old, new) = ranges.split_once(" +")?;
    let range = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (os, oc) = range(old)?;
    let (ns, nc) = range(new)?;
    Some(Hunk { header: line.into(), old_start: os, old_count: oc, new_start: ns, new_count: nc, context, lines: vec![] })
}

pub fn parse_diff(text: &str) -> Vec<FileDiff> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut files: Vec<FileDiff> = Vec::new();
    let mut in_hunk = false;
    for line in lines {
        if line.starts_with("diff --git ") || line.starts_with("diff --cc ") || line.starts_with("diff --combined ") {
            files.push(FileDiff {
                header: vec![line.into()],
                combined: !line.starts_with("diff --git "),
                ..Default::default()
            });
            in_hunk = false;
            continue;
        }
        let Some(file) = files.last_mut() else { continue };
        if line.starts_with("@@") {
            if let Some(h) = parse_hunk_header(line) {
                file.hunks.push(h);
                in_hunk = true;
                continue;
            }
            if !in_hunk {
                file.header.push(line.into());
                continue;
            }
        }
        if !in_hunk {
            if line.starts_with("Binary files ") || line == "GIT binary patch" {
                file.binary = true;
            }
            file.header.push(line.into());
            continue;
        }
        file.hunks.last_mut().unwrap().lines.push(line.into());
    }
    for f in &mut files {
        f.is_new = f.header.iter().any(|l| l.starts_with("new file mode"));
        f.is_deleted = f.header.iter().any(|l| l.starts_with("deleted file mode"));
        f.renamed_from = f.header.iter().find_map(|l| l.strip_prefix("rename from ").map(Into::into));
        f.mode_change = f.header.iter().any(|l| l.starts_with("old mode"));
    }
    files
}

pub fn number_hunk(h: &Hunk) -> Vec<NumberedLine> {
    let (mut o, mut n) = (h.old_start, h.new_start);
    h.lines
        .iter()
        .map(|l| match l.as_bytes().first() {
            Some(b'+') => {
                n += 1;
                NumberedLine { kind: LineKind::Add, old: None, new: Some(n - 1) }
            }
            Some(b'-') => {
                o += 1;
                NumberedLine { kind: LineKind::Del, old: Some(o - 1), new: None }
            }
            Some(b'\\') => NumberedLine { kind: LineKind::Meta, old: None, new: None },
            _ => {
                o += 1;
                n += 1;
                NumberedLine { kind: LineKind::Ctx, old: Some(o - 1), new: Some(n - 1) }
            }
        })
        .collect()
}

/// Builds a patch containing one hunk, optionally restricted to the selected line indexes.
/// `reverse = false`: applied forward (stage). Unselected `-` become context, unselected `+` are dropped.
/// `reverse = true`: applied with `--reverse` (unstage / discard). Unselected `+` become context,
/// unselected `-` are dropped. Returns `None` when nothing would change.
pub fn build_patch(file: &FileDiff, hunk: &Hunk, selected: Option<&HashSet<usize>>, reverse: bool) -> Option<String> {
    let mut body: Vec<String> = Vec::new();
    let (mut old_count, mut new_count, mut changes) = (0u32, 0u32, 0u32);
    let mut last_kept = false;
    for (i, line) in hunk.lines.iter().enumerate() {
        let sel = selected.map_or(true, |s| s.contains(&i));
        match line.as_bytes().first() {
            Some(b'-') => {
                if sel {
                    body.push(line.clone());
                    old_count += 1;
                    changes += 1;
                    last_kept = true;
                } else if !reverse {
                    body.push(format!(" {}", &line[1..]));
                    old_count += 1;
                    new_count += 1;
                    last_kept = true;
                } else {
                    last_kept = false;
                }
            }
            Some(b'+') => {
                if sel {
                    body.push(line.clone());
                    new_count += 1;
                    changes += 1;
                    last_kept = true;
                } else if reverse {
                    body.push(format!(" {}", &line[1..]));
                    old_count += 1;
                    new_count += 1;
                    last_kept = true;
                } else {
                    last_kept = false;
                }
            }
            Some(b'\\') => {
                if last_kept {
                    body.push(line.clone());
                }
            }
            _ => {
                body.push(line.clone());
                old_count += 1;
                new_count += 1;
                last_kept = true;
            }
        }
    }
    if changes == 0 {
        return None;
    }
    let ctx = if hunk.context.is_empty() { String::new() } else { format!(" {}", hunk.context) };
    let header = format!("@@ -{},{} +{},{} @@{}", hunk.old_start, old_count, hunk.new_start, new_count, ctx);
    let mut all = file.header.clone();
    all.push(header);
    all.extend(body);
    Some(all.join("\n") + "\n")
}

/// Whether partial (hunk / line) operations are safe for this file.
pub fn can_partial(file: &FileDiff) -> bool {
    !file.binary && !file.combined && !file.is_new && !file.is_deleted && !file.hunks.is_empty()
}

/// Byte ranges that changed between an old and a new line (word granularity), used to
/// emphasise the exact edit inside a modified line.
pub fn intraline(old: &str, new: &str) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::configure().algorithm(similar::Algorithm::Patience).diff_words(old, new);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let (mut ao, mut bo) = (0usize, 0usize);
    for ch in diff.iter_all_changes() {
        let len = ch.value().len();
        match ch.tag() {
            ChangeTag::Equal => {
                ao += len;
                bo += len;
            }
            ChangeTag::Delete => {
                push_range(&mut a, ao, ao + len);
                ao += len;
            }
            ChangeTag::Insert => {
                push_range(&mut b, bo, bo + len);
                bo += len;
            }
        }
    }
    // A line rewritten almost entirely reads better without highlights.
    let changed = |r: &Vec<(usize, usize)>| r.iter().map(|(s, e)| e - s).sum::<usize>();
    if changed(&a) * 10 > old.len() * 7 && changed(&b) * 10 > new.len() * 7 {
        return (vec![], vec![]);
    }
    (a, b)
}

fn push_range(v: &mut Vec<(usize, usize)>, s: usize, e: usize) {
    if let Some(last) = v.last_mut() {
        if last.1 == s {
            last.1 = e;
            return;
        }
    }
    v.push((s, e));
}

/// Pairs deleted and added lines inside a hunk (a block of `-` followed by a block of `+`)
/// so each pair can be highlighted with [`intraline`]. Returns (del_index, add_index) pairs.
pub fn pair_changes(h: &Hunk) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut i = 0;
    let n = h.lines.len();
    while i < n {
        if h.lines[i].starts_with('-') {
            let ds = i;
            while i < n && h.lines[i].starts_with('-') {
                i += 1;
            }
            let de = i;
            let as_ = i;
            while i < n && h.lines[i].starts_with('+') {
                i += 1;
            }
            let ae = i;
            for k in 0..(de - ds).min(ae - as_) {
                pairs.push((ds + k, as_ + k));
            }
        } else {
            i += 1;
        }
    }
    pairs
}

/// Conflict marker regions in a working file, for highlighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictPart {
    Normal,
    Marker,
    Ours,
    Base,
    Theirs,
}

pub fn conflict_parts(text: &str) -> Vec<ConflictPart> {
    let mut state = ConflictPart::Normal;
    text.lines()
        .map(|l| {
            if l.starts_with("<<<<<<<") {
                state = ConflictPart::Ours;
                ConflictPart::Marker
            } else if l.starts_with("|||||||") && state != ConflictPart::Normal {
                state = ConflictPart::Base;
                ConflictPart::Marker
            } else if l.starts_with("=======") && state != ConflictPart::Normal {
                state = ConflictPart::Theirs;
                ConflictPart::Marker
            } else if l.starts_with(">>>>>>>") && state != ConflictPart::Normal {
                state = ConflictPart::Normal;
                ConflictPart::Marker
            } else {
                state
            }
        })
        .collect()
}
