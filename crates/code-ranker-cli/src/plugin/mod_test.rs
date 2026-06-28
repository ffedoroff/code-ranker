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
        "c", "cpp", "csharp", "go", "js", "md", "python", "rust", "ts",
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

/// `detect_all` returns a sorted list of matching plugins (multiple is NORMAL).
#[test]
fn detect_all_returns_sorted_multi_set() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("pyproject.toml"), "").unwrap();
    let eff_cfgs: BTreeMap<String, toml::Table> = registry()
        .iter()
        .map(|p| {
            (
                p.name().to_string(),
                effective_plugin_config(p.name(), &BTreeMap::new()),
            )
        })
        .collect();
    let detected = detect_all(&eff_cfgs, d.path(), &PluginInput::default());
    assert!(
        detected.contains(&"python".to_string()),
        "python detected by pyproject.toml"
    );
    // Result is sorted.
    let mut sorted = detected.clone();
    sorted.sort_unstable();
    assert_eq!(detected, sorted, "detect_all output is sorted");
}

/// `detect_all` returns an empty list when there are no markers (no error —
/// error is `resolve_plugins`'s job).
#[test]
fn detect_all_returns_empty_on_no_markers() {
    let d = tempfile::tempdir().unwrap();
    let eff_cfgs: BTreeMap<String, toml::Table> = registry()
        .iter()
        .map(|p| {
            (
                p.name().to_string(),
                effective_plugin_config(p.name(), &BTreeMap::new()),
            )
        })
        .collect();
    let detected = detect_all(&eff_cfgs, d.path(), &PluginInput::default());
    assert!(
        detected.is_empty(),
        "empty directory should produce no detections, got: {detected:?}"
    );
}

/// `resolve_plugins` precedence: console (`arg`) > config (`cfg_plugins`) > auto-detect.
#[test]
fn resolve_plugins_precedence() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("pyproject.toml"), "").unwrap();

    let eff_cfgs: BTreeMap<String, toml::Table> = registry()
        .iter()
        .map(|p| {
            (
                p.name().to_string(),
                effective_plugin_config(p.name(), &BTreeMap::new()),
            )
        })
        .collect();
    let input = PluginInput::default();

    // Console --plugins wins over config plugins.
    let result = resolve_plugins(
        &["rust".to_string()],
        &["javascript".to_string()],
        &eff_cfgs,
        d.path(),
        &input,
        None,
    )
    .unwrap();
    assert_eq!(result, vec!["rust"], "console --plugins wins");

    // Config plugins win over auto-detect.
    let result = resolve_plugins(
        &[],
        &["rust".to_string()],
        &eff_cfgs,
        d.path(),
        &input,
        None,
    )
    .unwrap();
    assert_eq!(result, vec!["rust"], "config plugins win over auto-detect");

    // Auto-detect runs when neither console nor config provides a list.
    let result = resolve_plugins(&[], &[], &eff_cfgs, d.path(), &input, None).unwrap();
    assert!(
        result.contains(&"python".to_string()),
        "auto-detect picks up pyproject.toml"
    );
}

/// Language aliases resolve to the canonical name everywhere a language is named;
/// an unknown token is left untouched (a downstream lookup reports it).
#[test]
fn aliases_resolve_to_canonical() {
    assert_eq!(to_canonical("javascript"), "js");
    assert_eq!(to_canonical("ts"), "ts", "canonical name is idempotent");
    assert_eq!(to_canonical("py"), "python");
    assert_eq!(to_canonical("rs"), "rust");
    assert_eq!(to_canonical("c#"), "csharp");
    assert_eq!(to_canonical("rust"), "rust", "canonical name is idempotent");
    assert_eq!(to_canonical("nope"), "nope", "unknown token is left as-is");

    // `--plugins js,py` resolves to canonical names (→ canonical snapshot keys).
    let d = tempfile::tempdir().unwrap();
    let result = resolve_plugins(
        &["js".to_string(), "py".to_string()],
        &[],
        &BTreeMap::new(),
        d.path(),
        &PluginInput::default(),
        None,
    )
    .unwrap();
    assert_eq!(result, vec!["js", "python"]);
}

/// `resolve_plugins` errors on zero detection (with a config hint).
#[test]
fn resolve_plugins_errors_on_zero_detection() {
    let d = tempfile::tempdir().unwrap();
    let eff_cfgs: BTreeMap<String, toml::Table> = registry()
        .iter()
        .map(|p| {
            (
                p.name().to_string(),
                effective_plugin_config(p.name(), &BTreeMap::new()),
            )
        })
        .collect();

    let with_cfg = resolve_plugins(
        &[],
        &[],
        &eff_cfgs,
        d.path(),
        &PluginInput::default(),
        Some("/proj/code-ranker.toml"),
    )
    .unwrap_err();
    let msg = format!("{with_cfg:#}");
    assert!(
        msg.contains("plugins = ["),
        "zero-detect error should mention plugins = [...]: {msg}"
    );
    assert!(
        msg.contains("/proj/code-ranker.toml"),
        "error should reference the config file: {msg}"
    );

    let no_cfg =
        resolve_plugins(&[], &[], &eff_cfgs, d.path(), &PluginInput::default(), None).unwrap_err();
    assert!(
        format!("{no_cfg:#}").contains("code-ranker.toml"),
        "no-config case suggests creating a config: {no_cfg:#}"
    );
}

#[test]
fn levels_returns_the_spec_for_a_known_plugin_and_empty_for_unknown() {
    // A real plugin publishes its `files` level (no analysis); an unknown name is
    // an empty list, not a panic. `levels` reads the plugin's effective config, so
    // pass rust's real base config (an empty table would miss required vocab).
    let cfg = effective_plugin_config("rust", &BTreeMap::new());
    assert!(
        levels("rust", &cfg).iter().any(|l| l.name == "files"),
        "rust publishes a files level"
    );
    assert!(
        levels("nope", &cfg).is_empty(),
        "unknown plugin → no levels"
    );
}

#[test]
fn unknown_plugin_accessors_degrade_gracefully() {
    // Every registry accessor takes a plugin *name*; an unknown one must return the
    // documented empty/zero fallback (the `None` arm) rather than panic.
    let tmp = tempfile::tempdir().unwrap();
    let input = PluginInput::default();
    let mut graph = Graph::default();
    let cfg = toml::Table::new();

    assert!(
        analyze("nope", &cfg, tmp.path(), &input).is_err(),
        "analyze with an unknown plugin errors"
    );
    assert_eq!(
        annotate_metrics("nope", &cfg, &mut graph),
        0,
        "no metrics annotated for an unknown plugin"
    );
    assert!(
        function_units("nope", &cfg, &graph).is_empty(),
        "no function units for an unknown plugin"
    );
    assert!(
        principles("nope", &cfg, &input).is_empty(),
        "no principles for an unknown plugin"
    );
    // `metric_specs` returns the defaults verbatim for an unknown plugin.
    let defaults: BTreeMap<String, AttributeSpec> = [(
        "sloc".to_string(),
        AttributeSpec::new(code_ranker_plugin_api::attrs::ValueType::Int, "Source"),
    )]
    .into_iter()
    .collect();
    let out = metric_specs("nope", &cfg, defaults.clone());
    assert!(
        out.contains_key("sloc"),
        "defaults passed through unchanged"
    );
    assert_eq!(out.len(), defaults.len(), "no plugin refinement applied");
}

/// `effective_plugin_config` merges [languages.base] then [languages.<name>].
#[test]
fn effective_plugin_config_merges_base_then_lang() {
    let mut overrides: BTreeMap<String, toml::Table> = BTreeMap::new();

    // A base layer that all languages inherit.
    let mut base_table = toml::Table::new();
    base_table.insert(
        "skip_dirs".to_string(),
        toml::Value::Array(vec![toml::Value::String("vendor".to_string())]),
    );
    overrides.insert("base".to_string(), base_table);

    // A rust-specific layer that overrides one key.
    let mut rust_table = toml::Table::new();
    rust_table.insert(
        "skip_dirs".to_string(),
        toml::Value::Array(vec![toml::Value::String("target".to_string())]),
    );
    overrides.insert("rust".to_string(), rust_table);

    let eff = effective_plugin_config("rust", &overrides);
    // [languages.rust] completely replaced skip_dirs from [languages.base].
    let dirs = eff.get("skip_dirs").and_then(|v| v.as_array()).unwrap();
    assert!(
        dirs.iter().any(|v| v.as_str() == Some("target")),
        "rust layer should have 'target'"
    );

    // Python only inherits the base.
    let python_eff = effective_plugin_config("python", &overrides);
    let dirs = python_eff
        .get("skip_dirs")
        .and_then(|v| v.as_array())
        .unwrap();
    assert!(
        dirs.iter().any(|v| v.as_str() == Some("vendor")),
        "python should inherit base 'vendor'"
    );
}

/// `validate_extension_uniqueness` passes when all extensions are disjoint.
#[test]
fn validate_extension_uniqueness_passes_when_disjoint() {
    let mut eff_cfgs: BTreeMap<String, toml::Table> = BTreeMap::new();
    let mut rs = toml::Table::new();
    rs.insert(
        "extensions".to_string(),
        toml::Value::Array(vec![toml::Value::String("rs".to_string())]),
    );
    eff_cfgs.insert("rust".to_string(), rs);
    let mut py = toml::Table::new();
    py.insert(
        "extensions".to_string(),
        toml::Value::Array(vec![toml::Value::String("py".to_string())]),
    );
    eff_cfgs.insert("python".to_string(), py);
    let active = vec!["rust".to_string(), "python".to_string()];
    assert!(validate_extension_uniqueness(&active, &eff_cfgs).is_ok());
}

/// `validate_extension_uniqueness` errors when two plugins share an extension.
#[test]
fn validate_extension_uniqueness_errors_on_conflict() {
    let mut eff_cfgs: BTreeMap<String, toml::Table> = BTreeMap::new();
    let mut p1 = toml::Table::new();
    p1.insert(
        "extensions".to_string(),
        toml::Value::Array(vec![toml::Value::String("ts".to_string())]),
    );
    eff_cfgs.insert("typescript".to_string(), p1);
    let mut p2 = toml::Table::new();
    p2.insert(
        "extensions".to_string(),
        toml::Value::Array(vec![toml::Value::String("ts".to_string())]),
    );
    eff_cfgs.insert("javascript".to_string(), p2);
    let active = vec!["typescript".to_string(), "javascript".to_string()];
    let err = validate_extension_uniqueness(&active, &eff_cfgs).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("extension conflict") || msg.contains(".ts"),
        "should mention the conflicting extension: {msg}"
    );
}

/// `"base"` is NOT a registered plugin name — it is a reserved virtual key.
#[test]
fn base_is_not_a_registered_plugin() {
    let found = registry().iter().any(|p| p.name() == "base");
    assert!(
        !found,
        "'base' must not be a real plugin name — it is a reserved virtual key for [languages.base]"
    );
}
