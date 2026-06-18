//! Tests for `walk.rs` (wired via `#[path]` from that source).

use super::*;
use crate::config::IgnoreCfg;
use crate::test_support::write_file;
use std::path::PathBuf;

const ALL_ON: IgnoreCfg = IgnoreCfg {
    gitignore: true,
    ignore_files: true,
    hidden: true,
};

/// Collect `.rs` files, returning workspace-relative `/`-joined paths, sorted.
fn rs_files(root: &std::path::Path, skip_dirs: &[String], ignore: &IgnoreCfg) -> Vec<String> {
    let mut out: Vec<String> = collect(root, skip_dirs, ignore, |p| {
        p.extension().and_then(|x| x.to_str()) == Some("rs")
    })
    .iter()
    .map(|p: &PathBuf| {
        p.strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    })
    .collect();
    out.sort();
    out
}

#[test]
fn filters_by_extension_and_prunes_skip_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_file(root, "a.rs", "");
    write_file(root, "b.txt", ""); // wrong extension → excluded
    write_file(root, "src/c.rs", "");
    write_file(root, "target/d.rs", ""); // under a skip-dir → pruned

    let skip = vec!["target".to_string()];
    assert_eq!(rs_files(root, &skip, &ALL_ON), vec!["a.rs", "src/c.rs"]);
}

#[test]
fn hidden_flag_toggles_dotfiles() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_file(root, "a.rs", "");
    write_file(root, ".hidden/h.rs", "");

    // hidden = true → the dotted dir is skipped.
    let on = IgnoreCfg {
        hidden: true,
        ..ALL_ON
    };
    assert_eq!(rs_files(root, &[], &on), vec!["a.rs"]);

    // hidden = false → the dotted dir is walked.
    let off = IgnoreCfg {
        hidden: false,
        ..ALL_ON
    };
    assert_eq!(rs_files(root, &[], &off), vec![".hidden/h.rs", "a.rs"]);
}

#[test]
fn gitignore_respected_only_when_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // A bare `.git` dir marks this as a git repo root so git rules apply.
    std::fs::create_dir(root.join(".git")).unwrap();
    write_file(root, ".gitignore", "ignored.rs\n");
    write_file(root, "kept.rs", "");
    write_file(root, "ignored.rs", "");

    // gitignore = true → the ignored file is dropped.
    let on = IgnoreCfg {
        gitignore: true,
        ..ALL_ON
    };
    assert_eq!(rs_files(root, &[], &on), vec!["kept.rs"]);

    // gitignore = false → it is collected.
    let off = IgnoreCfg {
        gitignore: false,
        ..ALL_ON
    };
    assert_eq!(rs_files(root, &[], &off), vec!["ignored.rs", "kept.rs"]);
}

#[test]
fn dot_ignore_file_respected_when_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_file(root, ".ignore", "skip.rs\n");
    write_file(root, "skip.rs", "");
    write_file(root, "keep.rs", "");

    let on = IgnoreCfg {
        ignore_files: true,
        ..ALL_ON
    };
    assert_eq!(rs_files(root, &[], &on), vec!["keep.rs"]);

    let off = IgnoreCfg {
        ignore_files: false,
        ..ALL_ON
    };
    assert_eq!(rs_files(root, &[], &off), vec!["keep.rs", "skip.rs"]);
}
