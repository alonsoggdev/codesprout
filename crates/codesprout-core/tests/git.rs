//! Integration tests against real temporary repositories (port of the Electron
//! `test/git.test.js`, plus the features added in the Rust version).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use codesprout_core::diff::{build_patch, can_partial, parse_diff};
use codesprout_core::explorer::{layout, ColumnMode, LaneKind};
use codesprout_core::history::{self, SearchMode};
use codesprout_core::rebase::{self, TodoAction, TodoEntry};
use codesprout_core::repo::{self as g, CommitFlags, PushArgs};
use codesprout_core::{extras, flow, DiffOptions, FileChange, RepoState, ResetMode};

/// Temporary folder for a test. `CODESPROUT_TEST_ROOT` moves them elsewhere, e.g. into WSL
/// (`\\wsl.localhost\Ubuntu\tmp\cs-tests`, with `CODESPROUT_WSL_AGENT` pointing to the agent
/// built for x86_64-unknown-linux-musl) to run the whole suite with the distro's git.
fn tmpdir() -> tempfile::TempDir {
    static AGENT: std::sync::Once = std::sync::Once::new();
    AGENT.call_once(|| {
        if let Some(p) = std::env::var_os("CODESPROUT_WSL_AGENT") {
            codesprout_core::wsl::set_agent(Box::leak(fs::read(p).expect("agente de WSL").into_boxed_slice()));
        }
    });
    match std::env::var_os("CODESPROUT_TEST_ROOT") {
        Some(root) => tempfile::Builder::new().prefix("cs-").tempdir_in(root).unwrap(),
        None => tempfile::tempdir().unwrap(),
    }
}

/// Runs git through the app's runner, so repositories in WSL use the distro's git too.
fn sh(cwd: &Path, args: &[&str]) -> String {
    let args: Vec<String> =
        args.iter().map(|a| if Path::new(a).is_absolute() { codesprout_core::wsl::arg_path(cwd, Path::new(a)) } else { a.to_string() }).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = codesprout_core::runner::run(cwd, &args, &Default::default()).expect("git");
    assert!(o.ok(), "git {:?}: {}", args, o.stderr);
    o.text()
}

fn write(repo: &Path, rel: &str, text: impl AsRef<[u8]>) {
    let p = repo.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn read(repo: &Path, rel: &str) -> String {
    fs::read_to_string(repo.join(rel)).unwrap()
}

fn new_repo(root: &Path, name: &str) -> PathBuf {
    let repo = g::init(&root.join(name)).unwrap();
    for (k, v) in [
        ("user.name", "Tester"),
        ("user.email", "tester@example.com"),
        ("core.autocrlf", "false"),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
    ] {
        sh(&repo, &["config", k, v]);
    }
    sh(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    repo
}

fn commit_all(repo: &Path, msg: &str) -> String {
    g::stage_all(repo).unwrap();
    g::commit(repo, msg, CommitFlags::default()).unwrap()
}

fn paths(v: &[FileChange]) -> Vec<&str> {
    v.iter().map(|f| f.path.as_str()).collect()
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn unborn_repo_status_log_refs_unstage() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "unborn");
    assert!(g::log(&repo, 100, true).unwrap().is_empty());
    write(&repo, "a b.txt", "hola\n");
    write(&repo, "dir/ñandú.txt", "x\n");
    let st = g::status(&repo).unwrap();
    assert_eq!(paths(&st.unstaged), ["a b.txt", "dir/ñandú.txt"]);
    assert_eq!(st.unstaged[0].status, "?");
    g::stage(&repo, &s(&["a b.txt", "dir/ñandú.txt"])).unwrap();
    assert_eq!(g::status(&repo).unwrap().staged.len(), 2);
    g::unstage(&repo, &s(&["a b.txt"])).unwrap();
    assert_eq!(paths(&g::status(&repo).unwrap().staged), ["dir/ñandú.txt"]);
    assert_eq!(g::refs(&repo).unwrap().head, None);
    g::unstage_all(&repo).unwrap();
    assert!(g::status(&repo).unwrap().staged.is_empty());
}

#[test]
fn commit_log_details_branches_tags_special_names() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "basic");
    write(&repo, "readme.md", "uno\ndos\ntres\n");
    write(&repo, "[weird] name.txt", "w\n");
    let h1 = commit_all(&repo, "Primer commit\n\nCuerpo del mensaje");
    assert_eq!(h1.len(), 40);
    let st = g::status(&repo).unwrap();
    assert_eq!(st.change_count(), 0);
    let d = g::commit_details(&repo, &h1).unwrap();
    assert_eq!(d.message, "Primer commit\n\nCuerpo del mensaje");
    let mut files = paths(&d.files);
    files.sort();
    assert_eq!(files, ["[weird] name.txt", "readme.md"]);
    assert_eq!(d.signature.as_ref().unwrap().code, 'N');
    let readme = d.files.iter().find(|f| f.path == "readme.md").unwrap();
    let fd = g::commit_file_diff(&repo, &d.base, &h1, readme, &DiffOptions::default()).unwrap();
    assert!(fd.text.contains("+uno"));

    // literal pathspecs: the glob-looking name must be staged alone
    write(&repo, "[weird] name.txt", "w2\n");
    write(&repo, "w name.txt", "should not be staged\n");
    g::stage(&repo, &s(&["[weird] name.txt"])).unwrap();
    assert_eq!(paths(&g::status(&repo).unwrap().staged), ["[weird] name.txt"]);
    g::commit(&repo, "segundo", CommitFlags::default()).unwrap();
    fs::remove_file(repo.join("w name.txt")).unwrap();

    g::create_branch(&repo, "feature/x", None, true).unwrap();
    assert_eq!(g::refs(&repo).unwrap().current_branch.as_deref(), Some("feature/x"));
    assert!(g::create_branch(&repo, "bad name", None, false).is_err());
    assert!(g::create_branch(&repo, "-x", None, false).is_err());
    g::create_tag(&repo, "v1.0", None, "Versión 1", false).unwrap();
    g::create_tag(&repo, "light", Some(&h1), "", false).unwrap();
    let refs = g::refs(&repo).unwrap();
    let mut tags: Vec<_> = refs.tags.iter().map(|t| t.name.as_str()).collect();
    tags.sort();
    assert_eq!(tags, ["light", "v1.0"]);
    assert_eq!(refs.tags.iter().find(|t| t.name == "light").unwrap().hash, h1);
    assert_eq!(Some(&refs.tags.iter().find(|t| t.name == "v1.0").unwrap().hash), refs.head.as_ref());
    g::checkout(&repo, "main").unwrap();
    g::rename_branch(&repo, "feature/x", "feature/y").unwrap();
    g::delete_branch(&repo, "feature/y", false).unwrap();
    g::delete_tag(&repo, "light").unwrap();
    assert_eq!(g::refs(&repo).unwrap().local.len(), 1);
    let log = g::log(&repo, 10, true).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[1].hash, h1);
}

#[test]
fn partial_staging_hunks_and_lines_lf_and_crlf() {
    for eol in ["\n", "\r\n"] {
        let tmp = tmpdir();
        let repo = new_repo(tmp.path(), "partial");
        let lines: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
        write(&repo, "f.txt", lines.join(eol) + eol);
        commit_all(&repo, "base");
        let mut changed = lines.clone();
        changed[1] = "CHANGED 2".into();
        changed.splice(25..25, ["ADDED A".to_string(), "ADDED B".to_string()]);
        write(&repo, "f.txt", changed.join(eol) + eol);
        let f = FileChange::new("f.txt", "M", None);
        let o = DiffOptions::default();

        let diff = parse_diff(&g::working_diff(&repo, &f, false, &o).unwrap().text);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].hunks.len(), 2);
        assert!(can_partial(&diff[0]));

        // stage only the second hunk
        g::apply_patch(&repo, &build_patch(&diff[0], &diff[0].hunks[1], None, false).unwrap(), true, false).unwrap();
        let staged = parse_diff(&g::working_diff(&repo, &f, true, &o).unwrap().text);
        assert_eq!(staged[0].hunks.len(), 1);
        assert!(staged[0].hunks[0].lines.iter().any(|l| l.starts_with("+ADDED A")));

        // unstage only the line "ADDED B"
        let h = &staged[0].hunks[0];
        let idx = h.lines.iter().position(|l| l.starts_with("+ADDED B")).unwrap();
        let sel: HashSet<usize> = [idx].into();
        g::apply_patch(&repo, &build_patch(&staged[0], h, Some(&sel), true).unwrap(), true, true).unwrap();
        let staged = parse_diff(&g::working_diff(&repo, &f, true, &o).unwrap().text);
        let adds: Vec<String> =
            staged[0].hunks[0].lines.iter().filter(|l| l.starts_with('+')).map(|l| l.trim_end_matches('\r').to_string()).collect();
        assert_eq!(adds, ["+ADDED A"]);

        // stage only the deletion line of hunk 1 (keep "CHANGED 2" unstaged)
        let diff = parse_diff(&g::working_diff(&repo, &f, false, &o).unwrap().text);
        let h1 = &diff[0].hunks[0];
        let del = h1.lines.iter().position(|l| l.starts_with("-line 2")).unwrap();
        let sel: HashSet<usize> = [del].into();
        g::apply_patch(&repo, &build_patch(&diff[0], h1, Some(&sel), false).unwrap(), true, false).unwrap();
        let index = sh(&repo, &["show", ":f.txt"]);
        assert!(!index.contains(&format!("line 2{eol}")));
        assert!(!index.contains("CHANGED 2"));
        assert!(index.contains("ADDED A"));

        // discard the remaining working tree change of "ADDED B" only
        let diff = parse_diff(&g::working_diff(&repo, &f, false, &o).unwrap().text);
        let hb = diff[0].hunks.iter().find(|x| x.lines.iter().any(|l| l.starts_with("+ADDED B"))).unwrap();
        let ib = hb.lines.iter().position(|l| l.starts_with("+ADDED B")).unwrap();
        let sel: HashSet<usize> = [ib].into();
        g::apply_patch(&repo, &build_patch(&diff[0], hb, Some(&sel), true).unwrap(), false, true).unwrap();
        let wt = read(&repo, "f.txt");
        assert!(!wt.contains("ADDED B"));
        assert!(wt.contains("CHANGED 2"));
        assert!(wt.contains("ADDED A"));
        if eol == "\r\n" {
            let parts: Vec<&str> = wt.split('\n').collect();
            assert!(parts[..parts.len() - 1].iter().all(|l| l.ends_with('\r')), "CRLF preserved");
        }
    }
}

#[test]
fn renames_show_in_status_and_diff() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "rename");
    write(&repo, "old.txt", "contenido estable\n".repeat(20));
    commit_all(&repo, "base");
    fs::rename(repo.join("old.txt"), repo.join("new.txt")).unwrap();
    g::stage_all(&repo).unwrap();
    let st = g::status(&repo).unwrap();
    assert_eq!(st.staged, vec![FileChange::new("new.txt", "R", Some("old.txt"))]);
    let d = parse_diff(&g::working_diff(&repo, &st.staged[0], true, &DiffOptions::default()).unwrap().text);
    assert_eq!(d[0].renamed_from.as_deref(), Some("old.txt"));
    g::unstage(&repo, &s(&["new.txt", "old.txt"])).unwrap();
    assert!(g::status(&repo).unwrap().staged.is_empty());
}

#[test]
fn merge_conflicts_are_reported_resolved_and_continued() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "conflict");
    write(&repo, "c.txt", "base\n");
    commit_all(&repo, "base");
    let main = g::refs(&repo).unwrap().current_branch.unwrap();
    g::create_branch(&repo, "other", None, true).unwrap();
    write(&repo, "c.txt", "other\n");
    commit_all(&repo, "other change");
    g::checkout(&repo, &main).unwrap();
    write(&repo, "c.txt", "mine\n");
    commit_all(&repo, "main change");
    let res = g::merge(&repo, "other", false).unwrap();
    assert!(res.conflicts);
    let st = g::status(&repo).unwrap();
    assert_eq!(st.state, Some(RepoState::Merge));
    assert_eq!(paths(&st.conflicted), ["c.txt"]);
    assert!(st.merge_msg.contains("Merge branch 'other'"));
    g::resolve_conflict(&repo, "c.txt", false).unwrap();
    assert_eq!(read(&repo, "c.txt"), "other\n");
    assert!(!g::continue_operation(&repo, RepoState::Merge).unwrap().conflicts);
    assert_eq!(g::status(&repo).unwrap().state, None);
    let log = g::log(&repo, 100, true).unwrap();
    assert_eq!(log[0].parents.len(), 2);
    // the explorer shows "other" as its own lane, merged into main
    let refs = g::refs(&repo).unwrap();
    let l = layout(&log, &refs, false, ColumnMode::Compact);
    let other = l.lanes.iter().find(|x| x.name == "other").unwrap();
    assert_eq!(other.merged_into, Some(0));
    assert!(other.row >= 1);
    // merge that fails for a non-conflict reason must error
    assert!(g::merge(&repo, "does-not-exist", false).is_err());
    // abort path
    g::checkout(&repo, "other").unwrap();
    write(&repo, "c.txt", "other 2\n");
    commit_all(&repo, "other 2");
    g::checkout(&repo, &main).unwrap();
    write(&repo, "c.txt", "mine 2\n");
    commit_all(&repo, "mine 2");
    assert!(g::merge(&repo, "other", false).unwrap().conflicts);
    g::abort_operation(&repo, RepoState::Merge).unwrap();
    assert_eq!(g::status(&repo).unwrap().state, None);
    assert_eq!(read(&repo, "c.txt"), "mine 2\n");
}

#[test]
fn stash_cherry_pick_revert_reset() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "stash");
    write(&repo, "s.txt", "1\n");
    let base = commit_all(&repo, "base");
    write(&repo, "s.txt", "2\n");
    write(&repo, "untracked.txt", "u\n");
    g::stash_save(&repo, "mi stash", true, false).unwrap();
    let refs = g::refs(&repo).unwrap();
    assert_eq!(refs.stashes.len(), 1);
    assert!(refs.stashes[0].message.contains("mi stash"));
    assert_eq!(read(&repo, "s.txt"), "1\n");
    assert!(!repo.join("untracked.txt").exists());
    g::stash_apply(&repo, "stash@{0}", false).unwrap();
    assert_eq!(read(&repo, "s.txt"), "2\n");
    g::discard_all(&repo, true).unwrap();
    assert_eq!(read(&repo, "s.txt"), "1\n");
    assert!(!repo.join("untracked.txt").exists());
    g::stash_apply(&repo, "stash@{0}", true).unwrap();
    assert!(g::refs(&repo).unwrap().stashes.is_empty());
    g::discard_all(&repo, true).unwrap();
    write(&repo, "s.txt", "3\n");
    g::stash_save(&repo, "", false, false).unwrap();
    g::stash_drop(&repo, "stash@{0}").unwrap();
    assert!(g::refs(&repo).unwrap().stashes.is_empty());

    let main = g::refs(&repo).unwrap().current_branch.unwrap();
    g::create_branch(&repo, "pick", None, true).unwrap();
    write(&repo, "p.txt", "pick\n");
    let pick = commit_all(&repo, "to pick");
    g::checkout(&repo, &main).unwrap();
    assert!(!g::cherry_pick(&repo, &pick, false).unwrap().conflicts);
    assert_eq!(read(&repo, "p.txt"), "pick\n");
    let head = g::refs(&repo).unwrap().head.unwrap();
    assert!(!g::revert(&repo, &head, false).unwrap().conflicts);
    assert!(!repo.join("p.txt").exists());
    g::reset(&repo, &base, ResetMode::Hard).unwrap();
    assert_eq!(g::refs(&repo).unwrap().head, Some(base));
}

#[test]
fn clone_remotes_fetch_push_pull_upstream() {
    let tmp = tmpdir();
    let root = tmp.path();
    let bare = root.join("origin.git");
    sh(root, &["init", "--bare", "-b", "main", bare.to_str().unwrap()]);
    let a = new_repo(root, "clone-a");
    write(&a, "f.txt", "a\n");
    commit_all(&a, "a1");
    g::add_remote(&a, "origin", bare.to_str().unwrap()).unwrap();
    let branch = g::refs(&a).unwrap().current_branch.unwrap();
    let push = |repo: &Path, b: &str, up: bool| {
        g::push(repo, &PushArgs { remote: "origin".into(), branch: b.into(), set_upstream: up, ..Default::default() }, None)
    };
    push(&a, &branch, true).unwrap();
    let refs = g::refs(&a).unwrap();
    assert_eq!(refs.local[0].upstream.as_deref(), Some(format!("origin/{branch}").as_str()));
    assert_eq!(refs.local[0].upstream_remote.as_deref(), Some("origin"));
    assert_eq!(refs.local[0].upstream_branch.as_deref(), Some(branch.as_str()));

    let b = g::clone(bare.to_str().unwrap(), &root.join("clone-b"), None).unwrap();
    sh(&b, &["config", "user.name", "B"]);
    sh(&b, &["config", "user.email", "b@example.com"]);
    assert_eq!(read(&b, "f.txt").replace('\r', ""), "a\n");
    let err = g::clone(bare.to_str().unwrap(), &root.join("clone-b"), None).unwrap_err();
    assert!(err.to_string().contains("no está vacía"));

    write(&b, "f.txt", "b\n");
    commit_all(&b, "b1");
    push(&b, &branch, false).unwrap();
    g::fetch(&a, None, true, None).unwrap();
    assert_eq!(g::refs(&a).unwrap().local[0].behind, 1);
    assert!(!g::pull(&a, Some("origin"), Some(&branch), false, None).unwrap().conflicts);
    assert_eq!(read(&a, "f.txt"), "b\n");

    g::create_branch(&b, "feat", None, false).unwrap();
    push(&b, "feat", true).unwrap();
    g::fetch(&a, Some("origin"), true, None).unwrap();
    let refs = g::refs(&a).unwrap();
    assert!(refs.remote.iter().any(|r| r.name == "origin/feat" && r.remote == "origin" && r.branch == "feat"));
    g::checkout_remote(&a, "origin/feat", "feat").unwrap();
    let refs = g::refs(&a).unwrap();
    assert_eq!(refs.current_branch.as_deref(), Some("feat"));
    assert_eq!(refs.local.iter().find(|l| l.name == "feat").unwrap().upstream.as_deref(), Some("origin/feat"));
    g::checkout(&a, &branch).unwrap();
    // a branch that is not checked out is fast-forwarded in place, and refused once it diverges
    g::checkout(&b, "feat").unwrap();
    write(&b, "g.txt", "1
");
    commit_all(&b, "b2");
    push(&b, "feat", false).unwrap();
    g::fast_forward(&a, "origin", "feat", "feat", None).unwrap();
    assert_eq!(sh(&a, &["rev-parse", "feat"]), sh(&b, &["rev-parse", "HEAD"]));
    assert_eq!(g::refs(&a).unwrap().current_branch.as_deref(), Some(branch.as_str()));
    g::checkout(&a, "feat").unwrap();
    sh(&a, &["reset", "-q", "--hard", "HEAD~1"]);
    sh(&a, &["commit", "--allow-empty", "-q", "-m", "x"]);
    g::checkout(&a, &branch).unwrap();
    let err = g::fast_forward(&a, "origin", "feat", "feat", None).unwrap_err();
    assert!(err.to_string().contains("cámbiate a ella"), "{err}");
    g::delete_remote_branch(&a, "origin", "feat", None).unwrap();
    // local branches whose remote branch is gone (new): merged ones go, unmerged ones stay
    g::create_branch(&a, "done", None, false).unwrap();
    push(&a, "done", true).unwrap();
    g::delete_remote_branch(&b, "origin", "done", None).unwrap();
    g::fetch(&a, Some("origin"), true, None).unwrap();
    let gone = g::delete_gone_branches(&a, Some("origin")).unwrap();
    assert_eq!(gone.deleted, ["done"]);
    assert_eq!(gone.kept, ["feat"]);
    assert!(g::delete_gone_branches(&a, Some("otro")).unwrap().kept.is_empty());
    g::create_tag(&a, "v2", None, "", false).unwrap();
    g::push_tag(&a, "origin", "v2", None).unwrap();
    assert!(sh(&bare, &["tag"]).contains("v2"));
    g::delete_remote_tag(&a, "origin", "v2", None).unwrap();
    assert!(!sh(&bare, &["tag"]).contains("v2"));

    // remote editing (new): rename and change URL
    let info = g::remotes_info(&a).unwrap();
    assert_eq!(info.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["origin"]);
    g::rename_remote(&a, "origin", "upstream").unwrap();
    g::set_remote_url(&a, "upstream", "https://example.com/x.git").unwrap();
    let info = g::remotes_info(&a).unwrap();
    assert_eq!(info[0].name, "upstream");
    assert_eq!(info[0].url, "https://example.com/x.git");
    g::remove_remote(&a, "upstream").unwrap();
    assert!(g::refs(&a).unwrap().remotes.is_empty());
}

#[test]
fn identity_gitignore_read_working_file_resolve_repo() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "misc");
    g::set_identity(&repo, "Local Name", "local@example.com", false).unwrap();
    assert_eq!(g::identity(&repo).name, "Local Name");
    write(&repo, "build/out.log", "x");
    write(&repo, "#hash.txt", "x");
    g::ignore_paths(&repo, &s(&["build/out.log"]), true).unwrap();
    g::ignore_paths(&repo, &s(&["#hash.txt"]), false).unwrap();
    assert_eq!(paths(&g::status(&repo).unwrap().unstaged), [".gitignore"]);
    write(&repo, "bin.dat", [0u8, 1, 2, 3]);
    assert!(g::read_working_file(&repo, "bin.dat", 1 << 20).unwrap().binary);
    assert!(!g::read_working_file(&repo, ".gitignore", 1 << 20).unwrap().binary);
    let err = g::read_working_file(&repo, "../outside.txt", 1 << 20).unwrap_err();
    assert!(err.to_string().contains("fuera del repositorio"));
    fs::create_dir_all(repo.join("sub").join("deep")).unwrap();
    assert_eq!(g::resolve_repo(&repo.join("sub").join("deep")).unwrap(), repo);
    let err = g::resolve_repo(tmp.path()).unwrap_err();
    assert!(err.to_string().contains("no es un repositorio"));
}

#[test]
fn status_parser_handles_every_record_type() {
    let raw = [
        "# branch.oid abc",
        "# branch.head main",
        "# branch.upstream origin/main",
        "# branch.ab +2 -3",
        "1 .M N... 100644 100644 100644 aaa aaa file with spaces.txt",
        "1 A. N... 000000 100644 100644 000 bbb added.txt",
        "2 R. N... 100644 100644 100644 ccc ccc R100 new name.txt",
        "old name.txt",
        "u UU N... 100644 100644 100644 100644 a b c conflict.txt",
        "? untracked file.txt",
        "! ignored.txt",
        "",
    ]
    .join("\0");
    let st = g::parse_status(&raw);
    assert_eq!(st.branch.as_deref(), Some("main"));
    assert_eq!((st.ahead, st.behind), (2, 3));
    assert_eq!(
        st.staged,
        vec![FileChange::new("added.txt", "A", None), FileChange::new("new name.txt", "R", Some("old name.txt"))]
    );
    let un: Vec<_> = st.unstaged.iter().map(|f| (f.path.as_str(), f.status.as_str())).collect();
    assert_eq!(un, [("file with spaces.txt", "M"), ("untracked file.txt", "?")]);
    assert_eq!(st.conflicted, vec![FileChange::new("conflict.txt", "UU", None)]);
}

// ---------------------------------------------------------------- new in the Rust version

#[test]
fn file_history_follows_renames_and_blame() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "history");
    write(&repo, "a.txt", "uno\ndos\n");
    let c1 = commit_all(&repo, "crea a");
    write(&repo, "other.txt", "x\n");
    commit_all(&repo, "otro archivo");
    fs::rename(repo.join("a.txt"), repo.join("b.txt")).unwrap();
    commit_all(&repo, "renombra a b");
    write(&repo, "b.txt", "uno\nDOS\ntres\n");
    let c4 = commit_all(&repo, "edita b");
    let h = history::file_history(&repo, "b.txt", 100).unwrap();
    let subjects: Vec<_> = h.iter().map(|r| r.commit.subject.as_str()).collect();
    assert_eq!(subjects, ["edita b", "renombra a b", "crea a"]);
    assert_eq!(h[0].path, "b.txt");
    assert_eq!(h[1].status, "R");
    assert_eq!(h[2].path, "a.txt");

    let b = history::blame(&repo, "b.txt", None).unwrap();
    assert_eq!(b.lines.len(), 3);
    assert_eq!(b.lines[0].hash, c1);
    assert_eq!(b.lines[1].hash, c4);
    assert_eq!(b.lines[1].content, "DOS");
    assert_eq!(b.commits[&c4].summary, "edita b");
    assert_eq!(b.commits[&c4].author, "Tester");
}

#[test]
fn search_by_message_author_content_and_path() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "search");
    write(&repo, "x.txt", "alpha\n");
    commit_all(&repo, "Añade alpha");
    write(&repo, "y/z.txt", "beta\n");
    commit_all(&repo, "Fix: beta");
    write(&repo, "x.txt", "alpha\ngamma(1)\n");
    commit_all(&repo, "gamma");
    let subj = |v: Vec<codesprout_core::Commit>| v.into_iter().map(|c| c.subject).collect::<Vec<_>>();
    assert_eq!(subj(history::search(&repo, "fix:", SearchMode::Message, true, 100).unwrap()), ["Fix: beta"]);
    assert_eq!(history::search(&repo, "tester", SearchMode::Author, true, 100).unwrap().len(), 3);
    assert_eq!(subj(history::search(&repo, "gamma(1)", SearchMode::Content, true, 100).unwrap()), ["gamma"]);
    assert_eq!(subj(history::search(&repo, "gam+a", SearchMode::Regex, true, 100).unwrap()), ["gamma"]);
    assert_eq!(subj(history::search(&repo, "y", SearchMode::Path, true, 100).unwrap()), ["Fix: beta"]);
    assert!(history::search(&repo, "   ", SearchMode::Message, true, 100).unwrap().is_empty());
}

#[test]
fn reflog_recovers_reset_commits() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "reflog");
    write(&repo, "r.txt", "1\n");
    let base = commit_all(&repo, "base");
    write(&repo, "r.txt", "2\n");
    let lost = commit_all(&repo, "se perderá");
    g::reset(&repo, &base, ResetMode::Hard).unwrap();
    let entries = history::reflog(&repo, "HEAD", 50).unwrap();
    assert!(entries[0].action.starts_with("reset"));
    let found = entries.iter().find(|e| e.hash == lost).expect("lost commit in reflog");
    assert_eq!(found.subject, "se perderá");
    g::create_branch(&repo, "rescate", Some(&found.hash), false).unwrap();
    assert!(g::refs(&repo).unwrap().local.iter().any(|b| b.name == "rescate" && b.hash == lost));
}

#[test]
fn interactive_rebase_reorder_squash_reword_drop() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "rebase-i");
    write(&repo, "base.txt", "b\n");
    let base = commit_all(&repo, "base");
    for (f, m) in [("1.txt", "uno"), ("2.txt", "dos"), ("3.txt", "tres"), ("4.txt", "cuatro")] {
        write(&repo, f, m);
        commit_all(&repo, m);
    }
    let plan = rebase::plan(&repo, Some(&base)).unwrap();
    assert!(!plan.has_merges);
    let names: Vec<_> = plan.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(names, ["uno", "dos", "tres", "cuatro"]);
    let c = |i: usize| plan.commits[i].clone();
    // new order: tres (reworded), uno, dos squashed into uno, cuatro dropped
    let entries = vec![
        TodoEntry { commit: c(2), action: TodoAction::Reword("TRES nuevo\n\ncon cuerpo".into()) },
        TodoEntry { commit: c(0), action: TodoAction::Pick },
        TodoEntry { commit: c(1), action: TodoAction::Fixup },
        TodoEntry { commit: c(3), action: TodoAction::Drop },
    ];
    let r = rebase::run(&repo, Some(&base), &entries).unwrap();
    assert!(!r.conflicts, "{}", r.output);
    let log = g::log(&repo, 10, false).unwrap();
    let subjects: Vec<_> = log.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, ["uno", "TRES nuevo", "base"]);
    assert!(repo.join("2.txt").exists(), "fixup keeps the changes");
    assert!(!repo.join("4.txt").exists(), "drop removes the commit");
    assert_eq!(g::last_commit_message(&repo).unwrap(), "uno");
    let d = g::commit_details(&repo, &log[1].hash).unwrap();
    assert_eq!(d.message, "TRES nuevo\n\ncon cuerpo");
    // squash/fixup cannot be first
    let bad = vec![TodoEntry { commit: c(0), action: TodoAction::Squash }];
    assert!(rebase::build_todo(&bad, tmp.path()).is_err());
}

#[test]
fn interactive_rebase_edit_stops() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "rebase-edit");
    write(&repo, "a", "a");
    let base = commit_all(&repo, "base");
    write(&repo, "b", "b");
    commit_all(&repo, "b");
    let plan = rebase::plan(&repo, Some(&base)).unwrap();
    let entries = vec![TodoEntry { commit: plan.commits[0].clone(), action: TodoAction::Edit }];
    rebase::run(&repo, Some(&base), &entries).unwrap();
    assert_eq!(g::status(&repo).unwrap().state, Some(RepoState::Rebase));
    assert!(!g::continue_operation(&repo, RepoState::Rebase).unwrap().conflicts);
    assert_eq!(g::status(&repo).unwrap().state, None);
}

#[test]
fn git_flow_feature_and_release() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "flow");
    write(&repo, "app.txt", "v0\n");
    commit_all(&repo, "inicio");
    assert!(flow::config(&repo).is_none());
    let cfg = flow::FlowConfig { version_tag: "v".into(), ..Default::default() };
    flow::init(&repo, &cfg).unwrap();
    assert_eq!(flow::config(&repo).unwrap(), cfg);

    let b = flow::start(&repo, flow::FlowKind::Feature, "login").unwrap();
    assert_eq!(b, "feature/login");
    write(&repo, "login.txt", "ok\n");
    commit_all(&repo, "login");
    let refs = g::refs(&repo).unwrap();
    assert_eq!(flow::in_progress(&repo, flow::FlowKind::Feature, &refs.local), ["login"]);
    let r = flow::finish(&repo, flow::FlowKind::Feature, "login", &Default::default()).unwrap();
    assert!(!r.conflicts);
    let refs = g::refs(&repo).unwrap();
    assert_eq!(refs.current_branch.as_deref(), Some("develop"));
    assert!(!refs.local.iter().any(|b| b.name == "feature/login"));
    assert!(repo.join("login.txt").exists());

    flow::start(&repo, flow::FlowKind::Release, "1.0").unwrap();
    write(&repo, "app.txt", "v1\n");
    commit_all(&repo, "versión 1.0");
    flow::finish(&repo, flow::FlowKind::Release, "1.0", &Default::default()).unwrap();
    let refs = g::refs(&repo).unwrap();
    assert!(refs.tags.iter().any(|t| t.name == "v1.0"));
    let main = refs.local.iter().find(|b| b.name == "main").unwrap();
    let develop = refs.local.iter().find(|b| b.name == "develop").unwrap();
    assert_eq!(sh(&repo, &["show", &format!("{}:app.txt", main.hash)]), "v1\n");
    assert_eq!(sh(&repo, &["show", &format!("{}:app.txt", develop.hash)]), "v1\n");
}

#[test]
fn submodules_worktrees_patches_archive() {
    let tmp = tmpdir();
    let root = tmp.path();
    let lib = new_repo(root, "lib");
    write(&lib, "lib.txt", "lib\n");
    commit_all(&lib, "lib");
    let app = new_repo(root, "app");
    write(&app, "app.txt", "app\n");
    commit_all(&app, "app");

    // submodules (file protocol must be allowed explicitly in recent git)
    sh(&app, &["-c", "protocol.file.allow=always", "submodule", "add", lib.to_str().unwrap(), "vendor/lib"]);
    commit_all(&app, "añade submódulo");
    let subs = extras::submodules(&app).unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].path, "vendor/lib");
    assert_eq!(subs[0].state, extras::SubmoduleState::UpToDate);
    extras::submodule_deinit(&app, "vendor/lib").unwrap();
    assert_eq!(extras::submodules(&app).unwrap()[0].state, extras::SubmoduleState::NotInitialized);

    // worktrees
    let wt_path = root.join("app-wt");
    extras::worktree_add(&app, &wt_path, "wt-branch", true).unwrap();
    let wts = extras::worktrees(&app).unwrap();
    assert_eq!(wts.len(), 2);
    assert!(wts[0].main);
    assert_eq!(wts[1].branch.as_deref(), Some("wt-branch"));
    extras::worktree_remove(&app, &wt_path, false).unwrap();
    assert_eq!(extras::worktrees(&app).unwrap().len(), 1);

    // patches: export a commit, undo it, apply it back
    write(&app, "app.txt", "app 2\n");
    let h = commit_all(&app, "cambio exportable");
    let patch_dir = root.join("patches");
    let files = extras::create_patches(&app, &[h.clone()], &patch_dir).unwrap();
    assert_eq!(files.len(), 1);
    g::reset(&app, "HEAD~1", ResetMode::Hard).unwrap();
    let r = extras::apply_patch_file(&app, &files[0], false).unwrap();
    assert!(!r.conflicts);
    assert_eq!(g::log(&app, 1, false).unwrap()[0].subject, "cambio exportable");
    let plain = root.join("plain.diff");
    extras::save_diff_patch(&app, "HEAD~1", "HEAD", &plain).unwrap();
    g::reset(&app, "HEAD~1", ResetMode::Hard).unwrap();
    extras::apply_patch_file(&app, &plain, false).unwrap();
    assert_eq!(read(&app, "app.txt"), "app 2\n");
    assert_eq!(g::status(&app).unwrap().staged.len(), 1, "git apply --index stages the change");

    // archive
    let zip = root.join("app.zip");
    extras::archive(&app, "HEAD", &zip).unwrap();
    assert!(fs::metadata(&zip).unwrap().len() > 0);
}

#[test]
fn commit_template_and_custom_actions() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "template");
    write(&repo, ".gitmessage", "# comentario\nTipo: \n\nDescripción\n");
    sh(&repo, &["config", "commit.template", ".gitmessage"]);
    assert_eq!(extras::commit_template(&repo).as_deref(), Some("Tipo: \n\nDescripción"));
    assert_eq!(extras::split_args(r#"-a "dos palabras" $SHA"#), ["-a", "dos palabras", "$SHA"]);
    let ctx = extras::ActionContext { repo: repo.clone(), sha: Some("abc123".into()), ..Default::default() };
    let out = extras::run_custom_action("git", "rev-parse --verify --quiet $SHA^{commit}", &ctx);
    assert!(out.is_err(), "unknown sha fails and reports the error");
    let out = extras::run_custom_action("git", "--version", &ctx).unwrap();
    assert!(out.starts_with("git version"));
}

#[test]
fn explorer_layout_on_real_history() {
    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "explorer");
    write(&repo, "a", "1");
    commit_all(&repo, "a");
    g::create_branch(&repo, "feature/x", None, true).unwrap();
    write(&repo, "x", "1");
    commit_all(&repo, "x1");
    g::checkout(&repo, "main").unwrap();
    write(&repo, "b", "1");
    commit_all(&repo, "b");
    g::merge(&repo, "feature/x", true).unwrap();
    g::delete_branch(&repo, "feature/x", false).unwrap();
    write(&repo, "wip", "1");
    let log = g::log(&repo, 100, true).unwrap();
    let refs = g::refs(&repo).unwrap();
    let l = layout(&log, &refs, true, ColumnMode::Compact);
    let main = &l.lanes[l.nodes[0].lane];
    assert!(main.is_main && main.is_current);
    let inferred = l.lanes.iter().find(|x| x.kind == LaneKind::Inferred).expect("deleted branch lane");
    assert_eq!(inferred.name, "feature/x");
    assert_eq!(l.worktree, Some(log.len()));
}

#[test]
fn hosting_urls_and_credential_hosts() {
    use codesprout_core::credentials::{is_ssh_url, url_host, Provider};
    use codesprout_core::hosting::RemoteRef;
    assert_eq!(url_host("https://user@github.com/a/b.git").as_deref(), Some("github.com"));
    assert_eq!(url_host("git@gitlab.com:grupo/sub/proyecto.git").as_deref(), Some("gitlab.com"));
    assert_eq!(url_host("ssh://git@bitbucket.org:22/a/b.git").as_deref(), Some("bitbucket.org"));
    assert!(is_ssh_url("git@github.com:a/b.git"));
    assert!(!is_ssh_url("https://github.com/a/b.git"));
    let r = RemoteRef::parse("git@github.com:alonso/codesprout.git").unwrap();
    assert_eq!((r.provider, r.path.as_str()), (Provider::GitHub, "alonso/codesprout"));
    assert_eq!(r.new_pr_url("feature/x", "main"), "https://github.com/alonso/codesprout/compare/main...feature%2Fx?expand=1");
    let r = RemoteRef::parse("https://gitlab.com/grupo/sub/proyecto.git").unwrap();
    assert_eq!((r.provider, r.path.as_str()), (Provider::GitLab, "grupo/sub/proyecto"));
    let r = RemoteRef::parse("https://dev.azure.com/org/proj/_git/repo").unwrap();
    assert_eq!(r.web_url(), "https://dev.azure.com/org/proj/_git/repo");
    assert!(RemoteRef::parse("https://example.com/x.git").is_none());
}

#[test]
fn branch_names_are_cleaned_while_typing_and_accepted_by_git() {
    // while typing: spaces, accents and symbols; a trailing `/` or `_` stays so you can keep typing
    assert_eq!(g::branch_name_input("Añadir canción nueva"), "Anadir_cancion_nueva");
    assert_eq!(g::branch_name_input("feature/"), "feature/");
    assert_eq!(g::branch_name_input("arreglo  del   login "), "arreglo_del_login_");
    assert_eq!(g::branch_name_input(r"¿qué? ¡ya! @{x} ~^:*[\]"), "que_ya_x_");
    assert_eq!(g::branch_name_input("a//b..c"), "a/b.c");

    let tmp = tmpdir();
    let repo = new_repo(tmp.path(), "names");
    for (typed, expected) in [
        ("Corrección de errores ", "Correccion_de_errores"),
        ("feature/Pantalla de login", "feature/Pantalla_de_login"),
        ("hotfix/2.0.2", "hotfix/2.0.2"),
        ("-rara-", "rara"),
        ("/.oculta/x.lock/", "oculta/x"),
        ("feature//_ Ñandú .", "feature/Nandu"),
    ] {
        let name = g::branch_name(typed);
        assert_eq!(name, expected, "{typed:?}");
        g::check_ref_name(&repo, true, &name).unwrap_or_else(|e| panic!("{name:?}: {e}"));
    }
    assert_eq!(g::branch_name(" ¿? "), "");
}
