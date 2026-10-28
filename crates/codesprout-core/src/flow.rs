//! git-flow implemented with plain git commands, using the same `gitflow.*` configuration
//! keys as git-flow (AVH), so both tools can be used on the same repository.
//!
//! `finish` is idempotent: if a merge stops with conflicts, resolve and commit, then run
//! `finish` again and it continues with the steps that are still pending.

use std::path::Path;

use anyhow::{bail, Result};

use crate::model::OpResult;
use crate::repo::{self, config_value, set_config};
use crate::runner::{run, Opts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowKind {
    Feature,
    Release,
    Hotfix,
}

impl FlowKind {
    pub const ALL: [FlowKind; 3] = [FlowKind::Feature, FlowKind::Release, FlowKind::Hotfix];
    pub fn label(self) -> &'static str {
        match self {
            FlowKind::Feature => "Feature",
            FlowKind::Release => "Release",
            FlowKind::Hotfix => "Hotfix",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowConfig {
    pub master: String,
    pub develop: String,
    pub feature: String,
    pub release: String,
    pub hotfix: String,
    pub version_tag: String,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            master: "main".into(),
            develop: "develop".into(),
            feature: "feature/".into(),
            release: "release/".into(),
            hotfix: "hotfix/".into(),
            version_tag: String::new(),
        }
    }
}

impl FlowConfig {
    pub fn prefix(&self, kind: FlowKind) -> &str {
        match kind {
            FlowKind::Feature => &self.feature,
            FlowKind::Release => &self.release,
            FlowKind::Hotfix => &self.hotfix,
        }
    }
}

/// The repository's git-flow configuration, or None if it was never initialised.
pub fn config(repo: &Path) -> Option<FlowConfig> {
    let master = config_value(repo, "gitflow.branch.master");
    let develop = config_value(repo, "gitflow.branch.develop");
    if master.is_empty() || develop.is_empty() {
        return None;
    }
    let get = |k: &str, d: &str| {
        let v = config_value(repo, k);
        if v.is_empty() { d.to_string() } else { v }
    };
    Some(FlowConfig {
        master,
        develop,
        feature: get("gitflow.prefix.feature", "feature/"),
        release: get("gitflow.prefix.release", "release/"),
        hotfix: get("gitflow.prefix.hotfix", "hotfix/"),
        version_tag: config_value(repo, "gitflow.prefix.versiontag"),
    })
}

fn branch_exists(repo: &Path, name: &str) -> bool {
    let r = format!("refs/heads/{name}");
    run(repo, &["show-ref", "--verify", "-q", &r], &Opts::default()).map(|o| o.ok()).unwrap_or(false)
}

fn is_ancestor(repo: &Path, a: &str, b: &str) -> bool {
    run(repo, &["merge-base", "--is-ancestor", a, b], &Opts::default()).map(|o| o.ok()).unwrap_or(false)
}

pub fn init(repo: &Path, cfg: &FlowConfig) -> Result<()> {
    if !repo::has_head(repo) {
        bail!("El repositorio necesita al menos un commit antes de inicializar git-flow.");
    }
    if cfg.master.trim().is_empty() || cfg.develop.trim().is_empty() || cfg.master == cfg.develop {
        bail!("Las ramas de producción y de desarrollo deben tener nombres distintos.");
    }
    if !branch_exists(repo, &cfg.master) {
        bail!("No existe la rama «{}».", cfg.master);
    }
    set_config(repo, "gitflow.branch.master", &cfg.master, false)?;
    set_config(repo, "gitflow.branch.develop", &cfg.develop, false)?;
    set_config(repo, "gitflow.prefix.feature", &cfg.feature, false)?;
    set_config(repo, "gitflow.prefix.release", &cfg.release, false)?;
    set_config(repo, "gitflow.prefix.hotfix", &cfg.hotfix, false)?;
    set_config(repo, "gitflow.prefix.support", "support/", false)?;
    if cfg.version_tag.is_empty() {
        crate::runner::git(repo, &["config", "--local", "gitflow.prefix.versiontag", ""], &Opts::default())?;
    } else {
        set_config(repo, "gitflow.prefix.versiontag", &cfg.version_tag, false)?;
    }
    if !branch_exists(repo, &cfg.develop) {
        repo::create_branch(repo, &cfg.develop, Some(&cfg.master), false)?;
    }
    Ok(())
}

fn require(repo: &Path) -> Result<FlowConfig> {
    match config(repo) {
        Some(c) => Ok(c),
        None => bail!("git-flow no está inicializado en este repositorio."),
    }
}

/// What to do with uncommitted changes when starting a feature, release or hotfix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PendingChanges {
    /// Take them to the new branch: they are stashed, the branch is created from its base and
    /// the stash is popped there, so it works even if the base differs from the current branch.
    #[default]
    Carry,
    /// Leave them in a stash and start the branch clean.
    Stash,
}

fn stash_top(repo: &Path) -> Option<String> {
    run(repo, &["rev-parse", "-q", "--verify", "refs/stash"], &Opts::default()).ok().filter(|o| o.ok()).map(|o| o.text().trim().to_string())
}

/// Creates the branch and checks it out. `output` is the new branch name; `conflicts` means the
/// carried changes clashed with the base: they are left in conflict and the stash is kept.
pub fn start(repo: &Path, kind: FlowKind, name: &str, pending: PendingChanges) -> Result<OpResult> {
    let cfg = require(repo)?;
    let name = name.trim();
    if name.is_empty() {
        bail!("Escribe un nombre.");
    }
    let branch = format!("{}{}", cfg.prefix(kind), name);
    let base = if kind == FlowKind::Hotfix { &cfg.master } else { &cfg.develop };
    repo::check_ref_name(repo, true, &branch)?;
    if branch_exists(repo, &branch) {
        bail!("Ya existe la rama «{branch}».");
    }
    let before = stash_top(repo);
    let msg = match pending {
        PendingChanges::Carry => format!("CodeSprout: cambios para {branch}"),
        PendingChanges::Stash => format!("Antes de empezar {branch}"),
    };
    repo::stash_save(repo, &msg, true, false)?;
    // `stash push` with nothing to save succeeds without creating an entry
    let stashed = stash_top(repo) != before;
    if let Err(e) = repo::create_branch(repo, &branch, Some(base), true) {
        if stashed {
            repo::stash_apply(repo, "stash@{0}", true)?;
        }
        return Err(e);
    }
    if stashed && pending == PendingChanges::Carry {
        let r = match repo::stash_apply(repo, "stash@{0}", true) {
            Ok(r) => r,
            Err(e) => bail!("Rama {branch} creada, pero no se han podido aplicar tus cambios; siguen en el stash «{msg}».\n{e}"),
        };
        if r.conflicts {
            return Ok(OpResult {
                conflicts: true,
                output: format!("Rama {branch} creada, pero tus cambios chocan con {base}. Resuelve los conflictos; el stash «{msg}» se conserva por si acaso."),
            });
        }
    }
    Ok(OpResult { conflicts: false, output: branch })
}

/// Branches of a kind in progress (`feature/…`), without the prefix.
pub fn in_progress(repo: &Path, kind: FlowKind, local: &[crate::model::LocalBranch]) -> Vec<String> {
    let Some(cfg) = config(repo) else { return vec![] };
    let p = cfg.prefix(kind);
    local.iter().filter_map(|b| b.name.strip_prefix(p).map(Into::into)).collect()
}

#[derive(Clone, Debug, Default)]
pub struct FinishOptions {
    pub keep_branch: bool,
    /// Tag message for releases / hotfixes. Empty: the version name.
    pub tag_message: String,
    pub no_tag: bool,
}

fn merge_step(repo: &Path, target: &str, branch: &str, log: &mut Vec<String>) -> Result<Option<OpResult>> {
    if is_ancestor(repo, branch, target) {
        log.push(format!("{branch} ya estaba fusionada en {target}"));
        return Ok(None);
    }
    repo::checkout(repo, target)?;
    let msg = format!("Merge branch '{branch}' into {target}");
    let r = repo::conflict_aware(repo, &["merge", "--no-ff", "-m", &msg, branch], &Opts::default())?;
    if r.conflicts {
        return Ok(Some(OpResult {
            conflicts: true,
            output: format!(
                "Conflictos al fusionar {branch} en {target}. Resuélvelos, confirma la fusión y vuelve a pulsar «Finalizar»."
            ),
        }));
    }
    log.push(format!("Fusionada {branch} en {target}"));
    Ok(None)
}

pub fn finish(repo: &Path, kind: FlowKind, name: &str, o: &FinishOptions) -> Result<OpResult> {
    let cfg = require(repo)?;
    let branch = format!("{}{}", cfg.prefix(kind), name);
    if !branch_exists(repo, &branch) {
        bail!("No existe la rama «{branch}».");
    }
    let mut log = Vec::new();
    match kind {
        FlowKind::Feature => {
            if let Some(stop) = merge_step(repo, &cfg.develop, &branch, &mut log)? {
                return Ok(stop);
            }
        }
        FlowKind::Release | FlowKind::Hotfix => {
            if let Some(stop) = merge_step(repo, &cfg.master, &branch, &mut log)? {
                return Ok(stop);
            }
            if !o.no_tag {
                let tag = format!("{}{}", cfg.version_tag, name);
                let exists = run(repo, &["show-ref", "--verify", "-q", &format!("refs/tags/{tag}")], &Opts::default())
                    .map(|r| r.ok())
                    .unwrap_or(false);
                if !exists {
                    let msg = if o.tag_message.trim().is_empty() { tag.clone() } else { o.tag_message.clone() };
                    repo::create_tag(repo, &tag, Some(&cfg.master), &msg, false)?;
                    log.push(format!("Etiqueta {tag} creada"));
                }
            }
            if let Some(stop) = merge_step(repo, &cfg.develop, &branch, &mut log)? {
                return Ok(stop);
            }
        }
    }
    repo::checkout(repo, &cfg.develop)?;
    if !o.keep_branch {
        repo::delete_branch(repo, &branch, false)?;
        log.push(format!("Rama {branch} eliminada"));
    }
    Ok(OpResult { conflicts: false, output: log.join("\n") })
}
