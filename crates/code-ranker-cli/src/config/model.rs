//! Config data model: the serde-deserialized `Config` tree and threshold
//! number parsing (`_` separators, K/M/G suffixes).

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::LazyLock;

/// The built-in project-config defaults, compiled into the binary — the SINGLE
/// source of every default value (no default is hardcoded in Rust). A discovered
/// project `code-ranker.toml` (or `--config FILE`) is deep-merged on top of this
/// in [`super::load`], so a user overrides only what they spell out and inherits
/// the rest. [`Config::default`] and the per-section `Default` impls all source
/// their values from here.
pub const DEFAULTS: &str = include_str!("defaults.toml");

/// `defaults.toml` parsed once into a [`Config`]. The per-section `Default` impls
/// read their slice of this. Parsing relies on `defaults.toml` being COMPLETE
/// (every section present) so deserialization never falls back to a section's
/// `Default` — which would re-enter this `LazyLock`. The `builtin_defaults_complete`
/// test guards that invariant.
static BUILTIN: LazyLock<Config> =
    LazyLock::new(|| toml::from_str(DEFAULTS).expect("embedded defaults.toml parses into Config"));

// NOTE: `#[serde(default)]` is per FIELD (not on the container). A container-level
// `default` would call `Config::default()` (= `BUILTIN`) while parsing — including
// while `BUILTIN` itself is initializing — re-entering the `LazyLock` and
// deadlocking. Per-field defaults are lazy and use each field's own `Default`
// (`None`/empty for the scalar/map fields; the section structs' defaults are never
// invoked because `defaults.toml` always carries those sections).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Default plugin name (e.g. "rust", "python"). Overridden by --plugin.
    #[serde(default)]
    pub plugin: Option<String>,
    #[serde(default)]
    pub ignore: IgnoreConfig,
    #[serde(default)]
    pub rules: RulesConfig,
    #[serde(default)]
    pub output: OutputConfig,
    /// User-defined declarative metrics (`[metrics.<key>]`): a CEL `formula` plus
    /// optional spec fields. Computed per node at snapshot time and emitted like
    /// any built-in metric. Empty by default — absent → no change to output.
    #[serde(default)]
    pub metrics: BTreeMap<String, code_ranker_graph::MetricDef>,
    /// Optional analysis levels (`[levels]`). Off by default → only the `files`
    /// level is emitted, so default output is unchanged.
    #[serde(default)]
    pub levels: LevelsConfig,
    /// Project-level report-list patches (`[report]`): `columns` / `card` /
    /// `stats`, each a list-override (plain array = replace, or an op-table
    /// `{add,remove,replace,clear,prepend}`). Applied over the language's own
    /// `[report]` patch, so a project can surface its custom metrics in the table
    /// / card / JSON stats. Raw table; parsed by `list_override::report_override_section`.
    #[serde(default)]
    pub report: toml::Table,
    /// Project-defined Prompt-Generator presets (`[presets.<ID>]`), keyed by the
    /// preset id. Appended to the active plugin's catalog (a same-id project preset
    /// overrides the plugin's), so a project can recommend/scorecard on its own
    /// custom metric. Empty by default — absent → no change to output.
    #[serde(default)]
    pub presets: BTreeMap<String, PresetDef>,
}

impl Default for Config {
    /// The built-in defaults — a clone of the embedded `defaults.toml` (the
    /// single source of default values). Used when no project config is found,
    /// and as the merge base every discovered config layers over.
    fn default() -> Self {
        BUILTIN.clone()
    }
}

/// A project-config preset (`[presets.<ID>]`) — the table key is the id. Mirrors
/// the plugin [`Preset`](code_ranker_plugin_api::plugin::Preset) but with sane
/// defaults so a project entry needs only `sort_metric` (+ usually `title`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PresetDef {
    /// Button label; defaults to the id.
    pub label: Option<String>,
    /// Principle title (first heading of the generated prompt); defaults to the id.
    pub title: Option<String>,
    /// Prompt body (Markdown). Defaults to empty.
    pub prompt: String,
    /// Link to a principle doc, if any.
    pub doc_url: Option<String>,
    /// The metric the recommended-node list sorts by (an attribute key, or the
    /// pseudo-metric `"cycle"`). Required in practice — the lens the preset is.
    pub sort_metric: String,
    /// Connection sets the preset pre-selects: any of `"in"` / `"out"` / `"common"`.
    pub connections: Vec<String>,
}

impl PresetDef {
    /// Build the runtime [`Preset`](code_ranker_plugin_api::plugin::Preset) for
    /// this entry, defaulting `label` / `title` to the id.
    pub fn to_preset(&self, id: &str) -> code_ranker_plugin_api::plugin::Preset {
        code_ranker_plugin_api::plugin::Preset {
            id: id.to_string(),
            label: self.label.clone().unwrap_or_else(|| id.to_string()),
            title: self.title.clone().unwrap_or_else(|| id.to_string()),
            prompt: self.prompt.clone(),
            doc_url: self.doc_url.clone(),
            sort_metric: self.sort_metric.clone(),
            connections: self.connections.clone(),
        }
    }
}

/// `[levels]` — opt-in extra graph levels beyond `files`.
#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LevelsConfig {
    /// Emit the per-function (`functions`) level with sub-file metrics.
    pub functions: bool,
}

/// Per-format output config: `[output.json]` / `[output.html]` /
/// `[output.sarif]` / `[output.codequality]`, each with a `path` template and an
/// optional `enabled` flag.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OutputConfig {
    pub json: OutputArtifact,
    pub html: OutputArtifact,
    pub sarif: OutputArtifact,
    pub codequality: OutputArtifact,
    /// `prompt` / `scorecard` are flag-driven (off unless `--output.<fmt>` is
    /// passed); their `path` here only supplies the default destination template.
    pub prompt: OutputArtifact,
    pub scorecard: OutputArtifact,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OutputArtifact {
    pub path: Option<String>,
    pub enabled: Option<bool>,
}

// `Default` is the trivial type-default (empty/false) — a serde filler only,
// NOT the effective default. The real `[ignore]` defaults live in `defaults.toml`
// (the `BUILTIN` config), which every runtime config is merged over. Delegating
// this to `Config::default()` would deadlock the `BUILTIN` LazyLock (see `Config`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IgnoreConfig {
    pub paths: Vec<String>,
    /// Skip the language's test files during analysis. **On by default** —
    /// metrics and cycles describe production code unless you opt back in with
    /// `tests = false`. The plugin decides what counts as a test (see
    /// `LanguagePlugin::is_test_path`).
    #[serde(alias = "test_modules", alias = "test-modules")]
    pub tests: bool,
    /// Strip crates that appear only in [dev-dependencies].
    pub dev_only_crates: bool,
    /// Honour `.gitignore` (+ global gitignore + `.git/info/exclude`) while a
    /// directory-walking plugin collects source files, scoped to the analyzed
    /// root. **On by default.** The Rust plugin uses `cargo metadata`, not a
    /// walk, so it is unaffected.
    pub gitignore: bool,
    /// Honour `.ignore` files during file collection. **On by default.**
    pub ignore_files: bool,
    /// Skip hidden files / directories (dotfiles) during file collection.
    /// **On by default.**
    pub hidden: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct RulesConfig {
    pub cycles: CycleRules,
    pub thresholds: ThresholdRules,
}

/// A cycle check: disabled, or enabled with a maximum allowed count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CycleRule {
    /// Trivial type-default (serde filler only). The effective default —
    /// `Max(0)` (strict) — lives in `defaults.toml`'s `[rules.cycles]`.
    #[default]
    Off,
    Max(u32),
}

impl CycleRule {
    pub fn budget(self) -> Option<u32> {
        match self {
            CycleRule::Off => None,
            CycleRule::Max(n) => Some(n),
        }
    }
    pub fn is_off(self) -> bool {
        matches!(self, CycleRule::Off)
    }
}

impl<'de> Deserialize<'de> for CycleRule {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = CycleRule;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a bool (on/off) or a non-negative integer (max allowed cycles)")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<CycleRule, E> {
                Ok(if v { CycleRule::Max(0) } else { CycleRule::Off })
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<CycleRule, E> {
                u32::try_from(v)
                    .map(CycleRule::Max)
                    .map_err(|_| E::custom("cycle budget must be a non-negative integer"))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<CycleRule, E> {
                Ok(CycleRule::Max(v as u32))
            }
        }
        d.deserialize_any(V)
    }
}

// `Default` is the trivial type-default (`Off`/`Off`) — a serde filler only. The
// effective strict default (`Max(0)`) lives in `defaults.toml`'s `[rules.cycles]`,
// merged into every runtime config; see the `Config` / `IgnoreConfig` notes.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CycleRules {
    pub mutual: CycleRule,
    pub chain: CycleRule,
}

impl CycleRules {
    /// Budget for a cycle kind string (`"mutual"`/`"chain"`):
    /// `Some(max)` if enabled, `None` if disabled.
    pub fn budget_for(self, kind: &str) -> Option<u32> {
        match kind {
            "mutual" => self.mutual,
            "chain" => self.chain,
            _ => CycleRule::Off,
        }
        .budget()
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ThresholdRules {
    pub file: MetricThresholds,
}

/// Per-file metric thresholds, keyed by metric name (`sloc`, `cyclomatic`, `hk`,
/// …). **Any** per-file metric the engine emits is accepted — the registry
/// vocabulary (see [`super::metrics`]) plus the project's own custom
/// `[metrics.<key>]` — so this is an open map, not a fixed set of fields. Keys are
/// validated once in [`super::load`] after the full config is parsed (an unknown
/// key is a config error there). A value is a number with optional `_` separators
/// and a `K`/`M`/`G` suffix. Unset = no limit.
#[derive(Debug, Clone, Default)]
pub struct MetricThresholds {
    pub limits: BTreeMap<String, f64>,
}

impl MetricThresholds {
    /// The limit configured for `metric`, if any. (Test-only: evaluation now
    /// iterates `limits` directly in `violations`.)
    #[cfg(test)]
    pub fn get(&self, metric: &str) -> Option<f64> {
        self.limits.get(metric).copied()
    }
    /// Set (or override) the limit for `metric`.
    pub fn set(&mut self, metric: String, limit: f64) {
        self.limits.insert(metric, limit);
    }
}

impl<'de> Deserialize<'de> for MetricThresholds {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct MapV;
        impl<'de> serde::de::Visitor<'de> for MapV {
            type Value = MetricThresholds;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a table of `metric = limit` entries")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<MetricThresholds, M::Error> {
                // Keys are NOT validated here: a custom `[metrics.<key>]` is a
                // legal threshold target but isn't known at deserialize time
                // (serde can't see the sibling `[metrics]` table). Validation —
                // against the registry vocabulary ∪ the project's custom metrics —
                // happens once in `load`, after the whole config is parsed.
                let mut limits = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    let ThresholdNumber(val) = map.next_value()?;
                    limits.insert(key, val);
                }
                Ok(MetricThresholds { limits })
            }
        }
        d.deserialize_map(MapV)
    }
}

/// A single threshold value: a bare number or a string like `"5K"` / `"1.5M"`.
struct ThresholdNumber(f64);

impl<'de> Deserialize<'de> for ThresholdNumber {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = f64;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number, or a string like \"5K\" / \"1.5M\"")
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<f64, E> {
                Ok(v as f64)
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<f64, E> {
                Ok(v as f64)
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<f64, E> {
                Ok(v)
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<f64, E> {
                parse_number(v).map_err(E::custom)
            }
        }
        d.deserialize_any(V).map(ThresholdNumber)
    }
}

/// Parse a threshold value: a number with optional `_` separators and a
/// `K`/`M`/`G` suffix.
pub(crate) fn parse_number(s: &str) -> Result<f64> {
    let t = s.trim().replace('_', "");
    let (mult, body) = match t.bytes().last() {
        Some(b'k' | b'K') => (1e3, &t[..t.len() - 1]),
        Some(b'm' | b'M') => (1e6, &t[..t.len() - 1]),
        Some(b'g' | b'G') => (1e9, &t[..t.len() - 1]),
        _ => (1.0, t.as_str()),
    };
    let n: f64 = body.parse().with_context(|| {
        format!("invalid number {s:?} (expected e.g. 500000, 5_000_000, 5K, 1.5M)")
    })?;
    Ok(n * mult)
}

/// TOML rejects a bare `300K` (a `K`/`M`/`G` suffix makes it neither a number nor
/// a string), so without help a user must write `hk = "300K"`. This pre-pass lets
/// them write `hk = 300K` by quoting bare suffixed numbers **only inside a
/// `*thresholds*` table**, before the text reaches the TOML parser. Plain and
/// underscored integers stay native; already-quoted values and everything outside
/// a thresholds table are left untouched. The matching CLI form (`--threshold
/// file.hk=300K`) needs no help — it goes straight through [`parse_number`].
pub(crate) fn quote_suffixed_thresholds(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut in_thresholds = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            // Section header (`[t]` or `[[t]]`): a thresholds table enables quoting.
            let name = trimmed.trim_start_matches('[');
            in_thresholds = name
                .split(']')
                .next()
                .is_some_and(|s| s.contains("thresholds"));
        } else if in_thresholds && let Some(quoted) = quote_suffixed_value_line(line) {
            out.push_str(&quoted);
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// If `line` is a `key = <bare-suffixed-number>` assignment, return it with the
/// value quoted (formatting and any trailing comment preserved); else `None`.
fn quote_suffixed_value_line(line: &str) -> Option<String> {
    let eq = line.find('=')?;
    let key = line[..eq].trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let after = &line[eq + 1..];
    let (val_seg, comment) = match after.find('#') {
        Some(h) => after.split_at(h),
        None => (after, ""),
    };
    if !is_bare_suffixed_number(val_seg.trim()) {
        return None;
    }
    let lead: String = val_seg.chars().take_while(|c| c.is_whitespace()).collect();
    let trail: String = val_seg
        .chars()
        .rev()
        .take_while(|c| c.is_whitespace())
        .collect();
    Some(format!(
        "{}={lead}\"{}\"{trail}{comment}",
        &line[..eq],
        val_seg.trim()
    ))
}

/// Does `v` look like a bare `K`/`M`/`G`-suffixed number (`300K`, `1.5M`,
/// `5_000K`)? Already-quoted values and plain numbers return `false`.
fn is_bare_suffixed_number(v: &str) -> bool {
    let Some(last) = v.chars().last() else {
        return false;
    };
    if !matches!(last, 'k' | 'K' | 'm' | 'M' | 'g' | 'G') {
        return false;
    }
    let body = &v[..v.len() - 1];
    let mut seen_digit = false;
    let mut seen_dot = false;
    for c in body.chars() {
        match c {
            '0'..='9' => seen_digit = true,
            '_' => {}
            '.' if !seen_dot => seen_dot = true,
            _ => return false,
        }
    }
    seen_digit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_rules_effective_default_is_strict() {
        // The trivial `CycleRules::default()` is a serde filler (`Off`/`Off`); the
        // EFFECTIVE default lives in `defaults.toml` and is strict. `budget_for`
        // maps each kind to its budget.
        let trivial = CycleRules::default();
        assert!(trivial.mutual.is_off() && trivial.chain.is_off());

        let d = Config::default().rules.cycles;
        assert_eq!(d.mutual, CycleRule::Max(0));
        assert_eq!(d.chain, CycleRule::Max(0));
        assert_eq!(d.budget_for("mutual"), Some(0));
        assert_eq!(d.budget_for("chain"), Some(0));
        assert_eq!(d.budget_for("unknown"), None);
    }

    #[test]
    fn builtin_defaults_complete() {
        // The embedded `defaults.toml` is the single source of every default and
        // MUST be complete: each section present, so deserializing it never falls
        // back to a section's `Default` (which re-enters the `BUILTIN` LazyLock).
        // Spot-check the values the rest of the code relies on as "the defaults".
        let d = Config::default();
        assert!(d.ignore.tests && d.ignore.gitignore && d.ignore.ignore_files && d.ignore.hidden);
        assert!(!d.ignore.dev_only_crates && d.ignore.paths.is_empty());
        assert_eq!(d.rules.cycles.mutual, CycleRule::Max(0));
        assert_eq!(d.rules.cycles.chain, CycleRule::Max(0));
        assert!(d.rules.thresholds.file.limits.is_empty());
        assert!(!d.levels.functions);
        // Every output format has a default path; json/html are on, sarif/cq off.
        assert!(d.output.json.path.is_some() && d.output.json.enabled == Some(true));
        assert!(d.output.html.path.is_some() && d.output.html.enabled == Some(true));
        assert!(d.output.sarif.path.is_some() && d.output.sarif.enabled == Some(false));
        assert!(d.output.codequality.path.is_some() && d.output.codequality.enabled == Some(false));
        assert!(
            d.output.prompt.path.is_some() && d.output.scorecard.path.as_deref() == Some("stdout")
        );
        // No project plugin pinned by default (→ auto detection).
        assert!(d.plugin.is_none());
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
[rules.cycles]
mutual = true
chain = 7
[rules.thresholds.file]
loc = 800
sloc = 1_200
cyclomatic = 25
mi = \"5K\"
";
        let cfg: Config = toml::from_str(src).unwrap();
        assert_eq!(cfg.rules.cycles.mutual, CycleRule::Max(0));
        assert_eq!(cfg.rules.cycles.chain, CycleRule::Max(7));
        assert_eq!(cfg.rules.thresholds.file.get("loc"), Some(800.0));
        // `sloc` (and every other engine metric) is now accepted, not just `loc`.
        assert_eq!(cfg.rules.thresholds.file.get("sloc"), Some(1_200.0));
        assert_eq!(cfg.rules.thresholds.file.get("cyclomatic"), Some(25.0));
        assert_eq!(cfg.rules.thresholds.file.get("mi"), Some(5_000.0));
    }

    #[test]
    fn bare_suffixed_threshold_values_parse() {
        // TOML rejects a bare `300K`; the pre-pass quotes it (only inside a
        // thresholds table) so the config parses without the user adding quotes.
        let src = "
[rules.cycles]
mutual = true
[rules.thresholds.file]
hk = 300K
cyclomatic = 200      # plain int stays native
sloc = 1.5M           # fractional + suffix
";
        let cfg: Config = toml::from_str(&quote_suffixed_thresholds(src)).unwrap();
        assert_eq!(cfg.rules.thresholds.file.get("hk"), Some(300_000.0));
        assert_eq!(cfg.rules.thresholds.file.get("cyclomatic"), Some(200.0));
        assert_eq!(cfg.rules.thresholds.file.get("sloc"), Some(1_500_000.0));
    }

    #[test]
    fn suffix_quoting_is_scoped_to_thresholds_tables() {
        // A bare-suffixed value outside a thresholds table is NOT touched (it would
        // still be invalid TOML there — we only help where suffixes are meaningful).
        let outside = quote_suffixed_thresholds("[other]\nx = 300K\n");
        assert!(outside.contains("x = 300K"), "untouched outside: {outside}");
        let inside = quote_suffixed_thresholds("[rules.thresholds.file]\nhk = 300K\n");
        assert!(inside.contains("hk = \"300K\""), "quoted inside: {inside}");
        // Already-quoted and plain values are left as-is.
        let q = quote_suffixed_thresholds("[rules.thresholds.file]\na = \"5M\"\nb = 200\n");
        assert!(q.contains("a = \"5M\"") && q.contains("b = 200"), "{q}");
    }

    #[test]
    fn threshold_value_accepts_int_and_float() {
        // Exercises the per-value deserializer over both TOML scalar forms: an
        // integer (`visit_i64`) and a bare float (`visit_f64`).
        let cfg: Config =
            toml::from_str("[rules.thresholds.file]\ncyclomatic = 30\nmi = 12.5\n").unwrap();
        assert_eq!(cfg.rules.thresholds.file.get("cyclomatic"), Some(30.0));
        assert_eq!(cfg.rules.thresholds.file.get("mi"), Some(12.5));
    }

    #[test]
    fn project_preset_parses_with_id_defaults() {
        // `[presets.TSR]` keys the preset by its table name; `label`/`title`
        // default to the id, so a minimal entry needs only `sort_metric`.
        let cfg = toml::from_str::<Config>(
            "[presets.TSR]\nsort_metric = \"tsr\"\nprompt = \"fix the ratio\"\n",
        )
        .unwrap();
        let def = &cfg.presets["TSR"];
        let p = def.to_preset("TSR");
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
        let cfg = toml::from_str::<Config>("[rules.thresholds.file]\nslocc = 800\n").unwrap();
        assert_eq!(cfg.rules.thresholds.file.get("slocc"), Some(800.0));
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
}
