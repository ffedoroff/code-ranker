//! Tests for `markdown/structure.rs` (wired via `#[path]` from that source).

use super::*;
use std::fs;

#[test]
fn scan_link_dests_extracts_one_line() {
    let dests = scan_link_dests(
        "See [guide](./guide.md), [ext](https://x.com), [t](other.md \"hi\"), [a](#top).",
    );
    assert!(dests.contains(&"./guide.md".to_string()));
    assert!(dests.contains(&"https://x.com".to_string()));
    assert!(dests.contains(&"other.md".to_string()));
    // A pure `#anchor` strips to empty and is not a destination.
    assert!(!dests.iter().any(|d| d.starts_with('#')));
}

#[test]
fn link_graph_edges_local_md_only() {
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join("index.md"),
        "# Index\nSee [guide](guide.md) and [site](https://x.com).\n",
    )
    .unwrap();
    fs::write(
        d.path().join("guide.md"),
        "# Guide\nback to [index](index.md)\n",
    )
    .unwrap();

    let g = analyze(d.path()).unwrap();
    assert_eq!(
        g.nodes
            .iter()
            .filter(|n| n.kind == code_ranker_plugin_api::node::FILE)
            .count(),
        2
    );
    assert!(
        g.edges
            .iter()
            .any(|e| e.source.ends_with("index.md") && e.target.ends_with("guide.md")),
        "index → guide link edge"
    );
    // The external URL is not turned into a node/edge.
    assert!(
        g.nodes
            .iter()
            .all(|n| n.kind != code_ranker_plugin_api::node::EXTERNAL)
    );
}

#[test]
fn counts_headings_and_fenced_code_lines() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("doc.md"),
        "# Title\n\n## Section\n\n```rust\nlet a = 1;\nlet b = 2;\n```\n\nNot [code](https://x.com).\n",
    )
    .unwrap();
    let g = analyze(d.path()).unwrap();
    let n = g.nodes.iter().find(|n| n.name == "doc.md").unwrap();
    let int = |k: &str| match n.attrs.get(k) {
        Some(code_ranker_plugin_api::attrs::AttrValue::Int(v)) => *v,
        _ => 0,
    };
    assert_eq!(int("headings"), 2, "# and ##");
    assert_eq!(int("max_depth"), 2);
    assert_eq!(int("code_lines"), 2, "two lines inside the fence");
}

#[test]
fn non_md_and_subdir_links() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("docs")).unwrap();
    std::fs::write(d.path().join("docs/page.md"), "# Page\n").unwrap();
    // a non-.md link (ignored) and a sub-directory .md link (resolved relative).
    std::fs::write(
        d.path().join("index.md"),
        "# Index\n![logo](logo.png) and the [page](docs/page.md).\n",
    )
    .unwrap();

    let g = analyze(d.path()).unwrap();
    assert!(
        g.edges
            .iter()
            .any(|e| e.source.ends_with("index.md") && e.target.ends_with("docs/page.md")),
        "subdir .md link resolves to an edge"
    );
    // logo.png is not .md → no edge for it (only the one page.md edge).
    assert_eq!(g.edges.len(), 1, "the .png link is ignored");
}

#[test]
fn scan_link_dests_stops_on_unterminated_link() {
    // A `](` with no closing `)` ends the scan (`else break`); a complete link
    // before it is still captured.
    let dests = scan_link_dests("ok [a](a.md) then broken [b](unterminated");
    assert_eq!(dests, vec!["a.md".to_string()]);
    assert!(scan_link_dests("[x](no-close").is_empty());
}

#[test]
fn existing_non_md_link_is_local_ok_not_broken() {
    // A non-.md link whose target file exists on disk classifies as `LocalOk`
    // (a valid local asset) — it is neither an edge nor a broken link.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("logo.png"), [0x89u8, b'P', b'N', b'G']).unwrap();
    std::fs::write(d.path().join("index.md"), "# Index\n![logo](logo.png)\n").unwrap();
    let g = analyze(d.path()).unwrap();
    let n = g.nodes.iter().find(|n| n.name == "index.md").unwrap();
    let broken = match n.attrs.get("broken_links") {
        Some(code_ranker_plugin_api::attrs::AttrValue::Int(v)) => *v,
        _ => 0,
    };
    assert_eq!(broken, 0, "an existing local asset is not broken");
    assert!(g.edges.is_empty(), "a non-.md asset is not a graph edge");
}

#[test]
fn non_utf8_markdown_file_is_skipped() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("ok.md"), "# ok\n").unwrap();
    std::fs::write(d.path().join("bad.md"), [0xFFu8, 0xFE, 0x00]).unwrap();
    let g = analyze(d.path()).unwrap();
    assert!(g.nodes.iter().any(|n| n.name == "ok.md"));
    assert!(
        g.nodes.iter().all(|n| n.name != "bad.md"),
        "non-UTF8 md skipped"
    );
}
