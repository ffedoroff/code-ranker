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
        resolve_plugin(Some("rust"), Some("javascript"), d.path(), None).unwrap(),
        "rust",
        "explicit --plugin wins"
    );
    assert_eq!(
        resolve_plugin(None, Some("rust"), d.path(), None).unwrap(),
        "rust",
        "config wins over auto-detect"
    );
    assert_eq!(
        resolve_plugin(Some("auto"), None, d.path(), None).unwrap(),
        "python",
        "explicit auto -> detect"
    );
    assert_eq!(
        resolve_plugin(None, None, d.path(), None).unwrap(),
        "python",
        "no plugin -> detect"
    );
}

#[test]
fn levels_returns_the_spec_for_a_known_plugin_and_empty_for_unknown() {
    // A real plugin publishes its `files` level (no analysis); an unknown name is
    // an empty list, not a panic.
    assert!(
        levels("rust").iter().any(|l| l.name == "files"),
        "rust publishes a files level"
    );
    assert!(levels("nope").is_empty(), "unknown plugin → no levels");
}

#[test]
fn unknown_plugin_accessors_degrade_gracefully() {
    // Every registry accessor takes a plugin *name*; an unknown one must return the
    // documented empty/zero fallback (the `None` arm) rather than panic.
    let tmp = tempfile::tempdir().unwrap();
    let input = PluginInput::default();
    let mut graph = Graph::default();

    assert!(
        analyze("nope", tmp.path(), &input).is_err(),
        "analyze with an unknown plugin errors"
    );
    assert_eq!(
        annotate_metrics("nope", &mut graph),
        0,
        "no metrics annotated for an unknown plugin"
    );
    assert!(
        function_units("nope", &graph).is_empty(),
        "no function units for an unknown plugin"
    );
    assert!(
        principles("nope", &input).is_empty(),
        "no principles for an unknown plugin"
    );
    // `metric_specs` returns the defaults verbatim for an unknown plugin.
    let defaults: BTreeMap<String, AttributeSpec> = [(
        "sloc".to_string(),
        AttributeSpec::new(code_ranker_plugin_api::attrs::ValueType::Int, "Source"),
    )]
    .into_iter()
    .collect();
    let out = metric_specs("nope", defaults.clone());
    assert!(
        out.contains_key("sloc"),
        "defaults passed through unchanged"
    );
    assert_eq!(out.len(), defaults.len(), "no plugin refinement applied");
}

#[test]
fn resolve_plugin_failure_points_at_config() {
    // No marker resolves here, so the error guides the user to pin the language —
    // into the discovered config when one exists, else by creating `code-ranker.toml`.
    let d = tempfile::tempdir().unwrap();
    let with_cfg =
        resolve_plugin(None, None, d.path(), Some("/proj/code-ranker.toml")).unwrap_err();
    assert!(
        format!("{with_cfg:#}").contains("add `plugin = \"<name>\"` to /proj/code-ranker.toml"),
        "suggests editing the existing config: {with_cfg:#}"
    );
    let no_cfg = resolve_plugin(None, None, d.path(), None).unwrap_err();
    assert!(
        format!("{no_cfg:#}").contains("create a `code-ranker.toml`"),
        "suggests creating a config: {no_cfg:#}"
    );
}
