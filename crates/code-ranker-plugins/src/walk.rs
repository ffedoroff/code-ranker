//! Shared filesystem walk for the directory-walking plugins.
//!
//! Every language whose dependency graph comes from a directory scan (go,
//! python, markdown, csharp, c/cpp, js/ts) collects its source files through
//! [`collect`], so the `.gitignore` / `.ignore` / hidden-file behaviour (the
//! `[ignore]` config section, read into [`IgnoreCfg`]) is honoured uniformly and
//! in one place. The walk LOGIC lives here; *which* extensions / skip-dirs /
//! ignore-flags it uses is data the caller passes in. The Rust plugin resolves
//! files via `cargo metadata` (not a directory walk), so it does not use this.

use crate::config::IgnoreCfg;
use code_ranker_plugin_api::plugin::PluginInput;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

/// Build the file-walk [`IgnoreCfg`] from the orchestrator's [`PluginInput`]
/// (sourced from the CLI's `[ignore]` config). Each directory-walking plugin
/// calls this in `analyze` / `detect` and threads the result into [`collect`].
pub fn ignore_from(input: &PluginInput) -> IgnoreCfg {
    IgnoreCfg {
        gitignore: input.gitignore,
        ignore_files: input.ignore_files,
        hidden: input.hidden,
    }
}

/// Collect every file under `root` for which `keep_file` returns true (typically
/// an extension check), pruning any directory whose name is in `skip_dirs` and
/// honouring `ignore` (gitignore / `.ignore` / hidden files).
///
/// The walk is scoped to `root`: ignore files in directories ABOVE it are never
/// consulted (`parents(false)`), so an enclosing repository's rules never leak
/// into the analysis. Git-related rules apply git-faithfully — only when `root`
/// is within a git repository. Per-caller rules that are NOT general ignore
/// behaviour (test-path exclusion, language-specific skip suffixes) stay in the
/// caller as a post-filter on the returned list.
pub fn collect(
    root: &Path,
    skip_dirs: &[String],
    ignore: &IgnoreCfg,
    keep_file: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let skip: Vec<String> = skip_dirs.to_vec();
    let mut builder = WalkBuilder::new(root);
    builder
        .parents(false)
        .hidden(ignore.hidden)
        .ignore(ignore.ignore_files)
        .git_ignore(ignore.gitignore)
        .git_global(ignore.gitignore)
        .git_exclude(ignore.gitignore)
        .filter_entry(move |entry| {
            // Prune a skip-dir directory (and its whole subtree).
            if entry.file_type().is_some_and(|t| t.is_dir())
                && let Some(name) = entry.file_name().to_str()
            {
                return !skip.iter().any(|d| d == name);
            }
            true
        });
    builder
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(ignore::DirEntry::into_path)
        .filter(|p| keep_file(p))
        .collect()
}

#[cfg(test)]
#[path = "tests/walk.rs"]
mod walk_tests;
