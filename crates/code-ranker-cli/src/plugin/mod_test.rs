use super::*;
use std::path::PathBuf;

/// The registry is the single source of truth for which languages exist. Every
/// registered plugin MUST ship the committed e2e goldens — the `report`
/// snapshot (`code-ranker-report.json`), the `check` SARIF
/// (`code-ranker-check.sarif`), and the `check` Code Quality
/// (`code-ranker-check.codequality.json`) — under
/// `crates/code-ranker-plugins/src/languages/<name>/tests/sample/`.
///
/// This guard makes adding a language fail the build until its goldens are
/// committed, instead of the gap going unnoticed because no e2e case names it.
/// The plugin's `name()` maps directly to the language-module directory name.
#[test]
fn every_registered_plugin_has_committed_goldens() {
    // CARGO_MANIFEST_DIR = <repo>/crates/code-ranker-cli → repo root is 2 up.
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root two levels above the crate manifest")
        .to_path_buf();

    for plugin in registry() {
        let name = plugin.name();
        let sample = repo
            .join("crates")
            .join("code-ranker-plugins")
            .join("src")
            .join("languages")
            .join(name)
            .join("tests")
            .join("sample");
        for golden in [
            "code-ranker-report.json",
            "code-ranker-check.sarif",
            "code-ranker-check.codequality.json",
        ] {
            let path = sample.join(golden);
            assert!(
                path.is_file(),
                "plugin `{name}` is registered but its e2e golden `{golden}` is missing at \
                 {} — add a sample fixture and regenerate the goldens (see docs/e2e.md)",
                path.display()
            );
        }
    }
}

/// Self-registration guard: the `inventory` registry must contain EXACTLY this
/// set of plugins — by name, no more and no less. Catches a dropped submission
/// (linker/inventory regression), a missing/renamed plugin, AND an unexpected
/// new one (which must be added here deliberately). Each must also expose a
/// non-empty merged `config()` (what `--export-full-config` dumps).
#[test]
fn registry_holds_exactly_the_expected_plugins() {
    const EXPECTED: &[&str] = &[
        "c",
        "cpp",
        "csharp",
        "go",
        "javascript",
        "markdown",
        "python",
        "rust",
        "typescript",
    ];

    let mut found: Vec<&str> = registry().iter().map(|p| p.name()).collect();
    found.sort_unstable();
    assert_eq!(
        found, EXPECTED,
        "self-registered plugin set drifted from the expected list — update EXPECTED \
         (and ship the language's e2e goldens) if this is an intended add/remove; an \
         empty/short list means an inventory/linker regression dropped submissions"
    );

    for plugin in registry() {
        let name = plugin.name();
        assert!(
            !plugin.config().is_empty(),
            "plugin `{name}` exposes an empty config(); --export-full-config would be blank"
        );
    }
}

#[test]
fn resolve_plugin_precedence_explicit_then_config_then_auto() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("pyproject.toml"), "").unwrap();
    assert_eq!(
        resolve_plugin(Some("rust"), Some("javascript"), d.path()).unwrap(),
        "rust",
        "explicit --plugin wins"
    );
    assert_eq!(
        resolve_plugin(None, Some("rust"), d.path()).unwrap(),
        "rust",
        "config wins over auto-detect"
    );
    assert_eq!(
        resolve_plugin(Some("auto"), None, d.path()).unwrap(),
        "python",
        "explicit auto -> detect"
    );
    assert_eq!(
        resolve_plugin(None, None, d.path()).unwrap(),
        "python",
        "no plugin -> detect"
    );
}
