use super::*;

#[test]
fn load_merges_explicit_config_over_builtin_defaults() {
    // A partial `--config` file: it overrides one key and a threshold; every
    // other value must be INHERITED from the embedded `defaults.toml`.
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("ci.toml");
    std::fs::write(
        &cfg,
        "[ignore]\ntests = false\n[rules.thresholds.file]\nhk = \"1M\"\n",
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
fn load_layers_multiple_config_files_in_order_last_wins() {
    // Two `--config FILE` layers + an inline override; later wins at each step.
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.toml");
    let over = dir.path().join("over.toml");
    std::fs::write(&base, "[rules.thresholds.file]\nhk = 100\nsloc = 800\n").unwrap();
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
            formula: "1.0".into(),
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
