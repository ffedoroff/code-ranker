//! The [`LanguagePlugin`] trait + [`Options`] + [`Preset`].
//!
//! A plugin turns a workspace into nodes + edges at a requested level
//! ([`analyze`](LanguagePlugin::analyze)) and writes the per-file **complexity
//! metrics** for its own language onto those nodes ([`metrics`](LanguagePlugin::metrics)).
//! Metrics are a per-language concern — each plugin parses its own files with its
//! own grammar and calls the matching `code-ranker-complexity` engine — so there
//! is no central, by-extension metric dispatcher. The language-agnostic derived
//! data (cycles, Henry-Kafura, stats) is still filled centrally by the
//! orchestrator. The CLI holds the registry of plugins; it talks to them ONLY
//! through this trait and never names a concrete language.

use crate::graph::Graph;
use crate::level::{AttributeSpec, Level, Thresholds};
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

/// Free-form key/value options passed from the CLI (future `--plugin-opt k=v`).
/// `BTreeMap` for deterministic iteration order.
pub type Options = BTreeMap<String, String>;

/// Everything the orchestrator feeds a plugin from config + CLI input.
#[derive(Debug, Clone, Default)]
pub struct PluginInput {
    /// Glob patterns for paths to skip during analysis (config + CLI).
    pub ignore: Vec<String>,
    /// When `true`, the plugin must skip its own **test files** during the walk
    /// (mirrors `[ignore] tests`). What counts as a test is language-specific —
    /// see [`LanguagePlugin::is_test_path`] — so the detection lives in the
    /// plugin, not the CLI.
    pub ignore_tests: bool,
    /// Free-form key/value options. A plugin reads its own keys, ignores the rest.
    pub options: Options,
}

/// A Prompt-Generator preset (a refactoring principle): a ready-to-paste AI
/// instruction plus how the UI seeds the node selection for it. The orchestrator
/// builds a generic default set and hands it to [`LanguagePlugin::presets`],
/// which may pass it through, edit, drop or extend per language.
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

pub trait LanguagePlugin {
    /// Canonical name, e.g. `"rust"`. Used by `--plugin` and recorded in the
    /// snapshot. Each plugin has exactly one name (js and ts are separate).
    fn name(&self) -> &str;

    /// Can this plugin parse `workspace` (honoring `input`)?
    fn detect(&self, workspace: &Path, input: &PluginInput) -> bool;

    /// Levels this plugin can produce, each carrying its edge-kind / attribute /
    /// node-kind / cycle-kind semantics.
    fn levels(&self) -> Vec<Level>;

    /// Parse the workspace into a graph AT `level` (by name). **Structure only**:
    /// nodes (with their structural attributes) + edges. Metrics are added
    /// downstream. When `input.ignore_tests` is set, the plugin must drop its
    /// own test files here (it knows the language's conventions; see
    /// [`is_test_path`](Self::is_test_path)).
    fn analyze(&self, workspace: &Path, level: &str, input: &PluginInput) -> Result<Graph>;

    /// Write this language's per-file complexity metrics (cyclomatic, cognitive,
    /// Halstead, MI, LOC, …) onto the graph's `file` nodes, in place. The plugin
    /// parses each of its own files (by `node.id`, an absolute path) with its own
    /// grammar and calls the matching `code-ranker-complexity` engine. Returns the
    /// number of file nodes annotated. Default: none (a plugin that ships no
    /// metric engine).
    fn metrics(&self, _graph: &mut Graph) -> usize {
        0
    }

    /// Function-level metric nodes — one per sub-file unit (function / method /
    /// closure) — for the optional `functions` graph level. Each returned node
    /// carries its metrics in `attrs`, a per-language `kind`, and `parent` set to
    /// its **file node's id** (so the orchestrator can relativize and group it).
    /// `graph` is the just-parsed file graph with **absolute** file-path ids, so a
    /// plugin reads each file by `node.id`. Only called when the level is enabled;
    /// default: none (a plugin that ships no function-level support).
    fn function_units(&self, _graph: &Graph) -> Vec<crate::node::Node> {
        Vec::new()
    }

    /// Does this workspace-relative path (forward-slashed, no leading `./`) name
    /// a **test** file in this language? Used to drop tests during the walk when
    /// `PluginInput::ignore_tests` is set. Default: nothing is a test.
    fn is_test_path(&self, _rel_path: &str) -> bool {
        false
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

    /// Transform the orchestrator's generic default presets for this language.
    /// Default: pass them through unchanged. A plugin may reword a `prompt`,
    /// change a `sort_metric`, drop a preset, or add language-specific ones.
    fn presets(&self, defaults: Vec<Preset>, _input: &PluginInput) -> Vec<Preset> {
        defaults
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;

    /// A minimal plugin that implements only the required methods, so the trait's
    /// default hooks (`is_test_path` / `versions` / `roots` / `presets` /
    /// `metric_specs` / `thresholds` / `metrics`) are exercised as-is.
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
        fn analyze(&self, _w: &Path, _l: &str, _i: &PluginInput) -> Result<Graph> {
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
        let g = p.analyze(ws, "files", &input).expect("dummy analyze ok");
        assert!(g.nodes.is_empty() && g.edges.is_empty());

        assert!(!p.is_test_path("anything"), "default: nothing is a test");
        let empty_graph = Graph {
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        assert!(
            p.function_units(&empty_graph).is_empty(),
            "default: no function units"
        );
        assert!(p.versions(ws, &input).is_empty(), "default: no versions");
        assert!(p.roots(ws).is_empty(), "default: no roots");
        assert!(p.thresholds().is_empty(), "default: no thresholds");

        // presets / metric_specs default to pass-through (return input unchanged).
        assert!(p.presets(Vec::new(), &input).is_empty());
        let specs: BTreeMap<String, AttributeSpec> = BTreeMap::new();
        assert!(p.metric_specs(specs).is_empty());

        // metrics default: annotates nothing.
        let mut g = Graph {
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        assert_eq!(p.metrics(&mut g), 0);
    }

    #[test]
    fn detect_with_marker_checks_file_presence() {
        let dir = std::env::temp_dir();
        // a marker that (almost certainly) does not exist
        assert!(!detect_with_marker(&dir, "code-ranker-no-such-marker.xyz"));
    }
}
