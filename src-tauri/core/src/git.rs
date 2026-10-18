//! Git integration.
//!
//! Everything shells out to the `git` binary rather than linking libgit2: the
//! user's own config, credential helpers, hooks and `core.autocrlf` behaviour
//! are then respected exactly as they would be in a terminal.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::models::Checkpoint;

/// Result of running one git command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOutput {
    pub ok: bool,
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

fn run(cwd: &Path, args: &[&str]) -> Result<GitOutput> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        // Keep the agent's commits and diffs free of the user's identity noise.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| Error::io(format!("could not run git: {e}")))?;

    Ok(GitOutput {
        ok: out.status.success(),
        status: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Runs a git command and treats a non-zero exit as a user-visible error,
/// for callers that need the command to have succeeded.
pub fn run_checked(dir: &Path, args: &[&str]) -> Result<GitOutput> {
    let out = run(dir, args)?;
    if !out.ok {
        return Err(Error::msg(out.stderr.trim().to_string()));
    }
    Ok(out)
}

pub fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists() || run(dir, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|o| o.ok)
}

pub fn head(dir: &Path) -> Result<Option<String>> {
    let out = run(dir, &["rev-parse", "HEAD"])?;
    Ok(out.ok.then(|| out.stdout.trim().to_string()))
}

pub fn branch(dir: &Path) -> Result<Option<String>> {
    let out = run(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    Ok(out.ok.then(|| out.stdout.trim().to_string()))
}

/// Files that differ from HEAD, with their status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusEntry {
    pub path: String,
    pub status: String,
    pub staged: bool,
}

pub fn status(dir: &Path) -> Result<Vec<StatusEntry>> {
    let out = run(dir, &["status", "--porcelain=v1"])?;
    if !out.ok {
        return Err(Error::msg(out.stderr.trim().to_string()));
    }
    Ok(out
        .stdout
        .lines()
        .filter_map(|line| {
            // Porcelain v1 is `XY PATH`: two status characters then a space.
            // The untracked marker is exactly "??", so the flags must be
            // trimmed before comparing rather than sliced blindly.
            let (raw_flags, path) = line.split_at(3);
            let flags = raw_flags.trim_end();
            if flags.len() < 2 {
                return None;
            }
            let mut chars = flags.chars();
            let x = chars.next().unwrap_or(' ');
            let y = chars.next().unwrap_or(' ');
            let untracked = flags == "??";
            Some(StatusEntry {
                path: path.trim().to_string(),
                status: if untracked { "??".into() } else { y.to_string() },
                staged: x != ' ' && x != '?',
            })
        })
        .collect())
}

/// Working-tree diff against HEAD, including untracked files as additions.
pub fn diff(dir: &Path) -> Result<String> {
    let tracked = run(dir, &["diff", "HEAD"])?;
    if !tracked.ok {
        return Err(Error::msg(tracked.stderr.trim().to_string()));
    }
    let mut out = tracked.stdout;

    // `git diff` ignores untracked files, but they are exactly what an agent
    // just created, so they belong in the checkpoint. `--no-index` against
    // /dev/null is the portable way to render one as an addition.
    for entry in status(dir)? {
        if entry.status != "??" || entry.path.is_empty() {
            continue;
        }
        if let Ok(o) = run(dir, &["diff", "--no-index", "--", "/dev/null", &entry.path]) {
            out.push_str(&o.stdout);
        }
    }
    Ok(out)
}

/// Snapshot of the working tree that can be restored later.
///
/// Uncommitted changes are stashed under a generated label so they can be
/// reapplied; if there is nothing to stash the checkpoint records HEAD only.
/// Records the current working tree as a checkpoint and returns it.
pub fn create_checkpoint(
    store: &crate::db::Store,
    session_id: &str,
    message_id: Option<&str>,
    dir: &Path,
    label: &str,
) -> Result<Checkpoint> {
    let head = head(dir)?;
    // `stash create` records uncommitted work without touching the working
    // tree or the stash list, which is what a rewind target needs.
    let stash = run(dir, &["stash", "create"])?;
    let stashed = stash.stdout.trim();

    let cp = Checkpoint {
        id: crate::util::new_id("cp"),
        session_id: session_id.to_string(),
        message_id: message_id.map(str::to_string),
        label: label.to_string(),
        git_head: if stashed.is_empty() { head } else { Some(stashed.to_string()) },
        created_at: crate::models::now_ms(),
    };
    store.insert_checkpoint(&cp)?;
    Ok(cp)
}

/// Restores the workspace to a checkpoint.
///
/// Files the agent created since the checkpoint are removed and the tree is
/// restored to the tree recorded in the checkpoint. That tree is either the
/// commit that was checked out, or the stash commit holding uncommitted work,
/// so rewinding brings back work-in-progress rather than silently dropping it.
///
/// Refuses to touch anything while a merge, rebase or cherry-pick is in flight:
/// a half-applied git state is worse than a clear refusal.
pub fn restore_checkpoint(dir: &Path, cp: &Checkpoint) -> Result<()> {
    if !is_repo(dir) {
        return Err(Error::msg("this project is not a git repository"));
    }

    for marker in ["MERGE_HEAD", "REBASE_HEAD", "CHERRY_PICK_HEAD", "BISECT_LOG"] {
        if dir.join(".git").join(marker).exists() {
            return Err(Error::msg(format!(
                "git is mid-operation ({marker}); finish or abort it first"
            )));
        }
    }

    // The reset target is the checkpoint's tree when it recorded one, so
    // uncommitted work that existed at that moment is restored rather than
    // discarded along with the agent's changes.
    let target = cp.git_head.clone().unwrap_or_else(|| "HEAD".to_string());
    let reset = run(dir, &["reset", "--hard", &target])?;
    if !reset.ok {
        return Err(Error::msg(reset.stderr.trim().to_string()));
    }
    // Remove files the agent created that the checkpoint did not contain.
    let clean = run(dir, &["clean", "-fd"])?;
    if !clean.ok {
        return Err(Error::msg(clean.stderr.trim().to_string()));
    }
    Ok(())
}

/// Creates an isolated checkout so parallel agents cannot collide.
pub fn add_worktree(dir: &Path, path: &Path, branch: &str) -> Result<String> {
    std::fs::create_dir_all(path.parent().unwrap_or(path))
        .map_err(|e| Error::io(e.to_string()))?;
    let out = run(dir, &["worktree", "add", &path.to_string_lossy(), branch])?;
    if out.ok {
        Ok(branch.to_string())
    } else {
        Err(Error::msg(out.stderr.trim().to_string()))
    }
}

pub fn remove_worktree(dir: &Path, path: &Path) -> Result<()> {
    run(dir, &["worktree", "remove", "--force", &path.to_string_lossy()])?;
    Ok(())
}

pub fn list_worktrees(dir: &Path) -> Result<Vec<String>> {
    let out = run(dir, &["worktree", "list", "--porcelain"])?;
    Ok(out
        .stdout
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;

    /// Creates a throwaway repository with one commit, or `None` when git is
    /// unavailable on this machine.
    fn repo() -> Option<tempdir::TempDir> {
        let dir = tempdir::TempDir::new("octopus-git").ok()?;
        let p = dir.path();
        let ok = Command::new("git")
            .args(["init", "-q"])
            .current_dir(p)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return None;
        }
        std::fs::write(p.join("a.txt"), "one\n").ok()?;
        Command::new("git")
            .args(["-c", "user.email=t@e", "-c", "user.name=t", "add", "."])
            .current_dir(p)
            .status()
            .ok()?;
        Command::new("git")
            .args(["-c", "user.email=t@e", "-c", "user.name=t", "commit", "-qm", "init"])
            .current_dir(p)
            .status()
            .ok()?;
        Some(dir)
    }

    #[test]
    fn detects_a_repository_and_its_head() {
        let Some(dir) = repo() else {
            eprintln!("skipping: git unavailable");
            return;
        };
        assert!(is_repo(dir.path()));
        let h = head(dir.path()).unwrap().unwrap();
        assert_eq!(h.len(), 40, "expected a full sha, got {h}");
    }

    #[test]
    fn status_lists_untracked_files() {
        let Some(dir) = repo() else { return };
        std::fs::write(dir.path().join("new.txt"), "x").unwrap();
        let s = status(dir.path()).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].path, "new.txt");
        assert_eq!(s[0].status, "??");
    }

    #[test]
    fn diff_includes_untracked_files() {
        let Some(dir) = repo() else { return };
        std::fs::write(dir.path().join("new.txt"), "x").unwrap();
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        let d = diff(dir.path()).unwrap();
        assert!(d.contains("+two"), "tracked change missing:\n{d}");
        assert!(d.contains("new.txt"), "untracked file missing:\n{d}");
    }

    #[test]
    fn restore_undoes_agent_changes() {
        let Some(dir) = repo() else { return };
        std::fs::write(dir.path().join("a.txt"), "clobbered\n").unwrap();
        std::fs::write(dir.path().join("junk.txt"), "junk").unwrap();

        let cp = Checkpoint {
            id: "cp".into(),
            session_id: "s".into(),
            message_id: None,
            label: "before".into(),
            #[allow(clippy::needless_update)]
            git_head: head(dir.path()).unwrap(),
            created_at: 0,
        };
        restore_checkpoint(dir.path(), &cp).unwrap();

        // `core.autocrlf` rewrites line endings on checkout on Windows, so the
        // comparison normalizes them rather than assuming LF.
        let restored = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(restored.replace("\r\n", "\n"), "one\n");
        assert!(!dir.path().join("junk.txt").exists(), "agent file should be gone");
    }

    #[test]
    fn restore_brings_back_work_in_progress() {
        let Some(dir) = repo() else { return };
        // Uncommitted work that existed at checkpoint time must survive a
        // rewind, otherwise the user's own edits are destroyed along with the
        // agent's.
        std::fs::write(dir.path().join("a.txt"), "mine\n").unwrap();

        // The database must live outside the repository: `git clean -fd` would
        // otherwise try to delete the open database files.
        let db_dir = tempdir::TempDir::new("octopus-git-db").unwrap();
        let store = Store::new(crate::db::open(&db_dir.path().join("cp.db")).unwrap());
        // Checkpoints reference a session, so the foreign key has to be real.
        store
            .upsert_session(&crate::models::Session::new("test".into(), "test".into()))
            .unwrap();
        let session_id = store.list_sessions(None, 1).unwrap()[0].id.clone();

        let cp = create_checkpoint(&store, &session_id, None, dir.path(), "before")
            .expect("checkpoint");

        std::fs::write(dir.path().join("a.txt"), "agent clobber\n").unwrap();
        std::fs::write(dir.path().join("junk.txt"), "junk").unwrap();

        restore_checkpoint(dir.path(), &cp).unwrap();

        let restored = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(restored.replace("\r\n", "\n"), "mine\n");
        assert!(!dir.path().join("junk.txt").exists());
    }

    #[test]
    fn restore_refuses_outside_a_repository() {
        let dir = tempdir::TempDir::new("octopus-nogit").unwrap();
        let cp = Checkpoint {
            id: "cp".into(),
            session_id: "s".into(),
            message_id: None,
            label: "x".into(),
            #[allow(clippy::needless_update)]
            git_head: None,
            created_at: 0,
        };
        assert!(restore_checkpoint(dir.path(), &cp).is_err());
    }
}