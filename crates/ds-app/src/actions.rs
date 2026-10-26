//! Git operations started from the UI and the loaders that fill the views.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use ds_core::extras;
use ds_core::history;
use ds_core::hosting::{self, RemoteRef};
use ds_core::repo::{self as git, CommitFlags, PushArgs};
use ds_core::{DiffOptions, FileChange, OpHandle, OpResult, RepoState, ResetMode};

use crate::app::{App, Progress, ToastKind};
use crate::dialogs::Dialog;
use crate::repo_state::{
    is_image, BlameState, CompareState, DiffMode, DiffModel, FileHistoryState, ListKind, View, WORKTREE,
};

const READ_MAX: usize = 2 * 1024 * 1024;
const IMAGE_MAX: usize = 20 * 1024 * 1024;

pub fn open_terminal(repo: &Path) {
    #[cfg(windows)]
    {
        let try_spawn = |cmd: &str, args: &[&str]| Command::new(cmd).args(args).current_dir(repo).spawn().is_ok();
        let r = repo.to_string_lossy().into_owned();
        if try_spawn("wt.exe", &["-d", &r]) {
            return;
        }
        for base in [std::env::var("ProgramFiles").ok(), std::env::var("ProgramFiles(x86)").ok()].into_iter().flatten() {
            let gb = PathBuf::from(base).join("Git").join("git-bash.exe");
            if gb.exists() && Command::new(&gb).arg(format!("--cd={r}")).spawn().is_ok() {
                return;
            }
        }
        let _ = Command::new("cmd").args(["/c", "start", "powershell", "-NoExit"]).current_dir(repo).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").args(["-a", "Terminal"]).arg(repo).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for t in ["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "xterm"] {
            if Command::new(t).current_dir(repo).spawn().is_ok() {
                return;
            }
        }
    }
}

pub fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

impl App {
    // ---------------------------------------------------------------- operation runners

    /// Runs a local git operation with the busy lock; errors open a dialog. Refreshes afterwards.
    pub fn op<T: Send + 'static>(
        &mut self,
        label: &str,
        work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
        done: impl FnOnce(&mut App, T) + Send + 'static,
    ) {
        if let Some(b) = &self.busy {
            let msg = format!("Espera a que termine la operación en curso ({b}).");
            self.toast(msg, ToastKind::Warn);
            return;
        }
        self.busy = Some(label.to_string());
        let label = label.to_string();
        self.bus.spawn(work, move |app, r| {
            app.busy = None;
            match r {
                Ok(v) => done(app, v),
                Err(e) => app.show_git_error(&label, &e.to_string()),
            }
            app.refresh(false);
        });
    }

    /// Network operation with a progress dialog and cancel support.
    pub fn remote_op<T: Send + 'static>(
        &mut self,
        title: &str,
        work: impl FnOnce(&OpHandle) -> anyhow::Result<T> + Send + 'static,
        done: impl FnOnce(&mut App, T) + Send + 'static,
    ) {
        if let Some(b) = &self.busy {
            let msg = format!("Espera a que termine la operación en curso ({b}).");
            self.toast(msg, ToastKind::Warn);
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let handle = OpHandle::new(Some(tx));
        self.progress = Some(Progress { title: title.into(), lines: vec![], handle: handle.clone(), done: None, started: Instant::now() });
        self.busy = Some(title.to_string());
        let sender = self.sender();
        std::thread::spawn(move || {
            for line in rx {
                sender.send(move |app| {
                    if let Some(p) = &mut app.progress {
                        // git rewrites progress lines in place with \r: keep only the last of a kind
                        let kind = line.split(':').next().unwrap_or("").to_string();
                        if p.lines.last().is_some_and(|l| l.split(':').next() == Some(kind.as_str()) && line.contains('%')) {
                            p.lines.pop();
                        }
                        p.lines.push(line);
                    }
                });
            }
        });
        self.bus.spawn(
            move || work(&handle),
            move |app, r| {
                app.busy = None;
                match r {
                    Ok(v) => {
                        app.progress = None;
                        done(app, v);
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if let Some(p) = &mut app.progress {
                            p.done = Some(Err(friendly_error(&msg)));
                        }
                    }
                }
                app.refresh(false);
            },
        );
    }

    pub fn show_git_error(&mut self, title: &str, msg: &str) {
        let identity = ["user.email", "user.name", "tell me who you are", "empty ident"].iter().any(|k| msg.to_lowercase().contains(k));
        if identity {
            self.dialogs.push(Dialog::identity_needed(msg));
        } else {
            self.error_dialog(title, &friendly_error(msg));
        }
    }

    pub fn handle_conflicts(&mut self, r: &OpResult, what: &str) -> bool {
        if r.conflicts {
            self.toast(format!("{what} ha generado conflictos. Resuélvelos en «Cambios» y confirma."), ToastKind::Warn);
            self.set_view(View::Changes);
            true
        } else {
            false
        }
    }

    fn path(&self) -> Option<PathBuf> {
        self.active_path()
    }

    // ---------------------------------------------------------------- loaders

    pub fn load_detail(&mut self, key: &str, hash: String) {
        let Some(t) = self.tab_by_key(key) else { return };
        t.detail_token += 1;
        t.detail_loading = true;
        let token = t.detail_token;
        let path = t.path.clone();
        let key = key.to_string();
        self.bus.spawn(
            move || git::commit_details(&path, &hash),
            move |app, r| {
                let Some(t) = app.tab_by_key(&key) else { return };
                if t.detail_token != token {
                    return;
                }
                t.detail_loading = false;
                match r {
                    Ok(d) => {
                        t.detail_file = if d.files.is_empty() { None } else { Some(0) };
                        t.detail = Some(d);
                        t.detail_diff = None;
                        app.load_detail_diff(&key);
                    }
                    Err(e) => {
                        t.detail = None;
                        t.detail_diff = None;
                        app.toast(e.to_string(), ToastKind::Error);
                    }
                }
            },
        );
    }

    pub fn diff_options(&self) -> DiffOptions {
        let s = &self.store.settings;
        DiffOptions { ignore_whitespace: s.ignore_whitespace, context: (s.context_lines != 3).then_some(s.context_lines) }
    }

    pub fn load_detail_diff(&mut self, key: &str) {
        let opts = self.diff_options();
        let word = self.store.settings.word_diff;
        let Some(t) = self.tab_by_key(key) else { return };
        let (Some(d), Some(fi)) = (&t.detail, t.detail_file) else { return };
        let Some(f) = d.files.get(fi).cloned() else { return };
        t.diff_token += 1;
        let token = t.diff_token;
        let path = t.path.clone();
        let base = d.base.clone();
        let hash = d.hash.clone();
        let merge = d.parents.len() > 1;
        let k = format!("{}|{}|{}", key, hash, f.path);
        let key = key.to_string();
        self.bus.spawn(
            move || -> anyhow::Result<DiffModel> {
                let res = git::commit_file_diff(&path, &base, &hash, &f, &opts)?;
                let mut m = DiffModel::from_diff(k, f.clone(), DiffMode::Commit, &res.text, word, false);
                m.truncated = res.truncated;
                m.lossy = res.lossy;
                m.range = Some((base.clone(), hash.clone()));
                if merge {
                    m.note = Some("Commit de fusión: se muestran los cambios respecto al primer padre.".into());
                }
                if is_image(&f.path) {
                    let old = if f.status == "A" { None } else { git::read_blob(&path, &base, f.orig.as_deref().unwrap_or(&f.path), IMAGE_MAX).ok() };
                    let new = if f.status == "D" { None } else { git::read_blob(&path, &hash, &f.path, IMAGE_MAX).ok() };
                    m = m.with_image(old, new);
                }
                Ok(m)
            },
            move |app, r| {
                let Some(t) = app.tab_by_key(&key) else { return };
                if t.diff_token != token {
                    return;
                }
                match r {
                    Ok(m) => t.detail_diff = Some(m),
                    Err(e) => app.toast(format!("No se pudo cargar el diff: {e}"), ToastKind::Error),
                }
            },
        );
    }

    pub fn load_changes_diff(&mut self, key: &str) {
        let opts = self.diff_options();
        let word = self.store.settings.word_diff;
        let allow_partial = !opts.ignore_whitespace;
        let Some(t) = self.tab_by_key(key) else { return };
        let Some((kind, f)) = t.focused_file() else {
            t.changes_diff = None;
            return;
        };
        t.diff_token += 1;
        let token = t.diff_token;
        let path = t.path.clone();
        let k = format!("{key}|{kind:?}|{}", f.path);
        let key = key.to_string();
        self.bus.spawn(
            move || -> anyhow::Result<DiffModel> {
                let m = match kind {
                    ListKind::Conflicted => match git::read_working_file(&path, &f.path, READ_MAX) {
                        Ok(w) if w.binary => DiffModel::info(k, f.clone(), DiffMode::Conflict, "Archivo binario en conflicto: elige una versión entera."),
                        Ok(w) => {
                            let mut m = DiffModel::from_text(k, f.clone(), DiffMode::Conflict, &w.text);
                            m.note = Some(format!("Conflicto ({}). Edita el archivo, o elige una versión entera con los botones de arriba.", f.status));
                            m
                        }
                        Err(_) => DiffModel::info(
                            k,
                            f.clone(),
                            DiffMode::Conflict,
                            "El archivo no existe en el directorio de trabajo (una de las ramas lo eliminó). Elige una versión o márcalo como resuelto.",
                        ),
                    },
                    _ if f.status == "?" => {
                        let w = git::read_working_file(&path, &f.path, READ_MAX)?;
                        if w.directory {
                            DiffModel::info(k, f.clone(), DiffMode::Untracked, "Carpeta sin seguimiento (posible repositorio anidado).")
                        } else if w.binary {
                            let m = DiffModel::info(k, f.clone(), DiffMode::Untracked, &format!("Archivo binario nuevo ({} KB).", w.size / 1024));
                            if is_image(&f.path) {
                                m.with_image(None, std::fs::read(path.join(&f.path)).ok())
                            } else {
                                m
                            }
                        } else {
                            let mut m = DiffModel::from_text(k, f.clone(), DiffMode::Untracked, &w.text);
                            m.truncated = w.truncated;
                            m
                        }
                    }
                    _ => {
                        let staged = kind == ListKind::Staged;
                        let res = git::working_diff(&path, &f, staged, &opts)?;
                        let mode = if staged { DiffMode::Staged } else { DiffMode::Unstaged };
                        let mut m = DiffModel::from_diff(k, f.clone(), mode, &res.text, word, allow_partial);
                        m.truncated = res.truncated;
                        m.lossy = res.lossy;
                        if m.truncated {
                            m.partial = false;
                        }
                        if is_image(&f.path) {
                            let src = f.orig.as_deref().unwrap_or(&f.path);
                            let old = if f.status == "A" { None } else if staged { git::read_blob(&path, "HEAD", src, IMAGE_MAX).ok() } else { git::read_blob(&path, "", src, IMAGE_MAX).ok() };
                            let new = if f.status == "D" { None } else if staged { git::read_blob(&path, "", &f.path, IMAGE_MAX).ok() } else { std::fs::read(path.join(&f.path)).ok() };
                            m = m.with_image(old, new);
                        }
                        m
                    }
                };
                Ok(m)
            },
            move |app, r| {
                let Some(t) = app.tab_by_key(&key) else { return };
                if t.diff_token != token {
                    return;
                }
                match r {
                    Ok(m) => t.changes_diff = Some(m),
                    Err(e) => {
                        t.changes_diff = None;
                        app.toast(format!("No se pudo cargar el diff: {e}"), ToastKind::Error);
                    }
                }
            },
        );
    }

    pub fn load_reflog(&mut self, key: &str) {
        let Some(t) = self.tab_by_key(key) else { return };
        let path = t.path.clone();
        let key = key.to_string();
        self.bus.spawn(
            move || history::reflog(&path, "HEAD", 500),
            move |app, r| match r {
                Ok(v) => {
                    if let Some(t) = app.tab_by_key(&key) {
                        t.reflog = Some(v);
                    }
                }
                Err(e) => app.toast(e.to_string(), ToastKind::Error),
            },
        );
    }

    pub fn load_submodules(&mut self, key: &str) {
        let Some(t) = self.tab_by_key(key) else { return };
        let path = t.path.clone();
        let key = key.to_string();
        self.bus.spawn(
            move || extras::submodules(&path),
            move |app, r| match r {
                Ok(v) => {
                    if let Some(t) = app.tab_by_key(&key) {
                        t.submodules = Some(v);
                    }
                }
                Err(e) => app.toast(e.to_string(), ToastKind::Error),
            },
        );
    }

    pub fn load_worktrees(&mut self, key: &str) {
        let Some(t) = self.tab_by_key(key) else { return };
        let path = t.path.clone();
        let key = key.to_string();
        self.bus.spawn(
            move || extras::worktrees(&path),
            move |app, r| match r {
                Ok(v) => {
                    if let Some(t) = app.tab_by_key(&key) {
                        t.worktrees = Some(v);
                    }
                }
                Err(e) => app.toast(e.to_string(), ToastKind::Error),
            },
        );
    }

    pub fn load_lfs(&mut self, key: &str) {
        let Some(t) = self.tab_by_key(key) else { return };
        let path = t.path.clone();
        let key = key.to_string();
        self.bus.spawn(
            move || (extras::lfs_info(&path), extras::lfs_locks(&path).ok()),
            move |app, (info, locks)| match info {
                Ok(v) => {
                    if let Some(t) = app.tab_by_key(&key) {
                        t.lfs = Some(v);
                        t.lfs_locks = locks;
                    }
                }
                Err(e) => app.toast(e.to_string(), ToastKind::Error),
            },
        );
    }

    /// Detects the hosting service from the remotes and loads its open pull requests.
    pub fn load_prs(&mut self, key: &str) {
        let Some(t) = self.tab_by_key(key) else { return };
        let path = t.path.clone();
        t.prs.loading = true;
        t.prs.error = None;
        let key = key.to_string();
        self.bus.spawn(
            move || -> anyhow::Result<(Option<RemoteRef>, Vec<hosting::PullRequest>)> {
                let r = find_remote_ref(&path);
                let Some(r) = r else { return Ok((None, vec![])) };
                let list = hosting::pull_requests(&r);
                match list {
                    Ok(l) => Ok((Some(r), l)),
                    Err(e) => Err(anyhow::anyhow!("{e}")).map_err(|e: anyhow::Error| e.context(r.web_url())),
                }
            },
            move |app, r| {
                let Some(t) = app.tab_by_key(&key) else { return };
                t.prs.loading = false;
                match r {
                    Ok((remote, list)) => {
                        t.prs.list = remote.as_ref().map(|_| list);
                        t.prs.remote = remote;
                    }
                    Err(e) => {
                        let url = e.to_string();
                        let msg = e.root_cause().to_string();
                        t.prs.remote = RemoteRef::parse(&url).or(t.prs.remote.clone());
                        t.prs.error = Some(msg);
                    }
                }
            },
        );
    }

    pub fn open_file_history(&mut self, file: &str) {
        let Some(t) = self.tab_mut() else { return };
        t.file_history = Some(FileHistoryState { path: file.to_string(), revs: None, selected: None, diff: None, error: None });
        t.view = View::FileHistory;
        let path = t.path.clone();
        let key = t.key.clone();
        let f = file.to_string();
        self.bus.spawn(
            move || history::file_history(&path, &f, 2000),
            move |app, r| {
                let Some(t) = app.tab_by_key(&key) else { return };
                let Some(fh) = &mut t.file_history else { return };
                match r {
                    Ok(v) => {
                        fh.selected = if v.is_empty() { None } else { Some(0) };
                        fh.revs = Some(v);
                        app.load_file_history_diff(&key);
                    }
                    Err(e) => fh.error = Some(e.to_string()),
                }
            },
        );
    }

    pub fn load_file_history_diff(&mut self, key: &str) {
        let opts = self.diff_options();
        let word = self.store.settings.word_diff;
        let Some(t) = self.tab_by_key(key) else { return };
        let Some(fh) = &t.file_history else { return };
        let (Some(revs), Some(i)) = (&fh.revs, fh.selected) else { return };
        let Some(rev) = revs.get(i).cloned() else { return };
        let path = t.path.clone();
        t.diff_token += 1;
        let token = t.diff_token;
        let key = key.to_string();
        self.bus.spawn(
            move || -> anyhow::Result<DiffModel> {
                let base = match rev.commit.parents.first() {
                    Some(p) => p.clone(),
                    None => git::empty_tree(&path)?,
                };
                let d = git::commit_details(&path, &rev.commit.hash)?;
                let f = d.files.iter().find(|f| f.path == rev.path).cloned().unwrap_or_else(|| FileChange::new(&rev.path, &rev.status, None));
                let res = git::commit_file_diff(&path, &base, &rev.commit.hash, &f, &opts)?;
                let mut m = DiffModel::from_diff(format!("fh|{}|{}", rev.commit.hash, rev.path), f, DiffMode::Commit, &res.text, word, false);
                m.range = Some((base, rev.commit.hash.clone()));
                Ok(m)
            },
            move |app, r| {
                let Some(t) = app.tab_by_key(&key) else { return };
                if t.diff_token != token {
                    return;
                }
                if let Some(fh) = &mut t.file_history {
                    match r {
                        Ok(m) => fh.diff = Some(m),
                        Err(e) => fh.error = Some(e.to_string()),
                    }
                }
            },
        );
    }

    pub fn open_blame(&mut self, file: &str, rev: Option<String>) {
        let Some(t) = self.tab_mut() else { return };
        t.blame = Some(BlameState { path: file.to_string(), rev: rev.clone(), blame: None, error: None, selected: None });
        t.view = View::Blame;
        let path = t.path.clone();
        let key = t.key.clone();
        let f = file.to_string();
        self.bus.spawn(
            move || history::blame(&path, &f, rev.as_deref()),
            move |app, r| {
                let Some(t) = app.tab_by_key(&key) else { return };
                if let Some(b) = &mut t.blame {
                    match r {
                        Ok(v) => b.blame = Some(v),
                        Err(e) => b.error = Some(e.to_string()),
                    }
                }
            },
        );
    }

    pub fn open_compare(&mut self, from: String, to: String, from_label: String, to_label: String) {
        let Some(t) = self.tab_mut() else { return };
        t.compare = Some(CompareState { from: from.clone(), to: to.clone(), from_label, to_label, files: None, selected: None, diff: None, error: None });
        t.view = View::Compare;
        let path = t.path.clone();
        let key = t.key.clone();
        self.bus.spawn(
            move || git::range_files(&path, &from, &to),
            move |app, r| {
                let Some(t) = app.tab_by_key(&key) else { return };
                let Some(c) = &mut t.compare else { return };
                match r {
                    Ok(files) => {
                        c.selected = if files.is_empty() { None } else { Some(0) };
                        c.files = Some(files);
                        app.load_compare_diff(&key);
                    }
                    Err(e) => c.error = Some(e.to_string()),
                }
            },
        );
    }

    pub fn load_compare_diff(&mut self, key: &str) {
        let opts = self.diff_options();
        let word = self.store.settings.word_diff;
        let Some(t) = self.tab_by_key(key) else { return };
        let Some(c) = &t.compare else { return };
        let (Some(files), Some(i)) = (&c.files, c.selected) else { return };
        let Some(f) = files.get(i).cloned() else { return };
        let (from, to) = (c.from.clone(), c.to.clone());
        let path = t.path.clone();
        t.diff_token += 1;
        let token = t.diff_token;
        let key = key.to_string();
        self.bus.spawn(
            move || -> anyhow::Result<DiffModel> {
                let res = git::range_diff(&path, &from, &to, Some(&f), &opts)?;
                let mut m = DiffModel::from_diff(format!("cmp|{from}|{to}|{}", f.path), f.clone(), DiffMode::Compare, &res.text, word, false);
                m.range = Some((from.clone(), to.clone()));
                if is_image(&f.path) {
                    let old = git::read_blob(&path, &from, f.orig.as_deref().unwrap_or(&f.path), IMAGE_MAX).ok();
                    let new = git::read_blob(&path, &to, &f.path, IMAGE_MAX).ok();
                    m = m.with_image(old, new);
                }
                Ok(m)
            },
            move |app, r| {
                let Some(t) = app.tab_by_key(&key) else { return };
                if t.diff_token != token {
                    return;
                }
                if let Some(c) = &mut t.compare {
                    match r {
                        Ok(m) => c.diff = Some(m),
                        Err(e) => c.error = Some(e.to_string()),
                    }
                }
            },
        );
    }

    pub fn run_search(&mut self) {
        let max = self.store.settings.max_commits.max(1000);
        let all = self.store.settings.all_branches;
        let Some(t) = self.tab_mut() else { return };
        let q = t.search.query.trim().to_string();
        if q.is_empty() {
            t.search.results = None;
            return;
        }
        t.search.running = true;
        t.search.error = None;
        let mode = t.search.mode;
        let path = t.path.clone();
        let key = t.key.clone();
        self.bus.spawn(
            move || history::search(&path, &q, mode, all, max),
            move |app, r| {
                let Some(t) = app.tab_by_key(&key) else { return };
                t.search.running = false;
                match r {
                    Ok(v) => t.search.results = Some(v),
                    Err(e) => t.search.error = Some(e.to_string()),
                }
            },
        );
    }

    // ---------------------------------------------------------------- selection

    pub fn select_commit(&mut self, hash: String, center: bool) {
        let Some(t) = self.tab_mut() else { return };
        t.selected = Some(hash.clone());
        if center {
            t.explorer.center_on = Some(hash.clone());
            if let Some(&i) = t.commit_index.get(&hash) {
                t.list_scroll_to = Some(i);
            }
        }
        let key = t.key.clone();
        if hash != WORKTREE {
            self.load_detail(&key, hash);
        }
    }

    /// Shows a commit in the history view (from the sidebar, a parent link, etc.).
    pub fn reveal_commit(&mut self, hash: &str) {
        let Some(t) = self.tab_mut() else { return };
        let known = t.commit_index.contains_key(hash) || t.refs.as_ref().is_some_and(|r| r.stashes.iter().any(|s| s.hash == hash));
        if !known {
            self.toast("Ese commit no está entre los commits cargados (activa «Todas las ramas» o aumenta el límite en Ajustes).", ToastKind::Info);
            return;
        }
        if t.view != View::History {
            t.view = View::History;
        }
        self.select_commit(hash.to_string(), true);
    }

    // ---------------------------------------------------------------- file operations

    pub fn stage_files(&mut self, files: Vec<FileChange>) {
        let Some(p) = self.path() else { return };
        let paths = with_orig(&files);
        self.op("Preparar", move || git::stage(&p, &paths), |_, _| {});
    }

    pub fn unstage_files(&mut self, files: Vec<FileChange>) {
        let Some(p) = self.path() else { return };
        let paths = with_orig(&files);
        self.op("Quitar del área de preparación", move || git::unstage(&p, &paths), |_, _| {});
    }

    pub fn stage_all(&mut self) {
        let Some(p) = self.path() else { return };
        self.op("Preparar todo", move || git::stage_all(&p), |_, _| {});
    }

    pub fn unstage_all(&mut self) {
        let Some(p) = self.path() else { return };
        self.op("Quitar todo", move || git::unstage_all(&p), |_, _| {});
    }

    pub fn discard_files(&mut self, files: Vec<FileChange>) {
        if files.is_empty() {
            return;
        }
        let untracked: Vec<String> = files.iter().filter(|f| f.is_untracked()).map(|f| f.path.clone()).collect();
        let tracked: Vec<String> = files.iter().filter(|f| !f.is_untracked()).map(|f| f.path.clone()).collect();
        let mut parts = vec![];
        if !tracked.is_empty() {
            parts.push(format!("se descartarán los cambios de {}", crate::widgets::plural(tracked.len(), "archivo", "archivos")));
        }
        if !untracked.is_empty() {
            parts.push(crate::widgets::plural(untracked.len(), "archivo sin seguimiento irá a la papelera", "archivos sin seguimiento irán a la papelera"));
        }
        let mut msg = parts.join(" y ") + ".";
        if let Some(first) = msg.get(..1) {
            msg = first.to_uppercase() + &msg[1..];
        }
        let title = if files.len() == 1 { format!("Descartar «{}»", crate::widgets::split_path(&files[0].path).1) } else { "Descartar cambios".into() };
        let detail = (!tracked.is_empty()).then(|| "Los cambios descartados de archivos con seguimiento no se pueden recuperar.".to_string());
        self.confirm(&title, &msg, detail, "Descartar", true, move |app| {
            let Some(p) = app.path() else { return };
            app.op("Descartar", move || git::discard(&p, &tracked, &untracked), |_, _| {});
        });
    }

    pub fn ignore_files(&mut self, files: Vec<FileChange>, by_ext: bool) {
        let Some(p) = self.path() else { return };
        let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        self.op("Ignorar", move || git::ignore_paths(&p, &paths, by_ext), |app, _| app.ok("Añadido a .gitignore"));
    }

    pub fn mark_resolved(&mut self, files: Vec<FileChange>) {
        let Some(p) = self.path() else { return };
        let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        self.op("Marcar como resuelto", move || git::stage(&p, &paths), |_, _| {});
    }

    pub fn resolve_conflict(&mut self, f: FileChange, ours: bool) {
        let rebase = self.tab().and_then(|t| t.status.as_ref()).and_then(|s| s.state) == Some(RepoState::Rebase);
        let msg = format!("Se sustituirá «{}» por la versión {} y se marcará como resuelto.", f.path, if ours { "de la rama actual (ours)" } else { "entrante (theirs)" });
        let detail = rebase.then(|| "Durante un rebase, «mía» es la rama sobre la que se reaplica y «suya» el commit que se está aplicando.".to_string());
        self.confirm(if ours { "Usar mi versión" } else { "Usar la versión entrante" }, &msg, detail, "Resolver", false, move |app| {
            let Some(p) = app.path() else { return };
            app.op("Resolver conflicto", move || git::resolve_conflict(&p, &f.path, ours), |_, _| {});
        });
    }

    pub fn open_mergetool(&mut self, f: FileChange) {
        let Some(p) = self.path() else { return };
        self.op("Herramienta de fusión", move || extras::mergetool(&p, &f.path), |app, _| app.ok("Herramienta de fusión cerrada"));
    }

    pub fn open_difftool(&mut self, f: &FileChange, staged: bool, range: Option<(String, String)>) {
        let Some(p) = self.path() else { return };
        let path = f.path.clone();
        self.bus.spawn(
            move || extras::difftool(&p, &path, staged, range.as_ref().map(|(a, b)| (a.as_str(), b.as_str()))),
            |app, r| {
                if let Err(e) = r {
                    app.error_dialog("Herramienta de diff externa", &format!("{e}\n\nConfigúrala en Ajustes → Herramientas externas."));
                }
            },
        );
    }

    pub fn apply_partial(&mut self, patch: String, cached: bool, reverse: bool) {
        let Some(p) = self.path() else { return };
        self.op("Aplicar cambios parciales", move || git::apply_patch(&p, &patch, cached, reverse), |_, _| {});
    }

    // ---------------------------------------------------------------- commit

    pub fn can_commit(&self) -> (bool, &'static str) {
        let Some(t) = self.tab() else { return (false, "") };
        let Some(s) = &t.status else { return (false, "") };
        let merging = matches!(s.state, Some(RepoState::Merge | RepoState::CherryPick | RepoState::Revert));
        if self.busy.is_some() {
            (false, "Hay una operación en curso")
        } else if s.state == Some(RepoState::Rebase) {
            (false, "Durante un rebase usa «Continuar» en la barra superior")
        } else if !s.conflicted.is_empty() {
            (false, "Resuelve los conflictos primero")
        } else if t.message.trim().is_empty() {
            (false, "Escribe un mensaje")
        } else if !t.amend && s.staged.is_empty() && !merging {
            (false, "Prepara al menos un archivo")
        } else {
            (true, "")
        }
    }

    pub fn do_commit(&mut self) {
        if !self.can_commit().0 {
            return;
        }
        let Some(t) = self.tab() else { return };
        let published = t.refs.as_ref().is_some_and(|r| r.local.iter().any(|b| b.current && b.upstream.is_some() && b.ahead == 0 && !b.gone));
        if t.amend && published {
            self.confirm(
                "Modificar un commit ya publicado",
                "El último commit ya está en el remoto. Si lo modificas tendrás que hacer push forzado.",
                None,
                "Modificar igualmente",
                true,
                |app| app.commit_now(),
            );
        } else {
            self.commit_now();
        }
    }

    fn commit_now(&mut self) {
        let Some(t) = self.tab() else { return };
        let p = t.path.clone();
        let key = t.key.clone();
        let msg = t.message.clone();
        let flags = CommitFlags { amend: t.amend, sign: t.sign };
        let push_after = t.push_after;
        self.op(
            if flags.amend { "Modificar commit" } else { "Commit" },
            move || git::commit(&p, &msg, flags),
            move |app, _hash| {
                app.ok(if flags.amend { "Commit modificado" } else { "Commit creado" });
                if let Some(t) = app.tab_by_key(&key) {
                    t.message.clear();
                    t.amend = false;
                    t.merge_prefilled = true;
                }
                if push_after {
                    app.quick_push(&key);
                }
            },
        );
    }

    /// Pushes the current branch to its upstream, or opens the push dialog if it has none.
    pub fn quick_push(&mut self, key: &str) {
        let Some(t) = self.tab_by_key(key) else { return };
        let path = t.path.clone();
        let key = key.to_string();
        // refs may be stale right after a commit; read them fresh on the worker
        self.bus.spawn(
            move || git::refs(&path).map(|r| (path, r)),
            move |app, r| {
                let Ok((path, refs)) = r else { return };
                let b = refs.current_local().cloned();
                match b {
                    Some(b) if b.upstream.is_some() && b.upstream_remote.is_some() && !b.gone => {
                        let args = PushArgs {
                            remote: b.upstream_remote.clone().unwrap(),
                            branch: b.name.clone(),
                            remote_branch: b.upstream_branch.clone(),
                            ..Default::default()
                        };
                        let name = b.name.clone();
                        app.remote_op("Push", move |h| git::push(&path, &args, Some(h)), move |app, _| app.ok(format!("Push de {name} completado")));
                    }
                    _ => {
                        if app.active_key().as_deref() == Some(key.as_str()) {
                            app.open_push(None);
                        }
                    }
                }
            },
        );
    }

    pub fn toggle_amend(&mut self) {
        let Some(t) = self.tab_mut() else { return };
        t.amend = !t.amend;
        if t.amend && t.message.trim().is_empty() {
            let p = t.path.clone();
            let key = t.key.clone();
            self.bus.spawn(
                move || git::last_commit_message(&p),
                move |app, r| {
                    if let (Some(t), Ok(m)) = (app.tab_by_key(&key), r) {
                        if t.amend && t.message.trim().is_empty() {
                            t.message = m.clone();
                            t.amend_prefill = Some(m);
                        }
                    }
                },
            );
        } else if !t.amend && t.amend_prefill.as_ref() == Some(&t.message) {
            t.message.clear();
            t.amend_prefill = None;
        }
    }

    // ---------------------------------------------------------------- sequencer states

    pub fn abort_operation(&mut self, kind: RepoState) {
        self.confirm("Abortar operación", "Se descartará la operación en curso y el repositorio volverá al estado anterior.", None, "Abortar", true, move |app| {
            let Some(p) = app.path() else { return };
            app.op("Abortar", move || git::abort_operation(&p, kind), |app, _| app.ok("Operación abortada"));
        });
    }

    pub fn continue_operation(&mut self, kind: RepoState) {
        let Some(t) = self.tab() else { return };
        let p = t.path.clone();
        let key = t.key.clone();
        if kind == RepoState::Merge && !t.message.trim().is_empty() {
            let msg = t.message.clone();
            self.op("Confirmar fusión", move || git::commit(&p, &msg, CommitFlags::default()), move |app, _| {
                app.ok("Fusión completada");
                if let Some(t) = app.tab_by_key(&key) {
                    t.message.clear();
                }
            });
            return;
        }
        self.op("Continuar", move || git::continue_operation(&p, kind), |app, r| {
            if !app.handle_conflicts(&r, "La operación") {
                app.ok("Operación completada");
            }
        });
    }

    pub fn skip_rebase(&mut self) {
        self.confirm("Saltar commit", "Se omitirá el commit que se está aplicando y sus cambios se perderán del rebase.", None, "Saltar", true, |app| {
            let Some(p) = app.path() else { return };
            app.op("Saltar commit", move || git::skip_rebase(&p), |app, r| {
                app.handle_conflicts(&r, "El rebase");
            });
        });
    }

    // ---------------------------------------------------------------- branches & commits

    pub fn checkout_branch(&mut self, name: String) {
        let Some(t) = self.tab() else { return };
        if t.refs.as_ref().and_then(|r| r.current_branch.as_ref()) == Some(&name) {
            return;
        }
        let p = t.path.clone();
        let n = name.clone();
        self.op("Cambiar de rama", move || git::checkout(&p, &n), move |app, _| app.ok(format!("Ahora estás en {name}")));
    }

    pub fn checkout_remote_branch(&mut self, remote_name: String, branch: String) {
        let Some(t) = self.tab() else { return };
        let Some(refs) = &t.refs else { return };
        let existing = refs.local.iter().find(|b| b.name == branch).cloned();
        if let Some(e) = &existing {
            if e.upstream.as_deref() == Some(remote_name.as_str()) {
                self.checkout_branch(e.name.clone());
                return;
            }
        }
        let remote = remote_name.split('/').next().unwrap_or("").to_string();
        let suggested = if existing.is_some() { format!("{remote}-{branch}") } else { branch.clone() };
        self.dialogs.push(Dialog::checkout_remote(remote_name, suggested, existing.is_some()));
    }

    pub fn checkout_commit(&mut self, hash: String) {
        let short = hash[..8.min(hash.len())].to_string();
        self.confirm(
            "Checkout de un commit",
            &format!("Vas a pasar a «HEAD separado» en {short}. Los commits que hagas no pertenecerán a ninguna rama salvo que crees una."),
            None,
            "Hacer checkout",
            false,
            move |app| {
                let Some(p) = app.path() else { return };
                let h = hash.clone();
                app.op("Checkout", move || git::checkout_detached(&p, &h), move |app, _| app.ok(format!("HEAD en {short}")));
            },
        );
    }

    pub fn cherry_pick(&mut self, hash: String, subject: String, is_merge: bool) {
        let label = self.tab().map(|t| t.branch_label()).unwrap_or_default();
        let short = hash[..8.min(hash.len())].to_string();
        self.confirm("Cherry-pick", &format!("Aplicar «{subject}» ({short}) sobre {label}."), None, "Cherry-pick", false, move |app| {
            let Some(p) = app.path() else { return };
            let h = hash.clone();
            app.op("Cherry-pick", move || git::cherry_pick(&p, &h, is_merge), |app, r| {
                if !app.handle_conflicts(&r, "El cherry-pick") {
                    app.ok("Cherry-pick completado");
                }
            });
        });
    }

    pub fn revert_commit(&mut self, hash: String, subject: String, is_merge: bool) {
        let short = hash[..8.min(hash.len())].to_string();
        self.confirm("Revertir commit", &format!("Se creará un nuevo commit que deshace «{subject}» ({short})."), None, "Revertir", false, move |app| {
            let Some(p) = app.path() else { return };
            let h = hash.clone();
            app.op("Revertir", move || git::revert(&p, &h, is_merge), |app, r| {
                if !app.handle_conflicts(&r, "El revert") {
                    app.ok("Commit revertido");
                }
            });
        });
    }

    pub fn rebase_onto(&mut self, target: String, label: String) {
        let cur = self.tab().map(|t| t.branch_label()).unwrap_or_default();
        self.confirm(
            "Rebase",
            &format!("Se reaplicarán los commits de «{cur}» encima de «{label}»."),
            Some("No hagas rebase de commits que ya hayas publicado y compartido con otras personas.".into()),
            "Hacer rebase",
            false,
            move |app| {
                let Some(p) = app.path() else { return };
                let t = target.clone();
                app.op("Rebase", move || git::rebase(&p, &t), |app, r| {
                    if !app.handle_conflicts(&r, "El rebase") {
                        app.ok("Rebase completado");
                    }
                });
            },
        );
    }

    pub fn do_reset(&mut self, hash: String, mode: ResetMode) {
        let Some(p) = self.path() else { return };
        self.op("Reset", move || git::reset(&p, &hash, mode), move |app, _| app.ok(format!("Reset ({}) completado", mode.flag().trim_start_matches('-'))));
    }

    pub fn apply_stash(&mut self, reference: String, pop: bool) {
        let Some(p) = self.path() else { return };
        self.op(if pop { "Pop stash" } else { "Aplicar stash" }, move || git::stash_apply(&p, &reference, pop), move |app, r| {
            if !app.handle_conflicts(&r, "Aplicar el stash") {
                app.ok(if pop { "Stash aplicado y eliminado" } else { "Stash aplicado" });
            }
        });
    }

    pub fn drop_stash(&mut self, reference: String, message: String) {
        self.confirm("Eliminar stash", &format!("¿Eliminar «{message}»? No se puede deshacer."), None, "Eliminar", true, move |app| {
            let Some(p) = app.path() else { return };
            let r = reference.clone();
            app.op("Eliminar stash", move || git::stash_drop(&p, &r), |app, _| app.ok("Stash eliminado"));
        });
    }

    pub fn do_fetch(&mut self, remote: Option<String>) {
        let Some(t) = self.tab_mut() else { return };
        t.last_fetch = Instant::now();
        let p = t.path.clone();
        let title = remote.as_ref().map_or("Fetch".to_string(), |r| format!("Fetch {r}"));
        self.remote_op(&title, move |h| git::fetch(&p, remote.as_deref(), true, Some(h)), |app, _| app.ok("Fetch completado"));
    }

    pub fn push_tag(&mut self, remote: String, name: String) {
        let Some(p) = self.path() else { return };
        let n = name.clone();
        self.remote_op("Enviar etiqueta", move |h| git::push_tag(&p, &remote, &n, Some(h)), move |app, _| app.ok(format!("Etiqueta {name} enviada")));
    }

    pub fn delete_remote_branch(&mut self, remote: String, branch: String) {
        let Some(p) = self.path() else { return };
        let label = format!("{remote}/{branch}");
        self.remote_op("Eliminar rama remota", move |h| git::delete_remote_branch(&p, &remote, &branch, Some(h)), move |app, _| app.ok(format!("{label} eliminada")));
    }

    pub fn delete_remote_tag(&mut self, remote: String, name: String) {
        let Some(p) = self.path() else { return };
        let n = name.clone();
        self.remote_op("Eliminar etiqueta remota", move |h| git::delete_remote_tag(&p, &remote, &n, Some(h)), move |app, _| app.ok(format!("Etiqueta {name} eliminada del remoto")));
    }

    pub fn run_custom_action(&mut self, idx: usize, sha: Option<String>, file: Option<String>) {
        let Some(a) = self.store.settings.custom_actions.get(idx).cloned() else { return };
        let Some(t) = self.tab() else { return };
        let ctx = extras::ActionContext {
            repo: t.path.clone(),
            sha,
            file,
            branch: t.refs.as_ref().and_then(|r| r.current_branch.clone()),
        };
        let name = a.name.clone();
        self.op(&a.name, move || extras::run_custom_action(&a.program, &a.args, &ctx), move |app, out| {
            if a.show_output {
                app.dialogs.push(Dialog::output(&name, &out));
            } else {
                app.ok(format!("«{name}» terminado"));
            }
        });
    }

    pub fn copy(&mut self, ctx: &eframe::egui::Context, text: String, msg: &str) {
        ctx.copy_text(text);
        self.ok(msg);
    }
}

pub fn with_orig(files: &[FileChange]) -> Vec<String> {
    let mut v: Vec<String> = files.iter().flat_map(|f| f.paths()).collect();
    v.sort();
    v.dedup();
    v
}

pub fn find_remote_ref(path: &Path) -> Option<RemoteRef> {
    let remotes = git::remotes_info(path).ok()?;
    let pick = remotes.iter().find(|r| r.name == "origin").or(remotes.first())?;
    RemoteRef::parse(&pick.url).or_else(|| remotes.iter().find_map(|r| RemoteRef::parse(&r.url)))
}

/// Adds a hint to common authentication failures.
pub fn friendly_error(msg: &str) -> String {
    let l = msg.to_lowercase();
    if l.contains("authentication failed") || l.contains("could not read username") || l.contains("terminal prompts disabled") || l.contains("403") && l.contains("denied") {
        format!("{msg}\n\nParece un problema de credenciales: inicia sesión desde «Cuentas» (arriba a la derecha).")
    } else if l.contains("permission denied (publickey)") || l.contains("host key verification failed") {
        format!("{msg}\n\nLa conexión SSH ha fallado: comprueba que tu clave SSH está cargada (ssh-agent) y añadida en el servidor, o usa la URL HTTPS e inicia sesión desde «Cuentas».")
    } else {
        msg.to_string()
    }
}
