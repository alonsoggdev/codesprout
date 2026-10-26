//! Application state, frame loop, tabs, toolbar, status bar, refresh and notifications.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codesprout_core::explorer::{self, ColumnMode};
use codesprout_core::{repo as git, OpHandle, RepoState};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, FontId, Key, Layout, Rect, RichText, Sense, Stroke};

use crate::bus::{Bus, BusSender};
use crate::dialogs::Dialog;
use crate::icons::{self, Icon};
use crate::repo_state::{RepoTab, View, WORKTREE};
use crate::store::{RepoEntry, Store, ThemePref};
use crate::theme;
use crate::widgets;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warn,
    Error,
}

/// Notice shown in a small modal in the centre of the window. Successes and info close by
/// themselves; warnings and errors stay until closed (`until` is `None`).
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Option<Instant>,
}

/// Local operation in progress (checkout, commit, merge…), shown in a small modal with a spinner:
/// right away when `immediate`, otherwise only if it takes long enough to be noticed.
pub struct Working {
    pub text: String,
    pub since: Instant,
    pub immediate: bool,
}

/// Network operation in progress (clone, fetch, push…), shown with its git output.
pub struct Progress {
    pub title: String,
    pub lines: Vec<String>,
    pub handle: OpHandle,
    pub done: Option<Result<(), String>>,
    pub started: Instant,
}

pub struct App {
    pub store: Store,
    pub tabs: Vec<RepoTab>,
    pub active: Option<usize>,
    pub bus: Bus,
    pub busy: Option<String>,
    pub working: Option<Working>,
    pub dialogs: Vec<Dialog>,
    pub toasts: Vec<Toast>,
    pub progress: Option<Progress>,
    pub git_version: Option<String>,
    pub gcm_version: Option<String>,
    pub lfs_version: Option<String>,
    watcher: Option<notify::RecommendedWatcher>,
    watch_key: Option<String>,
    watch_flag: Arc<AtomicBool>,
    pending_refresh: Option<Instant>,
    dirty_since: Option<Instant>,
    was_focused: bool,
    applied_dark: Option<bool>,
    pub focus_search: bool,
    pub update: crate::updater::UpdateState,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let store = Store::load();
        let mut app = App {
            store,
            tabs: vec![],
            active: None,
            bus: Bus::new(cc.egui_ctx.clone()),
            busy: None,
            working: None,
            dialogs: vec![],
            toasts: vec![],
            progress: None,
            git_version: None,
            gcm_version: None,
            lfs_version: None,
            watcher: None,
            watch_key: None,
            watch_flag: Arc::new(AtomicBool::new(false)),
            pending_refresh: None,
            dirty_since: None,
            was_focused: true,
            applied_dark: None,
            focus_search: false,
            update: Default::default(),
        };
        let tabs = app.store.tabs.clone();
        for t in &tabs {
            app.tabs.push(RepoTab::new(PathBuf::from(t)));
        }
        let active = app.store.active.clone();
        app.active = active.and_then(|a| app.tabs.iter().position(|t| t.key == a));
        app.bus.spawn(
            || (git::version(), codesprout_core::credentials::gcm_version(), codesprout_core::extras::lfs_version()),
            |app, (v, gcm, lfs)| {
                app.gcm_version = gcm;
                app.lfs_version = lfs;
                match v {
                    Ok(v) => app.git_version = Some(v),
                    Err(e) => app.error_dialog("Git no está disponible", &e.to_string()),
                }
            },
        );
        if app.active.is_some() {
            app.activate(app.active);
        }
        app
    }

    pub fn sender(&self) -> BusSender {
        self.bus.sender()
    }

    // ---------------------------------------------------------------- persistence

    pub fn persist(&mut self) {
        self.store.tabs = self.tabs.iter().map(|t| t.key.clone()).collect();
        self.store.active = self.active.and_then(|i| self.tabs.get(i)).map(|t| t.key.clone());
        if self.dirty_since.is_none() {
            self.dirty_since = Some(Instant::now());
        }
    }

    pub fn save_now(&mut self) {
        self.store.tabs = self.tabs.iter().map(|t| t.key.clone()).collect();
        self.store.active = self.active.and_then(|i| self.tabs.get(i)).map(|t| t.key.clone());
        self.store.save();
        self.dirty_since = None;
    }

    // ---------------------------------------------------------------- notifications

    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        let until = match kind {
            ToastKind::Error | ToastKind::Warn => None,
            _ => Some(Instant::now() + Duration::from_millis(2200)),
        };
        self.toasts.push(Toast { text: text.into(), kind, until });
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    pub fn ok(&mut self, text: impl Into<String>) {
        self.toast(text, ToastKind::Success);
    }

    pub fn error_dialog(&mut self, title: &str, msg: &str) {
        self.dialogs.push(Dialog::error(title, msg));
    }

    // ---------------------------------------------------------------- tabs

    pub fn tab(&self) -> Option<&RepoTab> {
        self.active.and_then(|i| self.tabs.get(i))
    }

    pub fn tab_mut(&mut self) -> Option<&mut RepoTab> {
        self.active.and_then(move |i| self.tabs.get_mut(i))
    }

    pub fn tab_by_key(&mut self, key: &str) -> Option<&mut RepoTab> {
        self.tabs.iter_mut().find(|t| t.key == key)
    }

    pub fn active_key(&self) -> Option<String> {
        self.tab().map(|t| t.key.clone())
    }

    pub fn active_path(&self) -> Option<PathBuf> {
        self.tab().map(|t| t.path.clone())
    }

    pub fn activate(&mut self, idx: Option<usize>) {
        self.active = idx.filter(|&i| i < self.tabs.len());
        self.persist();
        self.watch_active();
        if self.active.is_some() {
            self.refresh(false);
        }
    }

    pub fn add_repo_entry(&mut self, path: &Path) {
        let key = path.to_string_lossy().into_owned();
        self.store.repos.retain(|r| !same_path(&r.path, &key));
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| key.clone());
        self.store.repos.insert(0, RepoEntry { path: key, name });
        self.persist();
    }

    /// Opens (or switches to) the repository containing `dir`.
    pub fn open_repo(&mut self, dir: PathBuf) {
        self.bus.spawn(
            move || git::resolve_repo(&dir),
            |app, r| match r {
                Ok(p) => app.open_resolved(p),
                Err(e) => app.toast(e.to_string(), ToastKind::Error),
            },
        );
    }

    pub fn open_resolved(&mut self, p: PathBuf) {
        self.add_repo_entry(&p);
        let key = p.to_string_lossy().into_owned();
        let idx = match self.tabs.iter().position(|t| same_path(&t.key, &key)) {
            Some(i) => i,
            None => {
                self.tabs.push(RepoTab::new(p));
                self.tabs.len() - 1
            }
        };
        self.activate(Some(idx));
    }

    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        self.tabs.remove(idx);
        let next = match self.active {
            Some(a) if a == idx => {
                if self.tabs.is_empty() {
                    None
                } else {
                    Some(idx.min(self.tabs.len() - 1))
                }
            }
            Some(a) if a > idx => Some(a - 1),
            other => other,
        };
        self.activate(next);
    }

    pub fn remove_repo(&mut self, path: &str) {
        self.store.repos.retain(|r| r.path != path);
        if let Some(i) = self.tabs.iter().position(|t| t.key == path) {
            self.close_tab(i);
        }
        self.persist();
    }

    // ---------------------------------------------------------------- file watcher

    fn watch_active(&mut self) {
        let key = self.active_key();
        if key == self.watch_key {
            return;
        }
        self.watcher = None;
        self.watch_key = key.clone();
        let Some(key) = key else { return };
        let sender = self.sender();
        let flag = self.watch_flag.clone();
        let k = key.clone();
        let w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            let relevant = ev.paths.iter().any(|p| {
                let s = p.to_string_lossy().replace('\\', "/");
                !(s.contains("/.git/objects") || s.contains("/.git/logs") || s.ends_with(".lock") || s.contains("/.git/codesprout"))
            });
            if relevant && !flag.swap(true, Ordering::SeqCst) {
                let k = k.clone();
                sender.send(move |app| app.on_fs_change(&k));
            }
        });
        if let Ok(mut w) = w {
            use notify::Watcher;
            if w.watch(Path::new(&key), notify::RecursiveMode::Recursive).is_ok() {
                self.watcher = Some(w);
            }
        }
    }

    fn on_fs_change(&mut self, key: &str) {
        self.watch_flag.store(false, Ordering::SeqCst);
        if self.active_key().as_deref() == Some(key) {
            self.pending_refresh = Some(Instant::now() + Duration::from_millis(350));
        }
    }

    pub fn schedule_refresh(&mut self, ms: u64) {
        self.pending_refresh = Some(Instant::now() + Duration::from_millis(ms));
    }

    // ---------------------------------------------------------------- refresh

    pub fn refresh(&mut self, full: bool) {
        if let Some(i) = self.active {
            self.refresh_tab(i, full);
        }
    }

    pub fn refresh_tab(&mut self, idx: usize, full: bool) {
        let max = self.store.settings.max_commits;
        let all = self.store.settings.all_branches;
        // one commit per row next to its text, so always chronological
        let mode = ColumnMode::Chronological;
        let Some(t) = self.tabs.get_mut(idx) else { return };
        if t.refreshing {
            t.refresh_again = true;
            t.refresh_full |= full;
            return;
        }
        t.refreshing = true;
        let path = t.path.clone();
        let key = t.key.clone();
        let old_sig = t.refs.as_ref().map(|r| r.sig.clone());
        let loaded = t.loaded;
        let old_commits = t.commits.clone();
        let had_worktree = t.layout.worktree.is_some();
        let need_template = t.template.is_none();
        self.bus.spawn(
            move || -> anyhow::Result<_> {
                let status = git::status(&path)?;
                let refs = git::refs(&path)?;
                let reload = full || !loaded || Some(&refs.sig) != old_sig.as_ref();
                let commits = if reload { Arc::new(git::log(&path, max, all)?) } else { old_commits };
                let wt = status.change_count() > 0;
                let layout =
                    if reload || wt != had_worktree { Some(Arc::new(explorer::layout(&commits, &refs, wt, mode))) } else { None };
                let flow = codesprout_core::flow::config(&path);
                let template = if need_template { codesprout_core::extras::commit_template(&path) } else { None };
                Ok((status, refs, reload.then_some(commits), layout, flow, template))
            },
            move |app, r| app.apply_refresh(&key, r),
        );
    }

    #[allow(clippy::type_complexity)]
    fn apply_refresh(
        &mut self,
        key: &str,
        r: anyhow::Result<(
            codesprout_core::Status,
            codesprout_core::Refs,
            Option<Arc<Vec<codesprout_core::Commit>>>,
            Option<Arc<explorer::Layout>>,
            Option<codesprout_core::flow::FlowConfig>,
            Option<String>,
        )>,
    ) {
        let Some(idx) = self.tabs.iter().position(|t| t.key == key) else { return };
        let is_active = self.active == Some(idx);
        let t = &mut self.tabs[idx];
        t.refreshing = false;
        match r {
            Err(e) => t.error = Some(e.to_string()),
            Ok((status, refs, commits, layout, flow, template)) => {
                let had_conflicts = t.status.as_ref().is_some_and(|s| !s.conflicted.is_empty());
                if !had_conflicts && !status.conflicted.is_empty() {
                    let first = status.conflicted[0].path.clone();
                    t.sel = crate::repo_state::FileSel {
                        list: Some(crate::repo_state::ListKind::Conflicted),
                        paths: [first.clone()].into(),
                        focus: Some(first.clone()),
                        anchor: Some(first.clone()),
                    };
                    t.last_focus = Some(first);
                }
                if status.state.is_none() {
                    t.merge_prefilled = false;
                    if t.merge_prefill.as_ref().is_some_and(|m| *m == t.message) {
                        t.message.clear();
                    }
                    t.merge_prefill = None;
                } else if status.state != Some(RepoState::Rebase)
                    && !status.merge_msg.is_empty()
                    && t.message.trim().is_empty()
                    && !t.merge_prefilled
                {
                    t.message = status.merge_msg.clone();
                    t.merge_prefill = Some(status.merge_msg.clone());
                    t.merge_prefilled = true;
                }
                t.status = Some(status);
                t.refs = Some(refs);
                t.flow = flow;
                if template.is_some() {
                    t.template = template;
                }
                if let Some(c) = commits {
                    t.commit_index = c.iter().enumerate().map(|(i, c)| (c.hash.clone(), i)).collect();
                    t.commits = c;
                    t.loaded = true;
                }
                if let Some(l) = layout {
                    t.layout = l;
                }
                t.error = None;
                t.rebuild_ref_map();
                t.prune_selection();
            }
        }
        let again = std::mem::take(&mut self.tabs[idx].refresh_again);
        let full = std::mem::take(&mut self.tabs[idx].refresh_full);
        if is_active {
            self.after_refresh();
        }
        if again {
            self.refresh_tab(idx, full);
        }
    }

    /// Keeps selections, details and diffs consistent with fresh data.
    pub fn after_refresh(&mut self) {
        let Some(t) = self.tab_mut() else { return };
        if t.status.is_none() {
            return;
        }
        let valid = |t: &RepoTab, h: &str| {
            h == WORKTREE && t.layout.worktree.is_some()
                || t.commit_index.contains_key(h)
                || t.refs.as_ref().is_some_and(|r| r.stashes.iter().any(|s| s.hash == h))
        };
        if t.selected.as_ref().is_some_and(|h| !valid(t, h)) {
            t.selected = None;
        }
        if t.compare_with.as_ref().is_some_and(|h| !valid(t, h)) {
            t.compare_with = None;
        }
        if t.selected.is_none() {
            t.selected = t.refs.as_ref().and_then(|r| r.head.clone()).filter(|h| t.commit_index.contains_key(h));
            if t.selected.is_some() && !t.explorer.initialized {
                t.explorer.center_on = t.selected.clone();
            }
        }
        let key = t.key.clone();
        let view = t.view;
        let sel = t.selected.clone();
        let need_detail = sel.as_ref().is_some_and(|h| h != WORKTREE && t.detail.as_ref().map(|d| &d.hash) != Some(h));
        match view {
            View::History => {
                if need_detail {
                    self.load_detail(&key, sel.unwrap());
                }
            }
            View::Changes => self.load_changes_diff(&key),
            _ => {}
        }
    }

    // ---------------------------------------------------------------- frame

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if !self.dialogs.is_empty() || self.progress.is_some() || self.working.is_some() || !self.toasts.is_empty() {
            return;
        }
        let (f5, t, w, one, two, tab, shift, f) = ctx.input_mut(|i| {
            let m = egui::Modifiers::COMMAND;
            (
                i.consume_key(egui::Modifiers::NONE, Key::F5),
                i.consume_key(m, Key::T),
                i.consume_key(m, Key::W),
                i.consume_key(m, Key::Num1),
                i.consume_key(m, Key::Num2),
                i.consume_key(m, Key::Tab) || i.consume_key(m | egui::Modifiers::SHIFT, Key::Tab),
                i.modifiers.shift,
                i.consume_key(m, Key::F),
            )
        });
        if f5 {
            self.refresh(true);
        }
        if t {
            self.activate(None);
        }
        if w {
            if let Some(i) = self.active {
                self.close_tab(i);
            }
        }
        if one {
            self.set_view(View::Changes);
        }
        if two {
            self.set_view(View::History);
        }
        if tab && self.tabs.len() > 1 {
            let n = self.tabs.len();
            let i = self.active.unwrap_or(0);
            self.activate(Some(if shift { (i + n - 1) % n } else { (i + 1) % n }));
        }
        if f && self.active.is_some() {
            self.set_view(View::History);
            self.focus_search = true;
        }
    }

    pub fn set_view(&mut self, view: View) {
        let Some(t) = self.tab_mut() else { return };
        t.view = view;
        let key = t.key.clone();
        match view {
            View::Changes => {
                t.prune_selection();
                self.load_changes_diff(&key);
            }
            View::History => self.after_refresh(),
            View::Reflog => self.load_reflog(&key),
            View::Submodules => self.load_submodules(&key),
            View::Worktrees => self.load_worktrees(&key),
            View::Lfs => self.load_lfs(&key),
            View::PullRequests => self.load_prs(&key),
            _ => {}
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let dark = match self.store.settings.theme {
            ThemePref::Dark => true,
            ThemePref::Light => false,
            ThemePref::System => ctx.system_theme().map_or(true, |t| t == egui::Theme::Dark),
        };
        if self.applied_dark != Some(dark) {
            theme::apply(ctx, dark);
            self.applied_dark = Some(dark);
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        // window focus -> refresh (catches changes made in other tools)
        let focused = ctx.input(|i| i.focused);
        if focused && !self.was_focused {
            self.schedule_refresh(60);
        }
        self.was_focused = focused;
        if let Some(at) = self.pending_refresh {
            if now >= at && self.busy.is_none() {
                self.pending_refresh = None;
                self.refresh(false);
            } else {
                ctx.request_repaint_after(at.saturating_duration_since(now) + Duration::from_millis(5));
            }
        }
        // auto fetch
        let min = self.store.settings.auto_fetch_min;
        if min > 0 && self.busy.is_none() && self.progress.is_none() {
            if let Some(t) = self.tab_mut() {
                let has_remotes = t.refs.as_ref().is_some_and(|r| !r.remotes.is_empty());
                if has_remotes && t.last_fetch.elapsed() >= Duration::from_secs(min as u64 * 60) {
                    t.last_fetch = Instant::now();
                    let path = t.path.clone();
                    self.busy = Some("Fetch automático".into());
                    self.bus.spawn(
                        move || git::fetch(&path, None, true, None),
                        |app, _r| {
                            app.busy = None;
                            app.refresh(false);
                        },
                    );
                }
            }
            ctx.request_repaint_after(Duration::from_secs(30));
        }
        if self.dirty_since.is_some_and(|d| d.elapsed() > Duration::from_millis(600)) {
            self.save_now();
        } else if self.dirty_since.is_some() {
            ctx.request_repaint_after(Duration::from_millis(650));
        }
        self.toasts.retain(|t| t.until.is_none_or(|u| u > now));
        if let Some(t) = self.toasts.iter().filter_map(|t| t.until).min() {
            ctx.request_repaint_after(t.saturating_duration_since(now));
        }
        // folders dropped on the window
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if let Some(p) = dropped.into_iter().next() {
            self.open_repo(p);
        }
    }

    fn tab_bar(&mut self, ctx: &egui::Context) {
        let c = theme::p();
        egui::TopBottomPanel::top("tabs").exact_height(38.0).frame(egui::Frame::new().fill(c.bg).inner_margin(egui::Margin::symmetric(8, 0))).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let mut activate = None;
                let mut close = None;
                // tabs are reordered by dragging them: (dragged tab, pointer x) and every tab's centre
                let mut dragging: Option<(usize, f32)> = None;
                let mut centres = Vec::with_capacity(self.tabs.len());
                for (i, t) in self.tabs.iter().enumerate() {
                    let active = self.active == Some(i);
                    let name = t.name();
                    let dirty = t.change_count() > 0;
                    let font = FontId::proportional(13.0);
                    let w = ui.painter().layout_no_wrap(name.clone(), font.clone(), c.text).size().x;
                    let (rect, _) = ui.allocate_exact_size(vec2((w + 64.0).min(240.0), 30.0), Sense::hover());
                    // the id follows the repository, not the position, so a drag survives the reorder
                    let resp = ui.interact(rect, egui::Id::new(("tab", &t.key)), Sense::click_and_drag());
                    centres.push(rect.center().x);
                    if resp.dragged() {
                        if let Some(p) = resp.interact_pointer_pos() {
                            dragging = Some((i, p.x));
                        }
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    }
                    let fill = if active || resp.dragged() { c.surface } else if resp.hovered() { c.hover } else { Color32::TRANSPARENT };
                    ui.painter().rect_filled(rect, egui::CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 }, fill);
                    if active {
                        ui.painter().line_segment([rect.left_top() + vec2(6.0, 0.5), rect.right_top() + vec2(-6.0, 0.5)], Stroke::new(2.0_f32, c.accent));
                    }
                    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), vec2(13.0, 13.0)), Icon::Box, if active { c.accent } else { c.muted });
                    let text_rect = Rect::from_min_max(pos2(rect.min.x + 27.0, rect.min.y), pos2(rect.max.x - 34.0, rect.max.y));
                    ui.painter().with_clip_rect(text_rect).text(
                        pos2(text_rect.min.x, rect.center().y),
                        Align2::LEFT_CENTER,
                        &name,
                        font,
                        if active { c.text } else { c.text2 },
                    );
                    if dirty {
                        ui.painter().circle_filled(pos2(rect.max.x - 30.0, rect.center().y), 3.0, c.yellow);
                    }
                    let close_rect = Rect::from_center_size(pos2(rect.max.x - 14.0, rect.center().y), vec2(18.0, 18.0));
                    // the tab takes the click; on the "x" it closes the tab
                    let over_close = resp.hover_pos().is_some_and(|p| close_rect.contains(p));
                    if over_close {
                        ui.painter().rect_filled(close_rect, 4.0, c.active);
                    }
                    if resp.hovered() || active {
                        icons::paint(ui.painter(), close_rect.shrink(5.0), Icon::Close, c.muted);
                    }
                    let clicked_close = resp.clicked() && resp.interact_pointer_pos().is_some_and(|p| close_rect.contains(p));
                    if clicked_close || resp.middle_clicked() {
                        close = Some(i);
                    } else if resp.clicked() {
                        activate = Some(i);
                    }
                    let path = t.key.clone();
                    resp.on_hover_text(&path).context_menu(|ui| {
                        if ui.button("Cerrar pestaña").clicked() {
                            close = Some(i);
                            ui.close_menu();
                        }
                        if ui.button("Mostrar en el explorador").clicked() {
                            let _ = open::that(&path);
                            ui.close_menu();
                        }
                        if ui.button("Abrir terminal aquí").clicked() {
                            crate::actions::open_terminal(Path::new(&path));
                            ui.close_menu();
                        }
                        if ui.button("Copiar ruta").clicked() {
                            ui.ctx().copy_text(path.clone());
                            ui.close_menu();
                        }
                    });
                }
                let home_active = self.active.is_none();
                let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 30.0), Sense::click());
                if home_active || resp.hovered() {
                    ui.painter().rect_filled(rect, egui::CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 }, if home_active { c.surface } else { c.hover });
                }
                icons::paint(ui.painter(), rect.shrink(9.0), if self.tabs.is_empty() { Icon::Home } else { Icon::Plus }, c.text2);
                if resp.on_hover_text("Repositorios (Ctrl+T)").clicked() {
                    activate = None;
                    self.activate(None);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let n = self.store.accounts.len();
                    let label = if n == 0 { "Iniciar sesión".to_string() } else { widgets::plural(n, "cuenta", "cuentas") };
                    if widgets::icon_text_button(ui, Icon::User, &label, true).clicked() {
                        self.dialogs.push(Dialog::accounts());
                    }
                    ui.add_space(6.0);
                    self.update_button(ui);
                });
                if let Some((from, x)) = dragging {
                    let to = centres.iter().enumerate().filter(|&(j, &cx)| j != from && cx < x).count();
                    if to != from {
                        let active_key = self.tab().map(|t| t.key.clone());
                        let tab = self.tabs.remove(from);
                        self.tabs.insert(to, tab);
                        self.active = active_key.and_then(|k| self.tabs.iter().position(|t| t.key == k));
                        self.persist();
                    }
                }
                if let Some(i) = close {
                    self.close_tab(i);
                } else if let Some(i) = activate {
                    if self.active != Some(i) {
                        self.activate(Some(i));
                    }
                }
            });
        });
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        let c = theme::p();
        let Some(t) = self.tab() else { return };
        let busy = self.busy.is_some();
        let ready = t.status.is_some();
        let n = t.change_count();
        let has_head = t.has_head();
        let remotes = t.refs.as_ref().map_or(0, |r| r.remotes.len());
        let cur = t.refs.as_ref().and_then(|r| r.current_local()).cloned();
        let branch_label = t.branch_label();
        let name = t.name();
        let flow_ready = t.flow.is_some();
        let has_hosting = t.prs.remote.is_some();
        let mut action: Option<&'static str> = None;
        egui::TopBottomPanel::top("toolbar")
            .exact_height(62.0)
            .frame(egui::Frame::new().fill(c.surface).inner_margin(egui::Margin::symmetric(10, 4)).stroke(Stroke::new(1.0_f32, c.border)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let mut tb = |ui: &mut egui::Ui, icon: Icon, label: &str, badge: Option<u32>, enabled: bool, tip: &str, id: &'static str| {
                        if tool_button(ui, icon, label, badge, enabled, tip).clicked() {
                            action = Some(id);
                        }
                    };
                    let no_remote = "No hay remotos configurados";
                    tb(ui, Icon::Commit, "Commit", (n > 0).then_some(n as u32), ready, "Ir a los cambios (Ctrl+1)", "commit");
                    tb(ui, Icon::Pull, "Pull", cur.as_ref().map(|b| b.behind).filter(|&b| b > 0), !busy && remotes > 0, if remotes > 0 { "Traer y fusionar cambios del remoto" } else { no_remote }, "pull");
                    tb(ui, Icon::Push, "Push", cur.as_ref().map(|b| b.ahead).filter(|&a| a > 0), !busy && remotes > 0, if remotes > 0 { "Enviar commits al remoto" } else { no_remote }, "push");
                    tb(ui, Icon::Fetch, "Fetch", None, !busy && remotes > 0, if remotes > 0 { "Descargar novedades de todos los remotos" } else { no_remote }, "fetch");
                    separator(ui);
                    tb(ui, Icon::Branch, "Rama", None, !busy && has_head, "Nueva rama", "branch");
                    tb(ui, Icon::Merge, "Merge", None, !busy && has_head, "Fusionar otra rama en la actual", "merge");
                    tb(ui, Icon::Stash, "Stash", None, !busy && n > 0 && has_head, "Guardar los cambios temporalmente", "stash");
                    tb(ui, Icon::Discard, "Descartar", None, !busy && n > 0, "Descartar todos los cambios", "discard");
                    separator(ui);
                    tb(ui, Icon::Flow, "Flow", None, !busy && has_head, if flow_ready { "git-flow: features, releases y hotfixes" } else { "Inicializar git-flow" }, "flow");
                    tb(ui, Icon::PullRequest, "Pull request", None, !busy && has_head, if has_hosting { "Crear una pull request" } else { "Crear una pull request (GitHub, GitLab, Bitbucket, Azure)" }, "pr");
                    tb(ui, Icon::Terminal, "Terminal", None, true, "Abrir un terminal en el repositorio", "terminal");
                    tb(ui, Icon::Folder, "Explorador", None, true, "Abrir la carpeta", "folder");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        tb(ui, Icon::Settings, "Ajustes", None, true, "Ajustes", "settings");
                        tb(ui, Icon::Refresh, "Actualizar", None, true, "Actualizar (F5)", "refresh");
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.add_space(10.0);
                            ui.with_layout(Layout::top_down(Align::Max), |ui| {
                                ui.label(RichText::new(&name).strong().size(14.0));
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&branch_label).size(12.0).color(c.text2));
                                    icons::show(ui, Icon::Branch, 12.0, c.muted);
                                });
                            });
                        });
                    });
                });
            });
        match action {
            Some("commit") => self.set_view(View::Changes),
            Some("pull") => self.open_pull(),
            Some("push") => self.open_push(None),
            Some("fetch") => self.do_fetch(None),
            Some("branch") => self.open_new_branch(None),
            Some("merge") => self.open_merge(),
            Some("stash") => self.dialogs.push(Dialog::stash()),
            Some("discard") => self.open_discard_all(),
            Some("flow") => self.open_flow(),
            Some("pr") => self.open_create_pr(None),
            Some("terminal") => {
                if let Some(p) = self.active_path() {
                    crate::actions::open_terminal(&p);
                }
            }
            Some("folder") => {
                if let Some(p) = self.active_path() {
                    let _ = open::that(p);
                }
            }
            Some("settings") => self.dialogs.push(Dialog::settings(self)),
            Some("refresh") => self.refresh(true),
            _ => {}
        }
    }

    fn banner(&mut self, ctx: &egui::Context) {
        let c = theme::p();
        let Some(t) = self.tab() else { return };
        let Some(s) = &t.status else { return };
        let Some(state) = s.state else { return };
        let n = s.conflicted.len();
        let text = if n > 0 {
            format!("{} — {} conflictos. Resuélvelos y márcalos como resueltos.", state.label(), widgets::plural(n, "archivo tiene", "archivos tienen"))
        } else {
            format!("{} — no quedan conflictos, ya puedes continuar.", state.label())
        };
        let busy = self.busy.is_some();
        let mut act = None;
        egui::TopBottomPanel::top("banner").frame(egui::Frame::new().fill(c.yellow_soft).inner_margin(egui::Margin::symmetric(14, 8))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                icons::show(ui, Icon::Alert, 18.0, c.yellow);
                ui.label(RichText::new(text).color(c.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if state == RepoState::Merge { "Confirmar fusión" } else { "Continuar" };
                    if widgets::primary_button(ui, label, n == 0 && !busy).clicked() {
                        act = Some("continue");
                    }
                    if state == RepoState::Rebase && ui.button("Saltar commit").clicked() {
                        act = Some("skip");
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Abortar")).clicked() {
                        act = Some("abort");
                    }
                    if n > 0 && ui.button("Ver conflictos").clicked() {
                        act = Some("view");
                    }
                });
            });
        });
        match act {
            Some("continue") => self.continue_operation(state),
            Some("skip") => self.skip_rebase(),
            Some("abort") => self.abort_operation(state),
            Some("view") => self.set_view(View::Changes),
            _ => {}
        }
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        let c = theme::p();
        egui::TopBottomPanel::bottom("status").exact_height(26.0).frame(egui::Frame::new().fill(c.bg).inner_margin(egui::Margin::symmetric(12, 0))).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let small = |t: String| RichText::new(t).size(12.0).color(c.text2);
                if let Some(t) = self.tab() {
                    if let Some(r) = &t.refs {
                        ui.horizontal(|ui| {
                            icons::show(ui, Icon::Branch, 12.0, c.muted);
                            ui.label(small(t.branch_label()));
                        });
                        if let Some(b) = r.current_local() {
                            ui.horizontal(|ui| {
                                icons::show(ui, Icon::Cloud, 13.0, c.muted);
                                match &b.upstream {
                                    Some(u) if b.gone => ui.label(small(format!("{u} (eliminada en el remoto)"))),
                                    Some(u) => ui.label(small(format!("{u} · ↑{} ↓{}", b.ahead, b.behind))),
                                    None => ui.label(small("Sin rama de seguimiento".into())),
                                };
                            });
                        }
                    }
                }
                if let Some(b) = &self.busy {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.label(small(format!("{b}…")));
                    });
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(t) = self.tab() {
                        ui.label(small(t.key.clone()));
                        if t.loaded {
                            let capped = t.commits.len() >= self.store.settings.max_commits;
                            let text = format!("{} commits{}", t.commits.len(), if capped { " (límite)" } else { "" });
                            let r = ui.label(small(text));
                            if capped {
                                r.on_hover_text("Límite configurable en Ajustes");
                            }
                        }
                    } else if let Some(v) = &self.git_version {
                        ui.label(small(v.clone()));
                    }
                });
            });
        });
    }

    /// Notices in a small modal in the centre. Clicking outside or Esc closes them, and so does
    /// "Cerrar" for warnings and errors, which don't close by themselves.
    fn toasts_ui(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() || self.working.is_some() {
            return;
        }
        let c = theme::p();
        let sticky = self.toasts.iter().any(|t| t.until.is_none());
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("notice")).show(ctx, |ui| {
            ui.set_width(360.0);
            for (i, t) in self.toasts.iter().enumerate() {
                if i > 0 {
                    ui.add_space(8.0);
                }
                let (icon, color) = match t.kind {
                    ToastKind::Success => (Icon::Check, c.green),
                    ToastKind::Info => (Icon::Eye, c.accent),
                    ToastKind::Warn => (Icon::Alert, c.yellow),
                    ToastKind::Error => (Icon::Alert, c.red),
                };
                ui.horizontal_top(|ui| {
                    icons::show(ui, icon, 18.0, color);
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(&t.text).size(14.0).color(c.text)).wrap());
                });
            }
            if sticky {
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::primary_button(ui, "Cerrar", true).clicked() {
                        close = true;
                    }
                });
            }
        });
        if close || modal.should_close() {
            self.toasts.clear();
        }
    }

    /// Spinner modal while a local operation runs (see [`Working`]).
    fn working_ui(&mut self, ctx: &egui::Context) {
        let Some(w) = &self.working else { return };
        let delay = Duration::from_millis(300);
        if !w.immediate && w.since.elapsed() < delay {
            ctx.request_repaint_after(delay - w.since.elapsed());
            return;
        }
        egui::Modal::new(egui::Id::new("working")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(20.0));
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(&w.text).size(14.0)).wrap());
            });
        });
    }

    /// Network operation in progress: a plain-language description, a progress bar when git
    /// reports a percentage, and git's own output folded away under "Ver detalles técnicos".
    fn progress_ui(&mut self, ctx: &egui::Context) {
        let Some(p) = &mut self.progress else { return };
        let c = theme::p();
        let mut close = false;
        egui::Modal::new(egui::Id::new("progress")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                match &p.done {
                    None => {
                        ui.add(egui::Spinner::new().size(18.0));
                    }
                    Some(Ok(())) => {
                        icons::show(ui, Icon::Check, 18.0, c.green);
                    }
                    Some(Err(_)) => {
                        icons::show(ui, Icon::Alert, 18.0, c.red);
                    }
                }
                ui.heading(&p.title);
            });
            ui.add_space(8.0);
            match &p.done {
                None => {
                    ui.label(RichText::new(progress_message(&p.title)).color(c.text2));
                    ui.add_space(6.0);
                    match git_stage(&p.lines) {
                        Some((stage, Some(pct))) => {
                            ui.add(egui::ProgressBar::new(pct).text(format!("{stage}… {} %", (pct * 100.0).round())));
                        }
                        Some((stage, None)) => {
                            ui.label(widgets::small(format!("{stage}…")));
                        }
                        None => {
                            ui.label(widgets::small("Conectando con el servidor…"));
                        }
                    }
                }
                Some(Ok(())) => {
                    ui.label("Completado.");
                }
                Some(Err(e)) => widgets::error_box(ui, e),
            }
            ui.add_space(8.0);
            egui::CollapsingHeader::new(widgets::small("Ver detalles técnicos")).id_salt("progress-log").default_open(false).show(ui, |ui| {
                egui::Frame::new().fill(c.bg).corner_radius(6).inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(180.0).stick_to_bottom(true).show(ui, |ui| {
                        if p.lines.is_empty() {
                            ui.label(widgets::small("Sin salida todavía."));
                        }
                        for l in p.lines.iter().rev().take(200).rev() {
                            ui.label(RichText::new(l).font(theme::mono(11.5)).color(c.text2));
                        }
                    });
                });
            });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &p.done {
                None => {
                    if ui.button("Cancelar").clicked() {
                        p.handle.cancel();
                    }
                    ui.label(widgets::small(format!("{} s", p.started.elapsed().as_secs())));
                    ctx.request_repaint_after(Duration::from_millis(500));
                }
                Some(_) => {
                    if widgets::primary_button(ui, "Cerrar", true).clicked() {
                        close = true;
                    }
                }
            });
        });
        if close {
            self.progress = None;
        }
    }
}

/// What a network operation does, in words for someone who doesn't know Git.
fn progress_message(title: &str) -> &'static str {
    let t = title.to_lowercase();
    if t.starts_with("pull") {
        "Descargando los cambios del servidor y añadiéndolos a tu rama."
    } else if t.starts_with("push") {
        "Enviando tus commits al servidor."
    } else if t.starts_with("fetch") {
        "Comprobando si hay novedades en el servidor."
    } else if t.starts_with("clonar") {
        "Descargando una copia del repositorio."
    } else if t.contains("submódulo") {
        "Descargando los submódulos del repositorio."
    } else if t.contains("lfs") {
        "Descargando los archivos grandes (LFS)."
    } else if t.contains("etiqueta") && t.starts_with("enviar") {
        "Enviando la etiqueta al servidor."
    } else {
        "Aplicando el cambio en el servidor."
    }
}

/// Current step of a git network command and its percentage, from the newest progress line
/// git printed ("Receiving objects:  45% (450/1000)") translated to plain words.
fn git_stage(lines: &[String]) -> Option<(&'static str, Option<f32>)> {
    lines.iter().rev().find_map(|line| {
        let line = line.trim().trim_start_matches("remote:").trim();
        let (phase, rest) = line.split_once(':')?;
        let stage = match phase.trim() {
            "Enumerating objects" | "Counting objects" => "Preparando",
            "Compressing objects" => "Comprimiendo",
            "Receiving objects" => "Descargando",
            "Writing objects" => "Subiendo",
            "Resolving deltas" => "Procesando",
            "Updating files" | "Checking out files" => "Actualizando archivos",
            "Filtering content" => "Descargando archivos grandes",
            _ => return None,
        };
        let pct = rest.split('%').next().and_then(|s| s.trim().parse::<f32>().ok()).filter(|_| rest.contains('%'));
        Some((stage, pct.map(|p| (p / 100.0).clamp(0.0, 1.0))))
    })
}

pub fn same_path(a: &str, b: &str) -> bool {
    if cfg!(target_os = "linux") {
        a == b
    } else {
        a.eq_ignore_ascii_case(b)
    }
}

fn separator(ui: &mut egui::Ui) {
    let c = theme::p();
    let (rect, _) = ui.allocate_exact_size(vec2(13.0, 36.0), Sense::hover());
    ui.painter().line_segment([rect.center_top(), rect.center_bottom()], Stroke::new(1.0_f32, c.border));
}

/// Toolbar button: icon over a label, with an optional count badge.
fn tool_button(ui: &mut egui::Ui, icon: Icon, label: &str, badge: Option<u32>, enabled: bool, tip: &str) -> egui::Response {
    let c = theme::p();
    let font = FontId::proportional(11.5);
    let w = ui.painter().layout_no_wrap(label.to_string(), font.clone(), c.text).size().x.max(30.0) + 16.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 52.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 7.0, c.hover);
    }
    let color = if enabled { c.text2 } else { c.muted.gamma_multiply(0.55) };
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.center().x, rect.min.y + 18.0), vec2(20.0, 20.0)), icon, if enabled && resp.hovered() { c.text } else { color });
    ui.painter().text(pos2(rect.center().x, rect.max.y - 11.0), Align2::CENTER_CENTER, label, font, color);
    if let Some(b) = badge {
        let text = b.to_string();
        let pos = pos2(rect.center().x + 13.0, rect.min.y + 8.0);
        let gw = ui.painter().layout_no_wrap(text.clone(), FontId::proportional(10.0), Color32::WHITE).size().x;
        let br = Rect::from_center_size(pos, vec2((gw + 8.0).max(16.0), 15.0));
        ui.painter().rect_filled(br, 7.5, c.accent);
        ui.painter().text(pos, Align2::CENTER_CENTER, text, FontId::proportional(10.0), Color32::WHITE);
    }
    resp.on_hover_text(tip)
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        for apply in self.bus.drain() {
            apply(self);
        }
        self.apply_theme(ctx);
        self.tick(ctx);
        self.update_tick(ctx);
        self.handle_shortcuts(ctx);
        self.tab_bar(ctx);
        if self.active.is_some() {
            self.toolbar(ctx);
            self.banner(ctx);
        }
        self.status_bar(ctx);
        let actions: Vec<(usize, String, crate::store::ActionScope)> =
            self.store.settings.custom_actions.iter().enumerate().map(|(i, a)| (i, a.name.clone(), a.scope)).collect();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("custom-actions"), actions));
        if self.active.is_some() {
            crate::views::sidebar::show(self, ctx);
            crate::views::central(self, ctx);
        } else {
            crate::views::home::show(self, ctx);
        }
        self.show_dialogs(ctx);
        self.progress_ui(ctx);
        self.working_ui(ctx);
        self.toasts_ui(ctx);
        // remember the window geometry
        let (rect, maximized) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().maximized.unwrap_or(false)));
        if let Some(r) = rect {
            let w = [r.min.x, r.min.y, r.width(), r.height()];
            if !maximized && self.store.settings.window != Some(w) {
                self.store.settings.window = Some(w);
            }
        }
        self.store.settings.maximized = maximized;
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.save_now();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_now();
    }
}
