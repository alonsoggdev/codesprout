//! Interactive rebase without an editor: the todo list is written by the app and handed to
//! git through GIT_SEQUENCE_EDITOR; rewording uses `exec git commit --amend -F <file>`.

use std::path::Path;

use anyhow::{bail, Result};

use crate::model::{Commit, OpResult};
use crate::repo::{conflict_aware, git_dir, parse_log, LOG_FORMAT};
use crate::runner::{out, sh_quote, Opts};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TodoAction {
    Pick,
    /// Keep the changes, replace the message.
    Reword(String),
    /// Meld into the previous commit, combining both messages.
    Squash,
    /// Meld into the previous commit, discarding this message.
    Fixup,
    /// Stop after applying, to amend the commit by hand.
    Edit,
    Drop,
}

impl TodoAction {
    pub fn label(&self) -> &'static str {
        match self {
            TodoAction::Pick => "Mantener",
            TodoAction::Reword(_) => "Cambiar mensaje",
            TodoAction::Squash => "Combinar (squash)",
            TodoAction::Fixup => "Combinar sin mensaje (fixup)",
            TodoAction::Edit => "Parar para editar",
            TodoAction::Drop => "Eliminar",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TodoEntry {
    pub commit: Commit,
    pub action: TodoAction,
}

pub struct Plan {
    /// Oldest first, as git applies them.
    pub commits: Vec<Commit>,
    /// The rebase would flatten merge commits.
    pub has_merges: bool,
}

/// Commits that an interactive rebase onto `base` would rewrite. `base = None` means the
/// whole history (`--root`).
pub fn plan(repo: &Path, base: Option<&str>) -> Result<Plan> {
    let range = match base {
        Some(b) => format!("{b}..HEAD"),
        None => "HEAD".into(),
    };
    let commits = parse_log(&out(repo, &["log", "--reverse", "--no-merges", LOG_FORMAT, &range, "--"])?);
    let merges = out(repo, &["rev-list", "--merges", "--count", &range, "--"])?;
    Ok(Plan { commits, has_merges: merges.trim() != "0" })
}

pub fn build_todo(entries: &[TodoEntry], msg_dir: &Path) -> Result<(String, Vec<(std::path::PathBuf, String)>)> {
    let mut lines = Vec::new();
    let mut files = Vec::new();
    let mut first_kept = true;
    for (i, e) in entries.iter().enumerate() {
        let short = &e.commit.hash;
        let subj = e.commit.subject.replace('\n', " ");
        match &e.action {
            TodoAction::Drop => {
                lines.push(format!("drop {short} {subj}"));
                continue;
            }
            TodoAction::Squash | TodoAction::Fixup if first_kept => {
                bail!("«{}» no puede combinarse: no hay ningún commit anterior que se mantenga.", e.commit.subject);
            }
            TodoAction::Squash => lines.push(format!("squash {short} {subj}")),
            TodoAction::Fixup => lines.push(format!("fixup {short} {subj}")),
            TodoAction::Edit => lines.push(format!("edit {short} {subj}")),
            TodoAction::Pick => lines.push(format!("pick {short} {subj}")),
            TodoAction::Reword(msg) => {
                if msg.trim().is_empty() {
                    bail!("El nuevo mensaje de «{}» está vacío.", e.commit.subject);
                }
                lines.push(format!("pick {short} {subj}"));
                let file = msg_dir.join(format!("reword-{i}.txt"));
                lines.push(format!("exec git commit --amend --allow-empty -F {}", sh_quote(&file)));
                files.push((file, msg.clone()));
            }
        }
        first_kept = false;
    }
    if entries.iter().all(|e| e.action == TodoAction::Drop) {
        bail!("No puedes eliminar todos los commits con un rebase interactivo. Usa «Reset» en su lugar.");
    }
    Ok((lines.join("\n") + "\n", files))
}

pub fn run(repo: &Path, base: Option<&str>, entries: &[TodoEntry]) -> Result<OpResult> {
    if entries.is_empty() {
        bail!("No hay commits que reescribir.");
    }
    let dir = git_dir(repo)?.join("codesprout");
    std::fs::create_dir_all(&dir)?;
    let (todo, msgs) = build_todo(entries, &dir)?;
    for (f, m) in &msgs {
        std::fs::write(f, m)?;
    }
    let todo_file = dir.join("rebase-todo.txt");
    std::fs::write(&todo_file, todo)?;
    let editor = format!("cp {}", sh_quote(&todo_file));
    let mut args = vec!["rebase", "-i"];
    match base {
        Some(b) => args.push(b),
        None => args.push("--root"),
    }
    conflict_aware(repo, &args, &Opts::default().with_env("GIT_SEQUENCE_EDITOR", editor))
}
