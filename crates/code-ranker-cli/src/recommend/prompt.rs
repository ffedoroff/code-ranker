//! The AI prompt builder behind the `prompt` report format — the console
//! counterpart of the HTML viewer's Prompt Generator (`composePrompt` +
//! `buildContent`).

use super::{
    Severity, attr_short, clean_path, fmt_val, in_focus, is_internal, num, reco_for, tier_count,
    top_cycle_groups,
};
use anyhow::{Result, bail};
use code_ranker_graph::level_graph::LevelGraph;
use code_ranker_plugin_api::{Principle, PromptTemplate, node::Node};

/// Substitute the template placeholders in one scaffolding line: `{id}` → the
/// active principle/metric id, `{lang}` → the resolved language (so a `docs`
/// pointer reads `code-ranker docs <lang> <id>`).
fn fill(line: &str, id: &str, lang: &str) -> String {
    line.replace("{id}", id).replace("{lang}", lang)
}

/// Compose the AI prompt for one principle — the same Markdown the HTML viewer's
/// Prompt Generator produces: intent + summary + principle link + task checklist,
/// then the ranked offending modules, then the principle's connection lists.
/// `focus_paths` (empty = no restriction) narrows the ranked modules to a subtree.
#[allow(clippy::too_many_arguments)] // a flat prompt-builder signature reads clearer than a params struct here
pub fn compose_prompt(
    level: &LevelGraph,
    principles: &[Principle],
    tmpl: &PromptTemplate,
    principle_id: &str,
    lang: &str,
    sev: Severity,
    top: Option<usize>,
    focus_paths: &[String],
) -> Result<String> {
    let Some(principle) = principles.iter().find(|p| p.id == principle_id) else {
        let known: Vec<&str> = principles.iter().map(|p| p.id.as_str()).collect();
        bail!(
            "unknown principle '{principle_id}'. Known principles: {}",
            known.join(", ")
        );
    };

    let reco = reco_for(level, &principle.sort_metric);
    // For the cycle (ADP) principle the unit is a whole cycle group, not a node:
    // `--top` counts CYCLES (default 1 — the single biggest chain), and every
    // member of each selected cycle is listed. Other principles rank nodes, and
    // the default count = the active tier's size (≥ 1). A cycle is a global unit,
    // so `--focus-path` only narrows the node-ranked (non-cycle) lists.
    let is_cycle = principle.sort_metric == "cycle";
    let cycle_groups = if is_cycle {
        top_cycle_groups(level, top.unwrap_or(1))
    } else {
        Vec::new()
    };
    let modules: Vec<&Node> = if is_cycle {
        cycle_groups
            .iter()
            .flat_map(|(_, members)| members.iter().copied())
            .collect()
    } else {
        let n = top.unwrap_or_else(|| tier_count(&reco, sev).max(1));
        reco.sorted
            .iter()
            .filter(|node| in_focus(node, focus_paths))
            .take(n)
            .copied()
            .collect()
    };

    let mut parts: Vec<String> = Vec::new();

    // 1. Principle intent + summary + link + task protocol.
    // Scaffolding prose (intro / doc-note / task protocol / focus) is DATA from
    // the snapshot's `prompt` template; only the Markdown skeleton + the principle's
    // own title/summary are assembled here. The doc-note points at the offline
    // `code-ranker docs <lang> <id>` command (no network URL).
    let mut head = String::new();
    head.push_str(&format!("# {}\n\n", principle.title));
    head.push_str(&tmpl.intro);
    head.push_str("\n\n## Summary\n\n");
    head.push_str(&principle.prompt);
    head.push_str("\n\n");
    // A doc exists for this principle/metric (signalled by `doc_url`): point the
    // agent at the offline `code-ranker docs <lang> <id>` command rather than a network URL.
    if principle.doc_url.is_some() {
        head.push_str(&fill(&tmpl.doc_note, principle_id, lang));
        head.push_str("\n\n");
    }
    head.push_str("## Task\n\n");
    for line in &tmpl.task {
        head.push_str(&fill(line, principle_id, lang));
        head.push('\n');
    }
    head.push('\n');
    head.push_str(&tmpl.focus);
    parts.push(head);

    // 2. The offending modules, ordered by the principle's metric (or listed as a
    //    cycle for cycle-based principles), each annotated with its value.
    if !modules.is_empty() {
        if is_cycle {
            let mut s = String::new();
            if cycle_groups.len() == 1 {
                let (g, members) = &cycle_groups[0];
                s.push_str(&format!(
                    "## Modules in a dependency cycle ({}, {} modules)\n\n",
                    g.kind,
                    members.len()
                ));
                if !tmpl.cycle_note.is_empty() {
                    s.push_str(&tmpl.cycle_note);
                    s.push_str("\n\n");
                }
                for n in members {
                    s.push_str(&format!("- `{}`\n", clean_path(&n.id)));
                }
            } else {
                s.push_str(&format!(
                    "## {} dependency cycles (every member listed)\n\n",
                    cycle_groups.len()
                ));
                for (i, (g, members)) in cycle_groups.iter().enumerate() {
                    s.push_str(&format!(
                        "### Cycle {} — {}, {} modules\n\n",
                        i + 1,
                        g.kind,
                        members.len()
                    ));
                    for n in members {
                        s.push_str(&format!("- `{}`\n", clean_path(&n.id)));
                    }
                    s.push('\n');
                }
            }
            parts.push(s.trim_end().to_string());
        } else {
            let m = &principle.sort_metric;
            let label = attr_short(level, m);
            // A single target reads as one module, not a ranking; the formula and a
            // repeated description are dropped (they live in `code-ranker docs <lang> <id>`).
            let mut s = if modules.len() == 1 {
                format!("## Target module ({label})\n\n")
            } else {
                format!("## Modules ordered by {label}\n\n")
            };
            if let Some(spec) = level.node_attributes.get(m) {
                // Skip the metric description when it already appears verbatim as the
                // Summary above — true for the metric lens, whose summary IS the
                // metric's description, so it would otherwise print twice.
                if let Some(d) = &spec.description
                    && d != &principle.prompt
                {
                    s.push_str(d);
                    s.push_str("\n\n");
                }
            }
            for n in &modules {
                match num(n, m) {
                    Some(v) if v != 0.0 => s.push_str(&format!(
                        "- `{}` ({label}: {})\n",
                        clean_path(&n.id),
                        fmt_val(v)
                    )),
                    _ => s.push_str(&format!("- `{}`\n", clean_path(&n.id))),
                }
            }
            parts.push(s.trim_end().to_string());
        }
    }

    // 3. The principle's connection lists (only those with edges), endpoints as paths.
    let module_ids: std::collections::HashSet<&str> =
        modules.iter().map(|n| n.id.as_str()).collect();
    let internal: std::collections::HashSet<&str> = level
        .nodes
        .iter()
        .filter(|n| is_internal(n))
        .map(|n| n.id.as_str())
        .collect();
    // Only FLOW edges (`uses`) drive coupling/cycles and HK — structural
    // `contains`/`reexports` are noise in the crossroads, so the prompt lists the
    // same edge set the metrics are computed over.
    // Match the viewer's `edgeIsFlow` (`flow !== false`): a kind is flow unless the
    // dictionary marks it `flow = false` (`contains`/`reexports`).
    let is_flow = |kind: &str| level.edge_kinds.get(kind).map(|k| k.flow).unwrap_or(true);
    let local_edges: Vec<&code_ranker_plugin_api::edge::Edge> = level
        .edges
        .iter()
        .filter(|e| {
            internal.contains(e.source.as_str())
                && internal.contains(e.target.as_str())
                && is_flow(&e.kind)
        })
        .collect();

    // The declaration line of a `uses`/`reexports` edge lives in its **source**
    // file (where the `use`/import is written), so always anchor `:line` there.
    // With a single focus module, drop the endpoint that *is* the focus: an `in`
    // edge's use-site is the dependant (`dependant:line`, focus is the implied
    // target); an `out` edge's use-site is the focus itself, so it carries the line
    // and the target is shown (`focus:line → target`). With several focus modules,
    // keep the full `source → target` form so each endpoint reads.
    let single = module_ids.len() == 1;
    let edge_line = |e: &code_ranker_plugin_api::edge::Edge| {
        if single && module_ids.contains(e.source.as_str()) {
            // out: focus → target. The use-site is in the focus file (named above),
            // so report the line as "line N" rather than repeating the focus path.
            let at = e.line.map(|l| format!(", line {l}")).unwrap_or_default();
            format!("- `{}` ({}{})", clean_path(&e.target), e.kind, at)
        } else if single {
            // in: dependant → focus; use-site is in the dependant (`dependant:line`).
            let at = e.line.map(|l| format!(":{l}")).unwrap_or_default();
            format!("- `{}{}` ({})", clean_path(&e.source), at, e.kind)
        } else {
            // multi-focus: keep both endpoints; line anchored on the source use-site.
            let at = e.line.map(|l| format!(":{l}")).unwrap_or_default();
            format!(
                "- `{}{}` → `{}` ({})",
                clean_path(&e.source),
                at,
                clean_path(&e.target),
                e.kind
            )
        }
    };
    let push_conn =
        |parts: &mut Vec<String>, title: &str, edges: Vec<&code_ranker_plugin_api::edge::Edge>| {
            if edges.is_empty() {
                return;
            }
            let mut s = format!("## Connections — {title}\n\n");
            s.push_str(
                &edges
                    .iter()
                    .map(|e| edge_line(e))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            parts.push(s);
        };

    let wants = |c: &str| principle.connections.iter().any(|x| x == c);
    if wants("common") {
        let inner: Vec<_> = local_edges
            .iter()
            .copied()
            .filter(|e| {
                module_ids.contains(e.source.as_str()) && module_ids.contains(e.target.as_str())
            })
            .collect();
        push_conn(&mut parts, "common", inner);
    }
    if wants("in") {
        let ins: Vec<_> = local_edges
            .iter()
            .copied()
            .filter(|e| {
                module_ids.contains(e.target.as_str()) && !module_ids.contains(e.source.as_str())
            })
            .collect();
        push_conn(&mut parts, "in", ins);
    }
    if wants("out") {
        let outs: Vec<_> = local_edges
            .iter()
            .copied()
            .filter(|e| {
                module_ids.contains(e.source.as_str()) && !module_ids.contains(e.target.as_str())
            })
            .collect();
        push_conn(&mut parts, "out", outs);
    }

    let mut out = parts.join("\n\n");
    out.push('\n');
    Ok(out)
}
