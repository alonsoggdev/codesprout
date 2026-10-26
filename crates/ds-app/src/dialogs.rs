//! Modal dialogs. Only the top dialog is shown; a dialog can push another on top of itself
//! (for example a confirmation) and stays underneath until that one closes.

use std::path::PathBuf;

use ds_core::credentials::{self, Provider};
use ds_core::flow::{self, FinishOptions, FlowConfig, FlowKind};
use ds_core::hosting::{self, HostedRepo, RemoteRef};
use ds_core::rebase::{self, TodoAction, TodoEntry};
use ds_core::repo::{self as git, PushArgs};
use ds_core::{extras, Commit, ResetMode};
use eframe::egui::{self, Align, Key, Layout, RichText};

use crate::actions::{find_remote_ref, pick_folder};
use crate::app::{App, ToastKind};
use crate::store::{ActionScope, CustomAction, ThemePref};
use crate::theme;
use crate::widgets::{self, field_label, text_field};

type Action = Box<dyn FnOnce(&mut App)>;

pub struct SettingsState {
    theme: ThemePref,
    max_commits: String,
    auto_fetch: u32,
    chronological: bool,
    word_diff: bool,
    sign_commits: bool,
    name: String,
    email: String,
    global: bool,
    diff_tool: String,
    merge_tool: String,
    actions: Vec<CustomAction>,
    repo: Option<PathBuf>,
    original: (String, String, String, String),
}

pub struct RebaseRow {
    commit: Commit,
    action: usize,
    message: String,
}

const REBASE_ACTIONS: [&str; 6] = ["Mantener", "Cambiar mensaje", "Combinar (squash)", "Combinar sin mensaje (fixup)", "Parar para editar", "Eliminar"];

pub enum Dialog {
    Error { title: String, msg: String },
    IdentityNeeded { msg: String, name: String, email: String, global: bool, error: Option<String> },
    Confirm { title: String, msg: String, detail: Option<String>, label: String, danger: bool, action: Option<Action> },
    Output { title: String, text: String },
    Stash { msg: String, untracked: bool, keep_index: bool },
    Settings(Box<SettingsState>),
    Accounts { host: String, provider: Provider, pending: Option<String>, error: Option<String>, gcm: Option<bool> },
    CheckoutRemote { remote_ref: String, name: String, exists: bool, error: Option<String> },
    NewBranch { name: String, start: Option<String>, start_label: String, checkout: bool, error: Option<String> },
    RenameBranch { old: String, new: String, error: Option<String> },
    DeleteBranch { name: String, force: bool, remote_too: bool, upstream: Option<(String, String, String)> },
    Merge { options: Vec<String>, target: String, fixed: Option<String>, no_ff: bool },
    Reset { hash: String, label: String, mode: ResetMode },
    Tag { hash: String, name: String, msg: String, push: bool, remote: String, remotes: Vec<String>, sign: bool, error: Option<String> },
    DeleteTag { name: String, remote: Option<String>, remote_too: bool },
    DiscardAll { tracked: usize, untracked: usize, with_untracked: bool },
    Pull { local: String, remotes: Vec<String>, remote: String, branches: Vec<(String, String)>, branch: String, rebase: bool, error: Option<String> },
    Push { locals: Vec<String>, local: String, remotes: Vec<String>, remote: String, remote_branch: String, upstream: bool, tags: bool, force: bool, error: Option<String> },
    AddRemote { name: String, url: String, error: Option<String> },
    EditRemote { name: String, new_name: String, url: String, old_url: String, error: Option<String> },
    Clone { url: String, parent: String, name: String, touched: bool, provider: Option<Provider>, repos: Option<Vec<HostedRepo>>, loading: bool, filter: String, use_ssh: bool, error: Option<String> },
    InteractiveRebase { base: Option<String>, base_label: String, rows: Option<Vec<RebaseRow>>, has_merges: bool, error: Option<String> },
    FlowInit { cfg: FlowConfig, branches: Vec<String>, error: Option<String> },
    Flow { cfg: FlowConfig, kind: FlowKind, name: String, keep: bool, tag_msg: String, error: Option<String> },
    CreatePr { remote: Option<RemoteRef>, title: String, body: String, source: String, target: String, locals: Vec<String>, targets: Vec<String>, draft: bool, error: Option<String> },
    LfsTrack { pattern: String, error: Option<String> },
    SubmoduleAdd { url: String, path: String, error: Option<String> },
    WorktreeAdd { path: String, branch: String, new_branch: bool, error: Option<String> },
}

enum Out {
    Keep,
    Close,
}

impl Dialog {
    pub fn error(title: &str, msg: &str) -> Self {
        Dialog::Error { title: title.into(), msg: msg.into() }
    }
    pub fn output(title: &str, text: &str) -> Self {
        Dialog::Output { title: title.into(), text: text.into() }
    }
    pub fn identity_needed(msg: &str) -> Self {
        Dialog::IdentityNeeded { msg: msg.into(), name: String::new(), email: String::new(), global: true, error: None }
    }
    pub fn stash() -> Self {
        Dialog::Stash { msg: String::new(), untracked: true, keep_index: false }
    }
    pub fn accounts() -> Self {
        Dialog::Accounts { host: "github.com".into(), provider: Provider::GitHub, pending: None, error: None, gcm: None }
    }
    pub fn checkout_remote(remote_ref: String, name: String, exists: bool) -> Self {
        Dialog::CheckoutRemote { remote_ref, name, exists, error: None }
    }
    pub fn settings(app: &App) -> Self {
        let s = &app.store.settings;
        let repo = app.active_path();
        let (name, email, global, diff_tool, merge_tool) = match &repo {
            Some(p) => {
                let id = git::identity(p);
                let global = !id.name.is_empty() && id.name == id.global_name;
                (id.name, id.email, global, git::config_value(p, "diff.tool"), git::config_value(p, "merge.tool"))
            }
            None => (String::new(), String::new(), true, String::new(), String::new()),
        };
        Dialog::Settings(Box::new(SettingsState {
            theme: s.theme,
            max_commits: s.max_commits.to_string(),
            auto_fetch: s.auto_fetch_min,
            chronological: s.chronological,
            word_diff: s.word_diff,
            sign_commits: s.sign_commits,
            original: (name.clone(), email.clone(), diff_tool.clone(), merge_tool.clone()),
            name,
            email,
            global,
            diff_tool,
            merge_tool,
            actions: s.custom_actions.clone(),
            repo,
        }))
    }
}

fn err(ui: &mut egui::Ui, e: &Option<String>) {
    if let Some(e) = e {
        ui.add_space(4.0);
        widgets::error_box(ui, e);
    }
}

/// Right-aligned button row. Returns (cancel, primary) clicks.
fn buttons(ui: &mut egui::Ui, primary: &str, danger: bool, enabled: bool) -> (bool, bool) {
    ui.add_space(10.0);
    let mut res = (false, false);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let ok = if danger { widgets::danger_button(ui, primary, enabled) } else { widgets::primary_button(ui, primary, enabled) };
        res.1 = ok.clicked();
        res.0 = ui.button("Cancelar").clicked();
    });
    res
}

fn check(ui: &mut egui::Ui, v: &mut bool, text: &str) {
    ui.checkbox(v, text);
}

impl App {
    pub fn confirm(&mut self, title: &str, msg: &str, detail: Option<String>, label: &str, danger: bool, action: impl FnOnce(&mut App) + 'static) {
        self.dialogs.push(Dialog::Confirm { title: title.into(), msg: msg.into(), detail, label: label.into(), danger, action: Some(Box::new(action)) });
    }

    pub fn show_dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialogs.pop() else { return };
        let below = self.dialogs.len();
        let esc = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut out = Out::Keep;
        let width = match &d {
            Dialog::Settings(_) | Dialog::InteractiveRebase { .. } | Dialog::Clone { .. } => 640.0,
            Dialog::CreatePr { .. } | Dialog::Output { .. } | Dialog::Accounts { .. } => 560.0,
            _ => 460.0,
        };
        let c = theme::p();
        egui::Modal::new(egui::Id::new(("dialog", below))).frame(egui::Frame::popup(&ctx.style()).inner_margin(20)).show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 8.0;
            out = self.dialog_body(ui, &mut d);
            let _ = c;
        });
        if esc && !matches!(d, Dialog::Accounts { pending: Some(_), .. }) {
            out = Out::Close;
        }
        if let Out::Keep = out {
            self.dialogs.insert(below, d);
        }
    }

    fn dialog_body(&mut self, ui: &mut egui::Ui, d: &mut Dialog) -> Out {
        let busy = self.busy.is_some();
        let path = self.active_path();
        match d {
            Dialog::Error { title, msg } => {
                ui.heading(title.as_str());
                widgets::error_box(ui, msg);
                ui.add_space(8.0);
                let mut close = false;
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Copiar").clicked() {
                        ui.ctx().copy_text(msg.clone());
                    }
                    close = widgets::primary_button(ui, "Aceptar", true).clicked();
                });
                if close {
                    return Out::Close;
                }
            }
            Dialog::Output { title, text } => {
                ui.heading(title.as_str());
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut text.as_str()).font(theme::mono(12.0)).desired_width(f32::INFINITY));
                });
                if widgets::primary_button(ui, "Cerrar", true).clicked() {
                    return Out::Close;
                }
            }
            Dialog::IdentityNeeded { msg, name, email, global, error } => {
                ui.heading("Identidad de Git");
                widgets::error_box(ui, msg);
                ui.label("Git necesita tu nombre y tu email para firmar los commits.");
                field_label(ui, "Nombre");
                text_field(ui, name, "Tu nombre");
                field_label(ui, "Email");
                text_field(ui, email, "tu@email.com");
                check(ui, global, "Usar en todos los repositorios (global)");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Guardar", false, true);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    if name.trim().is_empty() || email.trim().is_empty() {
                        *error = Some("Nombre y email son obligatorios.".into());
                    } else if let Some(p) = &path {
                        match git::set_identity(p, name, email, *global) {
                            Ok(()) => {
                                self.ok("Identidad guardada");
                                return Out::Close;
                            }
                            Err(e) => *error = Some(e.to_string()),
                        }
                    }
                }
            }
            Dialog::Confirm { title, msg, detail, label, danger, action } => {
                ui.heading(title.as_str());
                ui.label(msg.as_str());
                if let Some(dt) = detail {
                    ui.label(widgets::small(dt.as_str()));
                }
                let (cancel, ok) = buttons(ui, label, *danger, true);
                if cancel {
                    return Out::Close;
                }
                if ok || ui.input(|i| i.key_pressed(Key::Enter)) {
                    if let Some(a) = action.take() {
                        a(self);
                    }
                    return Out::Close;
                }
            }
            Dialog::Stash { msg, untracked, keep_index } => {
                ui.heading("Guardar cambios en stash");
                ui.label("Guarda temporalmente tus cambios y deja el directorio de trabajo limpio.");
                field_label(ui, "Mensaje");
                text_field(ui, msg, "Descripción (opcional)");
                check(ui, untracked, "Incluir archivos sin seguimiento");
                check(ui, keep_index, "Mantener los cambios preparados en el índice");
                let (cancel, ok) = buttons(ui, "Guardar stash", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let (Some(p), m, u, k) = (path, msg.trim().to_string(), *untracked, *keep_index) else { return Out::Close };
                    self.op("Stash", move || git::stash_save(&p, &m, u, k), |app, _| app.ok("Cambios guardados en stash"));
                    return Out::Close;
                }
            }
            Dialog::CheckoutRemote { remote_ref, name, exists, error } => {
                ui.heading("Crear rama local");
                ui.label(format!("Se creará una rama local que sigue a {remote_ref} y se cambiará a ella."));
                if *exists {
                    ui.label(widgets::small("Ya existe una rama local con ese nombre que no sigue a esta rama remota."));
                }
                field_label(ui, "Nombre de la rama local");
                text_field(ui, name, "");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Crear y cambiar", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let (Some(p), r, n) = (path, remote_ref.clone(), name.trim().to_string()) else { return Out::Close };
                    self.op("Checkout", move || git::checkout_remote(&p, &r, &n), |app, _| app.ok("Rama creada"));
                    return Out::Close;
                }
            }
            Dialog::NewBranch { name, start, start_label, checkout, error } => {
                ui.heading("Nueva rama");
                field_label(ui, "Nombre");
                let r = text_field(ui, name, "feature/mi-rama");
                r.request_focus();
                if name.contains(char::is_whitespace) {
                    *name = name.replace(char::is_whitespace, "-");
                }
                ui.label(widgets::small(format!("Desde: {start_label}")));
                check(ui, checkout, "Cambiar a la nueva rama");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Crear rama", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok || ui.input(|i| i.key_pressed(Key::Enter)) {
                    let n = name.trim().to_string();
                    let exists = self.tab().and_then(|t| t.refs.as_ref()).is_some_and(|r| r.local.iter().any(|b| b.name == n));
                    if n.is_empty() {
                        *error = Some("Escribe un nombre para la rama.".into());
                    } else if exists {
                        *error = Some(format!("Ya existe una rama llamada «{n}»."));
                    } else if let Some(p) = path {
                        let (s, co) = (start.clone(), *checkout);
                        let label = n.clone();
                        self.op("Crear rama", move || git::create_branch(&p, &n, s.as_deref(), co), move |app, _| app.ok(format!("Rama {label} creada")));
                        return Out::Close;
                    }
                }
            }
            Dialog::RenameBranch { old, new, error } => {
                ui.heading("Renombrar rama");
                field_label(ui, "Nuevo nombre");
                text_field(ui, new, "");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Renombrar", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let n = new.trim().replace(char::is_whitespace, "-");
                    if n.is_empty() || &n == old {
                        *error = Some("Escribe un nombre distinto.".into());
                    } else if let Some(p) = path {
                        let o = old.clone();
                        self.op("Renombrar rama", move || git::rename_branch(&p, &o, &n), |_, _| {});
                        return Out::Close;
                    }
                }
            }
            Dialog::DeleteBranch { name, force, remote_too, upstream } => {
                ui.heading("Eliminar rama");
                ui.label(format!("¿Eliminar la rama local {name}?"));
                check(ui, force, "Forzar aunque tenga commits sin fusionar");
                if let Some((_, _, label)) = upstream {
                    check(ui, remote_too, &format!("Eliminar también {label} en el remoto"));
                }
                let (cancel, ok) = buttons(ui, "Eliminar", true, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let Some(p) = path else { return Out::Close };
                    let (n, f) = (name.clone(), *force);
                    let remote = if *remote_too { upstream.clone() } else { None };
                    let label = n.clone();
                    self.op("Eliminar rama", move || git::delete_branch(&p, &n, f), move |app, _| {
                        app.ok(format!("Rama {label} eliminada"));
                        if let Some((r, b, _)) = remote {
                            app.delete_remote_branch(r, b);
                        }
                    });
                    return Out::Close;
                }
            }
            Dialog::Merge { options, target, fixed, no_ff } => {
                let cur = self.tab().map(|t| t.branch_label()).unwrap_or_default();
                ui.heading("Merge");
                match fixed {
                    Some(l) => {
                        ui.label(format!("Fusionar {l} en {cur}."));
                    }
                    None => {
                        ui.label(format!("Fusionar una rama en {cur}."));
                        field_label(ui, "Rama a fusionar");
                        widgets::combo(ui, "merge-target", target, options);
                    }
                }
                check(ui, no_ff, "Crear siempre un commit de fusión (--no-ff)");
                let (cancel, ok) = buttons(ui, "Fusionar", false, !busy && !target.is_empty());
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let (Some(p), t, nf) = (path, target.clone(), *no_ff) else { return Out::Close };
                    self.op("Merge", move || git::merge(&p, &t, nf), |app, r| {
                        if !app.handle_conflicts(&r, "La fusión") {
                            app.ok("Fusión completada");
                        }
                    });
                    return Out::Close;
                }
            }
            Dialog::Reset { hash, label, mode } => {
                let cur = self.tab().map(|t| t.branch_label()).unwrap_or_default();
                ui.heading("Reset");
                ui.label(format!("Mover {cur} a {label}."));
                for (m, t, desc) in [
                    (ResetMode::Soft, "Soft", "Mueve la rama; los cambios quedan preparados."),
                    (ResetMode::Mixed, "Mixed (recomendado)", "Mueve la rama; los cambios quedan en el directorio de trabajo sin preparar."),
                    (ResetMode::Hard, "Hard", "Mueve la rama y DESCARTA todos los cambios. No se puede deshacer."),
                ] {
                    ui.radio_value(mode, m, RichText::new(t).strong());
                    ui.label(widgets::small(format!("      {desc}")));
                }
                let (cancel, ok) = buttons(ui, "Reset", true, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    self.do_reset(hash.clone(), *mode);
                    return Out::Close;
                }
            }
            Dialog::Tag { hash, name, msg, push, remote, remotes, sign, error } => {
                ui.heading("Nueva etiqueta");
                field_label(ui, "Nombre");
                text_field(ui, name, "v1.0.0");
                ui.label(widgets::small(format!("En el commit {}", &hash[..8.min(hash.len())])));
                field_label(ui, "Mensaje");
                ui.add(egui::TextEdit::multiline(msg).hint_text("Opcional: si escribes un mensaje se crea una etiqueta anotada").desired_rows(3).desired_width(f32::INFINITY));
                check(ui, sign, "Firmar la etiqueta (GPG/SSH)");
                if !remotes.is_empty() {
                    ui.horizontal(|ui| {
                        check(ui, push, "Enviar al remoto");
                        widgets::combo(ui, "tag-remote", remote, remotes);
                    });
                }
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Crear etiqueta", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let n = name.trim().to_string();
                    let exists = self.tab().and_then(|t| t.refs.as_ref()).is_some_and(|r| r.tags.iter().any(|t| t.name == n));
                    if n.is_empty() {
                        *error = Some("Escribe un nombre.".into());
                    } else if exists {
                        *error = Some(format!("Ya existe la etiqueta «{n}»."));
                    } else if let Some(p) = path {
                        let (h, m, s) = (hash.clone(), msg.trim().to_string(), *sign);
                        let pushto = push.then(|| remote.clone());
                        let nn = n.clone();
                        self.op("Crear etiqueta", move || git::create_tag(&p, &n, Some(&h), &m, s), move |app, _| match pushto {
                            Some(r) => app.push_tag(r, nn),
                            None => app.ok("Etiqueta creada"),
                        });
                        return Out::Close;
                    }
                }
            }
            Dialog::DeleteTag { name, remote, remote_too } => {
                ui.heading("Eliminar etiqueta");
                ui.label(format!("¿Eliminar la etiqueta {name}?"));
                if let Some(r) = remote {
                    check(ui, remote_too, &format!("Eliminarla también de {r}"));
                }
                let (cancel, ok) = buttons(ui, "Eliminar", true, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let Some(p) = path else { return Out::Close };
                    let n = name.clone();
                    let rem = if *remote_too { remote.clone() } else { None };
                    let nn = n.clone();
                    self.op("Eliminar etiqueta", move || git::delete_tag(&p, &n), move |app, _| match rem {
                        Some(r) => app.delete_remote_tag(r, nn),
                        None => app.ok("Etiqueta eliminada"),
                    });
                    return Out::Close;
                }
            }
            Dialog::DiscardAll { tracked, untracked, with_untracked } => {
                ui.heading("Descartar todos los cambios");
                ui.label(format!("Se descartarán los cambios de {} (preparados y sin preparar).", widgets::plural(*tracked, "archivo con seguimiento", "archivos con seguimiento")));
                if *untracked > 0 {
                    check(ui, with_untracked, &format!("Eliminar también {}", widgets::plural(*untracked, "archivo sin seguimiento", "archivos sin seguimiento")));
                }
                ui.label(widgets::small("Esta acción no se puede deshacer. Si dudas, usa Stash."));
                let (cancel, ok) = buttons(ui, "Descartar todo", true, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let (Some(p), u) = (path, *with_untracked) else { return Out::Close };
                    self.op("Descartar todo", move || git::discard_all(&p, u), |app, _| app.ok("Cambios descartados"));
                    return Out::Close;
                }
            }
            Dialog::Pull { local, remotes, remote, branches, branch, rebase, error } => {
                ui.heading("Pull");
                ui.label(format!("Traer cambios a {local}."));
                ui.columns(2, |cols| {
                    field_label(&mut cols[0], "Remoto");
                    widgets::combo(&mut cols[0], "pull-remote", remote, remotes);
                    field_label(&mut cols[1], "Rama remota");
                    let opts: Vec<String> = branches.iter().filter(|(r, _)| r == remote).map(|(_, b)| b.clone()).collect();
                    widgets::combo(&mut cols[1], "pull-branch", branch, &opts);
                });
                check(ui, rebase, "Hacer rebase en lugar de merge (--rebase)");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Pull", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    if branch.trim().is_empty() {
                        *error = Some("Indica la rama remota (haz fetch si no aparece ninguna).".into());
                    } else if let Some(p) = path {
                        self.store.settings.pull_rebase = *rebase;
                        self.persist();
                        let (r, b, rb) = (remote.clone(), branch.trim().to_string(), *rebase);
                        self.remote_op("Pull", move |h| git::pull(&p, Some(&r), Some(&b), rb, Some(h)), |app, r| {
                            if !app.handle_conflicts(&r, "El pull") {
                                app.ok("Pull completado");
                            }
                        });
                        return Out::Close;
                    }
                }
            }
            Dialog::Push { locals, local, remotes, remote, remote_branch, upstream, tags, force, error } => {
                ui.heading("Push");
                let before = (local.clone(), remote.clone());
                ui.columns(2, |cols| {
                    field_label(&mut cols[0], "Rama local");
                    widgets::combo(&mut cols[0], "push-local", local, locals);
                    field_label(&mut cols[1], "Remoto");
                    widgets::combo(&mut cols[1], "push-remote", remote, remotes);
                });
                if before != (local.clone(), remote.clone()) {
                    if let Some(b) = self.tab().and_then(|t| t.refs.as_ref()).and_then(|r| r.local.iter().find(|b| &b.name == local)) {
                        *remote_branch = if b.upstream_remote.as_ref() == Some(remote) { b.upstream_branch.clone().unwrap_or(b.name.clone()) } else { b.name.clone() };
                        *upstream = b.upstream.is_none() || b.gone;
                    }
                }
                field_label(ui, "Rama remota");
                text_field(ui, remote_branch, "");
                check(ui, upstream, "Establecer como rama de seguimiento (upstream)");
                check(ui, tags, "Enviar también las etiquetas anotadas (--follow-tags)");
                ui.checkbox(force, RichText::new("Forzar push (--force-with-lease)").color(theme::p().red));
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Push", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let rb = remote_branch.trim().to_string();
                    if rb.is_empty() {
                        *error = Some("Indica la rama remota.".into());
                    } else if let Some(p) = path {
                        let args = PushArgs { remote: remote.clone(), branch: local.clone(), remote_branch: Some(rb.clone()), set_upstream: *upstream, force: *force, tags: *tags };
                        let name = local.clone();
                        let run = move |app: &mut App| {
                            app.remote_op("Push", move |h| git::push(&p, &args, Some(h)), move |app, _| app.ok(format!("Push de {name} completado")));
                        };
                        if *force {
                            let msg = format!("Se sobrescribirá {remote}/{rb} con tu versión local. Los commits remotos que no tengas se perderán.");
                            self.confirm("Push forzado", &msg, None, "Forzar push", true, run);
                        } else {
                            run(self);
                        }
                        return Out::Close;
                    }
                }
            }
            Dialog::AddRemote { name, url, error } => {
                ui.heading("Añadir remoto");
                field_label(ui, "Nombre");
                text_field(ui, name, "origin");
                field_label(ui, "URL");
                text_field(ui, url, "https://github.com/usuario/repo.git");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Añadir", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    if name.trim().is_empty() || url.trim().is_empty() {
                        *error = Some("Indica nombre y URL.".into());
                    } else if let Some(p) = path {
                        let (n, u) = (name.trim().to_string(), url.trim().to_string());
                        let label = n.clone();
                        self.op("Añadir remoto", move || git::add_remote(&p, &n, &u), move |app, _| app.ok(format!("Remoto {label} añadido")));
                        return Out::Close;
                    }
                }
            }
            Dialog::EditRemote { name, new_name, url, old_url, error } => {
                ui.heading("Editar remoto");
                field_label(ui, "Nombre");
                text_field(ui, new_name, "origin");
                field_label(ui, "URL");
                text_field(ui, url, "");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Guardar", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let Some(p) = path else { return Out::Close };
                    let (n, nn, u, ou) = (name.clone(), new_name.trim().to_string(), url.trim().to_string(), old_url.clone());
                    self.op(
                        "Editar remoto",
                        move || {
                            if u != ou {
                                git::set_remote_url(&p, &n, &u)?;
                            }
                            if nn != n {
                                git::rename_remote(&p, &n, &nn)?;
                            }
                            Ok(())
                        },
                        |app, _| app.ok("Remoto actualizado"),
                    );
                    return Out::Close;
                }
            }
            Dialog::Clone { url, parent, name, touched, provider, repos, loading, filter, use_ssh, error } => {
                ui.heading("Clonar repositorio");
                // repositories from signed-in accounts
                let accounts = self.store.accounts.clone();
                if !accounts.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(widgets::small("Tus repositorios:"));
                        for a in accounts.iter().filter(|a| a.provider != Provider::AzureDevOps) {
                            if ui.selectable_label(*provider == Some(a.provider), format!("{} ({})", a.provider.label(), a.username)).clicked() && !*loading {
                                *provider = Some(a.provider);
                                *loading = true;
                                *repos = None;
                                let (pv, host) = (a.provider, a.host.clone());
                                self.bus.spawn(move || hosting::user_repos(pv, &host), |app, r| {
                                    for d in app.dialogs.iter_mut() {
                                        if let Dialog::Clone { repos, loading, error, .. } = d {
                                            *loading = false;
                                            match &r {
                                                Ok(v) => *repos = Some(v.clone()),
                                                Err(e) => *error = Some(e.to_string()),
                                            }
                                        }
                                    }
                                });
                            }
                        }
                        if *loading {
                            ui.spinner();
                        }
                    });
                    if let Some(list) = repos {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(filter).hint_text("Filtrar").desired_width(220.0));
                            ui.checkbox(use_ssh, "Usar SSH");
                        });
                        let q = filter.to_lowercase();
                        egui::Frame::new().fill(theme::p().bg).corner_radius(6).inner_margin(4).show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                for r in list.iter().filter(|r| q.is_empty() || r.full_name.to_lowercase().contains(&q)) {
                                    let chosen = *url == r.clone_url || *url == r.ssh_url;
                                    let resp = widgets::row(ui, chosen, 24.0, |ui| {
                                        ui.label(&r.full_name);
                                        if r.private {
                                            widgets::pill(ui, "privado", theme::p().muted);
                                        }
                                        ui.label(widgets::small(r.description.chars().take(60).collect::<String>()));
                                    });
                                    if resp.clicked() {
                                        *url = if *use_ssh && !r.ssh_url.is_empty() { r.ssh_url.clone() } else { r.clone_url.clone() };
                                        *touched = false;
                                    }
                                }
                            });
                        });
                    }
                } else {
                    ui.label(widgets::small("Inicia sesión desde «Cuentas» para elegir entre tus repositorios de GitHub, GitLab o Bitbucket."));
                }
                field_label(ui, "URL del repositorio");
                let r = text_field(ui, url, "https://github.com/usuario/proyecto.git");
                if (r.changed() || !*touched) && !*touched {
                    let u = url.trim().trim_end_matches('/').trim_end_matches(".git");
                    *name = u.rsplit(['/', '\\', ':']).next().unwrap_or("").to_string();
                }
                field_label(ui, "Carpeta de destino");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(parent).hint_text("Carpeta donde se creará el clon").desired_width(ui.available_width() - 100.0));
                    if ui.button("Examinar…").clicked() {
                        if let Some(d) = pick_folder("Carpeta de destino") {
                            *parent = d.to_string_lossy().into_owned();
                        }
                    }
                });
                field_label(ui, "Nombre de la carpeta");
                if text_field(ui, name, "proyecto").changed() {
                    *touched = true;
                }
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Clonar", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let (u, dir, n) = (url.trim().to_string(), parent.trim().to_string(), name.trim().to_string());
                    if u.is_empty() {
                        *error = Some("Escribe la URL del repositorio.".into());
                    } else if dir.is_empty() {
                        *error = Some("Elige la carpeta de destino.".into());
                    } else if n.is_empty() || n.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
                        *error = Some("El nombre de la carpeta no es válido.".into());
                    } else {
                        self.store.settings.clone_dir = dir.clone();
                        self.persist();
                        let dest = PathBuf::from(dir).join(n);
                        self.remote_op("Clonar", move |h| git::clone(&u, &dest, Some(h)), |app, p| {
                            app.ok("Repositorio clonado");
                            app.open_resolved(p);
                        });
                        return Out::Close;
                    }
                }
            }
            Dialog::InteractiveRebase { base, base_label, rows, has_merges, error } => {
                ui.heading("Rebase interactivo");
                ui.label(format!("Reescribe los commits posteriores a {base_label}: reordénalos, combínalos, cambia sus mensajes o elimínalos."));
                if *has_merges {
                    widgets::info_box(ui, "Hay commits de fusión en el rango: el rebase los aplanará (desaparecerán los merges).", theme::p().yellow);
                }
                match rows {
                    None => {
                        ui.spinner();
                    }
                    Some(rows) => {
                        ui.label(widgets::small("Orden de aplicación: el primero es el más antiguo."));
                        let mut swap = None;
                        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                            let n = rows.len();
                            for (i, r) in rows.iter_mut().enumerate() {
                                egui::Frame::new().fill(theme::p().panel2).corner_radius(6).inner_margin(6).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        if crate::icons::button_sized(ui, crate::icons::Icon::ChevronUp, "Subir", 20.0, i > 0).clicked() {
                                            swap = Some((i, i - 1));
                                        }
                                        if crate::icons::button_sized(ui, crate::icons::Icon::ChevronDown, "Bajar", 20.0, i + 1 < n).clicked() {
                                            swap = Some((i, i + 1));
                                        }
                                        egui::ComboBox::from_id_salt(("rb-act", i)).selected_text(REBASE_ACTIONS[r.action]).width(190.0).show_ui(ui, |ui| {
                                            for (k, a) in REBASE_ACTIONS.iter().enumerate() {
                                                ui.selectable_value(&mut r.action, k, *a);
                                            }
                                        });
                                        ui.label(RichText::new(&r.commit.hash[..8]).font(theme::mono(11.5)).color(theme::p().muted));
                                        let subj = RichText::new(&r.commit.subject);
                                        ui.label(if r.action == 5 { subj.strikethrough().color(theme::p().muted) } else { subj });
                                    });
                                    if r.action == 1 {
                                        ui.add(egui::TextEdit::multiline(&mut r.message).desired_rows(2).desired_width(f32::INFINITY));
                                    }
                                });
                            }
                        });
                        if let Some((a, b)) = swap {
                            rows.swap(a, b);
                        }
                    }
                }
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Empezar rebase", false, !busy && rows.is_some());
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let entries: Vec<TodoEntry> = rows
                        .as_ref()
                        .unwrap()
                        .iter()
                        .map(|r| TodoEntry {
                            commit: r.commit.clone(),
                            action: match r.action {
                                1 => TodoAction::Reword(r.message.clone()),
                                2 => TodoAction::Squash,
                                3 => TodoAction::Fixup,
                                4 => TodoAction::Edit,
                                5 => TodoAction::Drop,
                                _ => TodoAction::Pick,
                            },
                        })
                        .collect();
                    if let Err(e) = rebase::build_todo(&entries, &std::env::temp_dir()) {
                        *error = Some(e.to_string());
                    } else if let Some(p) = path {
                        let b = base.clone();
                        self.op("Rebase interactivo", move || rebase::run(&p, b.as_deref(), &entries), |app, r| {
                            if !app.handle_conflicts(&r, "El rebase") {
                                app.ok("Rebase interactivo completado");
                            }
                        });
                        return Out::Close;
                    }
                }
            }
            Dialog::FlowInit { cfg, branches, error } => {
                ui.heading("Inicializar git-flow");
                ui.label("git-flow organiza el trabajo en ramas de features, releases y hotfixes. Es compatible con la extensión git-flow.");
                ui.columns(2, |cols| {
                    field_label(&mut cols[0], "Rama de producción");
                    widgets::combo(&mut cols[0], "flow-master", &mut cfg.master, branches);
                    field_label(&mut cols[1], "Rama de desarrollo");
                    text_field(&mut cols[1], &mut cfg.develop, "develop");
                });
                ui.columns(3, |cols| {
                    field_label(&mut cols[0], "Prefijo features");
                    text_field(&mut cols[0], &mut cfg.feature, "feature/");
                    field_label(&mut cols[1], "Prefijo releases");
                    text_field(&mut cols[1], &mut cfg.release, "release/");
                    field_label(&mut cols[2], "Prefijo hotfixes");
                    text_field(&mut cols[2], &mut cfg.hotfix, "hotfix/");
                });
                field_label(ui, "Prefijo de las etiquetas de versión");
                text_field(ui, &mut cfg.version_tag, "v (opcional)");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Inicializar", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let (Some(p), c) = (path, cfg.clone()) else { return Out::Close };
                    self.op("Inicializar git-flow", move || flow::init(&p, &c), |app, _| {
                        app.ok("git-flow inicializado");
                        app.open_flow_after_init();
                    });
                    return Out::Close;
                }
            }
            Dialog::Flow { cfg, kind, name, keep, tag_msg, error } => {
                ui.heading("git-flow");
                ui.horizontal(|ui| {
                    for k in FlowKind::ALL {
                        ui.selectable_value(kind, k, k.label());
                    }
                });
                ui.separator();
                let locals = self.tab().and_then(|t| t.refs.as_ref()).map(|r| r.local.clone()).unwrap_or_default();
                let prefix = cfg.prefix(*kind).to_string();
                let in_progress: Vec<String> = locals.iter().filter_map(|b| b.name.strip_prefix(&prefix).map(String::from)).collect();
                let base = if *kind == FlowKind::Hotfix { &cfg.master } else { &cfg.develop };
                ui.label(RichText::new(format!("Empezar {} nueva", kind.label().to_lowercase())).strong());
                ui.label(widgets::small(format!("Se crea {prefix}<nombre> desde {base}.")));
                let mut start = false;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(name).hint_text(if *kind == FlowKind::Feature { "nombre" } else { "versión, p. ej. 1.2.0" }).desired_width(260.0));
                    start = widgets::primary_button(ui, "Empezar", !busy && !name.trim().is_empty()).clicked();
                });
                ui.add_space(8.0);
                ui.label(RichText::new("En curso").strong());
                if in_progress.is_empty() {
                    ui.label(widgets::small("No hay ninguna en curso."));
                }
                if *kind != FlowKind::Feature {
                    field_label(ui, "Mensaje de la etiqueta (al finalizar)");
                    text_field(ui, tag_msg, "Opcional");
                }
                check(ui, keep, "Conservar la rama después de finalizar");
                let mut finish = None;
                for n in &in_progress {
                    ui.horizontal(|ui| {
                        crate::icons::show(ui, crate::icons::Icon::Branch, 14.0, theme::p().muted);
                        ui.label(format!("{prefix}{n}"));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add_enabled(!busy, egui::Button::new("Finalizar")).clicked() {
                                finish = Some(n.clone());
                            }
                        });
                    });
                }
                err(ui, error);
                ui.add_space(6.0);
                let close = ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.button("Cerrar").clicked()).inner;
                if start {
                    let (Some(p), k, n) = (path.clone(), *kind, name.trim().to_string()) else { return Out::Close };
                    self.op("git-flow: empezar", move || flow::start(&p, k, &n), |app, b| app.ok(format!("Rama {b} creada")));
                    return Out::Close;
                }
                if let Some(n) = finish {
                    let Some(p) = path else { return Out::Close };
                    let (k, o) = (*kind, FinishOptions { keep_branch: *keep, tag_message: tag_msg.clone(), no_tag: false });
                    self.op("git-flow: finalizar", move || flow::finish(&p, k, &n, &o), |app, r| {
                        if r.conflicts {
                            app.toast(r.output.clone(), ToastKind::Warn);
                            app.set_view(crate::repo_state::View::Changes);
                        } else {
                            app.ok(r.output.lines().last().unwrap_or("Finalizado").to_string());
                        }
                    });
                    return Out::Close;
                }
                if close {
                    return Out::Close;
                }
            }
            Dialog::CreatePr { remote, title, body, source, target, locals, targets, draft, error } => {
                ui.heading("Nueva pull request");
                match remote {
                    None => {
                        widgets::info_box(ui, "No se ha reconocido ningún remoto de GitHub, GitLab, Bitbucket o Azure DevOps en este repositorio.", theme::p().yellow);
                        if ui.button("Cerrar").clicked() {
                            return Out::Close;
                        }
                        return Out::Keep;
                    }
                    Some(r) => {
                        ui.label(widgets::small(format!("{} · {}", r.provider.label(), r.path)));
                    }
                }
                let r = remote.clone().unwrap();
                ui.columns(2, |cols| {
                    field_label(&mut cols[0], "Desde (rama)");
                    widgets::combo(&mut cols[0], "pr-source", source, locals);
                    field_label(&mut cols[1], "Hacia");
                    widgets::combo(&mut cols[1], "pr-target", target, targets);
                });
                field_label(ui, "Título");
                text_field(ui, title, "");
                field_label(ui, "Descripción");
                ui.add(egui::TextEdit::multiline(body).desired_rows(5).desired_width(f32::INFINITY));
                check(ui, draft, "Crear como borrador");
                let ahead_unpushed = self
                    .tab()
                    .and_then(|t| t.refs.as_ref())
                    .and_then(|rf| rf.local.iter().find(|b| &b.name == source))
                    .is_some_and(|b| b.upstream.is_none() || b.ahead > 0);
                if ahead_unpushed {
                    widgets::info_box(ui, "Esta rama tiene commits sin enviar o no existe en el remoto: haz push antes de crear la pull request.", theme::p().yellow);
                }
                err(ui, error);
                ui.add_space(8.0);
                let mut act = 0;
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if r.provider != Provider::AzureDevOps && widgets::primary_button(ui, "Crear", !busy).clicked() {
                        act = 1;
                    }
                    if ui.button("Abrir en el navegador").clicked() {
                        act = 2;
                    }
                    if ui.button("Cancelar").clicked() {
                        act = 3;
                    }
                });
                match act {
                    1 => {
                        let (t, b, s, tg, dr) = (title.clone(), body.clone(), source.clone(), target.clone(), *draft);
                        self.op("Crear pull request", move || hosting::create_pull_request(&r, &t, &b, &s, &tg, dr), |app, url| {
                            app.ok("Pull request creada");
                            let _ = open::that(&url);
                            if let Some(k) = app.active_key() {
                                app.load_prs(&k);
                            }
                        });
                        return Out::Close;
                    }
                    2 => {
                        let _ = open::that(r.new_pr_url(source, target));
                        return Out::Close;
                    }
                    3 => return Out::Close,
                    _ => {}
                }
            }
            Dialog::LfsTrack { pattern, error } => {
                ui.heading("Seguir archivos con LFS");
                ui.label("Los archivos que coincidan se guardarán con Git LFS (se añade a .gitattributes).");
                text_field(ui, pattern, "*.psd");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Añadir", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let (Some(p), pt) = (path, pattern.clone()) else { return Out::Close };
                    self.op("LFS track", move || extras::lfs_track(&p, &pt), |app, _| {
                        app.ok("Patrón añadido a .gitattributes");
                        if let Some(k) = app.active_key() {
                            app.load_lfs(&k);
                        }
                    });
                    return Out::Close;
                }
            }
            Dialog::SubmoduleAdd { url, path: sub, error } => {
                ui.heading("Añadir submódulo");
                field_label(ui, "URL");
                text_field(ui, url, "https://github.com/usuario/libreria.git");
                field_label(ui, "Carpeta dentro del repositorio");
                text_field(ui, sub, "vendor/libreria");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Añadir", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    let (Some(p), u, s) = (path, url.clone(), sub.clone()) else { return Out::Close };
                    self.remote_op("Añadir submódulo", move |h| extras::submodule_add(&p, &u, &s, Some(h)), |app, _| {
                        app.ok("Submódulo añadido");
                        if let Some(k) = app.active_key() {
                            app.load_submodules(&k);
                        }
                    });
                    return Out::Close;
                }
            }
            Dialog::WorktreeAdd { path: wt, branch, new_branch, error } => {
                ui.heading("Nuevo worktree");
                ui.label("Un worktree es otra carpeta con otra rama del mismo repositorio, para trabajar en paralelo.");
                field_label(ui, "Carpeta");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(wt).desired_width(ui.available_width() - 100.0));
                    if ui.button("Examinar…").clicked() {
                        if let Some(d) = pick_folder("Carpeta del worktree") {
                            *wt = d.to_string_lossy().into_owned();
                        }
                    }
                });
                field_label(ui, "Rama");
                text_field(ui, branch, "");
                check(ui, new_branch, "Crear la rama nueva");
                err(ui, error);
                let (cancel, ok) = buttons(ui, "Crear", false, !busy);
                if cancel {
                    return Out::Close;
                }
                if ok {
                    if wt.trim().is_empty() || branch.trim().is_empty() {
                        *error = Some("Indica la carpeta y la rama.".into());
                    } else if let Some(p) = path {
                        let (w, b, nb) = (PathBuf::from(wt.trim()), branch.trim().to_string(), *new_branch);
                        self.op("Crear worktree", move || extras::worktree_add(&p, &w, &b, nb), |app, _| {
                            app.ok("Worktree creado");
                            if let Some(k) = app.active_key() {
                                app.load_worktrees(&k);
                            }
                        });
                        return Out::Close;
                    }
                }
            }
            Dialog::Accounts { host, provider, pending, error, gcm } => {
                return self.accounts_body(ui, host, provider, pending, error, gcm);
            }
            Dialog::Settings(s) => return self.settings_body(ui, s),
        }
        Out::Keep
    }

    fn accounts_body(&mut self, ui: &mut egui::Ui, host: &mut String, provider: &mut Provider, pending: &mut Option<String>, error: &mut Option<String>, gcm: &mut Option<bool>) -> Out {
        let c = theme::p();
        ui.heading("Cuentas");
        if gcm.is_none() {
            *gcm = Some(credentials::gcm_configured());
        }
        ui.label("Inicia sesión una vez y Destiny Stone (y Git) usarán esa sesión para clonar, hacer push y gestionar pull requests. El inicio de sesión se hace en el navegador, con el SSO de tu organización si lo tiene.");
        match (&self.gcm_version, *gcm) {
            (None, _) => {
                widgets::info_box(ui, "No se encontró Git Credential Manager. Viene incluido con Git para Windows; en macOS y Linux instálalo desde github.com/git-ecosystem/git-credential-manager.", c.yellow);
            }
            (Some(_), Some(false)) => {
                widgets::info_box(ui, "Git Credential Manager está instalado pero no es tu gestor de credenciales.", c.yellow);
                if ui.button("Usar Git Credential Manager").clicked() {
                    match credentials::configure_gcm() {
                        Ok(()) => *gcm = Some(true),
                        Err(e) => *error = Some(e.to_string()),
                    }
                }
            }
            (Some(v), _) => {
                ui.label(widgets::small(format!("Git Credential Manager {v}")));
            }
        }
        ui.add_space(6.0);
        let mut logout = None;
        if self.store.accounts.is_empty() {
            ui.label(widgets::muted("Todavía no has iniciado sesión en ninguna plataforma."));
        }
        for (i, a) in self.store.accounts.iter().enumerate() {
            egui::Frame::new().fill(c.panel2).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    crate::icons::show(ui, crate::icons::Icon::User, 16.0, c.accent);
                    ui.label(RichText::new(&a.username).strong());
                    ui.label(widgets::muted(format!("{} · {}", a.provider.label(), a.host)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Cerrar sesión").clicked() {
                            logout = Some(i);
                        }
                    });
                });
            });
        }
        if let Some(i) = logout {
            let a = self.store.accounts.remove(i);
            let _ = credentials::logout(&a);
            self.persist();
        }
        ui.separator();
        ui.label(RichText::new("Iniciar sesión").strong());
        ui.horizontal(|ui| {
            for p in Provider::ALL {
                if ui.selectable_label(*provider == p, p.label()).clicked() {
                    *provider = p;
                    *host = p.default_host().to_string();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(widgets::small("Servidor"));
            ui.add(egui::TextEdit::singleline(host).desired_width(240.0)).on_hover_text("Cámbialo para servidores propios (GitHub Enterprise, GitLab autoalojado…)");
        });
        err(ui, error);
        ui.add_space(8.0);
        let mut close = false;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            match pending {
                Some(h) => {
                    ui.spinner();
                    ui.label(format!("Completa el inicio de sesión en el navegador ({h})…"));
                }
                None => {
                    let enabled = self.gcm_version.is_some() && *gcm == Some(true) && !host.trim().is_empty();
                    if widgets::primary_button(ui, &format!("Iniciar sesión en {}", provider.label()), enabled).clicked() {
                        *pending = Some(host.trim().to_string());
                        *error = None;
                        let (pv, h) = (*provider, host.trim().to_string());
                        self.bus.spawn(move || credentials::login(pv, &h), |app, r| {
                            let mut msg = None;
                            match &r {
                                Ok(a) => {
                                    app.store.accounts.retain(|x| !(x.host == a.host && x.username == a.username));
                                    app.store.accounts.push(a.clone());
                                    app.persist();
                                    msg = Some(format!("Sesión iniciada como {} en {}", a.username, a.provider.label()));
                                }
                                Err(_) => {}
                            }
                            for d in app.dialogs.iter_mut() {
                                if let Dialog::Accounts { pending, error, .. } = d {
                                    *pending = None;
                                    if let Err(e) = &r {
                                        *error = Some(e.to_string());
                                    }
                                }
                            }
                            if let Some(m) = msg {
                                app.ok(m);
                            }
                        });
                    }
                    if ui.button("Cerrar").clicked() {
                        close = true;
                    }
                }
            }
        });
        if close {
            Out::Close
        } else {
            Out::Keep
        }
    }

    fn settings_body(&mut self, ui: &mut egui::Ui, s: &mut SettingsState) -> Out {
        ui.heading("Ajustes");
        egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
            ui.columns(2, |cols| {
                field_label(&mut cols[0], "Tema");
                egui::ComboBox::from_id_salt("theme").selected_text(match s.theme {
                    ThemePref::Dark => "Oscuro",
                    ThemePref::Light => "Claro",
                    ThemePref::System => "Según el sistema",
                }).show_ui(&mut cols[0], |ui| {
                    ui.selectable_value(&mut s.theme, ThemePref::Dark, "Oscuro");
                    ui.selectable_value(&mut s.theme, ThemePref::Light, "Claro");
                    ui.selectable_value(&mut s.theme, ThemePref::System, "Según el sistema");
                });
                field_label(&mut cols[1], "Fetch automático");
                egui::ComboBox::from_id_salt("autofetch").selected_text(if s.auto_fetch == 0 { "Desactivado".to_string() } else { format!("Cada {} minutos", s.auto_fetch) }).show_ui(&mut cols[1], |ui| {
                    for m in [0, 5, 10, 15, 30] {
                        ui.selectable_value(&mut s.auto_fetch, m, if m == 0 { "Desactivado".to_string() } else { format!("Cada {m} minutos") });
                    }
                });
            });
            field_label(ui, "Commits a cargar en el historial (100 – 1.000.000)");
            ui.add(egui::TextEdit::singleline(&mut s.max_commits).desired_width(140.0));
            check(ui, &mut s.chronological, "Explorador de ramas cronológico (una columna por commit)");
            check(ui, &mut s.word_diff, "Resaltar las palabras cambiadas dentro de cada línea");
            check(ui, &mut s.sign_commits, "Firmar los commits por defecto (GPG/SSH)");
            if let Some(p) = &s.repo {
                ui.separator();
                ui.label(RichText::new(format!("Repositorio · {}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())).strong());
                ui.columns(2, |cols| {
                    field_label(&mut cols[0], "Nombre del autor");
                    text_field(&mut cols[0], &mut s.name, "Tu nombre");
                    field_label(&mut cols[1], "Email");
                    text_field(&mut cols[1], &mut s.email, "tu@email.com");
                });
                check(ui, &mut s.global, "Guardar la identidad para todos los repositorios (global)");
                ui.separator();
                ui.label(RichText::new("Herramientas externas").strong());
                ui.columns(2, |cols| {
                    field_label(&mut cols[0], "Herramienta de diff (diff.tool)");
                    text_field(&mut cols[0], &mut s.diff_tool, "vscode, bc, meld, kdiff3…");
                    field_label(&mut cols[1], "Herramienta de fusión (merge.tool)");
                    text_field(&mut cols[1], &mut s.merge_tool, "vscode, bc, meld, kdiff3…");
                });
                ui.label(widgets::small("Usa los nombres que entiende git (git difftool --tool-help). Para VS Code: «vscode», configurado con difftool.vscode.cmd."));
            }
            ui.separator();
            ui.label(RichText::new("Acciones personalizadas").strong());
            ui.label(widgets::small("Programas que aparecen en los menús contextuales. En los argumentos puedes usar $REPO, $SHA, $FILE y $BRANCH."));
            let mut remove = None;
            for (i, a) in s.actions.iter_mut().enumerate() {
                egui::Frame::new().fill(theme::p().panel2).corner_radius(6).inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut a.name).hint_text("Nombre").desired_width(160.0));
                        egui::ComboBox::from_id_salt(("scope", i)).selected_text(a.scope.label()).show_ui(ui, |ui| {
                            for sc in [ActionScope::Commit, ActionScope::File, ActionScope::Repo] {
                                ui.selectable_value(&mut a.scope, sc, sc.label());
                            }
                        });
                        ui.checkbox(&mut a.show_output, "Mostrar salida");
                        if crate::icons::button(ui, crate::icons::Icon::Trash, "Quitar").clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut a.program).hint_text("Programa (p. ej. code)").desired_width(160.0));
                        ui.add(egui::TextEdit::singleline(&mut a.args).hint_text("Argumentos (p. ej. --goto $FILE)").desired_width(f32::INFINITY));
                    });
                });
            }
            if let Some(i) = remove {
                s.actions.remove(i);
            }
            if ui.button("Añadir acción").clicked() {
                s.actions.push(CustomAction { name: "Nueva acción".into(), ..Default::default() });
            }
        });
        let mut error = None;
        let (cancel, ok) = buttons(ui, "Guardar", false, true);
        if cancel {
            return Out::Close;
        }
        if ok {
            let mc: usize = match s.max_commits.trim().replace('.', "").parse() {
                Ok(v) if (100..=1_000_000).contains(&v) => v,
                _ => {
                    error = Some("El número de commits debe estar entre 100 y 1.000.000.");
                    0
                }
            };
            if error.is_none() {
                if let Some(p) = &s.repo {
                    let res = (|| -> anyhow::Result<()> {
                        if (s.name.trim(), s.email.trim()) != (s.original.0.as_str(), s.original.1.as_str()) {
                            git::set_identity(p, &s.name, &s.email, s.global)?;
                        }
                        if s.diff_tool.trim() != s.original.2 {
                            git::set_config(p, "diff.tool", &s.diff_tool, true)?;
                        }
                        if s.merge_tool.trim() != s.original.3 {
                            git::set_config(p, "merge.tool", &s.merge_tool, true)?;
                        }
                        Ok(())
                    })();
                    if let Err(e) = res {
                        self.toast(e.to_string(), ToastKind::Error);
                    }
                }
                let st = &mut self.store.settings;
                let reload = mc != st.max_commits || s.chronological != st.chronological;
                st.theme = s.theme;
                st.max_commits = mc;
                st.auto_fetch_min = s.auto_fetch;
                st.chronological = s.chronological;
                st.word_diff = s.word_diff;
                st.sign_commits = s.sign_commits;
                st.custom_actions = s.actions.iter().filter(|a| !a.name.trim().is_empty() && !a.program.trim().is_empty()).cloned().collect();
                for t in &mut self.tabs {
                    t.sign = s.sign_commits;
                }
                self.persist();
                self.ok("Ajustes guardados");
                if reload {
                    self.refresh(true);
                }
                return Out::Close;
            }
        }
        if let Some(e) = error {
            widgets::error_box(ui, e);
        }
        Out::Keep
    }

    // ---------------------------------------------------------------- openers

    pub fn open_new_branch(&mut self, start: Option<String>) {
        let Some(t) = self.tab() else { return };
        if !t.has_head() {
            return;
        }
        let start_label = match &start {
            Some(h) => {
                let s = t.commit(h).map(|c| c.subject.clone()).unwrap_or_default();
                format!("{} · {}", &h[..8.min(h.len())], s)
            }
            None => format!("{} (HEAD)", t.branch_label()),
        };
        self.dialogs.push(Dialog::NewBranch { name: String::new(), start, start_label, checkout: true, error: None });
    }

    pub fn open_rename_branch(&mut self, name: String) {
        self.dialogs.push(Dialog::RenameBranch { new: name.clone(), old: name, error: None });
    }

    pub fn open_delete_branch(&mut self, name: String) {
        let Some(t) = self.tab() else { return };
        let b = t.refs.as_ref().and_then(|r| r.local.iter().find(|b| b.name == name)).cloned();
        let upstream = b.and_then(|b| match (b.upstream_remote, b.upstream_branch, b.upstream, b.gone) {
            (Some(r), Some(br), Some(label), false) => Some((r, br, label)),
            _ => None,
        });
        self.dialogs.push(Dialog::DeleteBranch { name, force: false, remote_too: false, upstream });
    }

    pub fn open_merge(&mut self) {
        let Some(r) = self.tab().and_then(|t| t.refs.clone()) else { return };
        let cur = r.current_branch.clone();
        let options: Vec<String> = r
            .local
            .iter()
            .filter(|b| Some(&b.name) != cur.as_ref())
            .map(|b| b.name.clone())
            .chain(r.remote.iter().map(|b| b.name.clone()))
            .collect();
        if options.is_empty() {
            self.toast("No hay otras ramas que fusionar.", ToastKind::Info);
            return;
        }
        self.dialogs.push(Dialog::Merge { target: options[0].clone(), options, fixed: None, no_ff: false });
    }

    pub fn open_merge_into(&mut self, reference: String, label: String) {
        self.dialogs.push(Dialog::Merge { options: vec![], target: reference, fixed: Some(label), no_ff: false });
    }

    pub fn open_reset(&mut self, hash: String) {
        let label = self.tab().and_then(|t| t.commit(&hash)).map(|c| format!("{} · {}", &hash[..8], c.subject)).unwrap_or_else(|| hash.clone());
        self.dialogs.push(Dialog::Reset { hash, label, mode: ResetMode::Mixed });
    }

    pub fn open_tag(&mut self, hash: String) {
        let remotes = self.tab().and_then(|t| t.refs.as_ref()).map(|r| r.remotes.clone()).unwrap_or_default();
        let remote = if remotes.iter().any(|r| r == "origin") { "origin".into() } else { remotes.first().cloned().unwrap_or_default() };
        self.dialogs.push(Dialog::Tag { hash, name: String::new(), msg: String::new(), push: false, remote, remotes, sign: false, error: None });
    }

    pub fn open_delete_tag(&mut self, name: String) {
        let remotes = self.tab().and_then(|t| t.refs.as_ref()).map(|r| r.remotes.clone()).unwrap_or_default();
        let remote = if remotes.iter().any(|r| r == "origin") { Some("origin".to_string()) } else { remotes.first().cloned() };
        self.dialogs.push(Dialog::DeleteTag { name, remote, remote_too: false });
    }

    pub fn open_discard_all(&mut self) {
        let Some(s) = self.tab().and_then(|t| t.status.clone()) else { return };
        let untracked = s.unstaged.iter().filter(|f| f.is_untracked()).count();
        self.dialogs.push(Dialog::DiscardAll { tracked: s.change_count() - untracked, untracked, with_untracked: false });
    }

    pub fn open_pull(&mut self) {
        let Some(r) = self.tab().and_then(|t| t.refs.clone()) else { return };
        if r.remotes.is_empty() {
            return self.toast("Este repositorio no tiene remotos.", ToastKind::Warn);
        }
        let Some(b) = r.current_local().cloned() else {
            return self.toast("No estás en ninguna rama (HEAD separado). Cambia a una rama para hacer pull.", ToastKind::Warn);
        };
        let remote = r.default_remote(Some(&b)).unwrap_or_default();
        let branches: Vec<(String, String)> = r.remote.iter().map(|x| (x.remote.clone(), x.branch.clone())).collect();
        let branch = if b.upstream_remote.as_ref() == Some(&remote) && b.upstream_branch.is_some() {
            b.upstream_branch.clone().unwrap()
        } else if branches.iter().any(|(r, br)| r == &remote && br == &b.name) {
            b.name.clone()
        } else {
            branches.iter().find(|(r, _)| r == &remote).map(|(_, b)| b.clone()).unwrap_or_default()
        };
        self.dialogs.push(Dialog::Pull { local: b.name.clone(), remotes: r.remotes.clone(), remote, branches, branch, rebase: self.store.settings.pull_rebase, error: None });
    }

    pub fn open_push(&mut self, branch: Option<String>) {
        let Some(r) = self.tab().and_then(|t| t.refs.clone()) else { return };
        if r.remotes.is_empty() {
            return self.toast("Este repositorio no tiene remotos. Añade uno desde la barra lateral.", ToastKind::Warn);
        }
        let Some(initial) = branch.and_then(|n| r.local.iter().find(|b| b.name == n).cloned()).or_else(|| r.current_local().cloned()).or_else(|| r.local.first().cloned()) else {
            return self.toast("No hay ramas locales que enviar.", ToastKind::Warn);
        };
        let remote = r.default_remote(Some(&initial)).unwrap_or_default();
        let remote_branch = if initial.upstream_remote.as_ref() == Some(&remote) { initial.upstream_branch.clone().unwrap_or(initial.name.clone()) } else { initial.name.clone() };
        self.dialogs.push(Dialog::Push {
            locals: r.local.iter().map(|b| b.name.clone()).collect(),
            local: initial.name.clone(),
            remotes: r.remotes.clone(),
            remote,
            remote_branch,
            upstream: initial.upstream.is_none() || initial.gone,
            tags: false,
            force: false,
            error: None,
        });
    }

    pub fn open_add_remote(&mut self) {
        let has_origin = self.tab().and_then(|t| t.refs.as_ref()).is_some_and(|r| r.remotes.iter().any(|x| x == "origin"));
        self.dialogs.push(Dialog::AddRemote { name: if has_origin { String::new() } else { "origin".into() }, url: String::new(), error: None });
    }

    pub fn open_edit_remote(&mut self, name: String) {
        let url = self.active_path().and_then(|p| git::remotes_info(&p).ok()).and_then(|l| l.into_iter().find(|r| r.name == name)).map(|r| r.url).unwrap_or_default();
        self.dialogs.push(Dialog::EditRemote { new_name: name.clone(), name, old_url: url.clone(), url, error: None });
    }

    pub fn open_clone(&mut self, url: Option<String>) {
        self.dialogs.push(Dialog::Clone {
            url: url.unwrap_or_default(),
            parent: self.store.settings.clone_dir.clone(),
            name: String::new(),
            touched: false,
            provider: None,
            repos: None,
            loading: false,
            filter: String::new(),
            use_ssh: false,
            error: None,
        });
    }

    pub fn open_interactive_rebase(&mut self, base: Option<String>) {
        let Some(t) = self.tab() else { return };
        let Some(p) = self.active_path() else { return };
        if t.change_count() > 0 {
            return self.toast("Confirma o guarda en stash tus cambios antes de un rebase interactivo.", ToastKind::Warn);
        }
        let base_label = match &base {
            Some(h) => t.commit(h).map(|c| format!("{} · {}", &h[..8], c.subject)).unwrap_or_else(|| h.clone()),
            None => "el inicio del historial".into(),
        };
        self.dialogs.push(Dialog::InteractiveRebase { base: base.clone(), base_label, rows: None, has_merges: false, error: None });
        self.bus.spawn(move || rebase::plan(&p, base.as_deref()), |app, r| {
            for d in app.dialogs.iter_mut() {
                if let Dialog::InteractiveRebase { rows, has_merges, error, .. } = d {
                    match &r {
                        Ok(plan) => {
                            *has_merges = plan.has_merges;
                            *rows = Some(plan.commits.iter().map(|c| RebaseRow { message: c.subject.clone(), commit: c.clone(), action: 0 }).collect());
                            if plan.commits.is_empty() {
                                *error = Some("No hay commits que reescribir después de ese punto.".into());
                            }
                        }
                        Err(e) => *error = Some(e.to_string()),
                    }
                }
            }
        });
    }

    pub fn open_flow(&mut self) {
        let Some(t) = self.tab() else { return };
        let Some(r) = &t.refs else { return };
        match t.flow.clone() {
            Some(cfg) => self.dialogs.push(Dialog::Flow { cfg, kind: FlowKind::Feature, name: String::new(), keep: false, tag_msg: String::new(), error: None }),
            None => {
                let branches: Vec<String> = r.local.iter().map(|b| b.name.clone()).collect();
                let master = ["main", "master"].iter().find(|n| branches.iter().any(|b| b == *n)).map(|s| s.to_string()).or_else(|| r.current_branch.clone()).unwrap_or_else(|| "main".into());
                self.dialogs.push(Dialog::FlowInit { cfg: FlowConfig { master, ..Default::default() }, branches, error: None });
            }
        }
    }

    fn open_flow_after_init(&mut self) {
        if let Some(p) = self.active_path() {
            if let Some(cfg) = flow::config(&p) {
                if let Some(t) = self.tab_mut() {
                    t.flow = Some(cfg);
                }
                self.open_flow();
            }
        }
    }

    pub fn open_create_pr(&mut self, source: Option<String>) {
        let Some(t) = self.tab() else { return };
        let Some(r) = t.refs.clone() else { return };
        let Some(p) = self.active_path() else { return };
        let remote = t.prs.remote.clone().or_else(|| find_remote_ref(&p));
        let source = source.or(r.current_branch.clone()).unwrap_or_default();
        let rem = r.default_remote(None).unwrap_or_default();
        let mut targets: Vec<String> = r.remote.iter().filter(|b| b.remote == rem && b.branch != source).map(|b| b.branch.clone()).collect();
        if targets.is_empty() {
            targets = r.local.iter().filter(|b| b.name != source).map(|b| b.name.clone()).collect();
        }
        let target = ["develop", "main", "master"].iter().find(|n| targets.iter().any(|t| t == *n)).map(|s| s.to_string()).or_else(|| targets.first().cloned()).unwrap_or_default();
        let title = t
            .commits
            .iter()
            .find(|c| r.local.iter().any(|b| b.name == source && b.hash == c.hash))
            .map(|c| c.subject.clone())
            .unwrap_or_else(|| source.replace(['-', '_', '/'], " "));
        self.dialogs.push(Dialog::CreatePr {
            remote,
            title,
            body: String::new(),
            locals: r.local.iter().map(|b| b.name.clone()).collect(),
            source,
            target,
            targets,
            draft: false,
            error: None,
        });
    }
}
