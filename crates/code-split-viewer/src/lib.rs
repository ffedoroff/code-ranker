//! Self-contained HTML viewer for Code Split: embeds one or two snapshots into
//! a single interactive HTML file (no CDN, no external requests), and extracts
//! a snapshot back out of a generated report.

use anyhow::{Context, Result};
use code_split_graph::Snapshot;

/// Pull the JSON out of `<script type="application/json" id="{id}">…</script>`
/// and parse it into a `Snapshot`. Returns `None` if the tag is absent or holds
/// `null`.
pub fn extract_embedded_snapshot(html: &str, id: &str) -> Option<Result<Snapshot>> {
    let needle = format!("id=\"{id}\">");
    let start = html.find(&needle)? + needle.len();
    let end = start + html[start..].find("</script>")?;
    let body = html[start..end].trim();
    if body.is_empty() || body == "null" {
        return None;
    }
    // Undo the `</` → `<\/` escaping applied when embedding.
    let json = body.replace("<\\/", "</");
    Some(serde_json::from_str(&json).with_context(|| format!("parsing embedded snapshot `{id}`")))
}

// ── Assets embedded at compile time ──────────────────────────────────────────
const ASSET_CSS: &str = include_str!("assets/index.css");
const ASSET_GV: &str = include_str!("assets/graphviz.umd.js");
const ASSET_SNARKDOWN: &str = include_str!("assets/snarkdown.umd.js");
const ASSET_SCHEMA: &str = include_str!("assets/schema.js");
const ASSET_DIFF: &str = include_str!("assets/diff.js");
const ASSET_LAYOUT: &str = include_str!("assets/layout.js");
const ASSET_UTILS: &str = include_str!("assets/utils.js");
const ASSET_MODAL: &str = include_str!("assets/modal.js");
const ASSET_PANZOOM: &str = include_str!("assets/panzoom.js");
const ASSET_DIAGRAM: &str = include_str!("assets/diagram.js");
const ASSET_UI: &str = include_str!("assets/ui.js");
const ASSET_SUMMARY: &str = include_str!("assets/summary.js");
const ASSET_EXPORT_POPUP: &str = include_str!("assets/export-popup.js");
const ASSET_NODE_TABLE: &str = include_str!("assets/node-table.js");
const ASSET_NAV: &str = include_str!("assets/nav.js");
const ASSET_APP: &str = include_str!("assets/app.js");
const ASSET_HTML: &str = include_str!("assets/index.html");

/// Render a self-contained viewer with the snapshot data embedded inline. The
/// snapshots are stored in `<script type="application/json">` tags
/// (`cs-baseline` / `cs-current`) so they can be both read by the viewer and
/// extracted from the HTML later (see [`extract_embedded_snapshot`]).
/// `current` only → review; both → diff.
pub fn render_html_viewer(baseline: Option<&Snapshot>, current: Option<&Snapshot>) -> String {
    // Embed as JSON in a typed script tag. Escape `</` so an embedded string can never
    // close the tag early; `JSON.parse` and serde both read `<\/` back as `</`.
    let embed = |id: &str, snap: Option<&Snapshot>| {
        let json = match snap {
            Some(s) => code_split_graph::to_canonical_string(s).expect("serialize snapshot"),
            None => "null".to_string(),
        };
        format!(
            "<script type=\"application/json\" id=\"{id}\">{}</script>",
            json.replace("</", "<\\/")
        )
    };
    let data_script = format!(
        "{}\n{}",
        embed("cs-baseline", baseline),
        embed("cs-current", current),
    );

    ASSET_HTML
        .replace(
            r#"<link rel="stylesheet" href="./index.css">"#,
            &format!("<style>{}</style>", ASSET_CSS),
        )
        .replace(
            r#"<script src="./graphviz.umd.js"></script>"#,
            &format!("<script>{}</script>", ASSET_GV),
        )
        .replace(
            r#"<script src="./snarkdown.umd.js"></script>"#,
            &format!("<script>{}</script>", ASSET_SNARKDOWN),
        )
        .replace(r#"<script src="./data.js"></script>"#, &data_script)
        .replace(
            r#"<script src="./schema.js"></script>"#,
            &format!("<script>{}</script>", ASSET_SCHEMA),
        )
        .replace(
            r#"<script src="./diff.js"></script>"#,
            &format!("<script>{}</script>", ASSET_DIFF),
        )
        .replace(
            r#"<script src="./layout.js"></script>"#,
            &format!("<script>{}</script>", ASSET_LAYOUT),
        )
        .replace(
            r#"<script src="./utils.js"></script>"#,
            &format!("<script>{}</script>", ASSET_UTILS),
        )
        .replace(
            r#"<script src="./modal.js"></script>"#,
            &format!("<script>{}</script>", ASSET_MODAL),
        )
        .replace(
            r#"<script src="./panzoom.js"></script>"#,
            &format!("<script>{}</script>", ASSET_PANZOOM),
        )
        .replace(
            r#"<script src="./diagram.js"></script>"#,
            &format!("<script>{}</script>", ASSET_DIAGRAM),
        )
        .replace(
            r#"<script src="./ui.js"></script>"#,
            &format!("<script>{}</script>", ASSET_UI),
        )
        .replace(
            r#"<script src="./summary.js"></script>"#,
            &format!("<script>{}</script>", ASSET_SUMMARY),
        )
        .replace(
            r#"<script src="./export-popup.js"></script>"#,
            &format!("<script>{}</script>", ASSET_EXPORT_POPUP),
        )
        .replace(
            r#"<script src="./node-table.js"></script>"#,
            &format!("<script>{}</script>", ASSET_NODE_TABLE),
        )
        .replace(
            r#"<script src="./nav.js"></script>"#,
            &format!("<script>{}</script>", ASSET_NAV),
        )
        .replace(
            r#"<script src="./app.js"></script>"#,
            &format!("<script>{}</script>", ASSET_APP),
        )
}
