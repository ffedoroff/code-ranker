use code_split_core::GitInfo;
use std::path::Path;
use std::process::Command;

pub fn collect(workspace: &Path) -> Option<GitInfo> {
    let branch = run_git(workspace, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let commit = run_git(workspace, &["rev-parse", "--short=12", "HEAD"])?;
    let dirty = count_dirty(workspace);
    let origin =
        run_git(workspace, &["config", "--get", "remote.origin.url"]).filter(|s| !s.is_empty());
    Some(GitInfo {
        branch,
        commit,
        dirty_files: dirty,
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
