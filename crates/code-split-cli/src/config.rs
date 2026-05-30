use anyhow::{Context, Result};
use code_split_core::graph::{CycleKind, Graph};
use code_split_core::snapshot::PluginGraphs;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

// ── Config structs ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// Default plugin name (e.g. "rust", "python"). Overridden by --plugin.
    pub plugin: Option<String>,
    pub ignore: IgnoreConfig,
    pub rules: RulesConfig,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct IgnoreConfig {
    pub paths: Vec<String>,
    /// Strip all inline `mod tests { … }` submodules (IDs ending with `::tests`).
    pub test_modules: bool,
    /// Strip crates that appear only in [dev-dependencies], never in [dependencies].
    pub dev_only_crates: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct RulesConfig {
    pub cycles: CycleRules,
    pub thresholds: ThresholdRules,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct CycleRules {
    /// Each cycle kind is either enabled (a cycle of that kind is a violation and
    /// fails `check`) or disabled (stripped from the snapshot, not reported).
    #[serde(rename = "test-embed")]
    pub test_embed: bool,
    pub mutual: bool,
    pub chain: bool,
}

impl Default for CycleRules {
    fn default() -> Self {
        Self {
            test_embed: false,
            mutual: true,
            chain: true,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct ThresholdRules {
    /// Per-node: flag any single node whose metric exceeds the limit.
    pub node: MetricThresholds,
    /// Graph-average: flag when the graph-wide average exceeds the limit.
    pub avg: MetricThresholds,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct MetricThresholds {
    pub hk: Option<f64>,
    pub cyclomatic: Option<f64>,
    pub cognitive: Option<f64>,
    pub fan_in: Option<f64>,
    pub fan_out: Option<f64>,
    pub loc: Option<f64>,
}

// ── Loading ────────────────────────────────────────────────────────────────────
//
// Priority (highest wins):
//   1. CLI flags   --ignore / --cycle-rule / --threshold
//   2. --config <file>  (explicit path)
//   3. code-split.toml     (cwd, then workspace root)
//   4. Cargo.toml       [workspace.metadata.code-split] or [package.metadata.code-split]
//   5. Built-in defaults

/// Loaded config together with the file it came from (for snapshot recording).
pub struct LoadedConfig {
    pub config: Config,
    /// Canonical path of the file that was used, if any.
    pub source_file: Option<String>,
}

pub fn load(
    workspace: &Path,
    config_entries: &[String],
    ignore_paths: &[String],
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<LoadedConfig> {
    // A `--config` entry is an inline `KEY=VALUE` override if it contains '=',
    // otherwise it is a path to a config file.
    let mut inline: Vec<&str> = Vec::new();
    let mut files: Vec<&str> = Vec::new();
    for e in config_entries {
        if e.contains('=') {
            inline.push(e);
        } else {
            files.push(e);
        }
    }
    let explicit = files.first().copied().map(Path::new);

    let (mut config, source_file) = load_file(workspace, explicit)?;
    apply_inline_overrides(&mut config, &inline)?;
    apply_cli_overrides(&mut config, ignore_paths, cycle_rules, thresholds)?;
    Ok(LoadedConfig {
        config,
        source_file,
    })
}

fn load_file(workspace: &Path, explicit: Option<&Path>) -> Result<(Config, Option<String>)> {
    // 1. Explicit --config
    if let Some(path) = explicit {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        return Ok((cfg, Some(path.display().to_string())));
    }

    let cwd = std::env::current_dir().unwrap_or_default();

    // 2. code-split.toml in cwd, then workspace root
    for dir in [cwd.as_path(), workspace] {
        let p = dir.join("code-split.toml");
        if p.exists() {
            let text =
                std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
            let cfg = toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))?;
            let canonical = p.canonicalize().unwrap_or(p);
            return Ok((cfg, Some(canonical.display().to_string())));
        }
    }

    // 3. Cargo.toml [workspace.metadata.code-split] / [package.metadata.code-split]
    for dir in [cwd.as_path(), workspace] {
        if let Some((cfg, src)) = load_from_cargo_toml(dir)? {
            return Ok((cfg, Some(src)));
        }
    }

    Ok((Config::default(), None))
}

fn load_from_cargo_toml(dir: &Path) -> Result<Option<(Config, String)>> {
    let cargo = dir.join("Cargo.toml");
    if !cargo.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&cargo).with_context(|| format!("reading {}", cargo.display()))?;
    let val: toml::Value =
        toml::from_str(&text).with_context(|| format!("parsing {}", cargo.display()))?;

    let section = val
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("code-split"))
        .or_else(|| {
            val.get("package")
                .and_then(|p| p.get("metadata"))
                .and_then(|m| m.get("code-split"))
        });

    if let Some(v) = section {
        let cfg: Config = v
            .clone()
            .try_into()
            .with_context(|| format!("parsing [*.metadata.code-split] in {}", cargo.display()))?;
        let canonical = cargo.canonicalize().unwrap_or(cargo);
        return Ok(Some((
            cfg,
            format!("{}#metadata.code-split", canonical.display()),
        )));
    }
    Ok(None)
}

// ── CLI overrides ──────────────────────────────────────────────────────────────

fn apply_cli_overrides(
    cfg: &mut Config,
    ignore_paths: &[String],
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<()> {
    cfg.ignore.paths.extend_from_slice(ignore_paths);

    for raw in cycle_rules {
        // Format: "kind=on|off", e.g. "test-embed=on"
        let (kind, state) = split_kv(raw, "cycle-rule")?;
        set_cycle(cfg, kind, parse_on_off(state)?)?;
    }

    for raw in thresholds {
        // Format: "scope.metric=N", e.g. "node.hk=500000" or "avg.cyclomatic=10"
        let (scope_metric, val_str) = split_kv(raw, "threshold")?;
        let val: f64 = val_str
            .parse()
            .with_context(|| format!("threshold value must be a number: {raw}"))?;
        let (scope, metric) = scope_metric
            .split_once('.')
            .with_context(|| format!("threshold must be scope.metric=N, got: {raw}"))?;
        set_threshold(cfg, scope, metric, val)?;
    }

    Ok(())
}

/// Apply `--config KEY=VALUE` inline overrides, where KEY is a dotted config key
/// (e.g. `rules.thresholds.node.cognitive=25`, `rules.cycles.mutual=on`, `plugin=rust`).
fn apply_inline_overrides(cfg: &mut Config, entries: &[&str]) -> Result<()> {
    for raw in entries {
        let (key, value) = raw
            .split_once('=')
            .with_context(|| format!("--config override must be KEY=VALUE, got: {raw}"))?;
        match key {
            "plugin" => cfg.plugin = Some(value.to_string()),
            "ignore.test_modules" => cfg.ignore.test_modules = parse_on_off(value)?,
            "ignore.dev_only_crates" => cfg.ignore.dev_only_crates = parse_on_off(value)?,
            "ignore.paths" => cfg
                .ignore
                .paths
                .extend(value.split(',').map(|s| s.trim().to_string())),
            _ if key.strip_prefix("rules.cycles.").is_some() => {
                let kind = key.strip_prefix("rules.cycles.").unwrap();
                set_cycle(cfg, kind, parse_on_off(value)?)?;
            }
            _ if key.strip_prefix("rules.thresholds.").is_some() => {
                let rest = key.strip_prefix("rules.thresholds.").unwrap();
                let (scope, metric) = rest.split_once('.').with_context(|| {
                    format!("threshold key must be rules.thresholds.SCOPE.METRIC, got: {key}")
                })?;
                let val: f64 = value
                    .parse()
                    .with_context(|| format!("threshold value must be a number: {raw}"))?;
                set_threshold(cfg, scope, metric, val)?;
            }
            other => anyhow::bail!("unknown config key {other:?}"),
        }
    }
    Ok(())
}

fn set_cycle(cfg: &mut Config, kind: &str, enabled: bool) -> Result<()> {
    match kind {
        "test-embed" => cfg.rules.cycles.test_embed = enabled,
        "mutual" => cfg.rules.cycles.mutual = enabled,
        "chain" => cfg.rules.cycles.chain = enabled,
        other => anyhow::bail!("unknown cycle kind {other:?}; expected test-embed|mutual|chain"),
    }
    Ok(())
}

fn set_threshold(cfg: &mut Config, scope: &str, metric: &str, val: f64) -> Result<()> {
    let bucket = match scope {
        "node" => &mut cfg.rules.thresholds.node,
        "avg" => &mut cfg.rules.thresholds.avg,
        other => anyhow::bail!("unknown threshold scope {other:?}; expected node|avg"),
    };
    match metric {
        "hk" => bucket.hk = Some(val),
        "cyclomatic" => bucket.cyclomatic = Some(val),
        "cognitive" => bucket.cognitive = Some(val),
        "fan_in" => bucket.fan_in = Some(val),
        "fan_out" => bucket.fan_out = Some(val),
        "loc" => bucket.loc = Some(val),
        other => anyhow::bail!(
            "unknown metric {other:?}; expected hk|cyclomatic|cognitive|fan_in|fan_out|loc"
        ),
    }
    Ok(())
}

fn split_kv<'a>(s: &'a str, flag: &str) -> Result<(&'a str, &'a str)> {
    s.split_once('=')
        .with_context(|| format!("--{flag} must be key=value, got: {s}"))
}

fn parse_on_off(s: &str) -> Result<bool> {
    match s {
        "on" | "true" => Ok(true),
        "off" | "false" => Ok(false),
        other => anyhow::bail!("expected on|off, got {:?}", other),
    }
}

// ── Path filtering ─────────────────────────────────────────────────────────────

pub fn apply_ignore(
    graphs: &mut PluginGraphs,
    ignore: &IgnoreConfig,
    target: &Path,
) -> Result<usize> {
    let gs = if ignore.paths.is_empty() {
        None
    } else {
        Some(build_glob_set(&ignore.paths)?)
    };
    let dev_only = if ignore.dev_only_crates {
        collect_dev_only_crates(target)
    } else {
        HashSet::new()
    };
    if gs.is_none() && !ignore.test_modules && dev_only.is_empty() {
        return Ok(0);
    }
    Ok(filter_graph(
        &mut graphs.modules,
        gs.as_ref(),
        ignore.test_modules,
        &dev_only,
    ) + filter_graph(
        &mut graphs.files,
        gs.as_ref(),
        ignore.test_modules,
        &dev_only,
    ) + filter_graph(
        &mut graphs.functions,
        gs.as_ref(),
        ignore.test_modules,
        &dev_only,
    ))
}

// ── Dev-only crate detection ───────────────────────────────────────────────────

/// Returns names of crates that are only reachable via dev-dependency edges
/// in the full transitive dependency graph (via `cargo metadata`).
fn collect_dev_only_crates(target: &Path) -> HashSet<String> {
    let out = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(target)
        .stderr(std::process::Stdio::null())
        .output()
        .expect("cargo metadata failed — is cargo installed?");
    assert!(
        out.status.success(),
        "cargo metadata exited with {}",
        out.status
    );

    let meta: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("cargo metadata produced invalid JSON");

    // id → package name
    let packages = meta["packages"].as_array().expect("packages array");
    let mut id_to_name: HashMap<&str, &str> = HashMap::new();
    for pkg in packages {
        if let (Some(id), Some(name)) = (pkg["id"].as_str(), pkg["name"].as_str()) {
            id_to_name.insert(id, name);
        }
    }

    // workspace member ids
    let workspace_members: HashSet<&str> = meta["workspace_members"]
        .as_array()
        .expect("workspace_members array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    // adjacency: pkg_id → [(dep_pkg_id, dev_only_edge)]
    // An edge is dev-only when every dep_kind has kind == "dev"
    // (kind == null means a normal runtime dependency).
    let nodes = meta["resolve"]["nodes"]
        .as_array()
        .expect("resolve.nodes array");
    let mut adj: HashMap<&str, Vec<(&str, bool)>> = HashMap::new();
    for node in nodes {
        let Some(id) = node["id"].as_str() else {
            continue;
        };
        let Some(deps) = node["deps"].as_array() else {
            continue;
        };
        let edges = deps
            .iter()
            .filter_map(|dep| {
                let dep_id = dep["pkg"].as_str()?;
                let kinds = dep["dep_kinds"].as_array()?;
                let dev_only = kinds.iter().all(|k| k["kind"].as_str() == Some("dev"));
                Some((dep_id, dev_only))
            })
            .collect();
        adj.insert(id, edges);
    }

    // BFS from workspace members following only non-dev edges.
    let mut regular: HashSet<&str> = workspace_members.iter().copied().collect();
    let mut queue: VecDeque<&str> = regular.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        for &(dep_id, dev_only) in adj.get(id).map(Vec::as_slice).unwrap_or(&[]) {
            if !dev_only && regular.insert(dep_id) {
                queue.push_back(dep_id);
            }
        }
    }

    // Everything in the graph but not regularly reachable is dev-only.
    adj.keys()
        .filter(|&&id| !regular.contains(id))
        .filter_map(|&id| id_to_name.get(id).map(|n| n.to_string()))
        .collect()
}

fn build_glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p).with_context(|| format!("invalid glob: {p}"))?);
    }
    Ok(b.build()?)
}

// Paths are stored as "{root}/sub/path" after relativize; strip the "{…}/" prefix.
fn strip_root_prefix(path: &str) -> &str {
    if path.starts_with('{')
        && let Some(idx) = path.find('}')
    {
        return path[idx + 1..].trim_start_matches('/');
    }
    path
}

fn filter_graph(
    graph: &mut Graph,
    gs: Option<&GlobSet>,
    test_modules: bool,
    dev_only: &HashSet<String>,
) -> usize {
    let removed: HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| {
            if let Some(gs) = gs
                && gs.is_match(strip_root_prefix(&n.path))
            {
                return true;
            }
            if test_modules && n.id.ends_with("::tests") {
                return true;
            }
            if !dev_only.is_empty() {
                // ID format after rewriting: "crate:rstest" or "crate:rstest@1.0.0"
                if let Some(crate_name) = n.id.strip_prefix("crate:") {
                    let base = crate_name.split('@').next().unwrap_or(crate_name);
                    if dev_only.contains(base) {
                        return true;
                    }
                }
            }
            false
        })
        .map(|n| n.id.clone())
        .collect();
    if removed.is_empty() {
        return 0;
    }
    let before = graph.nodes.len();
    graph.nodes.retain(|n| !removed.contains(&n.id));
    graph
        .edges
        .retain(|e| !removed.contains(&e.from) && !removed.contains(&e.to));
    for cg in &mut graph.cycles {
        cg.nodes.retain(|id| !removed.contains(id));
    }
    graph.cycles.retain(|cg| cg.nodes.len() >= 2);
    before - graph.nodes.len()
}

// ── Cycle rules ────────────────────────────────────────────────────────────────

pub fn apply_cycle_rules(graphs: &mut PluginGraphs, rules: &CycleRules) {
    apply_cycle_rules_graph(&mut graphs.modules, rules);
    apply_cycle_rules_graph(&mut graphs.files, rules);
    apply_cycle_rules_graph(&mut graphs.functions, rules);
}

fn apply_cycle_rules_graph(graph: &mut Graph, rules: &CycleRules) {
    let disabled: HashSet<CycleKind> = [
        (CycleKind::TestEmbed, rules.test_embed),
        (CycleKind::Mutual, rules.mutual),
        (CycleKind::Chain, rules.chain),
    ]
    .into_iter()
    .filter(|(_, enabled)| !*enabled)
    .map(|(k, _)| k)
    .collect();

    if disabled.is_empty() {
        return;
    }
    for node in &mut graph.nodes {
        if node
            .cycle_kind
            .as_ref()
            .map(|k| disabled.contains(k))
            .unwrap_or(false)
        {
            node.cycle_kind = None;
        }
    }
    graph.cycles.retain(|cg| !disabled.contains(&cg.kind));
}

// ── Threshold violations ───────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct Violation {
    pub graph: &'static str,
    /// Stable rule id, e.g. `cycle.chain` or `threshold.node.cognitive`.
    pub rule: String,
    pub message: String,
    /// Ranking weight for `--top` — higher is worse (breach ratio / cycle size).
    pub weight: f64,
}

pub fn check_violations(graphs: &PluginGraphs, rules: &RulesConfig) -> Vec<Violation> {
    let mut vs = Vec::new();
    check_graph_violations("modules", &graphs.modules, rules, &mut vs);
    check_graph_violations("files", &graphs.files, rules, &mut vs);
    check_graph_violations("functions", &graphs.functions, rules, &mut vs);
    vs
}

fn check_graph_violations(
    name: &'static str,
    graph: &Graph,
    rules: &RulesConfig,
    vs: &mut Vec<Violation>,
) {
    // Cycles: every remaining cycle group is of an enabled kind (disabled kinds
    // were already stripped by apply_cycle_rules), so each is a violation.
    // Ranked by SCC size — a larger cycle is grosser.
    for cg in &graph.cycles {
        push(
            vs,
            name,
            cycle_rule_id(&cg.kind),
            format!("{:?} cycle: {} nodes", cg.kind, cg.nodes.len()),
            cg.nodes.len() as f64,
        );
    }

    let nt = &rules.thresholds.node;

    for node in &graph.nodes {
        let Some(cx) = &node.complexity else { continue };

        if let (Some(limit), Some(c)) = (nt.hk, &cx.coupling)
            && c.hk > limit
        {
            push(
                vs,
                name,
                "threshold.node.hk".to_string(),
                format!("{}: hk {:.0} > {:.0}", node.id, c.hk, limit),
                c.hk / limit,
            );
        }
        if let Some(limit) = nt.cyclomatic
            && cx.cyclomatic > limit
        {
            push(
                vs,
                name,
                "threshold.node.cyclomatic".to_string(),
                format!(
                    "{}: cyclomatic {:.0} > {:.0}",
                    node.id, cx.cyclomatic, limit
                ),
                cx.cyclomatic / limit,
            );
        }
        if let Some(limit) = nt.cognitive
            && cx.cognitive > limit
        {
            push(
                vs,
                name,
                "threshold.node.cognitive".to_string(),
                format!("{}: cognitive {:.0} > {:.0}", node.id, cx.cognitive, limit),
                cx.cognitive / limit,
            );
        }
        if let (Some(limit), Some(c)) = (nt.fan_in, &cx.coupling)
            && c.fan_in as f64 > limit
        {
            push(
                vs,
                name,
                "threshold.node.fan_in".to_string(),
                format!("{}: fan_in {} > {:.0}", node.id, c.fan_in, limit),
                c.fan_in as f64 / limit,
            );
        }
        if let (Some(limit), Some(c)) = (nt.fan_out, &cx.coupling)
            && c.fan_out as f64 > limit
        {
            push(
                vs,
                name,
                "threshold.node.fan_out".to_string(),
                format!("{}: fan_out {} > {:.0}", node.id, c.fan_out, limit),
                c.fan_out as f64 / limit,
            );
        }
        if let (Some(limit), Some(loc)) = (nt.loc, &cx.loc)
            && loc.source > limit
        {
            push(
                vs,
                name,
                "threshold.node.loc".to_string(),
                format!("{}: loc {:.0} > {:.0}", node.id, loc.source, limit),
                loc.source / limit,
            );
        }
    }

    let at = &rules.thresholds.avg;
    let Some(stats) = &graph.stats else { return };

    if let Some(limit) = at.hk {
        let avg = stats.coupling.as_ref().map(|c| c.hk).unwrap_or(0.0);
        if avg > limit {
            push(
                vs,
                name,
                "threshold.avg.hk".to_string(),
                format!("avg hk {:.0} > {:.0}", avg, limit),
                avg / limit,
            );
        }
    }
    if let Some(limit) = at.cyclomatic
        && stats.cyclomatic > limit
    {
        push(
            vs,
            name,
            "threshold.avg.cyclomatic".to_string(),
            format!("avg cyclomatic {:.1} > {:.1}", stats.cyclomatic, limit),
            stats.cyclomatic / limit,
        );
    }
    if let Some(limit) = at.cognitive
        && stats.cognitive > limit
    {
        push(
            vs,
            name,
            "threshold.avg.cognitive".to_string(),
            format!("avg cognitive {:.1} > {:.1}", stats.cognitive, limit),
            stats.cognitive / limit,
        );
    }
    if let Some(limit) = at.fan_in {
        let avg = stats.coupling.as_ref().map(|c| c.fan_in).unwrap_or(0.0);
        if avg > limit {
            push(
                vs,
                name,
                "threshold.avg.fan_in".to_string(),
                format!("avg fan_in {:.1} > {:.1}", avg, limit),
                avg / limit,
            );
        }
    }
    if let Some(limit) = at.fan_out {
        let avg = stats.coupling.as_ref().map(|c| c.fan_out).unwrap_or(0.0);
        if avg > limit {
            push(
                vs,
                name,
                "threshold.avg.fan_out".to_string(),
                format!("avg fan_out {:.1} > {:.1}", avg, limit),
                avg / limit,
            );
        }
    }
    if let Some(limit) = at.loc {
        let avg = stats.loc.as_ref().map(|l| l.source).unwrap_or(0.0);
        if avg > limit {
            push(
                vs,
                name,
                "threshold.avg.loc".to_string(),
                format!("avg loc {:.0} > {:.0}", avg, limit),
                avg / limit,
            );
        }
    }
}

fn cycle_rule_id(kind: &CycleKind) -> String {
    match kind {
        CycleKind::TestEmbed => "cycle.test-embed",
        CycleKind::Mutual => "cycle.mutual",
        CycleKind::Chain => "cycle.chain",
    }
    .to_string()
}

fn push(vs: &mut Vec<Violation>, graph: &'static str, rule: String, message: String, weight: f64) {
    vs.push(Violation {
        graph,
        rule,
        message,
        weight,
    });
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use code_split_core::graph::{Complexity, CycleGroup, Node, NodeKind};

    #[test]
    fn parse_on_off_accepts_on_off_true_false() {
        let cases = vec![
            ("on", Some(true)),
            ("true", Some(true)),
            ("off", Some(false)),
            ("false", Some(false)),
            ("maybe", None),
            ("", None),
        ];
        for (input, expected) in cases {
            match expected {
                Some(b) => assert_eq!(parse_on_off(input).unwrap(), b, "for {input:?}"),
                None => assert!(parse_on_off(input).is_err(), "should reject {input:?}"),
            }
        }
    }

    #[test]
    fn cycle_rules_default_test_embed_off_others_on() {
        let d = CycleRules::default();
        assert!(!d.test_embed, "test-embed defaults off");
        assert!(d.mutual, "mutual defaults on");
        assert!(d.chain, "chain defaults on");
    }

    #[test]
    fn cli_override_sets_cycle_and_threshold() {
        let mut cfg = Config::default();
        apply_cli_overrides(
            &mut cfg,
            &[],
            &["test-embed=on".into(), "mutual=off".into()],
            &["node.cognitive=25".into(), "avg.hk=1000".into()],
        )
        .unwrap();
        assert!(cfg.rules.cycles.test_embed, "test-embed enabled");
        assert!(!cfg.rules.cycles.mutual, "mutual disabled");
        assert!(cfg.rules.cycles.chain, "chain untouched (default on)");
        assert_eq!(cfg.rules.thresholds.node.cognitive, Some(25.0));
        assert_eq!(cfg.rules.thresholds.avg.hk, Some(1000.0));
        assert_eq!(
            cfg.rules.thresholds.node.hk, None,
            "unset metric stays None"
        );
    }

    #[test]
    fn cli_override_rejects_invalid_with_context() {
        // (cycle_rules, thresholds, substring the error message must contain)
        let cases: Vec<(Vec<String>, Vec<String>, &str)> = vec![
            (vec!["mutual=loud".into()], vec![], "loud"),
            (vec!["bogus=on".into()], vec![], "bogus"),
            (vec![], vec!["node.bogus=1".into()], "bogus"),
            (vec![], vec!["nope.hk=1".into()], "nope"),
            (vec![], vec!["node.hk=NaNum".into()], "number"),
        ];
        for (cycles, thresholds, needle) in cases {
            let mut cfg = Config::default();
            let err = apply_cli_overrides(&mut cfg, &[], &cycles, &thresholds)
                .expect_err(&format!("should reject {cycles:?} {thresholds:?}"));
            let msg = format!("{err:#}");
            assert!(
                msg.contains(needle),
                "error {msg:?} should mention {needle:?}"
            );
        }
    }

    #[test]
    fn check_reports_enabled_cycle_group() {
        let mut graphs = PluginGraphs::default();
        graphs.modules.cycles.push(CycleGroup {
            kind: CycleKind::Chain,
            nodes: vec!["a".into(), "b".into(), "c".into()],
        });
        let vs = check_violations(&graphs, &RulesConfig::default());
        assert_eq!(vs.len(), 1, "one enabled cycle -> one violation");
        assert_eq!(vs[0].graph, "modules");
        assert!(
            vs[0].message.contains("Chain cycle"),
            "got {:?}",
            vs[0].message
        );
    }

    #[test]
    fn apply_cycle_rules_strips_disabled_kind() {
        let mut graphs = PluginGraphs::default();
        graphs.modules.cycles.push(CycleGroup {
            kind: CycleKind::TestEmbed,
            nodes: vec!["a".into(), "b".into()],
        });
        // default rules: test-embed is off -> stripped.
        apply_cycle_rules(&mut graphs, &CycleRules::default());
        assert!(graphs.modules.cycles.is_empty(), "disabled cycle stripped");
        assert!(
            check_violations(&graphs, &RulesConfig::default()).is_empty(),
            "a stripped cycle is not a violation"
        );
    }

    #[test]
    fn check_reports_node_threshold_breach_only_for_over_budget() {
        let mut graphs = PluginGraphs::default();
        graphs
            .functions
            .nodes
            .push(node_with_cognitive("fn:hot", 50.0));
        graphs
            .functions
            .nodes
            .push(node_with_cognitive("fn:cold", 5.0));
        let mut rules = RulesConfig::default();
        rules.thresholds.node.cognitive = Some(25.0);
        let vs = check_violations(&graphs, &rules);
        assert_eq!(vs.len(), 1, "only the over-budget node violates");
        assert!(vs[0].message.contains("fn:hot"), "got {:?}", vs[0].message);
        assert!(
            vs[0].message.contains("cognitive"),
            "got {:?}",
            vs[0].message
        );
    }

    #[test]
    fn inline_config_overrides_dotted_keys() {
        let mut cfg = Config::default();
        apply_inline_overrides(
            &mut cfg,
            &[
                "plugin=python",
                "rules.cycles.test-embed=on",
                "rules.cycles.mutual=off",
                "rules.thresholds.node.cognitive=25",
                "rules.thresholds.avg.hk=1000",
                "ignore.test_modules=true",
            ],
        )
        .unwrap();
        assert_eq!(cfg.plugin.as_deref(), Some("python"));
        assert!(cfg.rules.cycles.test_embed, "test-embed enabled inline");
        assert!(!cfg.rules.cycles.mutual, "mutual disabled inline");
        assert_eq!(cfg.rules.thresholds.node.cognitive, Some(25.0));
        assert_eq!(cfg.rules.thresholds.avg.hk, Some(1000.0));
        assert!(cfg.ignore.test_modules, "ignore.test_modules set inline");
    }

    #[test]
    fn inline_config_rejects_unknown_key() {
        let mut cfg = Config::default();
        let err = apply_inline_overrides(&mut cfg, &["rules.bogus.x=1"]).unwrap_err();
        assert!(format!("{err:#}").contains("bogus"), "got {err:#}");
    }

    fn node_with_cognitive(id: &str, cognitive: f64) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Fn,
            name: id.into(),
            path: "p".into(),
            parent: None,
            external: None,
            visibility: None,
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity: Some(Complexity {
                cognitive,
                ..Default::default()
            }),
            cycle_kind: None,
        }
    }
}
