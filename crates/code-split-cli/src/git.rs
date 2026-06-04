use code_split_graph::snapshot::GitInfo;
use std::path::Path;
use std::process::Command;

/// CLI-supplied overrides for the snapshot's git metadata. Each `Some` field
/// replaces the value `git` would report. They exist because CI environments
/// mangle the raw git view — a detached checkout reports the branch as `HEAD`,
/// and the untracked files a job creates before analysis inflate the dirty
/// count — so the caller can inject clean values (mapped from CI variables).
#[derive(Default)]
pub struct GitOverride {
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub dirty_files: Option<u32>,
    pub origin: Option<String>,
}

impl GitOverride {
    /// True when every field that `git` is otherwise required to produce
    /// (branch, commit, dirty count) is overridden. `origin` is optional and
    /// never gates this — it is `Option` in the snapshot regardless.
    fn covers_all_required(&self) -> bool {
        self.branch.is_some() && self.commit.is_some() && self.dirty_files.is_some()
    }
}

/// Build the snapshot's git metadata, letting any [`GitOverride`] field win over
/// the value read from `git`. When the override covers all required fields, `git`
/// is **never invoked** — the CI fast path, which also works outside a repo.
/// Otherwise each missing field falls back to `git`; if the repo yields no
/// branch/commit (not a git checkout) and they were not overridden, returns `None`.
pub fn collect(workspace: &Path, ov: &GitOverride) -> Option<GitInfo> {
    if ov.covers_all_required() {
        return Some(GitInfo {
            branch: ov.branch.clone().unwrap(),
            commit: ov.commit.clone().unwrap(),
            dirty_files: ov.dirty_files.unwrap(),
            origin: ov.origin.clone(),
        });
    }

    let branch = match ov.branch.clone() {
        Some(b) => b,
        None => run_git(workspace, &["rev-parse", "--abbrev-ref", "HEAD"])?,
    };
    let commit = match ov.commit.clone() {
        Some(c) => c,
        None => run_git(workspace, &["rev-parse", "--short=12", "HEAD"])?,
    };
    let dirty_files = ov.dirty_files.unwrap_or_else(|| count_dirty(workspace));
    let origin = ov.origin.clone().or_else(|| {
        run_git(workspace, &["config", "--get", "remote.origin.url"]).filter(|s| !s.is_empty())
    });
    Some(GitInfo {
        branch,
        commit,
        dirty_files,
        origin,
    })
}

fn run_git(workspace: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn count_dirty(workspace: &Path) -> u32 {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workspace)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count() as u32,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full override must produce the snapshot's git info *without* touching
    /// `git` — proven by running against a path that is not a repository.
    #[test]
    fn full_override_skips_git_even_outside_a_repo() {
        let non_repo = std::env::temp_dir();
        let ov = GitOverride {
            branch: Some("master".into()),
            commit: Some("841522b2aa09".into()),
            dirty_files: Some(1),
            origin: Some("https://github.com/dtolnay/anyhow".into()),
        };
        let info = collect(&non_repo, &ov).expect("full override yields git info");
        assert_eq!(info.branch, "master");
        assert_eq!(info.commit, "841522b2aa09");
        assert_eq!(info.dirty_files, 1);
        assert_eq!(info.origin.as_deref(), Some("https://github.com/dtolnay/anyhow"));
    }

    /// origin is optional: a full override of the required fields still skips git
    /// when origin is omitted, leaving origin `None`.
    #[test]
    fn origin_is_optional_and_does_not_gate_the_fast_path() {
        let non_repo = std::env::temp_dir();
        let ov = GitOverride {
            branch: Some("main".into()),
            commit: Some("deadbeef".into()),
            dirty_files: Some(0),
            origin: None,
        };
        let info = collect(&non_repo, &ov).expect("required fields override is enough");
        assert_eq!(info.origin, None);
    }

    /// A partial override outside a repo cannot fall back to git for the missing
    /// required field, so the whole thing is `None` (no usable git info).
    #[test]
    fn partial_override_outside_repo_yields_none() {
        let non_repo = std::env::temp_dir();
        let ov = GitOverride {
            branch: Some("main".into()),
            ..Default::default()
        };
        assert!(collect(&non_repo, &ov).is_none());
    }
}
