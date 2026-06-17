use anyhow::Result;
use code_ranker_plugin_api::{
    default_cycle_kinds, default_node_kinds,
    graph::Graph,
    level::{AttributeSpec, EdgeKindSpec, Grouping, Level, NodeKindSpec, Thresholds},
    log,
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput, Preset},
};
use std::collections::BTreeMap;
use std::path::Path;

use cargo_metadata::MetadataCommand;

mod cfg;
mod collapse;
mod crate_graph;
mod dialect;
mod ids;
mod internal;
mod module_graph;

use cfg::CONFIG;
use collapse::collapse_to_files;
use internal::GraphBuilder;

pub struct RustPlugin;

impl LanguagePlugin for RustPlugin {
    fn name(&self) -> &str {
        "rust"
    }

    fn detect(&self, workspace: &Path, _input: &PluginInput) -> bool {
        // Project-detect marker filenames are DATA: read from `config.toml`'s
        // `detect_markers` (the detect logic stays in Rust). Rust detects on
        // `Cargo.toml`. (The `cargo metadata` manifest path in `syn_analyze` is
        // separate — that is cargo machinery, not a detect-marker list.)
        crate::config::string_list(&CONFIG, "detect_markers")
            .iter()
            .any(|m| workspace.join(m).exists())
    }

    fn levels(&self) -> Vec<Level> {
        // Edge-kind vocabulary (`uses` / `contains` / `reexports` / `super`) is
        // data: read it from `[edge_kinds]` in `rust/config.toml` (which
        // overrides the shared `uses` and adds the Rust-only structural kinds).
        // `collapse.rs` tags edges with the same identifiers via
        // `config::edge_kind_id`, so the spec and the tagged `kind` can't drift.
        let edge_kinds: BTreeMap<String, EdgeKindSpec> = crate::config::edge_kinds(&CONFIG);

        // Structural node/edge attribute display specs are DATA: read from the
        // merged config (`[node_attributes]` / `[edge_attributes]`). The shared
        // `path`/`loc`/`visibility`/`external` come from `defaults.toml`; Rust's
        // `crate`/`version`/`items`/`unsafe` (and edge `visibility`) from `rust/config.toml`.
        let node_attributes = crate::config::node_attributes(&CONFIG);
        let edge_attributes = crate::config::edge_attributes(&CONFIG);

        vec![
            Level {
                name: "files".into(),
                edge_kinds,
                node_attributes,
                edge_attributes,
                attribute_groups: BTreeMap::new(),
                node_kinds: default_node_kinds(),
                cycle_kinds: default_cycle_kinds(),
                // Cluster the diagram by the owning crate (compilation unit), not by
                // the source folder. Falls back to `dir` if `crate` is ever absent.
                grouping: Some(Grouping {
                    // Group by the `crate` node attribute — its key is DATA,
                    // validated against `[node_attributes]`.
                    key: Some(
                        crate::config::attr_key(&CONFIG, "crate")
                            .expect("rust/config.toml [node_attributes] is missing `crate`")
                            .into(),
                    ),
                    function: None,
                }),
            },
            // Optional sub-file level (off by default; `[levels] functions`).
            Level {
                name: "functions".into(),
                edge_kinds: BTreeMap::new(),
                node_attributes: BTreeMap::new(),
                edge_attributes: BTreeMap::new(),
                attribute_groups: BTreeMap::new(),
                node_kinds: function_node_kinds(),
                cycle_kinds: default_cycle_kinds(),
                grouping: None,
            },
        ]
    }

    fn thresholds(&self) -> BTreeMap<String, Thresholds> {
        // Rust-calibrated info/warning limits, read from `[thresholds]` in
        // `rust.toml` (see that file for the calibration notes).
        crate::config::thresholds(&CONFIG)
            .into_iter()
            .map(|(k, t)| {
                (
                    k,
                    Thresholds {
                        info: t.info,
                        warning: t.warning,
                    },
                )
            })
            .collect()
    }

    fn presets(&self, _input: &PluginInput) -> Vec<Preset> {
        // The common catalog (from `defaults.toml`) plus the Rust-only metric
        // lenses (`[[presets]]` in `rust.toml`), with each `doc_url` resolved to
        // `{doc_base}/rust/<slug>.md`. All data-driven via the shared loader.
        crate::config::resolved_presets(&CONFIG)
    }

    fn report_overrides(&self) -> code_ranker_plugin_api::report::ReportOverride {
        // Rust's `[report]` patches: e.g. surface the `unsafe` column / stat.
        crate::list_override::report_override(&CONFIG)
    }

    fn analyze(&self, workspace: &Path, _level: &str, input: &PluginInput) -> Result<Graph> {
        let mut builder = GraphBuilder::new();
        syn_analyze(workspace, input.ignore_tests, &mut builder)?;
        let internal = builder.build();
        Ok(collapse_to_files(internal))
    }

    fn metrics(&self, graph: &Graph) -> Vec<(String, MetricInputs)> {
        // Each `.rs` file node is re-read (by its absolute-path `id`) and measured
        // by our `tree-sitter-rust` engine; `#[cfg(test)]` / `#[test]` items are
        // stripped first so metrics reflect production code only (their lines
        // become `tloc`). The orchestrator writes the returned inputs.
        let mut out = Vec::new();
        for node in &graph.nodes {
            if node.kind != code_ranker_plugin_api::node::FILE {
                continue;
            }
            let Ok(src) = std::fs::read(&node.id) else {
                continue;
            };
            if let Some(m) = rust_file_metrics(&src) {
                out.push((node.id.clone(), m));
            }
        }
        out
    }

    fn function_units(&self, graph: &Graph) -> Vec<(Node, MetricInputs)> {
        let mut out = Vec::new();
        for node in &graph.nodes {
            if node.kind != code_ranker_plugin_api::node::FILE {
                continue;
            }
            let Ok(src) = std::fs::read(&node.id) else {
                continue;
            };
            // Mirror file metrics: strip inline tests so test fns never appear.
            let (prod, _tloc) = strip_cfg_test(&src);
            for u in dialect::compute_functions(&prod) {
                let fnode = Node {
                    id: format!("{}#{}@{}", node.id, u.name, u.start_line),
                    kind: u.kind.clone(),
                    name: u.name.clone(),
                    parent: Some(node.id.clone()),
                    attrs: Default::default(),
                };
                out.push((fnode, u.inputs));
            }
        }
        out
    }

    fn is_test_path(&self, rel_path: &str) -> bool {
        // Cargo's integration-test / bench targets live under top-level
        // `tests/` / `benches/` dirs — DATA from `config.toml`'s `test_dirs`.
        // The predicate LOGIC (first path component ∈ that list) stays here.
        // (Inline `#[cfg(test)]` modules are a separate, attribute-based notion
        // handled during the syn walk.)
        let first = rel_path.split('/').next();
        crate::config::string_list(&CONFIG, "test_dirs")
            .iter()
            .any(|d| first == Some(d.as_str()))
    }

    fn versions(&self, _workspace: &Path, _input: &PluginInput) -> Vec<(String, String)> {
        version_string()
            .map(|rv| vec![("rustc".to_string(), rv)])
            .unwrap_or_default()
    }

    fn roots(&self, _workspace: &Path) -> Vec<(String, String)> {
        rust_toolchain_roots()
    }

    fn metric_specs(
        &self,
        defaults: BTreeMap<String, AttributeSpec>,
    ) -> BTreeMap<String, AttributeSpec> {
        // Apply the Rust `[specs.<key>]` overrides over the central builtin specs:
        // the production-only LOC nuance (`#[cfg(test)]` stripped) and the exact
        // Halstead operator/operand sets Rust counts.
        crate::config::apply_spec_overrides(defaults, &CONFIG)
    }
}

/// The Rust/Cargo toolchain path roots used to shorten external node ids in the
/// snapshot: `cargo` (`$CARGO_HOME`), `registry` (the crates.io source dir),
/// `rustup` (`$RUSTUP_HOME`), and `rust-src` (the stdlib source under the active
/// sysroot). These are Rust-specific, so they live here in the Rust plugin rather
/// than in the language-agnostic orchestrator.
fn rust_toolchain_roots() -> Vec<(String, String)> {
    let mut roots = Vec::new();
    let home = std::env::var("HOME").unwrap_or_default();

    let cargo = std::env::var("CARGO_HOME").unwrap_or_else(|_| format!("{home}/.cargo"));
    let rustup = std::env::var("RUSTUP_HOME").unwrap_or_else(|_| format!("{home}/.rustup"));

    if !cargo.is_empty() {
        // Auto-detect crates.io registry hash dir (e.g. index.crates.io-<hash>).
        let registry_src = format!("{cargo}/registry/src");
        if let Ok(entries) = std::fs::read_dir(&registry_src) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("index.crates.io") {
                    roots.push(("registry".to_string(), format!("{registry_src}/{name}")));
                    break;
                }
            }
        }
        roots.push(("cargo".to_string(), cargo));
    }
    if !rustup.is_empty() {
        // Add rust-src root: sysroot/lib/rustlib/src/rust/library — shortens stdlib
        // paths from {rustup}/toolchains/.../library/... to {rust-src}/...
        if which::which("rustc").is_ok()
            && let Ok(out) = log::timed("rustc --print sysroot", || {
                std::process::Command::new("rustc")
                    .args(["--print", "sysroot"])
                    .output()
            })
            && out.status.success()
        {
            let sysroot = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let rust_lib = format!("{sysroot}/lib/rustlib/src/rust/library");
            if std::path::Path::new(&rust_lib).exists() {
                roots.push(("rust-src".to_string(), rust_lib));
            }
        }
        roots.push(("rustup".to_string(), rustup));
    }
    roots
}

/// Syntactic stage: resolve the workspace via `cargo metadata` and build the
/// internal crate + module/use graphs.
fn syn_analyze(workspace: &Path, ignore_tests: bool, builder: &mut GraphBuilder) -> Result<()> {
    let manifest = workspace.join("Cargo.toml");
    // code-ranker is an offline tool: it never fetches from the network. See the
    // comment in the original lib.rs for the research notes on --offline vs
    // --no-deps vs full. Short version: --offline keeps external/cross-crate
    // edges AND never goes to the network; the cache must be warm.
    let metadata = log::timed("cargo metadata --offline", || {
        MetadataCommand::new()
            .manifest_path(&manifest)
            .other_options(vec!["--offline".to_string()])
            .exec()
    })
    .map_err(|err| offline_metadata_error(&manifest, err))?;

    crate_graph::contribute(&metadata, builder);
    module_graph::contribute(&metadata, ignore_tests, builder)?;
    Ok(())
}

fn offline_metadata_error(manifest: &Path, err: cargo_metadata::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "cargo metadata (offline) failed for {manifest}\n\n\
         code-ranker is an offline tool — it never downloads dependencies. It reads \
         the dependency graph from cargo's local cache, which must already be \
         populated for this project.\n\n\
         Warm the cache once (with network), then re-run code-ranker:\n    \
         cargo metadata --manifest-path {manifest} >/dev/null\n\
         (a prior `cargo build` / `cargo fetch` works too).\n\n\
         In CI: run code-ranker on the same image/cache as your build or test jobs, \
         where the cache is already warm.\n\n\
         Underlying cargo error: {err}",
        manifest = manifest.display(),
    )
}

fn version_string() -> Option<String> {
    which::which("rustc").ok()?;
    let out = log::timed("rustc --version", || {
        std::process::Command::new("rustc")
            .arg("--version")
            .output()
    })
    .ok()?;
    if out.status.success() {
        Some(
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .nth(1)
                .unwrap_or("unknown")
                .to_string(),
        )
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Complexity: strip inline tests, run the tree-sitter-rust engine, write metrics
// ─────────────────────────────────────────────────────────────────────────────

/// Per-language unit kinds for the `functions` level (rendered via this dict —
/// the viewer hardcodes no kind by name). Read from `[node_kinds]` in the merged
/// config: the shared `method` from `defaults.toml` plus Rust's own `fn`
/// (Rust labels its free functions `fn`, not the generic `function`). The
/// inherited generic `function` entry is also published; it is harmless on this
/// off-by-default level (the dialect's `fn_kind` only ever tags `fn` / `method`).
fn function_node_kinds() -> BTreeMap<String, NodeKindSpec> {
    crate::config::node_kinds(&CONFIG)
}

/// Measure Rust complexity metrics for one file from its source bytes.
/// `#[cfg(test)]` / `#[test]` / `#[bench]` items are stripped first (their lines
/// become `tloc`), then the generic engine via the rust dialect runs. Returns the
/// measured [`MetricInputs`] (`None` if the source did not parse); the orchestrator
/// writes them onto the node.
fn rust_file_metrics(src: &[u8]) -> Option<MetricInputs> {
    let (prod, tloc) = strip_cfg_test(src);
    let mut m = dialect::compute(&prod)?;
    m.tloc = tloc as f64;
    Some(m)
}

/// True if any attribute gates an item to tests: `#[test]`, `#[bench]`, or
/// `#[cfg(test)]` / `#[cfg(all(test, …))]` / `#[cfg(any(test, …))]`. A `test`
/// **identifier** inside `cfg(...)` is what matches — `cfg(feature = "test")`
/// (a string literal) does not.
fn is_test_attr(attr: &syn::Attribute) -> bool {
    // The `test` / `bench` / `cfg` attribute idents are DATA (`[syn]`).
    if attr.path().is_ident(cfg::SYN_TEST.as_str()) || attr.path().is_ident(cfg::SYN_BENCH.as_str())
    {
        return true;
    }
    if attr.path().is_ident(cfg::SYN_CFG.as_str())
        && let syn::Meta::List(list) = &attr.meta
    {
        return tokens_have_test_ident(list.tokens.clone());
    }
    false
}

/// Recursively scan a token stream for a bare `test` identifier (descends into
/// `all(...)` / `any(...)` groups).
fn tokens_have_test_ident(ts: proc_macro2::TokenStream) -> bool {
    ts.into_iter().any(|t| match t {
        proc_macro2::TokenTree::Ident(i) => i == cfg::SYN_TEST.as_str(),
        proc_macro2::TokenTree::Group(g) => tokens_have_test_ident(g.stream()),
        _ => false,
    })
}

/// Visitor collecting the 1-based, inclusive line ranges of test-only items
/// (`#[cfg(test)]` modules, `#[test]`/`#[cfg(test)]` fns), attribute line
/// included. It recurses into ordinary modules to catch nested test modules but
/// not into a test item it already captured.
#[derive(Default)]
struct TestSpans {
    ranges: Vec<(usize, usize)>,
}

impl TestSpans {
    fn record(&mut self, attrs: &[syn::Attribute], span: proc_macro2::Span) {
        use syn::spanned::Spanned;
        let start = attrs
            .iter()
            .map(|a| a.span().start().line)
            .chain(std::iter::once(span.start().line))
            .min()
            .unwrap_or(0);
        self.ranges.push((start, span.end().line));
    }
}

impl<'ast> syn::visit::Visit<'ast> for TestSpans {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        use syn::spanned::Spanned;
        if m.attrs.iter().any(is_test_attr) {
            self.record(&m.attrs, m.span());
        } else {
            syn::visit::visit_item_mod(self, m);
        }
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        use syn::spanned::Spanned;
        if f.attrs.iter().any(is_test_attr) {
            self.record(&f.attrs, f.span());
        }
    }
}

/// Step 1 of the Rust line accounting: remove `#[cfg(test)]` / `#[test]` /
/// `#[bench]` items so the production metrics (`sloc` / `cloc` / `blank` / `hk` /
/// complexity) are then measured on production code only. Returns the production
/// source **and** `tloc` — the number of test lines removed (the whole test
/// region: attribute, body, braces). Parse failures or no test items return the
/// source unchanged with `tloc = 0`.
fn strip_cfg_test(src: &[u8]) -> (Vec<u8>, usize) {
    use syn::visit::Visit;
    let Ok(text) = std::str::from_utf8(src) else {
        return (src.to_vec(), 0);
    };
    let Ok(file) = syn::parse_file(text) else {
        return (src.to_vec(), 0);
    };
    let mut spans = TestSpans::default();
    spans.visit_file(&file);
    if spans.ranges.is_empty() {
        return (src.to_vec(), 0);
    }
    let drop: std::collections::HashSet<usize> =
        spans.ranges.iter().flat_map(|&(s, e)| s..=e).collect();
    let tloc = drop.len();
    let mut out: String = text
        .lines()
        .enumerate()
        .filter(|(i, _)| !drop.contains(&(i + 1)))
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n");
    out.push('\n');
    (out.into_bytes(), tloc)
}

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod mod_rs_tests;
