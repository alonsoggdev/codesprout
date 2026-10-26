//! Per-repository UI state and the diff model shown by the diff viewer.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use codesprout_core::diff::{self, ConflictPart, FileDiff, LineKind};
use codesprout_core::explorer::Layout;
use codesprout_core::extras::{LfsInfo, Submodule, Worktree};
use codesprout_core::flow::FlowConfig;
use codesprout_core::history::{Blame, FileRevision, ReflogEntry, SearchMode};
use codesprout_core::hosting::{PullRequest, RemoteRef};
use codesprout_core::{Commit, CommitDetails, FileChange, Refs, Status};
use eframe::egui;

pub const WORKTREE: &str = "WORKTREE";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    History,
    Changes,
    Reflog,
    FileHistory,
    Blame,
    Compare,
    PullRequests,
    Submodules,
    Worktrees,
    Lfs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ListKind {
    Conflicted,
    Staged,
    Unstaged,
}

#[derive(Clone, Debug, Default)]
pub struct FileSel {
    pub list: Option<ListKind>,
    pub paths: BTreeSet<String>,
    pub focus: Option<String>,
    pub anchor: Option<String>,
}

#[derive(Clone, Debug)]
pub enum RefKind {
    Head,
    Branch { current: bool },
    Remote,
    Tag,
}

#[derive(Clone, Debug)]
pub struct RefBadge {
    pub kind: RefKind,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffMode {
    Commit,
    Staged,
    Unstaged,
    Untracked,
    Conflict,
    Compare,
}

#[derive(Clone, Debug)]
pub enum Row {
    Hunk { file: usize, hunk: usize },
    Line { file: usize, hunk: usize, line: usize, kind: LineKind, old: Option<u32>, new: Option<u32>, hl: Vec<(usize, usize)> },
    Text { no: u32, text: String, part: ConflictPart, added: bool },
    Info(String),
}

pub struct ImagePair {
    pub old: Option<egui::ColorImage>,
    pub new: Option<egui::ColorImage>,
    pub textures: Option<(Option<egui::TextureHandle>, Option<egui::TextureHandle>)>,
}

pub struct DiffModel {
    pub key: String,
    pub file: FileChange,
    pub mode: DiffMode,
    pub files: Vec<FileDiff>,
    pub rows: Vec<Row>,
    pub truncated: bool,
    pub lossy: bool,
    pub partial: bool,
    pub note: Option<String>,
    pub image: Option<ImagePair>,
    pub max_chars: usize,
    /// Selected lines: (file, hunk, line index in hunk)
    pub selected: HashSet<(usize, usize, usize)>,
    pub anchor: Option<usize>,
    /// For commit / compare diffs: the revisions compared.
    pub range: Option<(String, String)>,
}

pub fn is_image(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".ico"].iter().any(|e| p.ends_with(e))
}

pub fn decode_image(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

impl DiffModel {
    fn base(key: String, file: FileChange, mode: DiffMode) -> Self {
        Self {
            key,
            file,
            mode,
            files: vec![],
            rows: vec![],
            truncated: false,
            lossy: false,
            partial: false,
            note: None,
            image: None,
            max_chars: 0,
            selected: HashSet::new(),
            anchor: None,
            range: None,
        }
    }

    /// Builds rows from unified diff text. `partial` enables hunk / line staging.
    pub fn from_diff(key: String, file: FileChange, mode: DiffMode, text: &str, word_diff: bool, allow_partial: bool) -> Self {
        let mut m = Self::base(key, file, mode);
        m.files = diff::parse_diff(text);
        let mut max_chars = 0;
        for (fi, f) in m.files.iter().enumerate() {
            if f.binary {
                m.rows.push(Row::Info("Archivo binario: no se puede mostrar el contenido.".into()));
                continue;
            }
            if f.hunks.is_empty() {
                if f.mode_change {
                    m.rows.push(Row::Info("Solo ha cambiado el modo (permisos) del archivo.".into()));
                } else if f.renamed_from.is_some() {
                    m.rows.push(Row::Info("Archivo renombrado sin cambios en el contenido.".into()));
                }
                continue;
            }
            for (hi, h) in f.hunks.iter().enumerate() {
                m.rows.push(Row::Hunk { file: fi, hunk: hi });
                let nums = diff::number_hunk(h);
                let mut hl: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
                if word_diff {
                    for (d, a) in diff::pair_changes(h) {
                        let (x, y) = diff::intraline(&h.lines[d][1..], &h.lines[a][1..]);
                        if !x.is_empty() || !y.is_empty() {
                            hl.insert(d, x);
                            hl.insert(a, y);
                        }
                    }
                }
                for (li, line) in h.lines.iter().enumerate() {
                    max_chars = max_chars.max(line.chars().count());
                    let n = nums[li];
                    m.rows.push(Row::Line {
                        file: fi,
                        hunk: hi,
                        line: li,
                        kind: n.kind,
                        old: n.old,
                        new: n.new,
                        hl: hl.remove(&li).unwrap_or_default(),
                    });
                }
            }
        }
        if m.files.is_empty() {
            m.rows.push(Row::Info("No hay diferencias que mostrar.".into()));
        }
        m.partial = allow_partial && m.files.len() == 1 && diff::can_partial(&m.files[0]);
        m.max_chars = max_chars;
        m
    }

    pub fn from_text(key: String, file: FileChange, mode: DiffMode, text: &str) -> Self {
        let mut m = Self::base(key, file, mode);
        let parts = if mode == DiffMode::Conflict { diff::conflict_parts(text) } else { vec![] };
        for (i, line) in text.lines().enumerate() {
            m.max_chars = m.max_chars.max(line.chars().count());
            m.rows.push(Row::Text {
                no: i as u32 + 1,
                text: line.to_string(),
                part: parts.get(i).copied().unwrap_or(ConflictPart::Normal),
                added: mode == DiffMode::Untracked,
            });
        }
        if m.rows.is_empty() {
            m.rows.push(Row::Info("El archivo está vacío.".into()));
        }
        m
    }

    pub fn info(key: String, file: FileChange, mode: DiffMode, text: &str) -> Self {
        let mut m = Self::base(key, file, mode);
        m.rows.push(Row::Info(text.into()));
        m
    }

    pub fn with_image(mut self, old: Option<Vec<u8>>, new: Option<Vec<u8>>) -> Self {
        let old = old.and_then(|b| decode_image(&b));
        let new = new.and_then(|b| decode_image(&b));
        if old.is_some() || new.is_some() {
            self.image = Some(ImagePair { old, new, textures: None });
        }
        self
    }

}

#[derive(Default)]
pub struct ExplorerState {
    pub offset: egui::Vec2,
    pub initialized: bool,
    pub center_on: Option<String>,
    /// Lane names or commit text highlighted by the quick filter.
    pub filter: String,
    /// col -> node (one commit per row), rebuilt when the layout changes (identified by its pointer).
    pub by_col: HashMap<u32, usize>,
    pub grid_for: usize,
    pub last_zoom: f32,
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub mode: SearchMode,
    pub results: Option<Vec<Commit>>,
    pub running: bool,
    pub error: Option<String>,
}

pub struct FileHistoryState {
    pub path: String,
    pub revs: Option<Vec<FileRevision>>,
    pub selected: Option<usize>,
    pub diff: Option<DiffModel>,
    pub error: Option<String>,
}

pub struct BlameState {
    pub path: String,
    pub rev: Option<String>,
    pub blame: Option<Blame>,
    pub error: Option<String>,
    pub selected: Option<String>,
}

pub struct CompareState {
    pub from: String,
    pub to: String,
    pub from_label: String,
    pub to_label: String,
    pub files: Option<Vec<FileChange>>,
    pub selected: Option<usize>,
    pub diff: Option<DiffModel>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct PrState {
    pub remote: Option<RemoteRef>,
    pub list: Option<Vec<PullRequest>>,
    pub loading: bool,
    pub error: Option<String>,
}

pub struct RepoTab {
    pub path: PathBuf,
    pub key: String,
    pub view: View,
    pub status: Option<Status>,
    pub refs: Option<Refs>,
    pub commits: Arc<Vec<Commit>>,
    pub layout: Arc<Layout>,
    pub commit_index: HashMap<String, usize>,
    pub ref_map: HashMap<String, Vec<RefBadge>>,
    pub loaded: bool,
    pub error: Option<String>,
    pub refreshing: bool,
    pub refresh_again: bool,
    pub refresh_full: bool,
    pub selected: Option<String>,
    pub compare_with: Option<String>,
    pub detail: Option<CommitDetails>,
    pub detail_loading: bool,
    pub detail_file: Option<usize>,
    pub detail_diff: Option<DiffModel>,
    pub detail_token: u64,
    pub diff_token: u64,
    pub sel: FileSel,
    pub last_focus: Option<String>,
    pub changes_diff: Option<DiffModel>,
    pub message: String,
    pub amend: bool,
    pub amend_prefill: Option<String>,
    pub merge_prefill: Option<String>,
    pub merge_prefilled: bool,
    pub push_after: bool,
    pub sign: bool,
    pub explorer: ExplorerState,
    pub search: SearchState,
    pub reflog: Option<Vec<ReflogEntry>>,
    pub reflog_selected: Option<usize>,
    pub file_history: Option<FileHistoryState>,
    pub blame: Option<BlameState>,
    pub compare: Option<CompareState>,
    pub prs: PrState,
    pub submodules: Option<Vec<Submodule>>,
    pub worktrees: Option<Vec<Worktree>>,
    pub lfs: Option<LfsInfo>,
    pub lfs_locks: Option<Vec<codesprout_core::extras::LfsLock>>,
    pub flow: Option<FlowConfig>,
    pub last_fetch: Instant,
    pub template: Option<String>,
}

impl RepoTab {
    pub fn new(path: PathBuf) -> Self {
        let key = path.to_string_lossy().into_owned();
        Self {
            path,
            key,
            view: View::History,
            status: None,
            refs: None,
            commits: Arc::new(vec![]),
            layout: Arc::new(Layout::default()),
            commit_index: HashMap::new(),
            ref_map: HashMap::new(),
            loaded: false,
            error: None,
            refreshing: false,
            refresh_again: false,
            refresh_full: false,
            selected: None,
            compare_with: None,
            detail: None,
            detail_loading: false,
            detail_file: None,
            detail_diff: None,
            detail_token: 0,
            diff_token: 0,
            sel: FileSel::default(),
            last_focus: None,
            changes_diff: None,
            message: String::new(),
            amend: false,
            amend_prefill: None,
            merge_prefill: None,
            merge_prefilled: false,
            push_after: false,
            sign: false,
            explorer: ExplorerState::default(),
            search: SearchState::default(),
            reflog: None,
            reflog_selected: None,
            file_history: None,
            blame: None,
            compare: None,
            prs: PrState::default(),
            submodules: None,
            worktrees: None,
            lfs: None,
            lfs_locks: None,
            flow: None,
            last_fetch: Instant::now(),
            template: None,
        }
    }

    pub fn name(&self) -> String {
        self.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| self.key.clone())
    }

    pub fn change_count(&self) -> usize {
        self.status.as_ref().map_or(0, |s| s.change_count())
    }

    pub fn branch_label(&self) -> String {
        match &self.refs {
            None => "…".into(),
            Some(r) => match (&r.current_branch, &r.head) {
                (Some(b), _) => b.clone(),
                (None, Some(h)) => format!("HEAD separado ({})", &h[..7.min(h.len())]),
                _ => "sin commits".into(),
            },
        }
    }

    pub fn has_head(&self) -> bool {
        self.refs.as_ref().is_some_and(|r| r.head.is_some())
    }

    pub fn commit(&self, hash: &str) -> Option<&Commit> {
        self.commit_index.get(hash).and_then(|&i| self.commits.get(i))
    }

    pub fn list(&self, kind: ListKind) -> &[FileChange] {
        match (&self.status, kind) {
            (Some(s), ListKind::Conflicted) => &s.conflicted,
            (Some(s), ListKind::Staged) => &s.staged,
            (Some(s), ListKind::Unstaged) => &s.unstaged,
            (None, _) => &[],
        }
    }

    pub fn focused_file(&self) -> Option<(ListKind, FileChange)> {
        let kind = self.sel.list?;
        let focus = self.sel.focus.as_ref()?;
        self.list(kind).iter().find(|f| &f.path == focus).map(|f| (kind, f.clone()))
    }


    pub fn rebuild_ref_map(&mut self) {
        let mut map: HashMap<String, Vec<RefBadge>> = HashMap::new();
        if let Some(r) = &self.refs {
            let mut add = |hash: &str, b: RefBadge| map.entry(hash.to_string()).or_default().push(b);
            if r.current_branch.is_none() {
                if let Some(h) = &r.head {
                    add(h, RefBadge { kind: RefKind::Head, name: "HEAD".into() });
                }
            }
            for b in &r.local {
                add(&b.hash, RefBadge { kind: RefKind::Branch { current: b.current }, name: b.name.clone() });
            }
            for b in &r.remote {
                add(&b.hash, RefBadge { kind: RefKind::Remote, name: b.name.clone() });
            }
            for t in &r.tags {
                add(&t.hash, RefBadge { kind: RefKind::Tag, name: t.name.clone() });
            }
            for list in map.values_mut() {
                list.sort_by_key(|b| match b.kind {
                    RefKind::Branch { current: true } => 0,
                    RefKind::Head => 1,
                    RefKind::Branch { .. } => 2,
                    RefKind::Remote => 3,
                    RefKind::Tag => 4,
                });
            }
        }
        self.ref_map = map;
    }

    /// Keeps the file selection valid after a status refresh.
    pub fn prune_selection(&mut self) {
        let Some(s) = &self.status else { return };
        if let Some(k) = self.sel.list {
            let list = match k {
                ListKind::Conflicted => &s.conflicted,
                ListKind::Staged => &s.staged,
                ListKind::Unstaged => &s.unstaged,
            };
            let exists: HashSet<&str> = list.iter().map(|f| f.path.as_str()).collect();
            self.sel.paths.retain(|p| exists.contains(p.as_str()));
            if self.sel.focus.as_ref().is_some_and(|f| !exists.contains(f.as_str())) {
                self.sel.focus = None;
            }
            if self.sel.focus.is_none() {
                self.sel.focus = self.sel.paths.iter().next().cloned();
            }
        }
        if self.sel.focus.is_none() {
            let order = [(ListKind::Conflicted, &s.conflicted), (ListKind::Unstaged, &s.unstaged), (ListKind::Staged, &s.staged)];
            let mut pick = None;
            if let Some(prev) = &self.last_focus {
                pick = order.iter().find(|(_, l)| l.iter().any(|f| &f.path == prev)).map(|(k, _)| (*k, prev.clone()));
            }
            if pick.is_none() {
                pick = order.iter().find(|(_, l)| !l.is_empty()).map(|(k, l)| (*k, l[0].path.clone()));
            }
            self.sel = match pick {
                Some((k, p)) => FileSel { list: Some(k), paths: [p.clone()].into(), focus: Some(p.clone()), anchor: Some(p) },
                None => FileSel::default(),
            };
        }
        self.last_focus = self.sel.focus.clone();
    }
}
