use anyhow::{Context, Result};
use code_split_graph::{CycleGroup, LevelGraph};
use code_split_plugin_api::{AttrValue, Graph, Node};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::Path;

// ── Config structs ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// Default plugin name (e.g. "rust", "python"). Overridden by --plugin.
    pub plugin: Option<String>,
    pub ignore: IgnoreConfig,
    pub rules: RulesConfig,
    pub output: OutputConfig,
}

/// Per-format output config: `[output.json]` / `[output.html]`, each with a
/// `path` template and an optional `enabled` flag.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct OutputConfig {
    pub json: OutputArtifact,
    pub html: OutputArtifact,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct OutputArtifact {
    pub path: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct IgnoreConfig {
    pub paths: Vec<String>,
    /// Strip test files from the graph.
    #[serde(alias = "test_modules", alias = "test-modules")]
    pub tests: bool,
    /// Strip crates that appear only in [dev-dependencies].
    pub dev_only_crates: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct RulesConfig {
    pub cycles: CycleRules,
    pub thresholds: ThresholdRules,
}

/// A cycle check: disabled, or enabled with a maximum allowed count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleRule {
    Off,
    Max(u32),
}

impl CycleRule {
    pub fn budget(self) -> Option<u32> {
        match self {
            CycleRule::Off => None,
            CycleRule::Max(n) => Some(n),
        }
    }
    pub fn is_off(self) -> bool {
        matches!(self, CycleRule::Off)
    }
}

impl<'de> Deserialize<'de> for CycleRule {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = CycleRule;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a bool (on/off) or a non-negative integer (max allowed cycles)")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<CycleRule, E> {
                Ok(if v { CycleRule::Max(0) } else { CycleRule::Off })
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<CycleRule, E> {
                u32::try_from(v)
                    .map(CycleRule::Max)
                    .map_err(|_| E::custom("cycle budget must be a non-negative integer"))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<CycleRule, E> {
                Ok(CycleRule::Max(v as u32))
            }
        }
        d.deserialize_any(V)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct CycleRules {
    #[serde(rename = "test-embed")]
    pub test_embed: CycleRule,
    pub mutual: CycleRule,
    pub chain: CycleRule,
}

impl Default for CycleRules {
    fn default() -> Self {
        Self {
            test_embed: CycleRule::Off,
            mutual: CycleRule::Max(0),
            chain: CycleRule::Max(0),
        }
    }
}

impl CycleRules {
    /// Budget for a cycle kind string (`"test_embed"`/`"mutual"`/`"chain"`):
    /// `Some(max)` if enabled, `None` if disabled.
    pub fn budget_for(self, kind: &str) -> Option<u32> {
        match kind {
            "test_embed" => self.test_embed,
            "mutual" => self.mutual,
            "chain" => self.chain,
            _ => CycleRule::Off,
        }
        .budget()
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct ThresholdRules {
    pub file: MetricThresholds,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct MetricThresholds {
    #[serde(default, deserialize_with = "de_opt_number")]
    pub hk: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_number")]
    pub cyclomatic: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_number")]
    pub cognitive: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_number")]
    pub fan_in: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_number")]
    pub fan_out: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_number")]
    pub loc: Option<f64>,
}

/// Parse a threshold value: a number with optional `_` separators and a
/// `K`/`M`/`G` suffix.
fn parse_number(s: &str) -> Result<f64> {
    let t = s.trim().replace('_', "");
    let (mult, body) = match t.bytes().last() {
        Some(b'k' | b'K') => (1e3, &t[..t.len() - 1]),
        Some(b'm' | b'M') => (1e6, &t[..t.len() - 1]),
        Some(b'g' | b'G') => (1e9, &t[..t.len() - 1]),
        _ => (1.0, t.as_str()),
    };
    let n: f64 = body.parse().with_context(|| {
        format!("invalid number {s:?} (expected e.g. 500000, 5_000_000, 5K, 1.5M)")
    })?;
    Ok(n * mult)
}

fn de_opt_number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    struct V;
    impl serde::de::Visitor<'_> for V {
        type Value = f64;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a number, or a string like \"5K\" / \"1.5M\"")
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<f64, E> {
            Ok(v as f64)
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<f64, E> {
            Ok(v as f64)
        }
        fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<f64, E> {
            Ok(v)
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<f64, E> {
            parse_number(v).map_err(E::custom)
        }
    }
    d.deserialize_any(V).map(Some)
}

// ── Loading ────────────────────────────────────────────────────────────────────

pub struct LoadedConfig {
    pub config: Config,
    pub source_file: Option<String>,
}

pub fn load(
    workspace: &Path,
    config_entries: &[String],
    ignore_paths: &[String],
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<LoadedConfig> {
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
    if let Some(path) = explicit {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        return Ok((cfg, Some(path.display().to_string())));
    }

    let cwd = std::env::current_dir().unwrap_or_default();

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
        let (kind, state) = split_kv(raw, "cycle-rule")?;
        set_cycle(cfg, kind, parse_cycle_rule(state)?)?;
    }

    for raw in thresholds {
        let (path, val_str) = split_kv(raw, "threshold")?;
        let val = parse_number(val_str).with_context(|| format!("in --threshold {raw}"))?;
        let (scope, metric) = parse_threshold_path(path)?;
        set_threshold(cfg, scope, metric, val)?;
    }

    Ok(())
}

fn apply_inline_overrides(cfg: &mut Config, entries: &[&str]) -> Result<()> {
    for raw in entries {
        let (key, value) = raw
            .split_once('=')
            .with_context(|| format!("--config override must be KEY=VALUE, got: {raw}"))?;
        match key {
            "plugin" => cfg.plugin = Some(value.to_string()),
            "ignore.tests" | "ignore.test_modules" => cfg.ignore.tests = parse_on_off(value)?,
            "ignore.dev_only_crates" => cfg.ignore.dev_only_crates = parse_on_off(value)?,
            "ignore.paths" => cfg
                .ignore
                .paths
                .extend(value.split(',').map(|s| s.trim().to_string())),
            "output.json.path" => cfg.output.json.path = Some(value.to_string()),
            "output.html.path" => cfg.output.html.path = Some(value.to_string()),
            "output.json.enabled" => cfg.output.json.enabled = Some(parse_on_off(value)?),
            "output.html.enabled" => cfg.output.html.enabled = Some(parse_on_off(value)?),
            _ if key.strip_prefix("rules.cycles.").is_some() => {
                let kind = key.strip_prefix("rules.cycles.").unwrap();
                set_cycle(cfg, kind, parse_cycle_rule(value)?)?;
            }
            _ if key.strip_prefix("rules.thresholds.").is_some() => {
                let rest = key.strip_prefix("rules.thresholds.").unwrap();
                let (scope, metric) = parse_threshold_path(rest)?;
                let val = parse_number(value).with_context(|| format!("in --config {raw}"))?;
                set_threshold(cfg, scope, metric, val)?;
            }
            other => anyhow::bail!("unknown config key {other:?}"),
        }
    }
    Ok(())
}

fn set_cycle(cfg: &mut Config, kind: &str, rule: CycleRule) -> Result<()> {
    match kind {
        "test-embed" => cfg.rules.cycles.test_embed = rule,
        "mutual" => cfg.rules.cycles.mutual = rule,
        "chain" => cfg.rules.cycles.chain = rule,
        other => anyhow::bail!("unknown cycle kind {other:?}; expected test-embed|mutual|chain"),
    }
    Ok(())
}

fn parse_cycle_rule(s: &str) -> Result<CycleRule> {
    match s {
        "on" | "true" => Ok(CycleRule::Max(0)),
        "off" | "false" => Ok(CycleRule::Off),
        other => other.parse::<u32>().map(CycleRule::Max).with_context(|| {
            format!("cycle rule must be on|off or a non-negative integer, got {other:?}")
        }),
    }
}

fn parse_threshold_path(path: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = path.split('.').collect();
    match parts.as_slice() {
        [scope, metric] => Ok((scope, metric)),
        _ => anyhow::bail!("threshold must be file.METRIC, got: {path}"),
    }
}

fn set_threshold(cfg: &mut Config, scope: &str, metric: &str, val: f64) -> Result<()> {
    let st = match scope {
        "file" => &mut cfg.rules.thresholds.file,
        other => {
            anyhow::bail!("unknown threshold scope {other:?}; the only scope is `file`")
        }
    };
    set_metric(st, metric, val)
}

fn set_metric(bucket: &mut MetricThresholds, metric: &str, val: f64) -> Result<()> {
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

// ── Attr helpers ─────────────────────────────────────────────────────────────

/// Read a numeric node attribute (`Int` or `Float`) as `f64`.
fn attr_num(node: &Node, key: &str) -> Option<f64> {
    match node.attrs.get(key) {
        Some(AttrValue::Int(i)) => Some(*i as f64),
        Some(AttrValue::Float(f)) => Some(*f),
        _ => None,
    }
}

fn is_external(node: &Node) -> bool {
    node.kind == "external" || matches!(node.attrs.get("external"), Some(AttrValue::Bool(true)))
}

// ── Path filtering ─────────────────────────────────────────────────────────────

/// Strip nodes/edges matching ignore globs, the test-file heuristic, or
/// dev-only crates from the structural graph (before cycles/metrics).
pub fn apply_ignore(graph: &mut Graph, ignore: &IgnoreConfig, target: &Path) -> Result<usize> {
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
    if gs.is_none() && !ignore.tests && dev_only.is_empty() {
        return Ok(0);
    }
    Ok(filter_graph(graph, gs.as_ref(), ignore.tests, &dev_only))
}

fn looks_like_test(name: &str, path: &str) -> bool {
    let mut stem = name.to_ascii_lowercase();
    for ext in [".rs", ".py", ".ts", ".tsx", ".js", ".jsx"] {
        if let Some(s) = stem.strip_suffix(ext) {
            stem = s.to_string();
            break;
        }
    }
    if matches!(stem.as_str(), "tests" | "test" | "conftest")
        || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_tests")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
    {
        return true;
    }
    let p = path.replace('\\', "/");
    p.contains("/tests/") || p.contains("/__tests__/") || p.contains("/test/")
}

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

    let packages = meta["packages"].as_array().expect("packages array");
    let mut id_to_name: HashMap<&str, &str> = HashMap::new();
    for pkg in packages {
        if let (Some(id), Some(name)) = (pkg["id"].as_str(), pkg["name"].as_str()) {
            id_to_name.insert(id, name);
        }
    }

    let workspace_members: HashSet<&str> = meta["workspace_members"]
        .as_array()
        .expect("workspace_members array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

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

    let mut regular: HashSet<&str> = workspace_members.iter().copied().collect();
    let mut queue: VecDeque<&str> = regular.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        for &(dep_id, dev_only) in adj.get(id).map(Vec::as_slice).unwrap_or(&[]) {
            if !dev_only && regular.insert(dep_id) {
                queue.push_back(dep_id);
            }
        }
    }

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

/// Ids are `{root}/sub/path` after relativize; strip the `{…}/` prefix for glob
/// matching. External ids (`ext:name`) are returned as-is.
fn strip_root_prefix(id: &str) -> &str {
    if id.starts_with('{')
        && let Some(idx) = id.find('}')
    {
        return id[idx + 1..].trim_start_matches('/');
    }
    id
}

fn filter_graph(
    graph: &mut Graph,
    gs: Option<&GlobSet>,
    tests: bool,
    dev_only: &HashSet<String>,
) -> usize {
    let removed: HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| {
            if is_external(n) {
                if !dev_only.is_empty()
                    && let Some(name) = n.id.strip_prefix("ext:")
                {
                    let base = name.split('@').next().unwrap_or(name);
                    return dev_only.contains(base);
                }
                return false;
            }
            if let Some(gs) = gs
                && gs.is_match(strip_root_prefix(&n.id))
            {
                return true;
            }
            if tests && looks_like_test(&n.name, &n.id) {
                return true;
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
        .retain(|e| !removed.contains(&e.source) && !removed.contains(&e.target));
    before - graph.nodes.len()
}

// ── Cycle rules ────────────────────────────────────────────────────────────────

/// Strip disabled cycle kinds from the cycle groups and clear the matching
/// `cycle` node attributes.
pub fn apply_cycle_rules(cycles: &mut Vec<CycleGroup>, nodes: &mut [Node], rules: &CycleRules) {
    let disabled: HashSet<&str> = ["test_embed", "mutual", "chain"]
        .into_iter()
        .filter(|k| rules.budget_for(k).is_none())
        .collect();
    if disabled.is_empty() {
        return;
    }
    cycles.retain(|cg| !disabled.contains(cg.kind.as_str()));
    for node in nodes {
        if let Some(AttrValue::Str(k)) = node.attrs.get("cycle")
            && disabled.contains(k.as_str())
        {
            node.attrs.remove("cycle");
        }
    }
}

// ── Rule catalog ─────────────────────────────────────────────────────────────

pub struct RuleDoc {
    pub key: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    pub why: &'static str,
    pub fix: &'static str,
}

pub const RULES: &[RuleDoc] = &[
    RuleDoc {
        key: "cycle.mutual",
        group: "CYC",
        title: "Mutual dependency cycle",
        why: "Two units import each other (A ↔ B), so neither can be built, tested, or \
              understood in isolation — the tightest possible coupling.",
        fix: "Move the shared types into a third, lower-level unit both depend on; invert one \
              direction behind a trait/interface; or merge the two if they are really one concept.",
    },
    RuleDoc {
        key: "cycle.chain",
        group: "CYC",
        title: "Chain dependency cycle",
        why: "Three or more units form a strongly-connected component (A → B → C → A); the whole \
              component must be loaded and changed together, defeating modular boundaries.",
        fix: "Find the edge that closes the loop — usually one 'back' dependency pointing upward — \
              and invert or remove it, or introduce an abstraction layer between the units.",
    },
    RuleDoc {
        key: "cycle.test-embed",
        group: "CYC",
        title: "Test-embedded-in-production cycle",
        why: "Production code reaches a module that exists only for tests, coupling shippable code \
              to test scaffolding so the two cannot ship or be reasoned about separately.",
        fix: "Move test-only helpers into a test module/target, gate them behind a test feature, or \
              invert the dependency so tests depend on production code and never the reverse.",
    },
    RuleDoc {
        key: "cyclomatic",
        group: "CPX",
        title: "Cyclomatic complexity",
        why: "Cyclomatic complexity counts the independent paths through a unit; high values mean \
              many branches, which demand many tests and are easy to get wrong. A high graph \
              average means branching is spread across the codebase, not just one hotspot.",
        fix: "Split the function, replace branching with polymorphism or a lookup table, and pull \
              guard clauses to the top to flatten nesting. For an average breach, simplify the \
              worst offenders first (--top).",
    },
    RuleDoc {
        key: "cognitive",
        group: "CPX",
        title: "Cognitive complexity",
        why: "Cognitive complexity weights nested and interrupted control flow by how hard a human \
              finds it to follow; a high score reads as 'hard to hold in your head'. A high average \
              means readability is degrading broadly.",
        fix: "Extract nested blocks into named helpers, use early returns to cut nesting depth, and \
              avoid mixing several control structures in one function. For an average breach, target \
              the worst nodes first (--top).",
    },
    RuleDoc {
        key: "hk",
        group: "CPL",
        title: "Henry-Kafura coupling",
        why: "Henry-Kafura — loc × (fan_in × fan_out)² — flags units that are both highly connected \
              and large: change-amplifiers whose edits ripple widely across the system.",
        fix: "Cut fan-in or fan-out: narrow the public surface, split the unit by responsibility, or \
              route dependencies through a smaller interface. Shrinking its LOC also lowers hk.",
    },
    RuleDoc {
        key: "fan_in",
        group: "CPL",
        title: "Fan-in",
        why: "Many other units depend on this one, making it risky to change and a single point of \
              failure — though some hubs (shared types) carry high fan-in legitimately.",
        fix: "If the fan-in is unintended, split the unit so each caller depends only on the slice \
              it uses; otherwise stabilize the interface so high fan-in is safe.",
    },
    RuleDoc {
        key: "fan_out",
        group: "CPL",
        title: "Fan-out",
        why: "This unit depends on many others, so it breaks when any of them change and is hard to \
              test in isolation.",
        fix: "Group related dependencies behind a facade, inject collaborators instead of reaching \
              for them, or move logic closer to the data it uses to cut outgoing edges.",
    },
    RuleDoc {
        key: "loc",
        group: "SIZ",
        title: "Source size",
        why: "The unit has more source lines than allowed; large files/functions tend to hold several \
              responsibilities and are harder to review, test, and reuse.",
        fix: "Split by responsibility into smaller units, extract helpers, and separate data \
              definitions from behavior. For an average breach, break up the largest units first (--top).",
    },
];

pub fn rule_doc(id: &str) -> Option<&'static RuleDoc> {
    if id.starts_with("cycle.") {
        RULES.iter().find(|r| r.key == id)
    } else {
        let metric = id.rsplit('.').next().unwrap_or(id);
        RULES.iter().find(|r| r.key == metric)
    }
}

pub fn rule_tuning(id: &str) -> String {
    if let Some(kind) = id.strip_prefix("cycle.") {
        format!("disable with --cycle-rule {kind}=off   ·   rules.cycles.{kind} in code-split.toml")
    } else if let Some(rest) = id.strip_prefix("threshold.") {
        format!("set with --threshold {rest}=N   ·   rules.thresholds.{rest} in code-split.toml")
    } else {
        String::new()
    }
}

// ── Violations ───────────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct Violation {
    pub rule: String,
    pub group: &'static str,
    pub graph: &'static str,
    pub location: String,
    pub message: String,
    pub weight: f64,
}

impl Violation {
    pub fn summary(&self) -> String {
        if self.location.is_empty() {
            self.message.clone()
        } else {
            format!("{}: {}", self.location, self.message)
        }
    }
}

pub fn check_violations(
    graphs: &BTreeMap<String, LevelGraph>,
    rules: &RulesConfig,
) -> Vec<Violation> {
    let mut vs = Vec::new();
    if let Some(level) = graphs.get("files") {
        check_level_violations("files", level, rules, &mut vs);
    }
    vs
}

fn check_level_violations(
    name: &'static str,
    level: &LevelGraph,
    rules: &RulesConfig,
    vs: &mut Vec<Violation>,
) {
    // Cycles: remaining groups are all of enabled kinds; report only those over
    // their kind's budget. Ranked by SCC size.
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for cg in &level.cycles {
        *counts.entry(cg.kind.as_str()).or_insert(0) += 1;
    }
    for cg in &level.cycles {
        let count = counts[cg.kind.as_str()];
        let budget = rules.cycles.budget_for(&cg.kind).unwrap_or(0);
        if count as u32 <= budget {
            continue;
        }
        let mut message = describe_cycle(&cg.kind, &cg.nodes);
        if budget > 0 {
            message = format!("{message}  (over budget: {count} > {budget})");
        }
        push(
            vs,
            name,
            cycle_rule_id(&cg.kind),
            String::new(),
            message,
            cg.nodes.len() as f64,
        );
    }

    let bucket = &rules.thresholds.file;
    for node in &level.nodes {
        if is_external(node) {
            continue;
        }
        check_node_metrics(vs, name, "file", bucket, node);
    }
}

fn check_node_metrics(
    vs: &mut Vec<Violation>,
    graph: &'static str,
    scope: &str,
    t: &MetricThresholds,
    node: &Node,
) {
    let loc_id = node.id.clone();
    let check =
        |vs: &mut Vec<Violation>, limit: Option<f64>, key: &str, label: &str, metric: &str| {
            if let (Some(limit), Some(value)) = (limit, attr_num(node, key))
                && value > limit
            {
                push_threshold(
                    vs,
                    graph,
                    &format!("threshold.{scope}.{metric}"),
                    loc_id.clone(),
                    label,
                    value,
                    limit,
                    0,
                );
            }
        };
    check(vs, t.hk, "hk", "Henry-Kafura hk", "hk");
    check(
        vs,
        t.cyclomatic,
        "cyclomatic",
        "cyclomatic complexity",
        "cyclomatic",
    );
    check(
        vs,
        t.cognitive,
        "cognitive",
        "cognitive complexity",
        "cognitive",
    );
    check(vs, t.fan_in, "fan_in", "fan-in", "fan_in");
    check(vs, t.fan_out, "fan_out", "fan-out", "fan_out");
    check(vs, t.loc, "loc", "source loc", "loc");
}

fn describe_cycle(kind: &str, nodes: &[String]) -> String {
    let preview: Vec<&str> = nodes.iter().take(4).map(String::as_str).collect();
    let truncated = nodes.len() > preview.len();
    match kind {
        "mutual" => format!("mutual cycle between {}", preview.join(" ↔ ")),
        "chain" => {
            let chain = preview.join(" → ");
            let tail = if truncated {
                format!(" → … ({} nodes total)", nodes.len())
            } else {
                " → (back to start)".to_string()
            };
            format!("chain cycle: {chain}{tail}")
        }
        _ => {
            let extra = if truncated {
                format!(" (+{} more)", nodes.len() - preview.len())
            } else {
                String::new()
            };
            format!("test-embed cycle: {}{extra}", preview.join(" ↔ "))
        }
    }
}

fn cycle_rule_id(kind: &str) -> &'static str {
    match kind {
        "test_embed" => "cycle.test-embed",
        "mutual" => "cycle.mutual",
        "chain" => "cycle.chain",
        _ => "cycle.unknown",
    }
}

#[allow(clippy::too_many_arguments)]
fn push_threshold(
    vs: &mut Vec<Violation>,
    graph: &'static str,
    id: &str,
    location: String,
    metric: &str,
    value: f64,
    limit: f64,
    decimals: usize,
) {
    let ratio = if limit > 0.0 {
        value / limit
    } else {
        f64::INFINITY
    };
    let message = format!(
        "{metric} {value:.decimals$} exceeds limit {limit:.decimals$} ({ratio:.1}× over budget)"
    );
    push(vs, graph, id, location, message, ratio);
}

fn push(
    vs: &mut Vec<Violation>,
    graph: &'static str,
    id: &str,
    location: String,
    message: String,
    weight: f64,
) {
    let group = rule_doc(id).map(|d| d.group).unwrap_or("?");
    vs.push(Violation {
        rule: id.to_string(),
        group,
        graph,
        location,
        message,
        weight,
    });
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn file_node(id: &str, attrs: &[(&str, AttrValue)]) -> Node {
        let mut n = Node {
            id: id.into(),
            kind: "file".into(),
            name: id.into(),
            parent: None,
            attrs: Default::default(),
        };
        for (k, v) in attrs {
            n.attrs.insert((*k).into(), v.clone());
        }
        n
    }

    fn level_with(nodes: Vec<Node>, cycles: Vec<CycleGroup>) -> BTreeMap<String, LevelGraph> {
        let level = LevelGraph {
            nodes,
            cycles,
            ..Default::default()
        };
        BTreeMap::from([("files".to_string(), level)])
    }

    #[test]
    fn parse_on_off_accepts_on_off_true_false() {
        for (input, expected) in [
            ("on", true),
            ("true", true),
            ("off", false),
            ("false", false),
        ] {
            assert_eq!(parse_on_off(input).unwrap(), expected);
        }
        assert!(parse_on_off("maybe").is_err());
    }

    #[test]
    fn cycle_rules_default_test_embed_off_others_strict() {
        let d = CycleRules::default();
        assert_eq!(d.test_embed, CycleRule::Off);
        assert_eq!(d.mutual, CycleRule::Max(0));
        assert_eq!(d.chain, CycleRule::Max(0));
        assert_eq!(d.budget_for("mutual"), Some(0));
        assert_eq!(d.budget_for("test_embed"), None);
    }

    #[test]
    fn cli_override_sets_cycle_and_threshold() {
        let mut cfg = Config::default();
        apply_cli_overrides(
            &mut cfg,
            &[],
            &["test-embed=on".into(), "mutual=off".into()],
            &["file.cognitive=25".into(), "file.hk=1000".into()],
        )
        .unwrap();
        assert_eq!(cfg.rules.cycles.test_embed, CycleRule::Max(0));
        assert_eq!(cfg.rules.cycles.mutual, CycleRule::Off);
        assert_eq!(cfg.rules.thresholds.file.cognitive, Some(25.0));
        assert_eq!(cfg.rules.thresholds.file.hk, Some(1000.0));
    }

    #[test]
    fn check_reports_enabled_cycle_group() {
        let graphs = level_with(
            vec![],
            vec![CycleGroup {
                kind: "chain".into(),
                nodes: vec!["a".into(), "b".into(), "c".into()],
            }],
        );
        let vs = check_violations(&graphs, &RulesConfig::default());
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].rule, "cycle.chain");
        assert_eq!(vs[0].group, "CYC");
    }

    #[test]
    fn apply_cycle_rules_strips_disabled_kind() {
        let mut cycles = vec![CycleGroup {
            kind: "test_embed".into(),
            nodes: vec!["a".into(), "b".into()],
        }];
        let mut nodes: Vec<Node> = vec![];
        apply_cycle_rules(&mut cycles, &mut nodes, &CycleRules::default());
        assert!(cycles.is_empty(), "test-embed off -> stripped");
    }

    #[test]
    fn cycle_budget_allows_up_to_n() {
        let cycles: Vec<CycleGroup> = (0..3)
            .map(|i| CycleGroup {
                kind: "chain".into(),
                nodes: vec![format!("a{i}"), format!("b{i}"), format!("c{i}")],
            })
            .collect();
        let graphs = level_with(vec![], cycles);
        let mut rules = RulesConfig::default();
        rules.cycles.chain = CycleRule::Max(3);
        assert!(check_violations(&graphs, &rules).is_empty());
        rules.cycles.chain = CycleRule::Max(2);
        assert_eq!(check_violations(&graphs, &rules).len(), 3);
    }

    #[test]
    fn check_reports_node_threshold_breach() {
        let graphs = level_with(
            vec![
                file_node("hot.rs", &[("cognitive", AttrValue::Int(50))]),
                file_node("cold.rs", &[("cognitive", AttrValue::Int(5))]),
            ],
            vec![],
        );
        let mut rules = RulesConfig::default();
        rules.thresholds.file.cognitive = Some(25.0);
        let vs = check_violations(&graphs, &rules);
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].rule, "threshold.file.cognitive");
        assert!(vs[0].location.contains("hot.rs"));
    }

    #[test]
    fn loc_threshold_reads_loc_attr() {
        let graphs = level_with(
            vec![file_node("big.rs", &[("loc", AttrValue::Int(900))])],
            vec![],
        );
        let mut rules = RulesConfig::default();
        rules.thresholds.file.loc = Some(500.0);
        let vs = check_violations(&graphs, &rules);
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].rule, "threshold.file.loc");
        assert_eq!(vs[0].group, "SIZ");
    }

    #[test]
    fn apply_ignore_strips_test_files() {
        let mut g = Graph {
            nodes: vec![
                file_node("{target}/src/a.js", &[]),
                file_node("{target}/src/a.test.js", &[]),
            ],
            edges: vec![],
        };
        let ignore = IgnoreConfig {
            tests: true,
            ..Default::default()
        };
        let removed = apply_ignore(&mut g, &ignore, Path::new("/x")).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].id, "{target}/src/a.js");
    }

    #[test]
    fn parse_number_handles_separators_and_suffixes() {
        for (input, want) in [
            ("5_123_000", 5_123_000.0),
            ("5K", 5_000.0),
            ("1.5M", 1_500_000.0),
        ] {
            assert_eq!(parse_number(input).unwrap(), want);
        }
        for bad in ["", "K", "5X"] {
            assert!(parse_number(bad).is_err());
        }
    }

    #[test]
    fn config_toml_parses_cycles_and_thresholds() {
        let src = "
[rules.cycles]
test-embed = false
mutual = true
chain = 7
[rules.thresholds.file]
loc = 800
";
        let cfg: Config = toml::from_str(src).unwrap();
        assert_eq!(cfg.rules.cycles.test_embed, CycleRule::Off);
        assert_eq!(cfg.rules.cycles.mutual, CycleRule::Max(0));
        assert_eq!(cfg.rules.cycles.chain, CycleRule::Max(7));
        assert_eq!(cfg.rules.thresholds.file.loc, Some(800.0));
    }
}
