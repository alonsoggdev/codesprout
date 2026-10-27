//! Persisted state: settings, repository list, open tabs and accounts.

use std::collections::HashMap;
use std::path::PathBuf;

use codesprout_core::credentials::Account;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemePref {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ActionScope {
    #[default]
    Commit,
    File,
    Repo,
}

impl ActionScope {
    pub fn label(self) -> &'static str {
        match self {
            ActionScope::Commit => "Commit",
            ActionScope::File => "Archivo",
            ActionScope::Repo => "Repositorio",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CustomAction {
    pub name: String,
    pub program: String,
    pub args: String,
    pub scope: ActionScope,
    /// Show the output in a dialog when it finishes.
    pub show_output: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemePref,
    pub max_commits: usize,
    pub auto_fetch_min: u32,
    pub all_branches: bool,
    pub pull_rebase: bool,
    /// Last choice in the fetch dialog: delete local branches whose remote branch is gone.
    pub fetch_delete_gone: bool,
    pub clone_dir: String,
    /// Spacing between the lanes of the history graph.
    pub zoom: f32,
    pub ignore_whitespace: bool,
    pub context_lines: u32,
    pub word_diff: bool,
    pub sidebar_width: f32,
    pub detail_height: f32,
    pub changes_width: f32,
    pub collapsed: HashMap<String, bool>,
    pub custom_actions: Vec<CustomAction>,
    pub sign_commits: bool,
    pub window: Option<[f32; 4]>,
    pub maximized: bool,
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemePref::Dark,
            max_commits: 5000,
            auto_fetch_min: 0,
            all_branches: true,
            pull_rebase: false,
            fetch_delete_gone: false,
            clone_dir: String::new(),
            zoom: 1.0,
            ignore_whitespace: false,
            context_lines: 3,
            word_diff: true,
            sidebar_width: 250.0,
            detail_height: 330.0,
            changes_width: 380.0,
            collapsed: HashMap::new(),
            custom_actions: vec![],
            sign_commits: false,
            window: None,
            maximized: false,
            check_updates: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoEntry {
    pub path: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Store {
    pub settings: Settings,
    pub repos: Vec<RepoEntry>,
    pub tabs: Vec<String>,
    pub active: Option<String>,
    pub accounts: Vec<Account>,
}

fn file() -> Option<PathBuf> {
    let base = dirs::config_dir()?;
    let dir = base.join("CodeSprout");
    // the app used to be called Destiny Stone: keep its settings and repositories
    let legacy = base.join("DestinyStone");
    if !dir.exists() && legacy.is_dir() {
        let _ = std::fs::rename(&legacy, &dir);
    }
    Some(dir.join("state.json"))
}

impl Store {
    pub fn load() -> Store {
        let mut s: Store = file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default();
        s.tabs.retain(|t| s.repos.iter().any(|r| &r.path == t));
        if s.active.as_ref().is_some_and(|a| !s.tabs.contains(a)) {
            s.active = s.tabs.first().cloned();
        }
        s.settings.zoom = s.settings.zoom.clamp(0.4, 2.5);
        s
    }

    pub fn save(&self) {
        let Some(f) = file() else { return };
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = f.with_extension("json.tmp");
        if let Ok(text) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &f);
            }
        }
    }
}
