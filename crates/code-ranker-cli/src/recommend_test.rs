use super::*;
use code_ranker_plugin_api::{attrs::ValueType, level::AttributeSpec};
use std::collections::BTreeMap;

fn node_kind(id: &str, kind: &str, attrs: &[(&str, AttrValue)]) -> Node {
    let mut a: BTreeMap<String, AttrValue> = BTreeMap::new();
    for (k, v) in attrs {
        a.insert((*k).to_string(), v.clone());
    }
    Node {
        id: id.to_string(),
        kind: kind.to_string(),
        name: id.rsplit('/').next().unwrap_or(id).to_string(),
        parent: None,
        attrs: a,
    }
}
fn file_node(id: &str, attrs: &[(&str, AttrValue)]) -> Node {
    node_kind(id, "file", attrs)
}

fn level_with(nodes: Vec<Node>) -> LevelGraph {
    let mut na: BTreeMap<String, AttributeSpec> = BTreeMap::new();
    let mut hk = AttributeSpec::new(ValueType::Float, "HK");
    hk.short = Some("HK".into());
    hk.abbreviate = Some(true);
    hk.thresholds = Some(Thresholds {
        info: 100.0,
        warning: 1000.0,
    });
    na.insert("hk".into(), hk);
    let mut sloc = AttributeSpec::new(ValueType::Int, "SLOC");
    sloc.short = Some("SLOC".into());
    sloc.thresholds = Some(Thresholds {
        info: 50.0,
        warning: 200.0,
    });
    na.insert("sloc".into(), sloc);
    LevelGraph {
        node_attributes: na,
        nodes,
        ..Default::default()
    }
}

#[test]
fn reco_for_sorts_worst_first_and_counts_tiers() {
    let level = level_with(vec![
        file_node(
            "{target}/a.rs",
            &[
                ("hk", AttrValue::Float(2000.0)),
                ("sloc", AttrValue::Int(10)),
            ],
        ),
        file_node(
            "{target}/b.rs",
            &[
                ("hk", AttrValue::Float(150.0)),
                ("sloc", AttrValue::Int(10)),
            ],
        ),
        file_node(
            "{target}/c.rs",
            &[("hk", AttrValue::Float(10.0)), ("sloc", AttrValue::Int(10))],
        ),
        node_kind("ext:x", "external", &[]),
    ]);
    let r = reco_for(&level, "hk");
    // External excluded; worst-first by hk.
    assert_eq!(
        r.sorted.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        vec!["{target}/a.rs", "{target}/b.rs", "{target}/c.rs"]
    );
    assert_eq!(r.warning_count, 1, "only a.rs > 1000");
    assert_eq!(r.info_count, 2, "a.rs and b.rs > 100");
}

#[test]
fn reco_for_cycle_uses_cycle_members() {
    let level = level_with(vec![
        file_node(
            "{target}/a.rs",
            &[
                ("hk", AttrValue::Float(50.0)),
                ("cycle", AttrValue::Str("mutual".into())),
            ],
        ),
        file_node(
            "{target}/b.rs",
            &[
                ("hk", AttrValue::Float(80.0)),
                ("cycle", AttrValue::Str("mutual".into())),
            ],
        ),
        file_node("{target}/c.rs", &[("hk", AttrValue::Float(900.0))]),
    ]);
    let r = reco_for(&level, "cycle");
    assert_eq!(r.warning_count, 2);
    assert_eq!(r.info_count, 2);
    // Ranked by hk: b (80) before a (50).
    assert_eq!(r.sorted[0].id, "{target}/b.rs");
}

#[test]
fn worst_preset_picks_most_violations() {
    let level = level_with(vec![file_node(
        "{target}/a.rs",
        &[
            ("hk", AttrValue::Float(2000.0)),
            ("sloc", AttrValue::Int(10)),
            ("cycle", AttrValue::Str("mutual".into())),
        ],
    )]);
    let presets = vec![
        Preset {
            id: "SRP".into(),
            label: "SRP".into(),
            title: "SRP — x".into(),
            prompt: "p".into(),
            doc_url: None,
            sort_metric: "sloc".into(),
            connections: vec![],
        },
        Preset {
            id: "ADP".into(),
            label: "ADP".into(),
            title: "ADP — x".into(),
            prompt: "p".into(),
            doc_url: None,
            sort_metric: "cycle".into(),
            connections: vec!["common".into()],
        },
    ];
    // SRP: sloc 10 → 0 breaches; ADP: cycle → 1. ADP wins.
    assert_eq!(worst_preset(&level, &presets).as_deref(), Some("ADP"));
}

#[test]
fn compose_prompt_cycle_lists_modules_and_connections() {
    let mut level = level_with(vec![
        file_node(
            "{target}/a.rs",
            &[
                ("hk", AttrValue::Float(50.0)),
                ("cycle", AttrValue::Str("mutual".into())),
            ],
        ),
        file_node(
            "{target}/b.rs",
            &[
                ("hk", AttrValue::Float(80.0)),
                ("cycle", AttrValue::Str("mutual".into())),
            ],
        ),
    ]);
    // The cycle recommendation groups by the level's `cycles` (the SCC groups
    // the pipeline computes), not by per-node attrs.
    level.cycles.push(CycleGroup {
        kind: "mutual".into(),
        nodes: vec!["{target}/a.rs".into(), "{target}/b.rs".into()],
    });
    level.edges.push(code_ranker_plugin_api::edge::Edge {
        source: "{target}/a.rs".into(),
        target: "{target}/b.rs".into(),
        kind: "uses".into(),
        line: None,
        attrs: Default::default(),
    });
    let presets = vec![Preset {
        id: "ADP".into(),
        label: "ADP".into(),
        title: "ADP — Acyclic".into(),
        prompt: "the DAG rule".into(),
        doc_url: Some("http://x/adp.md".into()),
        sort_metric: "cycle".into(),
        connections: vec!["common".into()],
    }];
    let md = compose_prompt(
        &level,
        &presets,
        &code_ranker_graph::prompt_template(),
        "ADP",
        Severity::Auto,
        None,
        &[],
    )
    .unwrap();
    assert!(md.contains("# ADP — Acyclic"), "title heading: {md}");
    assert!(md.contains("## Summary\n\nthe DAG rule"), "summary body");
    assert!(
        md.contains("`code-ranker report --doc ADP`"),
        "offline doc command (id substituted): {md}"
    );
    assert!(
        !md.contains("Full principle:"),
        "no network URL link anymore: {md}"
    );
    assert!(
        md.contains("## Modules in a dependency cycle"),
        "cycle modules section"
    );
    assert!(
        md.contains("- `a.rs`") && md.contains("- `b.rs`"),
        "module paths cleaned: {md}"
    );
    assert!(md.contains("## Connections — common"), "common connections");
    assert!(md.contains("`a.rs` → `b.rs` (uses)"), "edge line");
    assert!(
        md.contains("191019-ADP.md") || md.contains("-ADP.md"),
        "save-report name carries preset id"
    );
}

#[test]
fn cycle_groups_rank_chain_first_then_size() {
    let mut level = level_with(vec![
        file_node("{target}/m1.rs", &[("hk", AttrValue::Float(9.0))]),
        file_node("{target}/m2.rs", &[("hk", AttrValue::Float(1.0))]),
        file_node("{target}/c1.rs", &[("hk", AttrValue::Float(1.0))]),
        file_node("{target}/c2.rs", &[("hk", AttrValue::Float(5.0))]),
        file_node("{target}/c3.rs", &[("hk", AttrValue::Float(2.0))]),
    ]);
    level.cycles = vec![
        CycleGroup {
            kind: "mutual".into(),
            nodes: vec!["{target}/m1.rs".into(), "{target}/m2.rs".into()],
        },
        CycleGroup {
            kind: "chain".into(),
            nodes: vec![
                "{target}/c1.rs".into(),
                "{target}/c2.rs".into(),
                "{target}/c3.rs".into(),
            ],
        },
    ];
    // --top 1 picks the chain (chains rank before mutuals), and lists all of
    // its members ordered by HK (c2 → c3 → c1).
    let top = top_cycle_groups(&level, 1);
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].0.kind, "chain");
    let ids: Vec<&str> = top[0].1.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["{target}/c2.rs", "{target}/c3.rs", "{target}/c1.rs"]);
}

#[test]
fn compose_prompt_metric_orders_and_respects_top() {
    let level = level_with(vec![
        file_node(
            "{target}/a.rs",
            &[
                ("hk", AttrValue::Float(2000.0)),
                ("sloc", AttrValue::Int(300)),
            ],
        ),
        file_node(
            "{target}/b.rs",
            &[
                ("hk", AttrValue::Float(50.0)),
                ("sloc", AttrValue::Int(100)),
            ],
        ),
    ]);
    let presets = vec![Preset {
        id: "SRP".into(),
        label: "SRP".into(),
        title: "SRP — Single".into(),
        prompt: "one reason".into(),
        doc_url: None,
        sort_metric: "sloc".into(),
        connections: vec![],
    }];
    let md = compose_prompt(
        &level,
        &presets,
        &code_ranker_graph::prompt_template(),
        "SRP",
        Severity::Warning,
        Some(1),
        &[],
    )
    .unwrap();
    assert!(
        md.contains("## Target module (SLOC)"),
        "single (--top 1) target heading: {md}"
    );
    assert!(
        md.contains("- `a.rs` (SLOC: 300)"),
        "worst module with value: {md}"
    );
    assert!(
        !md.contains("- `b.rs`"),
        "--top 1 keeps only the worst: {md}"
    );
}

#[test]
fn compose_prompt_unknown_preset_errors() {
    let level = level_with(vec![]);
    let presets = vec![Preset {
        id: "ADP".into(),
        label: "ADP".into(),
        title: "t".into(),
        prompt: "p".into(),
        doc_url: None,
        sort_metric: "cycle".into(),
        connections: vec![],
    }];
    let err = compose_prompt(
        &level,
        &presets,
        &code_ranker_graph::prompt_template(),
        "NOPE",
        Severity::Auto,
        None,
        &[],
    )
    .unwrap_err();
    assert!(format!("{err}").contains("unknown preset 'NOPE'"));
}

#[test]
fn scorecard_shows_principle_and_worst_modules() {
    let level = level_with(vec![
        file_node(
            "{target}/a.rs",
            &[
                ("hk", AttrValue::Float(50.0)),
                ("cycle", AttrValue::Str("mutual".into())),
            ],
        ),
        file_node(
            "{target}/b.rs",
            &[
                ("hk", AttrValue::Float(2000.0)),
                ("sloc", AttrValue::Int(300)),
            ],
        ),
    ]);
    let presets = vec![
        Preset {
            id: "ADP".into(),
            label: "ADP".into(),
            title: "ADP — Acyclic Dependencies".into(),
            prompt: "p".into(),
            doc_url: None,
            sort_metric: "cycle".into(),
            connections: vec![],
        },
        Preset {
            id: "SRP".into(),
            label: "SRP".into(),
            title: "SRP — Single Responsibility".into(),
            prompt: "p".into(),
            doc_url: None,
            sort_metric: "sloc".into(),
            connections: vec![],
        },
    ];
    let sc = render_scorecard(
        "rust",
        &level,
        &presets,
        &[Severity::Warning, Severity::Info],
        None,
        None,
        &[],
    )
    .unwrap();
    assert!(sc.contains("scorecard  (rust, 2 files)"), "header: {sc}");
    assert!(
        sc.contains("ADP") && sc.contains("Acyclic Dependencies"),
        "ADP row"
    );
    assert!(sc.contains("WORST MODULES"), "modules section");
    assert!(
        sc.contains("a.rs") && sc.contains("cycle"),
        "cycle node listed: {sc}"
    );
    assert!(
        sc.contains("b.rs") && sc.contains("HK"),
        "hk breach listed: {sc}"
    );
    assert!(
        sc.contains("→ code-ranker report . --output.prompt.path=… --top 1"),
        "next-step hint"
    );
}

/// A cycle preset for the narrowed-scorecard tests.
fn adp_preset() -> Preset {
    Preset {
        id: "ADP".into(),
        label: "ADP".into(),
        title: "ADP — Acyclic Dependencies".into(),
        prompt: "p".into(),
        doc_url: None,
        sort_metric: "cycle".into(),
        connections: vec![],
    }
}

fn srp_preset() -> Preset {
    Preset {
        id: "SRP".into(),
        label: "SRP".into(),
        title: "SRP — Single Responsibility".into(),
        prompt: "p".into(),
        doc_url: None,
        sort_metric: "sloc".into(),
        connections: vec![],
    }
}

/// Narrowing on a metric preset lists that metric's ranked modules under
/// WORST MODULES (the `narrow.is_some()` non-cycle branch).
#[test]
fn scorecard_narrowed_metric_lists_ranked_modules() {
    let level = level_with(vec![
        file_node("{target}/big.rs", &[("sloc", AttrValue::Int(300))]),
        file_node("{target}/small.rs", &[("sloc", AttrValue::Int(10))]),
    ]);
    let sc = render_scorecard(
        "rust",
        &level,
        &[srp_preset()],
        &[Severity::Warning],
        Some(2),
        Some(&Focus::Metric("sloc".into())),
        &[],
    )
    .unwrap();
    assert!(sc.contains("WORST MODULES"), "modules section: {sc}");
    assert!(
        sc.contains("big.rs") && sc.contains("SLOC 300"),
        "ranked module with metric head: {sc}"
    );
    // Worst-first: big.rs before small.rs.
    assert!(
        sc.find("big.rs") < sc.find("small.rs"),
        "ranked worst-first: {sc}"
    );
}

/// Narrowing on the cycle (ADP) preset lists every member of the top cycle
/// (the `narrow.is_some()` cycle branch), with the "one cycle" header.
#[test]
fn scorecard_narrowed_cycle_lists_all_members() {
    let mut level = level_with(vec![
        file_node(
            "{target}/a.rs",
            &[
                ("hk", AttrValue::Float(80.0)),
                ("cycle", AttrValue::Str("mutual".into())),
            ],
        ),
        file_node(
            "{target}/b.rs",
            &[
                ("hk", AttrValue::Float(50.0)),
                ("cycle", AttrValue::Str("mutual".into())),
            ],
        ),
    ]);
    level.cycles.push(CycleGroup {
        kind: "mutual".into(),
        nodes: vec!["{target}/a.rs".into(), "{target}/b.rs".into()],
    });
    let sc = render_scorecard(
        "rust",
        &level,
        &[adp_preset()],
        &[Severity::Warning],
        None,
        Some(&Focus::Metric("cycle".into())),
        &[],
    )
    .unwrap();
    assert!(
        sc.contains("one cycle (mutual, 2 modules)"),
        "single-cycle header: {sc}"
    );
    assert!(
        sc.contains("a.rs") && sc.contains("b.rs"),
        "all cycle members listed: {sc}"
    );
}

/// An unknown `--focus` name is a hard error naming both namespaces.
#[test]
fn resolve_focus_unknown_name_errors() {
    let level = level_with(vec![file_node("{target}/a.rs", &[])]);
    let err = resolve_focus(&level, &[srp_preset()], "zzz")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("unknown --focus-rule 'zzz'"),
        "names bad focus: {err}"
    );
    assert!(
        err.contains("sloc") && err.contains("cycle"),
        "lists known metrics: {err}"
    );
    assert!(err.contains("SRP"), "lists known principles: {err}");
}

/// `--focus` resolves a metric key (case-insensitive) and a principle id.
#[test]
fn resolve_focus_picks_metric_or_principle() {
    let level = level_with(vec![file_node("{target}/a.rs", &[])]);
    let presets = [srp_preset()];
    assert_eq!(
        resolve_focus(&level, &presets, "HK").unwrap(),
        Focus::Metric("hk".into()),
        "metric key matched case-insensitively"
    );
    assert_eq!(
        resolve_focus(&level, &presets, "SRP").unwrap(),
        Focus::Principle("SRP".into()),
        "principle id matched"
    );
    assert_eq!(
        resolve_focus(&level, &presets, "threshold.file.hk").unwrap(),
        Focus::Metric("hk".into()),
        "full threshold rule id maps to its metric"
    );
}

/// Info-tier breaches: a node over the info line (but under warning) is shown
/// with the ⓘ icon, and a worse metric pushes a co-occurring cycle into the
/// `+rest` list (the non-cycle-worst path).
#[test]
fn scorecard_info_tier_and_cycle_in_rest() {
    let level = level_with(vec![
        // info-only: sloc 80 > info 50, < warning 200.
        file_node("{target}/info.rs", &[("sloc", AttrValue::Int(80))]),
        // warning hk (ratio 2.0) beats the cycle (ratio 1.0) → cycle in +rest.
        file_node(
            "{target}/hot.rs",
            &[
                ("hk", AttrValue::Float(2000.0)),
                ("cycle", AttrValue::Str("mutual".into())),
            ],
        ),
    ]);
    let sc = render_scorecard(
        "rust",
        &level,
        &[srp_preset()],
        &[Severity::Warning, Severity::Info],
        None,
        None,
        &[],
    )
    .unwrap();
    assert!(
        sc.contains("info.rs") && sc.contains("ⓘ"),
        "info icon: {sc}"
    );
    assert!(
        sc.contains("hot.rs") && sc.contains("+cycle"),
        "cycle shown as a secondary breach: {sc}"
    );
}

/// With nothing over the selected tier, the scorecard says so and stops.
#[test]
fn scorecard_reports_no_breaches_when_clean() {
    let level = level_with(vec![file_node(
        "{target}/quiet.rs",
        &[("hk", AttrValue::Float(10.0)), ("sloc", AttrValue::Int(5))],
    )]);
    let sc = render_scorecard(
        "rust",
        &level,
        &[srp_preset()],
        &[Severity::Warning],
        None,
        None,
        &[],
    )
    .unwrap();
    assert!(
        sc.contains("No threshold breaches for the selected severity."),
        "clean report: {sc}"
    );
}

/// A two-cycle level: builds nodes + two `CycleGroup`s, returned ready for the
/// ADP (cycle) preset.
fn two_cycle_level() -> LevelGraph {
    let mut level = level_with(vec![
        file_node(
            "{target}/a.rs",
            &[("cycle", AttrValue::Str("mutual".into()))],
        ),
        file_node(
            "{target}/b.rs",
            &[("cycle", AttrValue::Str("mutual".into()))],
        ),
        file_node(
            "{target}/x.rs",
            &[("cycle", AttrValue::Str("chain".into()))],
        ),
        file_node(
            "{target}/y.rs",
            &[("cycle", AttrValue::Str("chain".into()))],
        ),
        file_node(
            "{target}/z.rs",
            &[("cycle", AttrValue::Str("chain".into()))],
        ),
    ]);
    level.cycles = vec![
        CycleGroup {
            kind: "chain".into(),
            nodes: vec![
                "{target}/x.rs".into(),
                "{target}/y.rs".into(),
                "{target}/z.rs".into(),
            ],
        },
        CycleGroup {
            kind: "mutual".into(),
            nodes: vec!["{target}/a.rs".into(), "{target}/b.rs".into()],
        },
    ];
    level
}

/// `--top 2` on the ADP prompt lists each cycle under its own heading (the
/// multi-cycle branch of `compose_prompt`).
#[test]
fn compose_prompt_lists_multiple_cycles() {
    let level = two_cycle_level();
    let md = compose_prompt(
        &level,
        &[adp_preset()],
        &code_ranker_graph::prompt_template(),
        "ADP",
        Severity::Auto,
        Some(2),
        &[],
    )
    .unwrap();
    assert!(
        md.contains("## 2 dependency cycles"),
        "multi-cycle header: {md}"
    );
    assert!(
        md.contains("### Cycle 1 — chain, 3 modules")
            && md.contains("### Cycle 2 — mutual, 2 modules"),
        "per-cycle headings: {md}"
    );
}

/// Narrowed ADP scorecard with `--top 2` uses the plural "N cycles" header.
#[test]
fn scorecard_narrowed_cycle_top_n_header() {
    let level = two_cycle_level();
    let sc = render_scorecard(
        "rust",
        &level,
        &[adp_preset()],
        &[Severity::Warning],
        Some(2),
        Some(&Focus::Metric("cycle".into())),
        &[],
    )
    .unwrap();
    assert!(
        sc.contains("2 cycles — all members listed:"),
        "header: {sc}"
    );
}

/// Narrowed ADP scorecard when there are no cycles at all → "(none)".
#[test]
fn scorecard_narrowed_cycle_with_none_says_none() {
    let level = level_with(vec![file_node("{target}/a.rs", &[])]);
    let sc = render_scorecard(
        "rust",
        &level,
        &[adp_preset()],
        &[Severity::Warning],
        None,
        Some(&Focus::Metric("cycle".into())),
        &[],
    )
    .unwrap();
    assert!(sc.contains("(none)"), "empty modules list: {sc}");
}

/// A principle name longer than the column width is clipped with an ellipsis.
#[test]
fn scorecard_clips_long_principle_name() {
    let level = level_with(vec![file_node(
        "{target}/a.rs",
        &[("hk", AttrValue::Float(2000.0))],
    )]);
    let preset = Preset {
        id: "LONG".into(),
        label: "LONG".into(),
        title: "LONG — A Very Long Principle Name That Exceeds The Column".into(),
        prompt: "p".into(),
        doc_url: None,
        sort_metric: "hk".into(),
        connections: vec![],
    };
    let sc = render_scorecard(
        "rust",
        &level,
        &[preset],
        &[Severity::Warning],
        None,
        None,
        &[],
    )
    .unwrap();
    assert!(sc.contains('…'), "long name clipped with ellipsis: {sc}");
}

#[test]
fn parse_severity_rejects_garbage() {
    assert_eq!(parse_severity("warning").unwrap(), Severity::Warning);
    assert!(parse_severity("nope").is_err());
}

/// `synth_metric_preset` frames a metric as its own "principle": title from
/// label+name, summary from description, `doc_url` extracted from the remediation
/// URL, and in/out/common connections for a coupling metric (none otherwise).
#[test]
fn synth_metric_preset_frames_metric() {
    let mut hk = AttributeSpec::new(ValueType::Float, "HK");
    hk.short = Some("HK".into());
    hk.name = Some("Henry–Kafura".into());
    hk.description = Some("coupling × size".into());
    hk.group = Some("coupling".into());
    hk.remediation = Some("Download and follow https://x/HK.md please".into());
    let mut sloc = AttributeSpec::new(ValueType::Int, "SLOC");
    sloc.description = Some("source lines".into());
    let mut na: BTreeMap<String, AttributeSpec> = BTreeMap::new();
    na.insert("hk".into(), hk);
    na.insert("sloc".into(), sloc);
    let level = LevelGraph {
        node_attributes: na,
        ..Default::default()
    };

    let p = synth_metric_preset(&level, "hk");
    assert_eq!(p.id, "hk");
    assert_eq!(p.sort_metric, "hk");
    assert_eq!(p.title, "HK — Henry–Kafura");
    assert_eq!(p.prompt, "coupling × size");
    assert_eq!(
        p.doc_url.as_deref(),
        Some("https://x/HK.md"),
        "url from remediation"
    );
    assert_eq!(
        p.connections,
        vec!["in", "out", "common"],
        "coupling → connections"
    );

    let q = synth_metric_preset(&level, "sloc");
    assert_eq!(q.title, "SLOC", "no `name` → title is the label");
    assert!(q.connections.is_empty(), "non-coupling → no connections");
    assert!(q.doc_url.is_none(), "no remediation URL → no doc link");
}

/// The metric lens must not print the metric description twice — once is the
/// Summary (the synth preset's `prompt`), so the modules section drops it.
#[test]
fn compose_prompt_metric_lens_omits_duplicate_description() {
    let desc = "coupling and size, quadratic in fan";
    let mut hk = AttributeSpec::new(ValueType::Float, "HK");
    hk.short = Some("HK".into());
    hk.description = Some(desc.into());
    hk.formula = Some("sloc × (fan_in × fan_out)²".into());
    let mut na: BTreeMap<String, AttributeSpec> = BTreeMap::new();
    na.insert("hk".into(), hk);
    let level = LevelGraph {
        node_attributes: na,
        nodes: vec![file_node("{target}/a.rs", &[("hk", AttrValue::Float(9.0))])],
        ..Default::default()
    };
    let preset = synth_metric_preset(&level, "hk"); // preset.prompt == desc
    let md = compose_prompt(
        &level,
        &[preset],
        &code_ranker_graph::prompt_template(),
        "hk",
        Severity::Auto,
        Some(1),
        &[],
    )
    .unwrap();
    assert_eq!(
        md.matches(desc).count(),
        1,
        "description appears once (Summary only), not again in the modules section: {md}"
    );
    assert!(
        !md.contains("**Formula:**"),
        "formula is dropped from the prompt — it lives in `--doc <id>`: {md}"
    );
}

/// `in_focus` mirrors `check`'s path matching: empty = no restriction; a folder
/// matches everything beneath it; an exact file matches; `./` and trailing `/`
/// normalize; anything outside is excluded.
#[test]
fn in_focus_matches_file_and_folder() {
    let n = file_node("{target}/crates/a/src/lib.rs", &[]);
    assert!(in_focus(&n, &[]), "empty = no restriction");
    assert!(in_focus(&n, &["crates/a".to_string()]), "folder prefix");
    assert!(
        in_focus(&n, &["crates/a/src/lib.rs".to_string()]),
        "exact file"
    );
    assert!(
        in_focus(&n, &["./crates/a/".to_string()]),
        "normalizes ./ and trailing /"
    );
    assert!(!in_focus(&n, &["crates/b".to_string()]), "outside the path");
}

/// A principle focus shows only that preset's row (others hidden) and ranks the
/// worst modules by its `sort_metric`.
#[test]
fn scorecard_focus_principle_shows_only_that_preset() {
    let level = level_with(vec![file_node(
        "{target}/big.rs",
        &[
            ("hk", AttrValue::Float(2000.0)),
            ("sloc", AttrValue::Int(300)),
        ],
    )]);
    let presets = [srp_preset(), adp_preset()];
    let sc = render_scorecard(
        "rust",
        &level,
        &presets,
        &[Severity::Warning, Severity::Info],
        None,
        Some(&Focus::Principle("SRP".into())),
        &[],
    )
    .unwrap();
    assert!(
        sc.contains("SRP") && sc.contains("Single Responsibility"),
        "focused principle row shown: {sc}"
    );
    assert!(!sc.contains("Acyclic"), "other principles hidden: {sc}");
    assert!(
        sc.contains("big.rs"),
        "worst modules ranked by the principle's sort_metric: {sc}"
    );
}

/// `--top 1` reduces the prompt to a single focus module: the connections are
/// rendered in the abbreviated single-focus form — an `out` edge as "line N"
/// (use-site in the focus file, named above) and an `in` edge as `dependant:line`.
#[test]
fn compose_prompt_single_focus_abbreviates_in_and_out_edges() {
    let mut level = level_with(vec![
        file_node("{target}/focus.rs", &[("hk", AttrValue::Float(2000.0))]),
        file_node("{target}/dependant.rs", &[("hk", AttrValue::Float(10.0))]),
        file_node("{target}/dep.rs", &[("hk", AttrValue::Float(5.0))]),
    ]);
    // in: dependant.rs → focus.rs (use-site in dependant); out: focus.rs → dep.rs.
    level.edges.push(code_ranker_plugin_api::edge::Edge {
        source: "{target}/dependant.rs".into(),
        target: "{target}/focus.rs".into(),
        kind: "uses".into(),
        line: Some(7),
        attrs: Default::default(),
    });
    level.edges.push(code_ranker_plugin_api::edge::Edge {
        source: "{target}/focus.rs".into(),
        target: "{target}/dep.rs".into(),
        kind: "uses".into(),
        line: Some(3),
        attrs: Default::default(),
    });
    let preset = Preset {
        id: "HK".into(),
        label: "HK".into(),
        title: "HK — Hotspot".into(),
        prompt: "the hotspot rule".into(),
        doc_url: None,
        sort_metric: "hk".into(),
        connections: vec!["in".into(), "out".into()],
    };
    let md = compose_prompt(
        &level,
        &[preset],
        &code_ranker_graph::prompt_template(),
        "HK",
        Severity::Auto,
        Some(1),
        &[],
    )
    .unwrap();
    assert!(
        md.contains("## Target module (HK)"),
        "single-target heading: {md}"
    );
    // out edge: focus → dep, use-site line in the focus file → "line 3".
    assert!(
        md.contains("`dep.rs` (uses, line 3)"),
        "out edge abbreviated: {md}"
    );
    // in edge: dependant → focus, use-site `dependant.rs:7`.
    assert!(
        md.contains("`dependant.rs:7` (uses)"),
        "in edge abbreviated: {md}"
    );
}
