use super::*;

/// The headline per-language feature: `[plugins.<lang>]` overrides `[plugins.base]`
/// for that language, while a language without its own block inherits the base.
#[test]
fn language_config_overrides_base_per_language() {
    let cfg: Config = toml::from_str(
        "[plugins.base.rules.thresholds.file]\n\
         hk = 350000\n\
         [plugins.base.ignore]\n\
         tests = true\n\
         [plugins.rust.rules.thresholds.file]\n\
         hk = 400000\n\
         [plugins.rust.ignore]\n\
         tests = false\n",
    )
    .unwrap();

    // rust: its own block wins over the base.
    let rust = cfg.language_config("rust").unwrap();
    assert_eq!(rust.rules.thresholds.file.get("hk"), Some(400000.0));
    assert!(!rust.ignore.tests, "rust overrides ignore.tests = false");

    // markdown: no block of its own → inherits the base.
    let md = cfg.language_config("markdown").unwrap();
    assert_eq!(md.rules.thresholds.file.get("hk"), Some(350000.0));
    assert!(
        md.ignore.tests,
        "markdown inherits base ignore.tests = true"
    );
}

#[test]
fn cycle_rules_effective_default_is_strict() {
    // The trivial `CycleRules::default()` is a serde filler (`Off`/`Off`); the
    // EFFECTIVE default lives in `defaults.toml` and is strict. `budget_for`
    // maps each kind to its budget.
    let trivial = CycleRules::default();
    assert!(trivial.mutual.is_off() && trivial.chain.is_off());

    let d = Config::default().language_config("base").unwrap();
    assert_eq!(d.rules.cycles.mutual, CycleRule::Max(0));
    assert_eq!(d.rules.cycles.chain, CycleRule::Max(0));
    assert_eq!(d.rules.cycles.budget_for("mutual"), Some(0));
    assert_eq!(d.rules.cycles.budget_for("chain"), Some(0));
    assert_eq!(d.rules.cycles.budget_for("unknown"), None);
}

#[test]
fn builtin_defaults_complete() {
    // The embedded `defaults.toml` is the single source of every default and
    // MUST be complete: each section present, so deserializing it never falls
    // back to a section's `Default` (which re-enters the `BUILTIN` LazyLock).
    // Spot-check the values the rest of the code relies on as "the defaults".
    let d = Config::default();
    let lc = d.language_config("base").unwrap();
    assert!(lc.ignore.tests && lc.ignore.gitignore && lc.ignore.ignore_files && lc.ignore.hidden);
    assert!(!lc.ignore.dev_only_crates && lc.ignore.paths.is_empty());
    assert_eq!(lc.rules.cycles.mutual, CycleRule::Max(0));
    assert_eq!(lc.rules.cycles.chain, CycleRule::Max(0));
    assert!(lc.rules.thresholds.file.limits.is_empty());
    assert!(!lc.levels.functions);
    // Every output format has a default path; json/html are on, sarif/cq off.
    assert!(d.output.json.path.is_some() && d.output.json.enabled == Some(true));
    assert!(d.output.html.path.is_some() && d.output.html.enabled == Some(true));
    assert!(d.output.sarif.path.is_some() && d.output.sarif.enabled == Some(false));
    assert!(d.output.codequality.path.is_some() && d.output.codequality.enabled == Some(false));
    assert!(d.output.prompt.path.is_some() && d.output.scorecard.path.as_deref() == Some("stdout"));
    // No project plugins pinned by default (→ auto-detect all markers).
    assert!(d.plugins.enabled.is_empty());
    // The built-in defaults live in the raw [plugins.base] block: `language_config`
    // is the right way to read them (tested above). There is no user-added language
    // key beyond "base" in the pristine defaults.
    assert!(!d.plugins.languages.contains_key("rust"));
}

#[test]
fn parse_number_handles_separators_and_suffixes() {
    for (input, want) in [
        ("5_123_000", 5_123_000.0),
        ("5K", 5_000.0),
        ("1.5M", 1_500_000.0),
    ] {
        assert_eq!(parse_number(input).unwrap(), want);
    }
    for bad in ["", "K", "5X"] {
        assert!(parse_number(bad).is_err());
    }
}

#[test]
fn config_toml_parses_cycles_and_thresholds() {
    let src = "
[plugins.base.rules.cycles]
mutual = true
chain = 7
[plugins.base.rules.thresholds.file]
loc = 800
sloc = 1_200
cyclomatic = 25
mi = \"5K\"
";
    let cfg: Config = toml::from_str(src).unwrap();
    let lc = cfg.language_config("base").unwrap();
    assert_eq!(lc.rules.cycles.mutual, CycleRule::Max(0));
    assert_eq!(lc.rules.cycles.chain, CycleRule::Max(7));
    assert_eq!(lc.rules.thresholds.file.get("loc"), Some(800.0));
    // `sloc` (and every other engine metric) is now accepted, not just `loc`.
    assert_eq!(lc.rules.thresholds.file.get("sloc"), Some(1_200.0));
    assert_eq!(lc.rules.thresholds.file.get("cyclomatic"), Some(25.0));
    assert_eq!(lc.rules.thresholds.file.get("mi"), Some(5_000.0));
}

#[test]
fn bare_suffixed_threshold_values_parse() {
    // TOML rejects a bare `300K`; the pre-pass quotes it (only inside a
    // thresholds table) so the config parses without the user adding quotes.
    let src = "
[plugins.base.rules.cycles]
mutual = true
[plugins.base.rules.thresholds.file]
hk = 300K
cyclomatic = 200      # plain int stays native
sloc = 1.5M           # fractional + suffix
";
    let cfg: Config = toml::from_str(&quote_suffixed_thresholds(src)).unwrap();
    let lc = cfg.language_config("base").unwrap();
    assert_eq!(lc.rules.thresholds.file.get("hk"), Some(300_000.0));
    assert_eq!(lc.rules.thresholds.file.get("cyclomatic"), Some(200.0));
    assert_eq!(lc.rules.thresholds.file.get("sloc"), Some(1_500_000.0));
}

#[test]
fn suffix_quoting_is_scoped_to_thresholds_tables() {
    // A bare-suffixed value outside a thresholds table is NOT touched (it would
    // still be invalid TOML there — we only help where suffixes are meaningful).
    let outside = quote_suffixed_thresholds("[other]\nx = 300K\n");
    assert!(outside.contains("x = 300K"), "untouched outside: {outside}");
    let inside = quote_suffixed_thresholds("[plugins.base.rules.thresholds.file]\nhk = 300K\n");
    assert!(inside.contains("hk = \"300K\""), "quoted inside: {inside}");
    // Already-quoted and plain values are left as-is.
    let q =
        quote_suffixed_thresholds("[plugins.base.rules.thresholds.file]\na = \"5M\"\nb = 200\n");
    assert!(q.contains("a = \"5M\"") && q.contains("b = 200"), "{q}");
}

#[test]
fn threshold_value_accepts_int_and_float() {
    // Exercises the per-value deserializer over both TOML scalar forms: an
    // integer (`visit_i64`) and a bare float (`visit_f64`).
    let cfg: Config =
        toml::from_str("[plugins.base.rules.thresholds.file]\ncyclomatic = 30\nmi = 12.5\n")
            .unwrap();
    let lc = cfg.language_config("base").unwrap();
    assert_eq!(lc.rules.thresholds.file.get("cyclomatic"), Some(30.0));
    assert_eq!(lc.rules.thresholds.file.get("mi"), Some(12.5));
}

#[test]
fn project_principle_parses_with_id_defaults() {
    // `[plugins.base.principles.TSR]` keys the principle by its table name;
    // `label`/`title` default to the id, so a minimal entry needs only `sort_metric`.
    let cfg = toml::from_str::<Config>(
        "[plugins.base.principles.TSR]\nsort_metric = \"tsr\"\nprompt = \"fix the ratio\"\n",
    )
    .unwrap();
    let lc = cfg.language_config("base").unwrap();
    let def = &lc.principles["TSR"];
    let p = def.to_principle("TSR");
    assert_eq!(p.id, "TSR");
    assert_eq!(p.label, "TSR");
    assert_eq!(p.title, "TSR");
    assert_eq!(p.sort_metric, "tsr");
    assert_eq!(p.prompt, "fix the ratio");
}

#[test]
fn threshold_keys_parse_without_validation() {
    // Deserialization records every key verbatim — a custom `[metrics.<key>]`
    // is invisible here, so validation is deferred to `load` (see
    // `super::load::validate_thresholds`). A mistyped key is caught there.
    let cfg =
        toml::from_str::<Config>("[plugins.base.rules.thresholds.file]\nslocc = 800\n").unwrap();
    let lc = cfg.language_config("base").unwrap();
    assert_eq!(lc.rules.thresholds.file.get("slocc"), Some(800.0));
}

#[test]
fn config_rejects_unknown_keys() {
    // A stale/mistyped key must be a hard error, not silently ignored.
    let top = toml::from_str::<Config>("oops = 1")
        .unwrap_err()
        .to_string();
    assert!(top.contains("unknown field"), "top-level: {top}");

    let nested = toml::from_str::<Config>("[output]\njson-name = \"x\"\n")
        .unwrap_err()
        .to_string();
    assert!(nested.contains("unknown field"), "nested: {nested}");
    assert!(nested.contains("json-name"), "names the bad key: {nested}");
}
