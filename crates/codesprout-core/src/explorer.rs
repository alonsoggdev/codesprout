//! Layout for the branch explorer (Plastic SCM style): every branch is a horizontal lane,
//! time flows left to right, and forks and merges are drawn as links between lanes.
//!
//! Git does not record which branch a commit was made on, so commits are assigned to lanes
//! by walking first parents from each branch tip, in priority order (main/master, develop,
//! the current branch, other local branches, remote branches). Commits left over belong to
//! branches that were deleted after being merged; they get "inferred" lanes named from the
//! merge commit message when possible ("Merge branch 'feature/x'").

use std::collections::HashMap;

use crate::model::{Commit, Refs};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneKind {
    Local,
    Remote,
    /// Recovered from merge messages: the branch no longer exists.
    Inferred,
    /// Commits reachable only from a tag or a detached HEAD.
    Detached,
}

#[derive(Clone, Debug)]
pub struct Lane {
    pub name: String,
    pub kind: LaneKind,
    pub row: u32,
    /// First and last column occupied by the lane's commits.
    pub start: u32,
    pub end: u32,
    /// Index of the lane's newest commit (its tip).
    pub tip: usize,
    /// Commit (on another lane) this lane was branched from.
    pub base: Option<usize>,
    pub parent_lane: Option<usize>,
    /// Merge commit that integrated this lane's tip into another lane.
    pub merged_into: Option<usize>,
    pub is_main: bool,
    pub is_current: bool,
    /// Stable palette index.
    pub color: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// Consecutive commits on the same lane.
    Line,
    /// First commit of a lane to the commit it was branched from.
    Branch,
    /// Second (or later) parent of a merge commit.
    Merge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    /// Parent (older) node.
    pub from: usize,
    /// Child (newer) node.
    pub to: usize,
    pub kind: EdgeKind,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Node {
    pub col: u32,
    pub row: u32,
    pub lane: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColumnMode {
    /// Commits share columns when they do not depend on each other: dense.
    #[default]
    Compact,
    /// One column per commit in date order: left-to-right is strictly chronological.
    Chronological,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// One per commit (same order as the input), plus the working-tree node if requested.
    pub nodes: Vec<Node>,
    pub lanes: Vec<Lane>,
    pub edges: Vec<Edge>,
    pub columns: u32,
    pub rows: u32,
    /// Index of the "uncommitted changes" node, if any.
    pub worktree: Option<usize>,
}

impl Layout {
    pub fn lane_of(&self, node: usize) -> Option<&Lane> {
        self.nodes.get(node).and_then(|n| self.lanes.get(n.lane))
    }
}

const MAIN_NAMES: [&str; 4] = ["main", "master", "trunk", "default"];

fn is_main_name(name: &str) -> bool {
    MAIN_NAMES.contains(&name)
}

/// Branch name recovered from a merge commit subject.
pub fn merged_branch_name(subject: &str) -> Option<String> {
    let s = subject.trim();
    let quoted = |s: &str| -> Option<String> {
        let a = s.find('\'')?;
        let b = s[a + 1..].find('\'')?;
        Some(s[a + 1..a + 1 + b].to_string())
    };
    if let Some(rest) = s.strip_prefix("Merge remote-tracking branch ") {
        let n = quoted(rest)?;
        return Some(n.split_once('/').map(|x| x.1.to_string()).unwrap_or(n));
    }
    if let Some(rest) = s.strip_prefix("Merge branch ") {
        return quoted(rest);
    }
    if let Some(rest) = s.strip_prefix("Merge pull request ") {
        // Merge pull request #12 from owner/feature-x
        let from = rest.split(" from ").nth(1)?;
        let name = from.split_whitespace().next()?;
        return Some(name.split_once('/').map(|x| x.1.to_string()).unwrap_or_else(|| name.to_string()));
    }
    if let Some(rest) = s.strip_prefix("Merged in ") {
        // Bitbucket: Merged in feature/x (pull request #3)
        return rest.split(" (").next().map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    }
    if let Some(rest) = s.strip_prefix("Merge ") {
        // Generic: Merge feature/x into develop
        let name = rest.split(" into ").next()?.trim().trim_matches('\'');
        if !name.is_empty() && !name.contains(' ') {
            return Some(name.to_string());
        }
    }
    None
}

fn color_for(name: &str) -> usize {
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h as usize
}

struct Tip {
    name: String,
    kind: LaneKind,
    idx: usize,
    is_main: bool,
    is_current: bool,
}

/// Computes the explorer layout. `commits` must be in `git log --date-order` order (newest
/// first, children before parents). `worktree` adds an "uncommitted changes" node after HEAD.
pub fn layout(commits: &[Commit], refs: &Refs, worktree: bool, mode: ColumnMode) -> Layout {
    let n = commits.len();
    let index: HashMap<&str, usize> = commits.iter().enumerate().map(|(i, c)| (c.hash.as_str(), i)).collect();
    let parents: Vec<Vec<usize>> = commits
        .iter()
        .map(|c| c.parents.iter().filter_map(|p| index.get(p.as_str()).copied()).collect())
        .collect();
    let first_parent_ok: Vec<bool> =
        commits.iter().map(|c| c.parents.first().map_or(false, |p| index.contains_key(p.as_str()))).collect();

    // ---- branch tips in priority order
    let mut tips: Vec<Tip> = Vec::new();
    let local_rank = |name: &str| -> u8 {
        if is_main_name(name) {
            0
        } else if name == "develop" || name == "development" || name == "dev" {
            1
        } else {
            3
        }
    };
    let mut locals: Vec<_> = refs.local.iter().filter_map(|b| index.get(b.hash.as_str()).map(|&i| (b, i))).collect();
    locals.sort_by_key(|(b, i)| (local_rank(&b.name).min(if b.current { 2 } else { 3 }), *i));
    for (b, i) in locals {
        tips.push(Tip {
            name: b.name.clone(),
            kind: LaneKind::Local,
            idx: i,
            is_main: is_main_name(&b.name),
            is_current: b.current,
        });
    }
    let mut remotes: Vec<_> = refs.remote.iter().filter_map(|b| index.get(b.hash.as_str()).map(|&i| (b, i))).collect();
    remotes.sort_by_key(|(b, i)| (if is_main_name(&b.branch) { 0 } else { 1 }, *i));
    let has_local_main = tips.iter().any(|t| t.is_main);
    for (b, i) in remotes {
        let main = is_main_name(&b.branch) && !has_local_main;
        // a remote main comes before every other branch when there is no local main
        let tip = Tip { name: b.name.clone(), kind: LaneKind::Remote, idx: i, is_main: main, is_current: false };
        if main {
            tips.insert(0, tip);
        } else {
            tips.push(tip);
        }
    }
    if refs.current_branch.is_none() {
        if let Some(&i) = refs.head.as_deref().and_then(|h| index.get(h)) {
            tips.push(Tip { name: "HEAD".into(), kind: LaneKind::Detached, idx: i, is_main: false, is_current: true });
        }
    }
    for t in &refs.tags {
        if let Some(&i) = index.get(t.hash.as_str()) {
            tips.push(Tip { name: t.name.clone(), kind: LaneKind::Detached, idx: i, is_main: false, is_current: false });
        }
    }

    // ---- assign commits to lanes
    let mut lane_of: Vec<Option<usize>> = vec![None; n];
    let mut lanes: Vec<Lane> = Vec::new();
    let new_lane = |lanes: &mut Vec<Lane>, lane_of: &mut Vec<Option<usize>>, t: Tip| {
        if lane_of[t.idx].is_some() {
            return;
        }
        let id = lanes.len();
        let mut c = t.idx;
        let mut base = None;
        loop {
            if let Some(_) = lane_of[c] {
                base = Some(c);
                break;
            }
            lane_of[c] = Some(id);
            if !first_parent_ok[c] {
                break;
            }
            c = parents[c][0];
        }
        lanes.push(Lane {
            color: if t.is_main { 0 } else { color_for(&t.name) },
            name: t.name,
            kind: t.kind,
            row: 0,
            start: 0,
            end: 0,
            tip: t.idx,
            base,
            parent_lane: None,
            merged_into: None,
            is_main: t.is_main,
            is_current: t.is_current,
        });
    };
    for t in tips {
        new_lane(&mut lanes, &mut lane_of, t);
    }

    // merges: non-first parent -> merge commit, for naming inferred lanes
    let mut merged_by: HashMap<usize, usize> = HashMap::new();
    for (i, ps) in parents.iter().enumerate() {
        let skip = if first_parent_ok[i] { 1 } else { 0 };
        for &p in ps.iter().skip(skip) {
            merged_by.entry(p).or_insert(i);
        }
    }
    for i in 0..n {
        if lane_of[i].is_none() {
            let name = merged_by
                .get(&i)
                .and_then(|&m| merged_branch_name(&commits[m].subject))
                .unwrap_or_else(|| "(rama eliminada)".to_string());
            new_lane(
                &mut lanes,
                &mut lane_of,
                Tip { name, kind: LaneKind::Inferred, idx: i, is_main: false, is_current: false },
            );
        }
    }
    for lane in &mut lanes {
        lane.merged_into = merged_by.get(&lane.tip).copied();
    }

    // ---- columns
    let mut col = vec![0u32; n];
    match mode {
        ColumnMode::Chronological => {
            for i in 0..n {
                col[i] = (n - 1 - i) as u32;
            }
        }
        ColumnMode::Compact => {
            // parents always come after their children in date order: walk oldest first
            for i in (0..n).rev() {
                col[i] = parents[i].iter().map(|&p| col[p] + 1).max().unwrap_or(0);
            }
        }
    }
    let mut columns = col.iter().copied().max().map_or(0, |m| m + 1);

    let mut nodes: Vec<Node> =
        (0..n).map(|i| Node { col: col[i], row: 0, lane: lane_of[i].unwrap_or(0) }).collect();

    // ---- working tree node after HEAD
    let mut worktree_idx = None;
    if worktree {
        let head_idx = refs.head.as_deref().and_then(|h| index.get(h)).copied();
        let lane = match head_idx {
            Some(h) => nodes[h].lane,
            None => {
                lanes.push(Lane {
                    name: refs.current_branch.clone().unwrap_or_else(|| "main".into()),
                    kind: LaneKind::Local,
                    row: 0,
                    start: 0,
                    end: 0,
                    tip: n,
                    base: None,
                    parent_lane: None,
                    merged_into: None,
                    is_main: true,
                    is_current: true,
                    color: 0,
                });
                lanes.len() - 1
            }
        };
        // chronologically the uncommitted changes are the newest thing: always the last column
        let c = match mode {
            ColumnMode::Chronological => columns,
            ColumnMode::Compact => head_idx.map_or(0, |h| nodes[h].col + 1),
        };
        columns = columns.max(c + 1);
        nodes.push(Node { col: c, row: 0, lane });
        worktree_idx = Some(n);
    }

    // ---- lane extents
    for l in &mut lanes {
        l.start = u32::MAX;
        l.end = 0;
    }
    for nd in &nodes {
        let l = &mut lanes[nd.lane];
        l.start = l.start.min(nd.col);
        l.end = l.end.max(nd.col);
    }
    for i in 0..lanes.len() {
        if lanes[i].start == u32::MAX {
            lanes[i].start = 0;
        }
        lanes[i].parent_lane = lanes[i].base.map(|b| nodes[b].lane);
    }

    // ---- rows: main on top, every other lane in the first free row under its parent
    let mut occupied: Vec<Vec<(u32, u32)>> = Vec::new();
    let mut order: Vec<usize> = (0..lanes.len()).collect();
    // parents before children so the parent's row is known
    order.sort_by_key(|&i| (if lanes[i].is_main { 0 } else { 1 }, depth(&lanes, i), i));
    for &i in &order {
        let from = lanes[i].base.map_or(lanes[i].start, |b| nodes[b].col);
        let to = lanes[i].end.max(lanes[i].merged_into.map_or(0, |m| nodes[m].col.saturating_sub(1)));
        let min_row = if lanes[i].is_main {
            0
        } else {
            lanes[i].parent_lane.map_or(1, |p| lanes[p].row + 1).max(1)
        };
        let mut row = min_row as usize;
        loop {
            if occupied.len() <= row {
                occupied.resize(row + 1, Vec::new());
            }
            let free = occupied[row].iter().all(|&(s, e)| to < s || from > e);
            if free {
                break;
            }
            row += 1;
        }
        occupied[row].push((from, to));
        lanes[i].row = row as u32;
    }
    for nd in &mut nodes {
        nd.row = lanes[nd.lane].row;
    }
    let rows = lanes.iter().map(|l| l.row + 1).max().unwrap_or(1);

    // ---- edges
    let mut edges = Vec::new();
    for (i, ps) in parents.iter().enumerate() {
        for (k, &p) in ps.iter().enumerate() {
            let kind = if k == 0 && first_parent_ok[i] {
                if nodes[p].lane == nodes[i].lane { EdgeKind::Line } else { EdgeKind::Branch }
            } else {
                EdgeKind::Merge
            };
            edges.push(Edge { from: p, to: i, kind });
        }
    }
    if let (Some(w), Some(h)) = (worktree_idx, refs.head.as_deref().and_then(|h| index.get(h))) {
        edges.push(Edge { from: *h, to: w, kind: EdgeKind::Line });
    }

    Layout { nodes, lanes, edges, columns, rows, worktree: worktree_idx }
}

fn depth(lanes: &[Lane], mut i: usize) -> usize {
    let mut d = 0;
    while let Some(p) = lanes[i].parent_lane {
        d += 1;
        if d > lanes.len() {
            break;
        }
        i = p;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LocalBranch, RemoteBranch};

    fn c(hash: &str, parents: &[&str], subject: &str) -> Commit {
        Commit { hash: hash.into(), parents: parents.iter().map(|s| s.to_string()).collect(), subject: subject.into(), ..Default::default() }
    }

    fn local(name: &str, hash: &str, current: bool) -> LocalBranch {
        LocalBranch { name: name.into(), hash: hash.into(), current, ..Default::default() }
    }

    #[test]
    fn names_from_merge_messages() {
        assert_eq!(merged_branch_name("Merge branch 'feature/x' into develop").as_deref(), Some("feature/x"));
        assert_eq!(merged_branch_name("Merge branch 'fix'").as_deref(), Some("fix"));
        assert_eq!(merged_branch_name("Merge remote-tracking branch 'origin/dev'").as_deref(), Some("dev"));
        assert_eq!(merged_branch_name("Merge pull request #12 from alice/feature-y").as_deref(), Some("feature-y"));
        assert_eq!(merged_branch_name("Merged in bug/7 (pull request #3)").as_deref(), Some("bug/7"));
        assert_eq!(merged_branch_name("Arregla el login"), None);
    }

    #[test]
    fn fork_and_merge_lanes() {
        // main: A - B - M (merge of F2);   feature: A - F1 - F2 (branch deleted)
        let commits = vec![
            c("M", &["B", "F2"], "Merge branch 'feature/login'"),
            c("F2", &["F1"], "f2"),
            c("B", &["A"], "b"),
            c("F1", &["A"], "f1"),
            c("A", &[], "a"),
        ];
        let refs = Refs { local: vec![local("main", "M", true)], current_branch: Some("main".into()), head: Some("M".into()), ..Default::default() };
        let l = layout(&commits, &refs, false, ColumnMode::Compact);
        assert_eq!(l.lanes.len(), 2);
        let main = &l.lanes[0];
        assert!(main.is_main && main.row == 0 && main.name == "main");
        let feat = &l.lanes[1];
        assert_eq!(feat.name, "feature/login");
        assert_eq!(feat.kind, LaneKind::Inferred);
        assert_eq!(feat.row, 1);
        assert_eq!(feat.base, Some(4)); // forked from A
        assert_eq!(feat.merged_into, Some(0)); // merged by M
        // A, F1/B, F2, M in compact columns
        assert_eq!(l.nodes[4].col, 0);
        assert_eq!(l.nodes[3].col, 1);
        assert_eq!(l.nodes[2].col, 1);
        assert_eq!(l.nodes[1].col, 2);
        assert_eq!(l.nodes[0].col, 3);
        let kinds: Vec<_> = l.edges.iter().map(|e| (e.from, e.to, e.kind)).collect();
        assert!(kinds.contains(&(2, 0, EdgeKind::Line)));
        assert!(kinds.contains(&(1, 0, EdgeKind::Merge)));
        assert!(kinds.contains(&(4, 3, EdgeKind::Branch)));
        assert!(kinds.contains(&(3, 1, EdgeKind::Line)));
    }

    #[test]
    fn main_first_even_when_another_branch_is_current() {
        let commits = vec![c("C", &["B"], "c"), c("B", &["A"], "b"), c("A", &[], "a")];
        let refs = Refs {
            local: vec![local("topic", "C", true), local("master", "B", false)],
            current_branch: Some("topic".into()),
            head: Some("C".into()),
            ..Default::default()
        };
        let l = layout(&commits, &refs, true, ColumnMode::Chronological);
        assert_eq!(l.lanes[0].name, "master");
        assert_eq!(l.lanes[0].row, 0);
        assert_eq!(l.lanes[1].name, "topic");
        assert_eq!(l.lanes[1].row, 1);
        assert!(l.lanes[1].is_current);
        // worktree node sits after HEAD on the current lane
        let w = l.worktree.unwrap();
        assert_eq!(l.nodes[w].lane, 1);
        assert_eq!(l.nodes[w].col, l.nodes[0].col + 1);
    }

    #[test]
    fn remote_ahead_gets_its_own_lane_and_rows_are_reused() {
        // main A-B; origin/main has C on top of B; two short features reuse row 1
        let commits = vec![
            c("C", &["B"], "c"),
            c("G", &["B"], "g"),
            c("B", &["A"], "b"),
            c("F", &["A"], "f"),
            c("A", &[], "a"),
        ];
        let refs = Refs {
            local: vec![local("main", "B", true), local("f1", "F", false), local("f2", "G", false)],
            remote: vec![RemoteBranch { remote: "origin".into(), branch: "main".into(), name: "origin/main".into(), hash: "C".into() }],
            current_branch: Some("main".into()),
            head: Some("B".into()),
            ..Default::default()
        };
        let l = layout(&commits, &refs, false, ColumnMode::Chronological);
        let by_name = |n: &str| l.lanes.iter().find(|x| x.name == n).unwrap();
        assert_eq!(by_name("main").row, 0);
        assert_eq!(by_name("origin/main").kind, LaneKind::Remote);
        // f1 spans cols 0..1 and f2 spans 2..3 (from B at col 2): they share row 1
        assert_eq!(by_name("f1").row, 1);
        assert_eq!(by_name("f2").row, 1);
        // origin/main spans 2..4 and overlaps f2
        assert_eq!(by_name("origin/main").row, 2);
        assert_eq!(l.rows, 3);
    }
}
