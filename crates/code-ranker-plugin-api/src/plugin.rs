//! The [`LanguagePlugin`] trait + [`Options`] + [`Preset`].
//!
//! A plugin turns a workspace into nodes + edges at a requested level
//! ([`analyze`](LanguagePlugin::analyze)) and **measures** the per-file **complexity
//! metrics** for its own language ([`metrics`](LanguagePlugin::metrics)), returning
//! the raw [`MetricInputs`](crate::metrics::MetricInputs) for the orchestrator to
//! write. Measuring is a per-language concern — each plugin parses its own files
//! with its own grammar and engine — so there is no central, by-extension metric
//! dispatcher; but the *writing* (tier-2 derivation + node enrichment) and the
//! language-agnostic derived data (cycles, Henry-Kafura, stats) are filled
//! centrally by the orchestrator, so a plugin needs no dependency on the
//! graph/enrichment crate. Plugins SELF-REGISTER via [`inventory::submit!`] into
//! [`registry`]; the CLI works only with that array through this trait and never
//! names a concrete language.

use crate::graph::Graph;
use crate::level::{AttributeSpec, Level, Thresholds};
use crate::metrics::MetricInputs;
use crate::node::Node;
use crate::report::ReportOverride;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Return `true` when `workspace` contains the given marker file. A generic,
/// language-agnostic detection helper for marker-based plugins (e.g. JS →
/// `"package.json"`, TS → `"tsconfig.json"`). Lives here, not in any one language
/// plugin, so every plugin can reuse it without depending on a sibling plugin.
pub fn detect_with_marker(workspace: &Path, marker: &str) -> bool {
    workspace.join(marker).exists()
}

/// Everything the orchestrator feeds a plugin from config + CLI input.
#[derive(Debug, Clone, Default)]
pub struct PluginInput {
    /// Glob patterns for paths to skip during analysis (config + CLI).
    pub ignore: Vec<String>,
    /// When `true`, the plugin must skip its own **test files** during the walk
    /// (mirrors `[ignore] tests`). What counts as a test is language-specific, so
    /// the detection lives in the plugin (during
    /// [`analyze`](LanguagePlugin::analyze)), not the CLI.
    pub ignore_tests: bool,
    /// When `true`, a directory-walking plugin honours `.gitignore` (+ global
    /// gitignore + `.git/info/exclude`) while collecting source files, scoped to
    /// the analyzed root (mirrors `[ignore] gitignore`). The Rust plugin resolves
    /// files via `cargo metadata`, not a walk, so it ignores this.
    pub gitignore: bool,
    /// When `true`, a directory-walking plugin honours `.ignore` files while
    /// collecting source files (mirrors `[ignore] ignore_files`).
    pub ignore_files: bool,
    /// When `true`, a directory-walking plugin skips hidden files / directories
    /// (dotfiles) while collecting source files (mirrors `[ignore] hidden`).
    pub hidden: bool,
}

/// A Prompt-Generator preset (a refactoring principle): a ready-to-paste AI
/// instruction plus how the UI seeds the node selection for it. Each plugin
/// builds its own set from config via [`LanguagePlugin::presets`] (the common
/// catalog plus any language-specific presets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    /// Stable id / short code shown on the button (e.g. `"ADP"`).
    pub id: String,
    /// Button label (usually the id).
    pub label: String,
    /// Full principle title (first heading of the generated prompt).
    pub title: String,
    /// The prompt body (Markdown, language-neutral by default).
    pub prompt: String,
    /// Link to the full principle doc, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_url: Option<String>,
    /// The metric the recommended-node list sorts by (an attribute key, or the
    /// pseudo-metric `"cycle"`).
    pub sort_metric: String,
    /// Which connection sets the preset pre-selects: any of `"in"`/`"out"`/`"common"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<String>,
}

pub trait LanguagePlugin: Sync {
    /// Canonical name, e.g. `"rust"`. Used by `--plugin` and recorded in the
    /// snapshot. Each plugin has exactly one name (js and ts are separate).
    fn name(&self) -> &str;

    /// The plugin's fully-merged config table (its inheritance chain
    /// `defaults.toml ⊕ [base] ⊕ <lang>.toml`). Surfaced for `--export-full-config`
    /// so a user can inspect every effective parameter. Default: empty (a stub /
    /// test plugin with no config file).
    fn config(&self) -> toml::Table {
        toml::Table::new()
    }

    /// Can this plugin parse `workspace` (honoring `input`)?
    fn detect(&self, workspace: &Path, input: &PluginInput) -> bool;

    /// Levels this plugin can produce, each carrying its edge-kind / attribute /
    /// node-kind / cycle-kind semantics.
    fn levels(&self) -> Vec<Level>;

    /// Parse the workspace into the file-level graph. **Structure only**: nodes
    /// (with their structural attributes) + edges. Metrics are added downstream.
    /// When `input.ignore_tests` is set, the plugin must drop its own test files
    /// here (it knows the language's conventions).
    fn analyze(&self, workspace: &Path, input: &PluginInput) -> Result<Graph>;

    /// **Measure** this language's per-file complexity tier-1 counts and return
    /// them keyed by `file` node id (an absolute path). The plugin parses each of
    /// its own files (by `node.id`) with its own grammar and engine, returning a
    /// [`MetricInputs`] per file; it does **not** write them. The orchestrator
    /// runs the tier-2 registry and writes every metric onto the node — so the
    /// plugin needs no dependency on the graph/enrichment crate. Default: none (a
    /// plugin that ships no metric engine).
    fn metrics(&self, _graph: &Graph) -> Vec<(String, MetricInputs)> {
        Vec::new()
    }

    /// Function-level metric units — one per sub-file unit (function / method /
    /// closure) — for the optional `functions` graph level. Each returned pair is
    /// the unit's [`Node`] (its per-language `kind`, `name`, and `parent` = its
    /// **file node's id**, but **no metrics yet**) plus the unit's measured
    /// [`MetricInputs`]; the orchestrator writes the metrics onto the node. `graph`
    /// is the just-parsed file graph with **absolute** file-path ids, so a plugin
    /// reads each file by `node.id`. Only called when the level is enabled;
    /// default: none (a plugin that ships no function-level support).
    fn function_units(&self, _graph: &Graph) -> Vec<(Node, MetricInputs)> {
        Vec::new()
    }

    /// Toolchain versions to record in the snapshot, e.g. `[("rustc", "1.88.0")]`.
    fn versions(&self, _workspace: &Path, _input: &PluginInput) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Named external-path roots for this language, as `(name, absolute_path)`
    /// pairs, used to shorten node ids in the snapshot (a path under a root is
    /// rewritten to `{name}/…`). These are **language-specific** — e.g. Rust
    /// returns `cargo` / `registry` / `rustup` / `rust-src`; a Python plugin would
    /// return its virtualenv / site-packages; JS/TS would return `node_modules`.
    /// The orchestrator always adds the generic `target` root itself, so a plugin
    /// returns only its own toolchain/dependency locations. Default: none.
    ///
    /// This keeps language/toolchain knowledge inside the plugin instead of the
    /// language-agnostic orchestrator (mirrors [`versions`](Self::versions)).
    fn roots(&self, _workspace: &Path) -> Vec<(String, String)> {
        Vec::new()
    }

    /// The Prompt-Generator presets for this language. A plugin builds them from
    /// its own config (the common catalog in `defaults.toml` merged with the
    /// language's `<lang>.toml`, with each `doc_url` resolved). Default: none (a
    /// plugin that ships no presets).
    fn presets(&self, _input: &PluginInput) -> Vec<Preset> {
        Vec::new()
    }

    /// Transform the orchestrator's **language-neutral** default complexity metric
    /// specs (key → [`AttributeSpec`], from `code-ranker-graph`'s `metric_specs`)
    /// for this language. Default: pass them through unchanged. A plugin may reword
    /// a `description` to add language-specific nuance (e.g. Rust noting that
    /// `sloc` / `lloc` / `cloc` / `blank` exclude inline `#[cfg(test)]` items) — so
    /// the shared catalog stays neutral and each language refines only what differs.
    fn metric_specs(
        &self,
        defaults: BTreeMap<String, AttributeSpec>,
    ) -> BTreeMap<String, AttributeSpec> {
        defaults
    }

    /// Language-calibrated per-metric thresholds (attribute key → tiers). The
    /// orchestrator overlays these onto the attribute specs. Default: none.
    fn thresholds(&self) -> BTreeMap<String, Thresholds> {
        BTreeMap::new()
    }

    /// Per-language patches over the global report lists — the table `columns`,
    /// the card-featured metrics, and the JSON `stats` keys (all inherited from
    /// the metric catalog). A language adds its own metric (e.g. Rust `unsafe`),
    /// drops some, or reorders, via its `<lang>.toml` `[report]` section. The
    /// orchestrator applies the patch over the catalog defaults, then prunes to
    /// keys present. Default: no override (use the catalog lists as-is).
    fn report_overrides(&self) -> ReportOverride {
        ReportOverride::default()
    }
}

/// A self-registered language plugin. Each plugin in the plugins crate submits one
/// via [`inventory::submit!`]; the binary's registry is assembled by the linker, so
/// NO central code lists the plugins and no caller (the CLI) ever names a language.
/// Plugins are zero-sized unit structs, so a `&'static` reference is free.
pub struct PluginRegistration(pub &'static dyn LanguagePlugin);

inventory::collect!(PluginRegistration);

/// Every self-registered language plugin. The CLI works only through this array
/// and the [`LanguagePlugin`] trait — it never names a concrete language.
///
/// Order is link order and is NOT significant: auto-detection treats multiple
/// matches as an error (it never picks by position), and any user-facing listing
/// sorts by [`LanguagePlugin::name`].
pub fn registry() -> Vec<&'static dyn LanguagePlugin> {
    inventory::iter::<PluginRegistration>()
        .map(|entry| entry.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;

    /// A minimal plugin that implements only the required methods, so the trait's
    /// default hooks (`versions` / `roots` / `presets` / `metric_specs` /
    /// `thresholds` / `metrics`) are exercised as-is.
    struct Dummy;
    impl LanguagePlugin for Dummy {
        fn name(&self) -> &str {
            "dummy"
        }
        fn detect(&self, _w: &Path, _i: &PluginInput) -> bool {
            false
        }
        fn levels(&self) -> Vec<crate::level::Level> {
            Vec::new()
        }
        fn analyze(&self, _w: &Path, _i: &PluginInput) -> Result<Graph> {
            Ok(Graph {
                nodes: Vec::new(),
                edges: Vec::new(),
            })
        }
    }

    #[test]
    fn trait_default_hooks_are_noops() {
        let p = Dummy;
        let ws = Path::new("/tmp");
        let input = PluginInput::default();

        // Exercise the required methods too, so the dummy carries no dead code.
        assert_eq!(p.name(), "dummy");
        assert!(!p.detect(ws, &input));
        assert!(p.levels().is_empty());
        let g = p.analyze(ws, &input).expect("dummy analyze ok");
        assert!(g.nodes.is_empty() && g.edges.is_empty());

        let empty_graph = Graph {
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        assert!(
            p.function_units(&empty_graph).is_empty(),
            "default: no function units"
        );
        assert!(p.metrics(&empty_graph).is_empty(), "default: no metrics");
        assert!(p.versions(ws, &input).is_empty(), "default: no versions");
        assert!(p.roots(ws).is_empty(), "default: no roots");
        assert!(p.thresholds().is_empty(), "default: no thresholds");

        // config defaults to an empty table (a stub with no config file).
        assert!(p.config().is_empty(), "default: empty config table");

        // presets defaults to none; metric_specs defaults to pass-through.
        assert!(p.presets(&input).is_empty());
        let specs: BTreeMap<String, AttributeSpec> = BTreeMap::new();
        assert!(p.metric_specs(specs).is_empty());

        // report_overrides defaults to a no-op (catalog lists kept as-is).
        let ro = p.report_overrides();
        assert!(ro.columns.is_noop() && ro.card.is_noop() && ro.stats.is_noop());
    }

    #[test]
    fn detect_with_marker_checks_file_presence() {
        let dir = std::env::temp_dir();
        // a marker that (almost certainly) does not exist
        assert!(!detect_with_marker(&dir, "code-ranker-no-such-marker.xyz"));
    }
}
