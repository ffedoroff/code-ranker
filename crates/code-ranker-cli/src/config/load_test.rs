use super::*;

/// A config-file body prefixed with the required `version` line. Fixtures must not
/// hardcode the number — it comes from the single `CONFIG_VERSION` constant.
fn v(body: &str) -> String {
    format!(
        "version = \"{}\"\n{body}",
        code_ranker_graph::version::CONFIG_VERSION
    )
}

#[test]
fn load_merges_explicit_config_over_builtin_defaults() {
    // A partial `--config` file: it overrides one key and a threshold; every
    // other value must be INHERITED from the embedded `defaults.toml`.
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("ci.toml");
    std::fs::write(
        &cfg,
        v("[ignore]\ntests = false\n[rules.thresholds.file]\nhk = \"1M\"\n"),
    )
    .unwrap();

    let loaded = load(dir.path(), &[cfg.display().to_string()], &[], &[], &[]).unwrap();
    let c = &loaded.config;

    // Overridden by the file:
    assert!(!c.ignore.tests);
    assert_eq!(c.rules.thresholds.file.get("hk"), Some(1_000_000.0));
    // Inherited from the built-in defaults (not in the file):
    assert!(c.ignore.gitignore && c.ignore.hidden);
    assert_eq!(c.rules.cycles.mutual, CycleRule::Max(0));
    assert!(c.output.json.path.is_some());
    // The merged raw table is exposed for `--export-full-config`.
    assert!(loaded.merged.contains_key("output"));
    assert_eq!(
        loaded.source_file.as_deref(),
        Some(cfg.display().to_string()).as_deref()
    );
}

#[test]
fn load_requires_matching_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("code-ranker.toml");
    let run = || load(dir.path(), &[cfg.display().to_string()], &[], &[], &[]);

    let err = || format!("{:#}", run().err().unwrap());

    // Missing `version` → error naming the required value.
    std::fs::write(&cfg, "[ignore]\ntests = false\n").unwrap();
    assert!(err().contains("missing the required `version`"));

    // Older schema → migrate hint.
    std::fs::write(&cfg, "version = \"1.0\"\n").unwrap();
    let m = err();
    assert!(
        m.contains("expects") && m.contains("migrate the config"),
        "{m}"
    );

    // Newer schema (config from a future build) → upgrade hint.
    std::fs::write(&cfg, "version = \"99.0\"\n").unwrap();
    assert!(err().contains("upgrade code-ranker"));

    // Unparseable version → generic both-ways hint (neither side orders).
    std::fs::write(&cfg, "version = \"abc\"\n").unwrap();
    assert!(err().contains("migrate the config, or upgrade"));

    // Matching schema → ok.
    std::fs::write(&cfg, v("[ignore]\ntests = false\n")).unwrap();
    assert!(run().is_ok());
}

#[test]
fn load_layers_multiple_config_files_in_order_last_wins() {
    // Two `--config FILE` layers + an inline override; later wins at each step.
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.toml");
    let over = dir.path().join("over.toml");
    std::fs::write(&base, v("[rules.thresholds.file]\nhk = 100\nsloc = 800\n")).unwrap();
    std::fs::write(&over, "[rules.thresholds.file]\nhk = 5\n").unwrap();

    let loaded = load(
        dir.path(),
        &[
            base.display().to_string(),
            over.display().to_string(),
            "rules.thresholds.file.sloc=1".to_string(), // inline, applied last
        ],
        &[],
        &[],
        &[],
    )
    .unwrap();
    let t = &loaded.config.rules.thresholds.file;
    // `over.toml` overrode `hk`; `base.toml`'s `sloc` then beaten by the inline.
    assert_eq!(t.get("hk"), Some(5.0));
    assert_eq!(t.get("sloc"), Some(1.0));
    // The log label joins the files in apply order.
    assert_eq!(
        loaded.source_file.as_deref(),
        Some(format!("{} ⊕ {}", base.display(), over.display())).as_deref()
    );
}

#[test]
fn load_auto_discovers_code_ranker_toml_in_workspace() {
    // No explicit `--config`: a `code-ranker.toml` in the workspace dir is found
    // by auto-discovery and merged over the built-in defaults.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("code-ranker.toml"),
        v("[rules.thresholds.file]\nhk = 42\n"),
    )
    .unwrap();

    let loaded = load(dir.path(), &[], &[], &[], &[]).unwrap();
    assert_eq!(loaded.config.rules.thresholds.file.get("hk"), Some(42.0));
    let src = loaded.source_file.expect("discovered source file");
    assert!(src.ends_with("code-ranker.toml"), "{src}");
}

#[test]
fn load_auto_discovers_cargo_workspace_metadata() {
    // No `code-ranker.toml`, but `[workspace.metadata.code-ranker]` in a Cargo.toml
    // supplies the config (the `table_from_cargo_toml` fallback).
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        format!(
            "[workspace]\nmembers = []\n[workspace.metadata.code-ranker]\nversion = \"{}\"\n[workspace.metadata.code-ranker.rules.thresholds.file]\nhk = 7\n",
            code_ranker_graph::version::CONFIG_VERSION
        ),
    )
    .unwrap();

    let loaded = load(dir.path(), &[], &[], &[], &[]).unwrap();
    assert_eq!(loaded.config.rules.thresholds.file.get("hk"), Some(7.0));
    let src = loaded.source_file.expect("discovered source file");
    assert!(src.ends_with("#metadata.code-ranker"), "{src}");
}

#[test]
fn load_falls_back_to_builtin_defaults_when_no_config_found() {
    // An empty dir (no code-ranker.toml, no Cargo.toml) → pure built-in defaults.
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(dir.path(), &[], &[], &[], &[]).unwrap();
    assert!(loaded.source_file.is_none());
    assert!(loaded.config.output.json.path.is_some(), "defaults present");
}

#[test]
fn parse_on_off_accepts_on_off_true_false() {
    for (input, expected) in [
        ("on", true),
        ("true", true),
        ("off", false),
        ("false", false),
    ] {
        assert_eq!(parse_on_off(input).unwrap(), expected);
    }
    assert!(parse_on_off("maybe").is_err());
}

#[test]
fn cli_override_sets_cycle_and_threshold() {
    let mut cfg = Config::default();
    apply_cli_overrides(
        &mut cfg,
        &[],
        &["chain=on".into(), "mutual=off".into()],
        &["file.cognitive=25".into(), "file.hk=1000".into()],
    )
    .unwrap();
    assert_eq!(cfg.rules.cycles.chain, CycleRule::Max(0));
    assert_eq!(cfg.rules.cycles.mutual, CycleRule::Off);
    assert_eq!(cfg.rules.thresholds.file.get("cognitive"), Some(25.0));
    assert_eq!(cfg.rules.thresholds.file.get("hk"), Some(1000.0));
}

#[test]
fn inline_overrides_set_each_key() {
    let mut cfg = Config::default();
    apply_inline_overrides(
        &mut cfg,
        &[
            "plugin=rust",
            "ignore.tests=on",
            "ignore.dev_only_crates=true",
            "ignore.paths=a/**, b/**",
            "output.json.path=out.json",
            "output.html.path=out.html",
            "output.json.enabled=off",
            "output.html.enabled=true",
            "rules.cycles.chain=7",
            "rules.thresholds.file.loc=800",
            "rules.thresholds.file.sloc=1200",
        ],
    )
    .unwrap();
    assert_eq!(cfg.plugin.as_deref(), Some("rust"));
    assert!(cfg.ignore.tests && cfg.ignore.dev_only_crates);
    assert_eq!(cfg.ignore.paths, ["a/**", "b/**"]);
    assert_eq!(cfg.output.json.path.as_deref(), Some("out.json"));
    assert_eq!(cfg.output.html.path.as_deref(), Some("out.html"));
    assert_eq!(cfg.output.json.enabled, Some(false));
    assert_eq!(cfg.output.html.enabled, Some(true));
    assert_eq!(cfg.rules.cycles.chain, CycleRule::Max(7));
    assert_eq!(cfg.rules.thresholds.file.get("loc"), Some(800.0));
    assert_eq!(cfg.rules.thresholds.file.get("sloc"), Some(1200.0));
}

#[test]
fn inline_overrides_set_template_and_remaining_ignore_keys() {
    let mut cfg = Config::default();
    apply_inline_overrides(
        &mut cfg,
        &[
            "ignore.test_modules=off", // alias of ignore.tests
            "ignore.gitignore=off",
            "ignore.ignore_files=off",
            "ignore.hidden=off",
            "templates.prompt=my-prompt.md",
            "templates.languages.rust.SRP=docs/srp.md",
            "templates.languages.rust.HK=docs/hk.md",
        ],
    )
    .unwrap();
    assert!(!cfg.ignore.tests);
    assert!(!cfg.ignore.gitignore);
    assert!(!cfg.ignore.ignore_files);
    assert!(!cfg.ignore.hidden);
    assert_eq!(cfg.templates.prompt.as_deref(), Some("my-prompt.md"));
    let rust = cfg.templates.languages.get("rust").unwrap();
    assert_eq!(rust.get("SRP").map(String::as_str), Some("docs/srp.md"));
    assert_eq!(rust.get("HK").map(String::as_str), Some("docs/hk.md"));
}

#[test]
fn inline_template_override_requires_lang_and_id() {
    // `templates.languages.<lang>.<ID>` — a key missing the `.<ID>` segment errors.
    let mut cfg = Config::default();
    let err = apply_inline_overrides(&mut cfg, &["templates.languages.rust=x.md"])
        .unwrap_err()
        .to_string();
    assert!(err.contains("templates.languages.<lang>.<ID>"), "{err}");
}

#[test]
fn inline_overrides_reject_bad_input() {
    let mut cfg = Config::default();
    assert!(apply_inline_overrides(&mut cfg, &["no_equals_sign"]).is_err());
    assert!(apply_inline_overrides(&mut cfg, &["totally.unknown=1"]).is_err());
}

#[test]
fn parse_cycle_rule_variants() {
    assert_eq!(parse_cycle_rule("on").unwrap(), CycleRule::Max(0));
    assert_eq!(parse_cycle_rule("true").unwrap(), CycleRule::Max(0));
    assert_eq!(parse_cycle_rule("off").unwrap(), CycleRule::Off);
    assert_eq!(parse_cycle_rule("false").unwrap(), CycleRule::Off);
    assert_eq!(parse_cycle_rule("7").unwrap(), CycleRule::Max(7));
    assert!(parse_cycle_rule("-1").is_err());
    assert!(parse_cycle_rule("nope").is_err());
}

#[test]
fn parse_threshold_path_shape() {
    assert_eq!(parse_threshold_path("file.loc").unwrap(), ("file", "loc"));
    assert!(parse_threshold_path("loc").is_err());
    assert!(parse_threshold_path("a.b.c").is_err());
}

#[test]
fn set_metric_records_every_key() {
    // `set_metric` only records the limit now — validity is checked later by
    // `validate_thresholds` (which can see the project's custom metrics).
    let mut b = MetricThresholds::default();
    for m in ["hk", "cyclomatic", "sloc", "mi", "bugs", "bogus"] {
        set_metric(&mut b, m, 1.0).unwrap();
        assert_eq!(b.get(m), Some(1.0));
    }
}

#[test]
fn validate_thresholds_accepts_registry_and_custom_keys() {
    use code_ranker_graph::MetricDef;

    // A registry metric is always valid.
    let mut cfg = Config::default();
    cfg.rules.thresholds.file.set("hk".into(), 1.0);
    assert!(validate_thresholds(&cfg).is_ok());

    // An unknown key with no matching custom metric is rejected, named.
    cfg.rules.thresholds.file.set("tsr".into(), 1.0);
    let err = validate_thresholds(&cfg).unwrap_err().to_string();
    assert!(err.contains("tsr"), "names the bad key: {err}");

    // …but once `[metrics.tsr]` exists, the same threshold is accepted.
    cfg.metrics.insert(
        "tsr".into(),
        MetricDef {
            formula_cel: "1.0".into(),
            ..Default::default()
        },
    );
    assert!(validate_thresholds(&cfg).is_ok());
}

#[test]
fn set_threshold_and_cycle_reject_unknowns() {
    let mut cfg = Config::default();
    assert!(set_threshold(&mut cfg, "function", "loc", 1.0).is_err());
    set_threshold(&mut cfg, "file", "hk", 5.0).unwrap();
    assert_eq!(cfg.rules.thresholds.file.get("hk"), Some(5.0));
    assert!(set_cycle(&mut cfg, "weird", CycleRule::Off).is_err());
}

#[test]
fn split_kv_requires_equals() {
    assert_eq!(split_kv("a=b", "x").unwrap(), ("a", "b"));
    assert!(split_kv("noeq", "x").is_err());
}
