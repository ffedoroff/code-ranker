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
/// The version a `code-ranker.toml` must declare in `version` — the **config + CLI**
/// format version [`code_ranker_graph::version::CONFIG_VERSION`] (separate from the
/// JSON-snapshot `SCHEMA_VERSION`; see `docs/versions.md`). The loader requires an
/// exact match, failing with a migrate / upgrade hint instead of a cryptic
/// `unknown field` error.
pub const CONFIG_SCHEMA_VERSION: &str = code_ranker_graph::version::CONFIG_VERSION;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Config-schema version (`major.minor`, e.g. `"5.0"`) — **required** in a
    /// `code-ranker.toml`. Validated against [`CONFIG_SCHEMA_VERSION`] at load.
    /// `Option` so a missing value yields our migrate-hint error, not serde's.
    #[serde(default)]
    pub version: Option<String>,
    /// The `[plugins]` table: the active-language list (`enabled = [...]`) plus the
    /// per-language config blocks (`[plugins.<lang>]`, with the shared `[plugins.base]`).
    #[serde(default)]
    pub plugins: PluginsConfig,
    /// Output artifacts (`[output]`) — **global** (one report per run covers every
    /// language), so this is not per-language.
    #[serde(default)]
    pub output: OutputConfig,
    /// Per-file doc-corpus overrides (`[templates.languages.<lang>.<ID>]`): use a
    /// file from disk in place of the embedded `languages/<lang>/<ID>.md`. Global.
    #[serde(default)]
    pub templates: TemplatesConfig,
}

/// The `[plugins]` table. `enabled` is the active-language list; every other key is
/// a per-language config block (`[plugins.<lang>]`), with the reserved `"base"` key
/// inherited by every language. `enabled` and `base` are therefore reserved and
/// cannot be language names. The blocks are free-form: plugin-config keys
/// (`extensions`, `detect_markers`, …) are consumed via `effective_plugin_config`,
/// while the orchestrator sections (`ignore`/`rules`/`metrics`/`levels`/`report`/
/// `principles`) are read via [`Config::language_config`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PluginsConfig {
    /// Active languages (e.g. `["rust", "markdown"]`). Empty → auto-detect all.
    /// Overridden by `--plugins`.
    #[serde(default)]
    pub enabled: Vec<String>,
    /// Per-language config blocks keyed by language (plus the reserved `"base"`).
    /// Captures every `[plugins]` key other than `enabled`.
    #[serde(flatten)]
    pub languages: BTreeMap<String, toml::Table>,
}

/// The orchestrator-read config sections that are now **per-language**, resolved
/// for one language by [`Config::language_config`] (defaults' `[plugins.base]` ⊕
/// user `[plugins.base]` ⊕ user `[plugins.<lang>]`). Plugin-config keys in the same
/// block (`extensions`, …) are ignored here — they feed the plugin separately.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LangConfig {
    pub ignore: IgnoreConfig,
    pub rules: RulesConfig,
    pub metrics: BTreeMap<String, code_ranker_graph::MetricDef>,
    pub levels: LevelsConfig,
    pub report: toml::Table,
    pub principles: BTreeMap<String, PrincipleDef>,
}

/// The orchestrator config-section keys carried inside a `[plugins.<lang>]` block.
/// (Other keys in the block are plugin config, consumed via `effective_plugin_config`.)
pub(crate) const LANG_SECTION_KEYS: &[&str] = &[
    "ignore",
    "rules",
    "metrics",
    "levels",
    "report",
    "principles",
];

/// Doc-corpus override map (`[templates.languages.<lang>.<ID>]`): `lang → (ID →
/// file path)`. A configured path is read from disk in place of the embedded
/// `languages/<lang>/<ID>.md` (see `crate::templates`). Empty by default — its
/// `Default` is plain empty maps (no `defaults.toml` slice), so it never re-enters
/// the `BUILTIN` `LazyLock` while that initializes.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplatesConfig {
    /// `<lang> → (<ID> → override file path)`.
    #[serde(default)]
    pub languages: BTreeMap<String, BTreeMap<String, String>>,
    /// `[templates] prompt = "<path>"` — override the Prompt-Generator scaffolding
    /// (`metrics/prompt.md`) with a file from disk (same `## <field>` Markdown
    /// shape). `None` → the built-in prompt template.
    #[serde(default)]
    pub prompt: Option<String>,
}

impl Default for Config {
    /// The built-in defaults — a clone of the embedded `defaults.toml` (the
    /// single source of default values). Used when no project config is found,
    /// and as the merge base every discovered config layers over.
    fn default() -> Self {
        BUILTIN.clone()
    }
}

/// A project-config principle (`[principles.<ID>]`) — the table key is the id. Mirrors
/// the plugin [`Principle`](code_ranker_plugin_api::Principle) but with sane
/// defaults so a project entry needs only `sort_metric` (+ usually `title`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PrincipleDef {
    /// Button label; defaults to the id.
    pub label: Option<String>,
    /// Principle title (first heading of the generated prompt); defaults to the id.
    pub title: Option<String>,
    /// Prompt body (Markdown). Defaults to empty.
    pub prompt: String,
    /// Link to a principle doc, if any.
    pub doc_url: Option<String>,
    /// The metric the recommended-node list sorts by (an attribute key, or the
    /// pseudo-metric `"cycle"`). Required in practice — the lens the principle is.
    pub sort_metric: String,
    /// Connection sets the principle pre-selects: any of `"in"` / `"out"` / `"common"`.
    pub connections: Vec<String>,
}

impl PrincipleDef {
    /// Build the runtime [`Principle`](code_ranker_plugin_api::Principle) for
    /// this entry, defaulting `label` / `title` to the id.
    pub fn to_principle(&self, id: &str) -> code_ranker_plugin_api::Principle {
        code_ranker_plugin_api::Principle {
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

/// Merge the project's `[principles.<ID>]` over a plugin's principle catalog: a
/// same-id project principle replaces the plugin's (in place, keeping catalog
/// order), a new id is appended. So a project can recommend / scorecard / document
/// its own custom metric. Lives here (next to [`PrincipleDef`]) so both the
/// analysis pipeline and the analysis-free `docs` command share one merge without
/// either depending on the other.
pub(crate) fn merge_project_principles(
    mut catalog: Vec<code_ranker_plugin_api::Principle>,
    project: &std::collections::BTreeMap<String, PrincipleDef>,
) -> Vec<code_ranker_plugin_api::Principle> {
    for (id, def) in project {
        let p = def.to_principle(id);
        match catalog.iter_mut().find(|e| e.id == p.id) {
            Some(existing) => *existing = p,
            None => catalog.push(p),
        }
    }
    catalog
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
    /// `scorecard` is flag-driven (off unless `--output.scorecard` is passed); its
    /// `path` here only supplies the default destination template.
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
    /// `tests = false`. The plugin decides what counts as a test, during its
    /// own walk (see `PluginInput::ignore_tests`).
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
    /// Custom config-defined checks (`[rules.checks.<id>]`): a CEL boolean `when`
    /// predicate over each file node's values (numeric / string attributes plus
    /// derived `path`/`name`/`stem`/`ext`/`dir`, the dependency lists
    /// `deps`/`rdeps`, and the file collections `files`/`siblings`) and a
    /// `message`. Empty by default → no extra checks. Lets a project write a
    /// linter rule entirely in config (see `code_ranker_graph::CheckDef`).
    pub checks: BTreeMap<String, code_ranker_graph::CheckDef>,
    /// Reusable named CEL helpers (`[rules.defs]`, `name = "<cel expr>"`),
    /// expanded into a check's `when` before compilation. A helper may reference
    /// an earlier helper. Empty by default. Lets a project name a shared
    /// vocabulary (e.g. `is_domain = 'contains(path, "/domain/")'`).
    pub defs: BTreeMap<String, String>,
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
    /// Set (or override) the limit for `metric`. Used by tests; overrides now write
    /// raw `[plugins.<lang>]` tables rather than mutating typed thresholds.
    #[allow(dead_code)]
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

#[cfg(test)]
#[path = "model_test.rs"]
mod tests;
