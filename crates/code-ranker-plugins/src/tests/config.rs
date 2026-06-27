use super::*;
use code_ranker_plugin_api::list_override::report_override;
use code_ranker_plugin_api::toml_merge::{deep_merge, merge_principles};
use toml::{Table, Value};

/// Deep-merge recurses into tables and lets the overlay win per key.
#[test]
fn deep_merge_overrides_per_key_and_recurses() {
    let base: Table = "[a]\nx = 1\ny = 2\n[b]\nk = \"base\"\n".parse().unwrap();
    let overlay: Table = "[a]\ny = 20\nz = 3\n[c]\nnew = true\n".parse().unwrap();
    let merged = deep_merge(base, overlay);

    let a = merged["a"].as_table().unwrap();
    assert_eq!(a["x"].as_integer(), Some(1), "base-only key kept");
    assert_eq!(a["y"].as_integer(), Some(20), "overlay overrides base key");
    assert_eq!(a["z"].as_integer(), Some(3), "overlay-only key added");
    assert_eq!(
        merged["b"]["k"].as_str(),
        Some("base"),
        "base-only table kept"
    );
    assert!(
        merged["c"]["new"].as_bool().unwrap(),
        "overlay-only table added"
    );
}

/// A non-table overlay value replaces a base table outright.
#[test]
fn deep_merge_replaces_table_with_scalar() {
    let base: Table = "[a]\nx = 1\n".parse().unwrap();
    let overlay: Table = "a = 5\n".parse().unwrap();
    let merged = deep_merge(base, overlay);
    assert_eq!(merged["a"].as_integer(), Some(5));
}

/// `[[principles]]` arrays merge by `id`: a same-id overlay principle replaces the
/// base entry in place; a new id appends.
#[test]
fn principles_merge_by_id() {
    let base: Table = r#"
[[principles]]
id = "A"
title = "base A"
[[principles]]
id = "B"
title = "base B"
"#
    .parse()
    .unwrap();
    let overlay: Table = r#"
[[principles]]
id = "B"
title = "overlay B"
[[principles]]
id = "C"
title = "overlay C"
"#
    .parse()
    .unwrap();
    let merged = deep_merge(base, overlay);
    let arr = merged["principles"].as_array().unwrap();
    let ids: Vec<&str> = arr.iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["A", "B", "C"], "B replaced in place, C appended");
    // B took the overlay's title.
    let b = arr.iter().find(|p| p["id"].as_str() == Some("B")).unwrap();
    assert_eq!(b["title"].as_str(), Some("overlay B"));
}

/// A principle without a string `id` is appended verbatim rather than matched.
#[test]
fn principles_without_id_are_appended() {
    let base = vec![toml::Value::Table({
        let mut t = Table::new();
        t.insert("id".into(), "A".into());
        t
    })];
    let overlay = vec![toml::Value::Table(Table::new())];
    let merged = merge_principles(base, overlay);
    assert_eq!(merged.len(), 2);
}

/// The shared loader merges `defaults.toml` under `rust.toml` and exposes the
/// Rust principles / spec overrides.
#[test]
fn load_rust_exposes_sections() {
    let cfg = load(include_str!("../languages/rust/config.toml"));

    let ps = principles(&cfg);
    let ids: Vec<&str> = ps.iter().map(|p| p.id.as_str()).collect();
    // The full catalog is inherited from `defaults.toml`: 13 design principles.
    // The metric-lens principles (HK/SLOC/FANIN/FANOUT) were removed — each metric
    // now carries its own prompt doc. Rust adds no own principles.
    assert_eq!(
        ids,
        [
            "CPX", "ADP", "SRP", "OCP", "LSP", "ISP", "DIP", "DRY", "KISS", "LoD", "MISU", "CoI",
            "YAGNI"
        ]
    );
    let kiss = ps.iter().find(|p| p.id == "KISS").unwrap();
    assert_eq!(kiss.sort_metric, "cognitive");

    let specs = spec_overrides(&cfg);
    for k in ["sloc", "lloc", "cloc", "blank"] {
        assert!(
            specs[k]
                .description
                .as_deref()
                .unwrap()
                .contains("#[cfg(test)]"),
            "{k} override mentions cfg(test)"
        );
    }
}

/// `edge_kinds` reads the `[edge_kinds]` table. A language that adds none (here,
/// the empty overlay) inherits exactly the shared `uses` from `defaults.toml`;
/// Rust overrides `uses` and adds its structural kinds.
#[test]
fn edge_kinds_inherit_shared_uses_and_rust_overrides() {
    // Empty overlay → only the shared `uses` (import-dependency wording).
    let base = edge_kinds(&load(""));
    let keys: Vec<&str> = base.keys().map(String::as_str).collect();
    assert_eq!(keys, ["uses"], "shared catalog has only `uses`");
    let uses = &base["uses"];
    assert!(uses.flow);
    assert_eq!(uses.label.as_deref(), Some("uses"));
    assert_eq!(
        uses.description.as_deref(),
        Some("Import dependency \u{2014} this file imports from the other.")
    );

    // Rust overrides `uses` and adds three structural kinds.
    let rust = edge_kinds(&load(include_str!("../languages/rust/config.toml")));
    let mut keys: Vec<&str> = rust.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["contains", "reexports", "super", "uses"]);
    assert!(rust["uses"].flow);
    assert!(
        rust["uses"]
            .description
            .as_deref()
            .unwrap()
            .starts_with("Code dependency"),
        "Rust overrides the shared `uses` description"
    );
    assert!(!rust["contains"].flow);
    assert_eq!(rust["reexports"].label.as_deref(), Some("reexport"));
    assert!(!rust["super"].flow);
}

/// `edge_kind_id` returns the identifier only when the kind is declared in
/// `[edge_kinds]`, so the structure builder can never tag an edge with a kind
/// the level descriptor does not also publish.
#[test]
fn edge_kind_id_validates_against_config() {
    let rust = load(include_str!("../languages/rust/config.toml"));
    assert_eq!(edge_kind_id(&rust, "uses"), Some("uses"));
    assert_eq!(edge_kind_id(&rust, "contains"), Some("contains"));
    assert_eq!(edge_kind_id(&rust, "super"), Some("super"));
    assert_eq!(edge_kind_id(&rust, "nonexistent"), None);
}

/// `node_kinds` reads the `[node_kinds]` table: the shared `function` / `method`
/// from `defaults.toml`, plus a language's own (Rust's `fn`).
#[test]
fn node_kinds_inherit_shared_and_rust_adds_fn() {
    let base = node_kinds(&load(""));
    let mut keys: Vec<&str> = base.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["function", "method"], "shared function/method");
    assert_eq!(base["function"].fill.as_deref(), Some("#dbe9f4"));
    assert_eq!(base["method"].stroke.as_deref(), Some("#5d8a3a"));

    let rust = node_kinds(&load(include_str!("../languages/rust/config.toml")));
    let mut keys: Vec<&str> = rust.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["fn", "function", "method"], "Rust adds `fn`");
    assert_eq!(rust["fn"].label.as_deref(), Some("Function"));
}

/// ECMAScript declares its `arrow` / `generator` function-unit display specs in
/// its own `[node_kinds]` (the single home for ECMAScript vocab), inheriting the
/// shared `function` / `method` from `defaults.toml`.
#[test]
fn node_kinds_ecmascript_adds_arrow_and_generator() {
    let ecma = node_kinds(&load(include_str!("../languages/ecmascript/config.toml")));
    let mut keys: Vec<&str> = ecma.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["arrow", "function", "generator", "method"]);
    assert_eq!(ecma["arrow"].label.as_deref(), Some("Arrow fn"));
    assert_eq!(ecma["arrow"].stroke.as_deref(), Some("#8a5d8a"));
    assert_eq!(ecma["generator"].plural.as_deref(), Some("Generators"));
    assert_eq!(ecma["generator"].fill.as_deref(), Some("#f6e9cf"));
}

/// `units` reads the `[units]` table: the function-unit `kind` id strings a
/// dialect's `fn_kind` emits, keyed by its internal classification slot. There
/// is no shared default, so each language carries its own.
#[test]
fn units_carry_per_language_kind_id_strings() {
    let rust = units(&load(include_str!("../languages/rust/config.toml")));
    assert_eq!(rust["method"], "method");
    assert_eq!(rust["default"], "fn");

    let python = units(&load(include_str!("../languages/python/config.toml")));
    assert_eq!(python["method"], "method");
    assert_eq!(python["default"], "function");

    let ecma = units(&load(include_str!("../languages/ecmascript/config.toml")));
    assert_eq!(ecma["method"], "method");
    assert_eq!(ecma["arrow"], "arrow");
    assert_eq!(ecma["generator"], "generator");
    assert_eq!(ecma["default"], "function");

    assert!(units(&load("")).is_empty(), "no shared [units] default");
}

/// `string_table` reads a free-form `[<section>]` sub-table of string values:
/// the import-graph `[structure]` tree-sitter node-kind vocab the structure
/// builders key on (language-specific, no shared default).
#[test]
fn string_table_reads_structure_node_kinds() {
    let python = string_table(
        &load(include_str!("../languages/python/config.toml")),
        "structure",
    );
    assert_eq!(python["import_statement"], "import_statement");
    assert_eq!(python["import_from_statement"], "import_from_statement");
    assert_eq!(python["dotted_name"], "dotted_name");
    assert_eq!(python["aliased_import"], "aliased_import");

    let ecma = string_table(
        &load(include_str!("../languages/ecmascript/config.toml")),
        "structure",
    );
    assert_eq!(ecma["export_statement"], "export_statement");
    assert_eq!(ecma["call_expression"], "call_expression");
    assert_eq!(ecma["require"], "require");

    assert!(
        string_table(&load(""), "structure").is_empty(),
        "no shared [structure] default"
    );
}

/// `string_list` reads a top-level array-of-strings verbatim (order preserved),
/// and returns empty for an absent or non-array key. These are the file-
/// collection / import-resolution / project-detect / skip DATA lists that drive
/// which files are analyzed and how imports resolve, so order and contents must
/// be transcribed exactly (the e2e goldens depend on them).
#[test]
fn string_list_reads_data_lists_verbatim() {
    let js = load(include_str!("../languages/javascript/config.toml"));
    assert_eq!(string_list(&js, "extensions"), ["js", "jsx", "mjs", "cjs"]);
    assert_eq!(string_list(&js, "detect_markers"), ["package.json"]);

    let ts = load(include_str!("../languages/typescript/config.toml"));
    assert_eq!(string_list(&ts, "extensions"), ["ts", "tsx", "mts", "cts"]);
    // resolution_order is significant: TS-first, then JS fallbacks.
    assert_eq!(
        string_list(&ts, "resolution_order"),
        ["ts", "tsx", "mts", "cts", "js", "jsx"]
    );
    assert_eq!(string_list(&ts, "detect_markers"), ["tsconfig.json"]);

    let py = load(include_str!("../languages/python/config.toml"));
    assert_eq!(string_list(&py, "extensions"), ["py"]);
    assert_eq!(
        string_list(&py, "detect_markers"),
        ["pyproject.toml", "setup.py", "setup.cfg"]
    );
    assert_eq!(
        string_list(&py, "skip_dirs"),
        ["venv", "__pycache__", "node_modules"]
    );

    let rust = load(include_str!("../languages/rust/config.toml"));
    assert_eq!(string_list(&rust, "detect_markers"), ["Cargo.toml"]);

    let ecma = load(include_str!("../languages/ecmascript/config.toml"));
    assert_eq!(
        string_list(&ecma, "skip_dirs"),
        [
            "node_modules",
            "dist",
            "target",
            "build",
            "out",
            ".venv",
            "__pycache__"
        ]
    );
    assert_eq!(
        string_list(&ecma, "skip_suffixes"),
        [
            ".gen.ts",
            ".config.ts",
            ".config.js",
            ".min.js",
            ".min.ts",
            ".umd.js",
            ".bundle.js"
        ]
    );

    // Absent key → empty (no panic).
    assert!(string_list(&rust, "extensions").is_empty());
    assert!(string_list(&load(""), "detect_markers").is_empty());
}

/// The common catalog lives in `defaults.toml` and is inherited by every
/// language; `resolved_principles` returns it (catalog first, language principles
/// appended) with `label = id`. Each `doc_url` resolves to its own
/// `{doc_base}/{doc_lang}/{id}.md` for a language that overrides the id
/// (`doc_overrides`), and to the shared `{doc_base}/base/{id}.md` fallback
/// otherwise.
#[test]
fn resolved_principles_inherit_catalog_and_resolve_doc_urls() {
    let rust = resolved_principles(&load(include_str!("../languages/rust/config.toml")));
    let ids: Vec<&str> = rust.iter().map(|p| p.id.as_str()).collect();
    // The full catalog in `defaults.toml` order: 13 design principles. All come
    // from `defaults.toml` (inherited by every language); Rust adds no own principles.
    // (The metric-lens principles were removed — metrics carry their own docs now.)
    assert_eq!(
        ids,
        [
            "CPX", "ADP", "SRP", "OCP", "LSP", "ISP", "DIP", "DRY", "KISS", "LoD", "MISU", "CoI",
            "YAGNI"
        ]
    );
    // Rust ships a full own corpus (`doc_overrides = "*"`), so every doc_url
    // routes to its own `rust/` folder. label = id.
    let cpx = &rust[0];
    assert_eq!(cpx.label, "CPX");
    assert_eq!(
        cpx.doc_url.as_deref(),
        Some("https://github.com/ffedoroff/code-ranker/blob/main/languages/rust/CPX.md")
    );

    // Languages with no own principles inherit the full 13-entry catalog, and JS
    // shares the TypeScript corpus.
    let py = resolved_principles(&load(include_str!("../languages/python/config.toml")));
    assert_eq!(py.len(), 13);
    assert!(py[0].doc_url.as_deref().unwrap().contains("/python/"));
    let js = resolved_principles(&load(include_str!("../languages/javascript/config.toml")));
    assert_eq!(js.len(), 13);
    assert!(js[0].doc_url.as_deref().unwrap().contains("/typescript/"));

    // A language with no own corpus (no `doc_overrides`) inherits every doc from
    // the shared `base/` fallback — fixing what used to be a dead `/go/` link.
    let go = resolved_principles(&load(include_str!("../languages/go/config.toml")));
    assert_eq!(go.len(), 13);
    assert_eq!(
        go[0].doc_url.as_deref(),
        Some("https://github.com/ffedoroff/code-ranker/blob/main/languages/base/CPX.md")
    );
    assert!(
        go.iter()
            .all(|p| p.doc_url.as_deref().unwrap().contains("/base/")),
        "every go doc_url falls back to base"
    );
}

/// The selective `doc_overrides = ["SRP", …]` form: only the listed ids resolve
/// to the language's own folder; every other principle falls back to `base/`.
#[test]
fn resolved_principles_partial_doc_overrides_route_only_listed_ids() {
    let cfg = load("doc_lang = \"mylang\"\ndoc_overrides = [\"SRP\", \"DIP\"]\n");
    let principles = resolved_principles(&cfg);
    let url = |id: &str| {
        principles
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.doc_url.clone())
            .unwrap()
    };
    // Listed ids route to the language's own folder…
    assert!(url("SRP").contains("/mylang/SRP.md"), "{}", url("SRP"));
    assert!(url("DIP").contains("/mylang/DIP.md"), "{}", url("DIP"));
    // …everything else falls back to the shared base corpus.
    assert!(url("CPX").contains("/base/CPX.md"), "{}", url("CPX"));
    assert!(url("KISS").contains("/base/KISS.md"), "{}", url("KISS"));
}

/// An inherited list is patched in place by an op-table (the list-override DSL);
/// a plain array still replaces it wholesale (the historical behaviour).
#[test]
fn deep_merge_patches_inherited_lists_via_op_table() {
    let strs = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect()
    };
    let base: Table = "xs = [\"a\", \"b\", \"c\"]\nys = [\"keep\"]\n"
        .parse()
        .unwrap();
    let overlay: Table = "xs = { remove = [\"b\"], add = [\"d\"] }\nys = [\"replaced\"]\n"
        .parse()
        .unwrap();
    let merged = deep_merge(base, overlay);
    assert_eq!(
        strs(&merged["xs"]),
        ["a", "c", "d"],
        "op-table mutates in place"
    );
    assert_eq!(strs(&merged["ys"]), ["replaced"], "plain array replaces");
}

/// `principles` merges by id only array-vs-array; a non-array overlay replaces it.
#[test]
fn principles_replaced_by_non_array_overlay() {
    let base: Table = "principles = [{ id = \"a\" }]\n".parse().unwrap();
    let overlay: Table = "principles = \"none\"\n".parse().unwrap();
    let merged = deep_merge(base, overlay);
    assert_eq!(merged["principles"].as_str(), Some("none"));
}

/// Integration: the real Rust config carries the demo `[report]` override — five
/// Halstead-derivative columns dropped, `unsafe` added to both the columns and the
/// JSON `stats` — read via the plugin-api `report_override` over the merged config.
#[test]
fn rust_config_report_override_is_the_demo() {
    let rust = report_override(&load(include_str!("../languages/rust/config.toml")));
    for dropped in ["volume", "effort", "time", "length", "vocabulary"] {
        assert!(
            rust.columns.remove.contains(&dropped.to_string()),
            "rust drops the `{dropped}` column"
        );
    }
    assert!(rust.columns.add.contains(&"unsafe".to_string()));
    assert!(rust.stats.add.contains(&"unsafe".to_string()));
}

/// Static regression guard: no file extension is claimed by two plugins in
/// their default (static) configs. This catches build-time authoring bugs
/// before they become runtime ambiguity errors; Rust has no `extensions` entry
/// (it uses `cargo metadata` for file discovery), which is also fine (empty
/// list → no contribution to the map).
#[test]
fn registry_extensions_are_unique_across_plugins() {
    use std::collections::HashMap;
    let mut ext_to_plugins: HashMap<String, Vec<String>> = HashMap::new();
    for plugin in code_ranker_plugin_api::plugin::registry() {
        let cfg = plugin.config();
        for ext in string_list(&cfg, "extensions") {
            ext_to_plugins
                .entry(ext)
                .or_default()
                .push(plugin.name().to_string());
        }
    }
    let conflicts: Vec<String> = ext_to_plugins
        .iter()
        .filter(|(_, plugins)| plugins.len() > 1)
        .map(|(ext, plugins)| format!(".{ext}: {:?}", plugins))
        .collect();
    assert!(
        conflicts.is_empty(),
        "extension(s) claimed by multiple plugins (built-in default configs): {}",
        conflicts.join(", ")
    );
}
