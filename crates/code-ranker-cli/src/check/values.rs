//! `--suggest-config` current-values dump: render the project's measured per-file
//! maxima and cycle counts as ready-to-paste `code-ranker.toml` blocks. Extracted
//! verbatim from `check.rs` to keep that file's aggregate complexity in budget.

use crate::config;
use code_ranker_graph::level_graph::LevelGraph;
use std::collections::BTreeMap;

/// The six threshold metrics, in display order.
const METRICS: [&str; 6] = ["cyclomatic", "cognitive", "hk", "fan_in", "fan_out", "loc"];

/// Print the current measured values per scope as ready-to-paste `code-ranker.toml`
/// threshold blocks: the per-unit worst value (`single`) and the graph-wide
/// average (`avg`). Lets a user pin today's numbers as a baseline that passes.
pub(super) fn print_current_values(
    graphs: &BTreeMap<String, LevelGraph>,
    cycles: &config::CycleRules,
) {
    let Some(level) = graphs.get("files") else {
        return;
    };
    println!();
    println!("Current config — copy the blocks below into code-ranker.toml:");

    // Cycle budgets: today's count per kind (paste to forbid adding more).
    println!();
    println!(
        "# cycles: max allowed count per kind (today's count — raise only to allow more; false = off)"
    );
    println!("[rules.cycles]");
    for (key, kind, rule) in [
        ("mutual", "mutual", cycles.mutual),
        ("chain", "chain", cycles.chain),
    ] {
        if rule.is_off() {
            println!("{key:<12}= false");
        } else {
            let n = level.cycles.iter().filter(|c| c.kind == kind).count();
            println!("{key:<12}= {n}");
        }
    }

    // Thresholds: measured per-file maxima to pin as a baseline.
    println!();
    println!("# thresholds: the worst single file (max) per metric");
    print_scope_values("file", level);
}

/// Emit a `[rules.thresholds.<scope>]` block with the per-file metric maxima,
/// read from the flat node `attrs`.
fn print_scope_values(scope: &str, level: &LevelGraph) {
    let attr = |n: &code_ranker_plugin_api::node::Node, key: &str| -> f64 {
        match n.attrs.get(key) {
            Some(code_ranker_plugin_api::attrs::AttrValue::Int(i)) => *i as f64,
            Some(code_ranker_plugin_api::attrs::AttrValue::Float(f)) => *f,
            _ => 0.0,
        }
    };
    let mut max = [0f64; 6];
    let mut any = false;
    for n in &level.nodes {
        if n.kind == "external" {
            continue;
        }
        any = true;
        max[0] = max[0].max(attr(n, "cyclomatic"));
        max[1] = max[1].max(attr(n, "cognitive"));
        max[2] = max[2].max(attr(n, "hk"));
        max[3] = max[3].max(attr(n, "fan_in"));
        max[4] = max[4].max(attr(n, "fan_out"));
        max[5] = max[5].max(attr(n, "loc"));
    }
    if !any {
        return;
    }
    print_toml_block(&format!("[rules.thresholds.{scope}]"), &max, false);
}

/// Print one TOML table, one `metric = value` line per non-zero metric. With
/// `round_up`, fractional values (averages) are ceiled so a strict `>` check
/// still passes at the printed limit.
fn print_toml_block(header: &str, vals: &[f64; 6], round_up: bool) {
    let rows: Vec<(&str, u64)> = METRICS
        .iter()
        .zip(vals)
        .filter_map(|(name, &v)| {
            let n = if round_up { v.ceil() } else { v.round() } as u64;
            (n > 0).then_some((*name, n))
        })
        .collect();
    if rows.is_empty() {
        return;
    }
    println!();
    println!("{header}");
    for (name, v) in rows {
        println!("{name:<12}= {}", group_digits(v));
    }
}

/// Format an integer with `_` thousands separators (e.g. 512712 → "512_712"),
/// matching the human number syntax accepted by `--threshold` / the config.
fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let len = s.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push('_');
        }
        out.push(ch);
    }
    out
}
