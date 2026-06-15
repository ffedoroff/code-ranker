//! Guard: every tree-sitter grammar used in the workspace must resolve to
//! EXACTLY ONE version in `Cargo.lock`.
//!
//! The metrics engines and the language plugins must parse each language with
//! the same grammar — if two versions of `tree-sitter-<lang>` (or the core
//! `tree-sitter` runtime) coexist, a file can be parsed two different ways in a
//! single run and the structure graph and the metrics can silently disagree
//! (the class of bug that the per-language version pins used to cause). This
//! test fails the build the moment a second version sneaks in, e.g. via a
//! `=x.y.z` pin in one crate while the workspace moves on, or a new dependency
//! that bundles its own copy.
//!
//! Dependency-free: scans `Cargo.lock` directly (no toml crate needed).

/// Core runtime + the language grammars we own a metric engine / plugin for.
/// A new grammar must be added here when a language is added.
const GUARDED: &[&str] = &[
    "tree-sitter",
    "tree-sitter-rust",
    "tree-sitter-python",
    "tree-sitter-javascript",
    "tree-sitter-typescript",
];

#[test]
fn each_grammar_resolves_to_a_single_version() {
    // tests/ sits in crates/code-ranker-cli; the lockfile is at the workspace root.
    let lock_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
    let lock = std::fs::read_to_string(lock_path)
        .unwrap_or_else(|e| panic!("cannot read {lock_path}: {e}"));

    // Cargo.lock is a sequence of `[[package]]` blocks, each with `name = "..."`
    // then `version = "..."`. Collect the versions seen per guarded name.
    let mut versions: std::collections::BTreeMap<&str, Vec<String>> =
        GUARDED.iter().map(|&n| (n, Vec::new())).collect();

    let mut current_name: Option<&str> = None;
    for line in lock.lines() {
        if let Some(rest) = line.strip_prefix("name = \"") {
            let name = rest.trim_end_matches('"');
            current_name = GUARDED.iter().copied().find(|&g| g == name);
        } else if let Some(rest) = line.strip_prefix("version = \"") {
            if let Some(name) = current_name {
                versions
                    .get_mut(name)
                    .expect("guarded name")
                    .push(rest.trim_end_matches('"').to_string());
            }
            current_name = None;
        }
    }

    let offenders: Vec<String> = versions
        .iter()
        .filter(|(_, vs)| vs.len() > 1)
        .map(|(name, vs)| format!("{name}: {}", vs.join(", ")))
        .collect();

    assert!(
        offenders.is_empty(),
        "multiple versions of a tree-sitter grammar in Cargo.lock — a language \
         would be parsed inconsistently across crates. Unify on one version \
         (drop any `=x.y.z` pin; use `{{ workspace = true }}`). Offenders:\n{}",
        offenders.join("\n")
    );
}
