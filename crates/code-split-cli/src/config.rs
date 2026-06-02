use anyhow::{Context, Result};
use code_split_core::graph::{Complexity, CycleKind, Graph, Node};
use code_split_core::snapshot::PluginGraphs;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Deserializer};
use std::collections::{HashMap, HashSet, VecDeque};
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
/// `path` template and an optional `enabled` flag. Overridden by the
/// `--output.<fmt>` / `--output.<fmt>.path` CLI flags.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct OutputConfig {
    pub json: OutputArtifact,
    pub html: OutputArtifact,
}

/// One artifact's destination and on/off state. `path` accepts the placeholders
/// `{project-dir}`, `{ts}`, `{git-hash}`, `{git-hash-N}`, or `stdout`/`-`.
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
    /// Strip test files from the graph (Rust `tests/`, Python `test_*.py` /
    /// `tests/`, JS/TS `*.test.*` / `__tests__/`, etc.). Accepted in config as
    /// `tests` or the legacy `test_modules`.
    #[serde(alias = "test_modules", alias = "test-modules")]
    pub tests: bool,
    /// Strip crates that appear only in [dev-dependencies], never in [dependencies].
    pub dev_only_crates: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct RulesConfig {
    pub cycles: CycleRules,
    pub thresholds: ThresholdRules,
}

/// A cycle check: disabled, or enabled with a **maximum allowed count** of cycles
/// of that kind. In config / on the CLI: `false`/`off` = disabled, `true`/`on` =
/// `Max(0)` (strict — any cycle fails), and an integer `n` = `Max(n)` (up to `n`
/// allowed, the `n+1`-th fails — lets you pin today's count and forbid new ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleRule {
    Off,
    Max(u32),
}

impl CycleRule {
    /// The allowed-count budget if enabled, else `None` (disabled).
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
    /// Each cycle kind is disabled (stripped, never reported) or enabled with a
    /// max-count budget; a kind over its budget fails `check`.
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
    /// Budget for a cycle kind: `Some(max)` if enabled, `None` if disabled.
    pub fn budget_for(self, kind: CycleKind) -> Option<u32> {
        match kind {
            CycleKind::TestEmbed => self.test_embed,
            CycleKind::Mutual => self.mutual,
            CycleKind::Chain => self.chain,
        }
        .budget()
    }
}

/// Thresholds for the single file graph: per-file metric limits. The only scope
/// is `file`; there are no graph-average thresholds.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct ThresholdRules {
    /// Per-file metric limits.
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

/// Parse a threshold value: a plain number, with `_` digit separators, and an
/// optional `K`/`M`/`G` multiplier suffix (×10³ / ×10⁶ / ×10⁹, case-insensitive).
/// Examples: `500000`, `5_123_000`, `5K`, `1.5M`, `2G`.
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

/// serde adaptor: deserialize a threshold value from a TOML number (`5_000_000`,
/// `5000.0`) or a string with a multiplier suffix (`"5K"`, `"1.5M"`).
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
        // Format: "kind=on|off|N", e.g. "test-embed=on", "chain=7"
        let (kind, state) = split_kv(raw, "cycle-rule")?;
        set_cycle(cfg, kind, parse_cycle_rule(state)?)?;
    }

    for raw in thresholds {
        // Format: "file.METRIC=N", e.g. "file.loc=800", "file.cognitive=25".
        let (path, val_str) = split_kv(raw, "threshold")?;
        let val = parse_number(val_str).with_context(|| format!("in --threshold {raw}"))?;
        let (scope, metric) = parse_threshold_path(path)?;
        set_threshold(cfg, scope, metric, val)?;
    }

    Ok(())
}

/// Apply `--config KEY=VALUE` inline overrides, where KEY is a dotted config key
/// (e.g. `rules.thresholds.function.cognitive=25`, `rules.cycles.mutual=on`, `plugin=rust`).
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

/// Parse a cycle-rule value: `on`/`true` = strict (`Max(0)`), `off`/`false` =
/// disabled, or a non-negative integer = max allowed count.
fn parse_cycle_rule(s: &str) -> Result<CycleRule> {
    match s {
        "on" | "true" => Ok(CycleRule::Max(0)),
        "off" | "false" => Ok(CycleRule::Off),
        other => other.parse::<u32>().map(CycleRule::Max).with_context(|| {
            format!("cycle rule must be on|off or a non-negative integer, got {other:?}")
        }),
    }
}

/// Parse a threshold key path into `(scope, metric)`. Accepts `file.METRIC`.
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
    if gs.is_none() && !ignore.tests && dev_only.is_empty() {
        return Ok(0);
    }
    Ok(filter_graph(
        &mut graphs.files,
        gs.as_ref(),
        ignore.tests,
        &dev_only,
    ))
}

/// Heuristic: does this file node look like a test file? Matches common test
/// conventions across Rust / Python / JS / TS by file name and path.
fn looks_like_test(name: &str, path: &str) -> bool {
    let mut stem = name.to_ascii_lowercase();
    for ext in [".rs", ".py", ".ts", ".tsx", ".js", ".jsx"] {
        if let Some(s) = stem.strip_suffix(ext) {
            stem = s.to_string();
            break;
        }
    }
    // `stem` is e.g. "tests", "test_foo", "foo_test", "widget.test", "widget.spec".
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
    tests: bool,
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
            if tests && looks_like_test(&n.name, &n.path) {
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
    apply_cycle_rules_graph(&mut graphs.files, rules);
}

fn apply_cycle_rules_graph(graph: &mut Graph, rules: &CycleRules) {
    let disabled: HashSet<CycleKind> = [
        (CycleKind::TestEmbed, rules.test_embed),
        (CycleKind::Mutual, rules.mutual),
        (CycleKind::Chain, rules.chain),
    ]
    .into_iter()
    .filter(|(_, rule)| rule.is_off())
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

// ── Rule catalog ─────────────────────────────────────────────────────────────
//
// Every diagnostic is identified by its dotted rule id (e.g. `threshold.file.loc`)
// — the same string used as the config key and CLI flag — and tagged with a
// concern group, mirrored in docs/ERRORS.md:
//   CYC — dependency cycles
//   CPX — control-flow complexity (cyclomatic, cognitive)
//   CPL — coupling (Henry-Kafura, fan-in, fan-out)
//   SIZ — size (lines of code)
//
// Threshold rules are scope-agnostic in the catalog: one entry per metric covers
// every scope (`node`/`file`/`module`/`function` single-unit, `avg` graph-wide),
// since the rationale and fix are the same whichever scope set the limit.

/// One catalog entry, keyed either by a cycle id (`cycle.mutual`) or — for
/// thresholds — by the bare metric name (`cognitive`, `loc`, …). Holds the
/// concern group and the human-facing rationale shown in `check` output and ERRORS.md.
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

/// Look up the catalog entry for a dotted rule id. Cycle ids match by full id;
/// threshold ids (`threshold.<scope>.<metric>`) resolve by their bare metric, so
/// every scope shares one catalog entry.
pub fn rule_doc(id: &str) -> Option<&'static RuleDoc> {
    if id.starts_with("cycle.") {
        RULES.iter().find(|r| r.key == id)
    } else {
        let metric = id.rsplit('.').next().unwrap_or(id);
        RULES.iter().find(|r| r.key == metric)
    }
}

/// How to tune or silence a rule, derived from its id (shown on the `tune:` line
/// and in ERRORS.md). Empty for ids with no knob.
pub fn rule_tuning(id: &str) -> String {
    if let Some(kind) = id.strip_prefix("cycle.") {
        format!("disable with --cycle-rule {kind}=off   ·   rules.cycles.{kind} in code-split.toml")
    } else if let Some(rest) = id.strip_prefix("threshold.") {
        // rest is e.g. "file.cognitive" | "file.loc"
        format!("set with --threshold {rest}=N   ·   rules.thresholds.{rest} in code-split.toml")
    } else {
        String::new()
    }
}

// ── Violations ───────────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct Violation {
    /// Stable dotted rule id — identical to the config key and CLI flag that
    /// controls it, e.g. `cycle.chain`, `threshold.function.cognitive`, `threshold.file.loc`.
    pub rule: String,
    /// Concern group: `CYC` / `CPX` / `CPL` / `SIZ`.
    pub group: &'static str,
    pub graph: &'static str,
    /// Where the breach is: "id — path:line" for node/threshold rules; empty for
    /// graph-average rules and cycles (whose members are listed in `message`).
    pub location: String,
    /// Measurement / description, e.g. "cognitive 50 exceeds limit 25 (2.0× over budget)".
    pub message: String,
    /// Ranking weight for `--top` — higher is worse (breach ratio / cycle size).
    pub weight: f64,
}

impl Violation {
    /// Combined one-liner for json/github/sarif: "<location>: <message>".
    pub fn summary(&self) -> String {
        if self.location.is_empty() {
            self.message.clone()
        } else {
            format!("{}: {}", self.location, self.message)
        }
    }
}

pub fn check_violations(graphs: &PluginGraphs, rules: &RulesConfig) -> Vec<Violation> {
    let mut vs = Vec::new();
    check_graph_violations("files", &graphs.files, rules, &mut vs);
    vs
}

fn check_graph_violations(
    name: &'static str,
    graph: &Graph,
    rules: &RulesConfig,
    vs: &mut Vec<Violation>,
) {
    // Cycles: remaining groups are all of enabled kinds (Off kinds were stripped
    // by apply_cycle_rules). A kind's cycles are reported only when their count
    // exceeds the kind's budget; with the strict default (budget 0) every cycle is
    // a violation. Ranked by SCC size — a larger cycle is grosser.
    let mut counts: HashMap<CycleKind, usize> = HashMap::new();
    for cg in &graph.cycles {
        *counts.entry(cg.kind).or_insert(0) += 1;
    }
    for cg in &graph.cycles {
        let count = counts[&cg.kind];
        let budget = rules.cycles.budget_for(cg.kind).unwrap_or(0);
        if count as u32 <= budget {
            continue; // within the allowed count
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

    // Thresholds: the only scope is `file`, checked per file node.
    let (scope, bucket) = match name {
        "files" => ("file", &rules.thresholds.file),
        _ => return,
    };
    for node in &graph.nodes {
        let Some(cx) = &node.complexity else { continue };
        check_node_metrics(vs, name, scope, bucket, &node_location(node), cx);
    }
}

/// Check one single-unit threshold bucket against a node, emitting
/// `threshold.<scope>.<metric>` violations for whichever limits it breaches.
fn check_node_metrics(
    vs: &mut Vec<Violation>,
    graph: &'static str,
    scope: &str,
    t: &MetricThresholds,
    loc: &str,
    cx: &Complexity,
) {
    if let (Some(limit), Some(c)) = (t.hk, &cx.coupling)
        && c.hk > limit
    {
        push_threshold(
            vs,
            graph,
            &format!("threshold.{scope}.hk"),
            loc.to_string(),
            "Henry-Kafura hk",
            c.hk,
            limit,
            0,
        );
    }
    if let Some(limit) = t.cyclomatic
        && cx.cyclomatic > limit
    {
        push_threshold(
            vs,
            graph,
            &format!("threshold.{scope}.cyclomatic"),
            loc.to_string(),
            "cyclomatic complexity",
            cx.cyclomatic,
            limit,
            0,
        );
    }
    if let Some(limit) = t.cognitive
        && cx.cognitive > limit
    {
        push_threshold(
            vs,
            graph,
            &format!("threshold.{scope}.cognitive"),
            loc.to_string(),
            "cognitive complexity",
            cx.cognitive,
            limit,
            0,
        );
    }
    if let (Some(limit), Some(c)) = (t.fan_in, &cx.coupling)
        && (c.fan_in as f64) > limit
    {
        push_threshold(
            vs,
            graph,
            &format!("threshold.{scope}.fan_in"),
            loc.to_string(),
            "fan-in",
            c.fan_in as f64,
            limit,
            0,
        );
    }
    if let (Some(limit), Some(c)) = (t.fan_out, &cx.coupling)
        && (c.fan_out as f64) > limit
    {
        push_threshold(
            vs,
            graph,
            &format!("threshold.{scope}.fan_out"),
            loc.to_string(),
            "fan-out",
            c.fan_out as f64,
            limit,
            0,
        );
    }
    if let (Some(limit), Some(l)) = (t.loc, &cx.loc)
        && l.source > limit
    {
        push_threshold(
            vs,
            graph,
            &format!("threshold.{scope}.loc"),
            loc.to_string(),
            "source loc",
            l.source,
            limit,
            0,
        );
    }
}

/// A clickable "id — path:line" location for a node, falling back to just the id.
fn node_location(node: &Node) -> String {
    match (node.path.as_str(), node.line) {
        (p, Some(l)) if !p.is_empty() => format!("{} — {}:{}", node.id, p, l),
        (p, _) if !p.is_empty() => format!("{} — {}", node.id, p),
        _ => node.id.clone(),
    }
}

/// Human-readable description of a cycle, with a short preview of its members.
fn describe_cycle(kind: &CycleKind, nodes: &[String]) -> String {
    let preview: Vec<&str> = nodes.iter().take(4).map(String::as_str).collect();
    let truncated = nodes.len() > preview.len();
    match kind {
        CycleKind::Mutual => format!("mutual cycle between {}", preview.join(" ↔ ")),
        CycleKind::Chain => {
            let chain = preview.join(" → ");
            let tail = if truncated {
                format!(" → … ({} nodes total)", nodes.len())
            } else {
                " → (back to start)".to_string()
            };
            format!("chain cycle: {chain}{tail}")
        }
        CycleKind::TestEmbed => {
            let extra = if truncated {
                format!(" (+{} more)", nodes.len() - preview.len())
            } else {
                String::new()
            };
            format!("test-embed cycle: {}{extra}", preview.join(" ↔ "))
        }
    }
}

fn cycle_rule_id(kind: &CycleKind) -> &'static str {
    match kind {
        CycleKind::TestEmbed => "cycle.test-embed",
        CycleKind::Mutual => "cycle.mutual",
        CycleKind::Chain => "cycle.chain",
    }
}

/// Push a threshold breach, composing a self-contained measurement message and
/// using the breach ratio (value / limit) as the `--top` ranking weight.
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
    use code_split_core::graph::{CycleGroup, Loc, NodeKind};

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
        assert_eq!(d.test_embed, CycleRule::Off, "test-embed defaults off");
        assert_eq!(
            d.mutual,
            CycleRule::Max(0),
            "mutual defaults strict (0 allowed)"
        );
        assert_eq!(
            d.chain,
            CycleRule::Max(0),
            "chain defaults strict (0 allowed)"
        );
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
        assert_eq!(
            cfg.rules.cycles.test_embed,
            CycleRule::Max(0),
            "test-embed enabled"
        );
        assert_eq!(cfg.rules.cycles.mutual, CycleRule::Off, "mutual disabled");
        assert_eq!(
            cfg.rules.cycles.chain,
            CycleRule::Max(0),
            "chain untouched (default)"
        );
        assert_eq!(cfg.rules.thresholds.file.cognitive, Some(25.0));
        assert_eq!(cfg.rules.thresholds.file.hk, Some(1000.0));
        assert_eq!(
            cfg.rules.thresholds.file.fan_in, None,
            "unset metric stays None"
        );
    }

    #[test]
    fn cli_override_rejects_invalid_with_context() {
        // (cycle_rules, thresholds, substring the error message must contain)
        let cases: Vec<(Vec<String>, Vec<String>, &str)> = vec![
            (vec!["mutual=loud".into()], vec![], "loud"),
            (vec!["bogus=on".into()], vec![], "bogus"),
            (vec![], vec!["file.bogus=1".into()], "bogus"),
            (vec![], vec!["nope.hk=1".into()], "nope"),
            (vec![], vec!["file.hk=NaNum".into()], "number"),
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
        graphs.files.cycles.push(CycleGroup {
            kind: CycleKind::Chain,
            nodes: vec!["a".into(), "b".into(), "c".into()],
        });
        let vs = check_violations(&graphs, &RulesConfig::default());
        assert_eq!(vs.len(), 1, "one enabled cycle -> one violation");
        assert_eq!(vs[0].graph, "files");
        assert_eq!(vs[0].rule, "cycle.chain");
        assert_eq!(vs[0].group, "CYC", "chain cycle group");
        assert!(
            vs[0].message.contains("chain cycle"),
            "got {:?}",
            vs[0].message
        );
    }

    #[test]
    fn apply_cycle_rules_strips_disabled_kind() {
        let mut graphs = PluginGraphs::default();
        graphs.files.cycles.push(CycleGroup {
            kind: CycleKind::TestEmbed,
            nodes: vec!["a".into(), "b".into()],
        });
        // default rules: test-embed is off -> stripped.
        apply_cycle_rules(&mut graphs, &CycleRules::default());
        assert!(graphs.files.cycles.is_empty(), "disabled cycle stripped");
        assert!(
            check_violations(&graphs, &RulesConfig::default()).is_empty(),
            "a stripped cycle is not a violation"
        );
    }

    #[test]
    fn cycle_budget_allows_up_to_n_and_fails_on_more() {
        let mut graphs = PluginGraphs::default();
        for i in 0..3 {
            graphs.files.cycles.push(CycleGroup {
                kind: CycleKind::Chain,
                nodes: vec![format!("a{i}"), format!("b{i}"), format!("c{i}")],
            });
        }
        // Budget 3: exactly 3 chain cycles -> within budget, no violation.
        let mut rules = RulesConfig::default();
        rules.cycles.chain = CycleRule::Max(3);
        assert!(
            check_violations(&graphs, &rules).is_empty(),
            "3 cycles within budget 3 -> pass"
        );
        // Budget 2: 3 > 2 -> every cycle reported, with the over-budget note.
        rules.cycles.chain = CycleRule::Max(2);
        let vs = check_violations(&graphs, &rules);
        assert_eq!(vs.len(), 3, "over budget -> all cycles reported");
        assert!(
            vs[0].message.contains("over budget: 3 > 2"),
            "got {:?}",
            vs[0].message
        );
    }

    #[test]
    fn parse_cycle_rule_accepts_on_off_and_int() {
        assert_eq!(parse_cycle_rule("on").unwrap(), CycleRule::Max(0));
        assert_eq!(parse_cycle_rule("true").unwrap(), CycleRule::Max(0));
        assert_eq!(parse_cycle_rule("off").unwrap(), CycleRule::Off);
        assert_eq!(parse_cycle_rule("false").unwrap(), CycleRule::Off);
        assert_eq!(parse_cycle_rule("7").unwrap(), CycleRule::Max(7));
        assert!(parse_cycle_rule("-1").is_err(), "negative rejected");
        assert!(parse_cycle_rule("nope").is_err(), "garbage rejected");
    }

    #[test]
    fn config_toml_parses_cycle_bool_and_int() {
        let src = "
[rules.cycles]
test-embed = false
mutual = true
chain = 7
";
        let cfg: Config = toml::from_str(src).expect("parse config");
        assert_eq!(cfg.rules.cycles.test_embed, CycleRule::Off);
        assert_eq!(
            cfg.rules.cycles.mutual,
            CycleRule::Max(0),
            "true => strict (0)"
        );
        assert_eq!(cfg.rules.cycles.chain, CycleRule::Max(7), "int => budget");
    }

    #[test]
    fn check_reports_node_threshold_breach_only_for_over_budget() {
        let mut graphs = PluginGraphs::default();
        graphs
            .files
            .nodes
            .push(node_with_cognitive("file:hot.rs", 50.0));
        graphs
            .files
            .nodes
            .push(node_with_cognitive("file:cold.rs", 5.0));
        let mut rules = RulesConfig::default();
        rules.thresholds.file.cognitive = Some(25.0);
        let vs = check_violations(&graphs, &rules);
        assert_eq!(vs.len(), 1, "only the over-budget node violates");
        assert_eq!(vs[0].rule, "threshold.file.cognitive");
        assert_eq!(vs[0].group, "CPX", "cognitive group");
        assert!(
            vs[0].location.contains("file:hot.rs"),
            "location {:?}",
            vs[0].location
        );
        assert!(
            vs[0].message.contains("cognitive") && vs[0].message.contains("over budget"),
            "got {:?}",
            vs[0].message
        );
    }

    #[test]
    fn file_scope_loc_threshold_hits_files_graph_nodes() {
        let mut graphs = PluginGraphs::default();
        graphs.files.nodes.push(node_with_loc("file:big.rs", 900.0));

        let mut file_rules = RulesConfig::default();
        file_rules.thresholds.file.loc = Some(500.0);
        let fv = check_violations(&graphs, &file_rules);
        assert_eq!(fv.len(), 1, "file.loc hits the files-graph node");
        assert_eq!(fv[0].rule, "threshold.file.loc");
        assert_eq!(fv[0].graph, "files");
        assert_eq!(fv[0].group, "SIZ");
        assert!(
            fv[0].location.contains("file:big.rs"),
            "got {:?}",
            fv[0].location
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
                "rules.thresholds.file.cognitive=25",
                "rules.thresholds.file.hk=1000",
                "ignore.tests=true",
            ],
        )
        .unwrap();
        assert_eq!(cfg.plugin.as_deref(), Some("python"));
        assert_eq!(
            cfg.rules.cycles.test_embed,
            CycleRule::Max(0),
            "test-embed enabled inline"
        );
        assert_eq!(
            cfg.rules.cycles.mutual,
            CycleRule::Off,
            "mutual disabled inline"
        );
        assert_eq!(cfg.rules.thresholds.file.cognitive, Some(25.0));
        assert_eq!(cfg.rules.thresholds.file.hk, Some(1000.0));
        assert!(cfg.ignore.tests, "ignore.tests set inline");
    }

    #[test]
    fn config_toml_parses_file_thresholds() {
        let src = "
[rules.thresholds.file]
loc = 800
cognitive = 30
";
        let cfg: Config = toml::from_str(src).expect("parse config");
        assert_eq!(cfg.rules.thresholds.file.loc, Some(800.0));
        assert_eq!(cfg.rules.thresholds.file.cognitive, Some(30.0));
        assert_eq!(cfg.rules.thresholds.file.hk, None);
    }

    #[test]
    fn config_toml_parses_output_paths() {
        let src = "
[output.json]
path = \"{project-dir}-{ts}.json\"
enabled = false

[output.html]
path = \"custom.html\"
";
        let cfg: Config = toml::from_str(src).expect("parse config");
        assert_eq!(
            cfg.output.json.path.as_deref(),
            Some("{project-dir}-{ts}.json")
        );
        assert_eq!(cfg.output.json.enabled, Some(false));
        assert_eq!(cfg.output.html.path.as_deref(), Some("custom.html"));
        assert_eq!(cfg.output.html.enabled, None, "unset enabled stays None");
        // Unset → None (caller falls back to the built-in default).
        let empty: Config = toml::from_str("").unwrap();
        assert!(empty.output.json.path.is_none());
    }

    #[test]
    fn inline_config_overrides_output_path_and_enabled() {
        let mut cfg = Config::default();
        apply_inline_overrides(
            &mut cfg,
            &[
                "output.json.path=dist/snap.json",
                "output.html.enabled=false",
            ],
        )
        .unwrap();
        assert_eq!(cfg.output.json.path.as_deref(), Some("dist/snap.json"));
        assert_eq!(cfg.output.html.enabled, Some(false));
    }

    #[test]
    fn inline_config_rejects_unknown_key() {
        let mut cfg = Config::default();
        let err = apply_inline_overrides(&mut cfg, &["rules.bogus.x=1"]).unwrap_err();
        assert!(format!("{err:#}").contains("bogus"), "got {err:#}");
    }

    #[test]
    fn parse_number_handles_separators_and_suffixes() {
        let ok = [
            ("500000", 500_000.0),
            ("5_123_000", 5_123_000.0),
            ("5K", 5_000.0),
            ("5k", 5_000.0),
            ("5M", 5_000_000.0),
            ("1.5M", 1_500_000.0),
            ("2G", 2_000_000_000.0),
            ("  42  ", 42.0),
        ];
        for (input, want) in ok {
            assert_eq!(parse_number(input).unwrap(), want, "for {input:?}");
        }
        for bad in ["", "K", "5X", "abc", "5MM"] {
            assert!(parse_number(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn config_toml_accepts_number_suffixes_and_separators() {
        // String values may carry a K/M/G suffix; bare integers may use `_`
        // separators (native TOML) and coerce to f64.
        let src = "
[rules.thresholds.file]
hk = \"5M\"
fan_out = 50
loc = 5_123
";
        let cfg: Config = toml::from_str(src).expect("parse config");
        assert_eq!(cfg.rules.thresholds.file.hk, Some(5_000_000.0));
        assert_eq!(cfg.rules.thresholds.file.fan_out, Some(50.0));
        assert_eq!(cfg.rules.thresholds.file.loc, Some(5_123.0));
    }

    #[test]
    fn cli_threshold_accepts_suffix_value() {
        let mut cfg = Config::default();
        apply_cli_overrides(
            &mut cfg,
            &[],
            &[],
            &["file.hk=5M".into(), "file.loc=1_500".into()],
        )
        .unwrap();
        assert_eq!(cfg.rules.thresholds.file.hk, Some(5_000_000.0));
        assert_eq!(cfg.rules.thresholds.file.loc, Some(1_500.0));
    }

    #[test]
    fn rule_catalog_keys_unique_and_groups_valid() {
        let mut keys = HashSet::new();
        for r in RULES {
            assert!(keys.insert(r.key), "duplicate key {}", r.key);
            assert!(
                matches!(r.group, "CYC" | "CPX" | "CPL" | "SIZ"),
                "{} has unknown group {}",
                r.key,
                r.group
            );
            assert!(
                !r.why.is_empty() && !r.fix.is_empty(),
                "{} is missing why/fix prose",
                r.key
            );
        }
        assert_eq!(RULES.len(), 9, "3 cycle rules + 6 threshold metrics");
    }

    #[test]
    fn every_emitted_rule_id_resolves_to_a_catalog_entry() {
        // The full id space check_graph_violations can emit: cycles, plus every
        // threshold scope × metric. rule_doc must resolve each (a miss would surface
        // as group "?" at runtime).
        let cycles = ["cycle.mutual", "cycle.chain", "cycle.test-embed"];
        for id in cycles {
            assert!(rule_doc(id).is_some(), "no catalog entry for {id}");
        }
        let metrics = ["hk", "cyclomatic", "cognitive", "fan_in", "fan_out", "loc"];
        for m in metrics {
            let id = format!("threshold.file.{m}");
            let doc = rule_doc(&id).unwrap_or_else(|| panic!("no catalog entry for {id}"));
            assert_eq!(doc.key, m, "{id} resolved to the wrong entry {}", doc.key);
        }
    }

    #[test]
    fn rule_tuning_describes_cycle_and_threshold_knobs() {
        let cyc = rule_tuning("cycle.mutual");
        assert!(cyc.contains("--cycle-rule mutual=off"), "got {cyc:?}");
        assert!(cyc.contains("rules.cycles.mutual"), "got {cyc:?}");

        let thr = rule_tuning("threshold.file.cognitive");
        assert!(thr.contains("--threshold file.cognitive=N"), "got {thr:?}");
        assert!(
            thr.contains("rules.thresholds.file.cognitive"),
            "got {thr:?}"
        );
    }

    #[test]
    fn violation_summary_combines_location_and_message() {
        let with_loc = Violation {
            rule: "threshold.file.cognitive".into(),
            group: "CPX",
            graph: "files",
            location: "file:src/a.rs — src/a.rs".into(),
            message: "cognitive 50 exceeds limit 25 (2.0× over budget)".into(),
            weight: 2.0,
        };
        assert!(
            with_loc
                .summary()
                .starts_with("file:src/a.rs — src/a.rs: cognitive")
        );

        let no_loc = Violation {
            rule: "cycle.chain".into(),
            group: "CYC",
            graph: "files",
            location: String::new(),
            message: "chain cycle: a.rs → b.rs → a.rs".into(),
            weight: 1.8,
        };
        assert_eq!(
            no_loc.summary(),
            no_loc.message,
            "no location -> message only"
        );
    }

    fn node_with_cognitive(id: &str, cognitive: f64) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::File,
            name: id.into(),
            path: "p".into(),
            parent: None,
            external: None,
            version: None,
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

    fn node_with_loc(id: &str, source: f64) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::File,
            name: id.into(),
            path: "p".into(),
            parent: None,
            external: None,
            version: None,
            visibility: None,
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity: Some(Complexity {
                loc: Some(Loc {
                    source,
                    logical: 0.0,
                    comments: 0.0,
                    blank: 0.0,
                }),
                ..Default::default()
            }),
            cycle_kind: None,
        }
    }
}
