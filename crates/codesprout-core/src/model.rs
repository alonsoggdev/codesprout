use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    /// Git status letter: M A D R C T U, or `?` for untracked. Conflicts use two letters (UU, AA…).
    pub status: String,
    pub orig: Option<String>,
}

impl FileChange {
    pub fn new(path: &str, status: &str, orig: Option<&str>) -> Self {
        Self { path: path.into(), status: status.into(), orig: orig.map(Into::into) }
    }
    pub fn is_untracked(&self) -> bool {
        self.status == "?"
    }
    /// The path plus the original path of a rename, for pathspecs.
    pub fn paths(&self) -> Vec<String> {
        let mut v = vec![self.path.clone()];
        if let Some(o) = &self.orig {
            v.push(o.clone());
        }
        v
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepoState {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

impl RepoState {
    pub fn command(self) -> &'static str {
        match self {
            RepoState::Merge => "merge",
            RepoState::Rebase => "rebase",
            RepoState::CherryPick => "cherry-pick",
            RepoState::Revert => "revert",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            RepoState::Merge => "Fusión en curso",
            RepoState::Rebase => "Rebase en curso",
            RepoState::CherryPick => "Cherry-pick en curso",
            RepoState::Revert => "Revert en curso",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub oid: Option<String>,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: Vec<FileChange>,
    pub unstaged: Vec<FileChange>,
    pub conflicted: Vec<FileChange>,
    pub state: Option<RepoState>,
    pub merge_msg: String,
}

impl Status {
    pub fn change_count(&self) -> usize {
        self.staged.len() + self.unstaged.len() + self.conflicted.len()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalBranch {
    pub name: String,
    pub hash: String,
    pub upstream: Option<String>,
    pub upstream_remote: Option<String>,
    pub upstream_branch: Option<String>,
    pub current: bool,
    pub ahead: u32,
    pub behind: u32,
    pub gone: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoteBranch {
    pub remote: String,
    pub branch: String,
    /// `remote/branch`
    pub name: String,
    pub hash: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tag {
    pub name: String,
    pub hash: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stash {
    pub reference: String,
    pub hash: String,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct Refs {
    pub remotes: Vec<String>,
    pub local: Vec<LocalBranch>,
    pub remote: Vec<RemoteBranch>,
    pub tags: Vec<Tag>,
    pub stashes: Vec<Stash>,
    pub head: Option<String>,
    pub current_branch: Option<String>,
    /// Changes whenever any ref moves; used to decide when to reload the log.
    pub sig: String,
}

impl Refs {
    pub fn current_local(&self) -> Option<&LocalBranch> {
        self.local.iter().find(|b| b.current)
    }
    pub fn default_remote(&self, branch: Option<&LocalBranch>) -> Option<String> {
        if let Some(r) = branch.and_then(|b| b.upstream_remote.clone()) {
            if self.remotes.contains(&r) {
                return Some(r);
            }
        }
        if self.remotes.iter().any(|r| r == "origin") {
            return Some("origin".into());
        }
        self.remotes.first().cloned()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub subject: String,
}

#[derive(Clone, Debug, Default)]
pub struct Signature {
    /// %G?: G good, B bad, U good-unknown validity, X/Y expired, R revoked, E cannot check, N none
    pub code: char,
    pub signer: String,
    pub key: String,
}

impl Signature {
    pub fn label(&self) -> &'static str {
        match self.code {
            'G' => "Firma válida",
            'U' => "Firma válida (confianza desconocida)",
            'X' => "Firma válida, pero caducada",
            'Y' => "Firmado con una clave caducada",
            'R' => "Firmado con una clave revocada",
            'B' => "Firma NO válida",
            'E' => "Firmado (no se puede verificar: falta la clave)",
            _ => "Sin firma",
        }
    }
    pub fn is_signed(&self) -> bool {
        self.code != 'N'
    }
}

#[derive(Clone, Debug, Default)]
pub struct CommitDetails {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub committer: String,
    pub committer_email: String,
    pub commit_time: i64,
    pub message: String,
    /// Diff base: first parent, or the empty tree for root commits.
    pub base: String,
    pub files: Vec<FileChange>,
    pub signature: Option<Signature>,
}

#[derive(Clone, Debug, Default)]
pub struct DiffText {
    pub text: String,
    pub truncated: bool,
    pub lossy: bool,
}

#[derive(Clone, Debug, Default)]
pub struct WorkingFile {
    pub directory: bool,
    pub binary: bool,
    pub size: u64,
    pub truncated: bool,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Identity {
    pub name: String,
    pub email: String,
    pub global_name: String,
    pub global_email: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}

/// Result of commands that may stop with conflicts instead of failing.
#[derive(Clone, Debug, Default)]
pub struct OpResult {
    pub conflicts: bool,
    pub output: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

impl ResetMode {
    pub fn flag(self) -> &'static str {
        match self {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DiffOptions {
    pub ignore_whitespace: bool,
    /// Lines of context; `None` uses git's default (3).
    pub context: Option<u32>,
}

impl DiffOptions {
    pub fn args(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.ignore_whitespace {
            v.push("-w".to_string());
        }
        if let Some(c) = self.context {
            v.push(format!("-U{c}"));
        }
        v
    }
}
