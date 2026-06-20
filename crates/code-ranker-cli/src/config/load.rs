//! Config loading: discover `code-ranker.toml` (or `Cargo.toml` metadata),
//! apply inline `KEY=VALUE` and `--cycle-rule` / `--threshold` CLI overrides.

use super::model::{
    Config, CycleRule, DEFAULTS, MetricThresholds, parse_number, quote_suffixed_thresholds,
};
use anyhow::{Context, Result};
use code_ranker_plugin_api::log;
use code_ranker_plugin_api::toml_merge::deep_merge;
use std::path::Path;
use toml::Table;

pub struct LoadedConfig {
    pub config: Config,
    pub source_file: Option<String>,
    /// The raw merged project table (`built-in defaults ⊕ discovered config`),
    /// before deserialization into [`Config`]. Kept so `--export-full-config` can
    /// dump every effective project parameter. Does NOT include the transient
    /// per-run `--config KEY=VALUE` / `--threshold` / `--cycle-rule` flag overrides.
    pub merged: Table,
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
    let explicit: Vec<&Path> = files.iter().map(|f| Path::new(*f)).collect();

    // Discover the user's config layers as raw tables, then DEEP-MERGE them over
    // the built-in defaults IN ORDER (left to right, later wins): the binary always
    // carries a complete default config, and each file overrides only the keys it
    // spells out (see `defaults.toml`). The merge reuses the plugins' `deep_merge`,
    // so op-table list overrides (`{add,remove,replace,…}`) compose across layers.
    // Multiple `--config FILE` flags layer in command-line order; with explicit
    // files, auto-discovery of `code-ranker.toml` is skipped.
    let (layers, source_file) = discover_user_tables(workspace, &explicit)?;
    match &source_file {
        Some(p) => log::line(&format!("config: {p}")),
        None => log::line("config: built-in defaults (no config file found)"),
    }
    let merged = layers.into_iter().fold(builtin_table(), deep_merge);
    let mut config: Config = merged
        .clone()
        .try_into()
        .context("applying project config over the built-in defaults")?;

    apply_inline_overrides(&mut config, &inline)?;
    apply_cli_overrides(&mut config, ignore_paths, cycle_rules, thresholds)?;
    validate_thresholds(&config)?;
    Ok(LoadedConfig {
        config,
        source_file,
        merged,
    })
}

/// The built-in default config as a raw table — the merge base every discovered
/// config layers over. Parsed from the embedded `defaults.toml` (the single
/// source of default values).
fn builtin_table() -> Table {
    DEFAULTS
        .parse()
        .expect("embedded defaults.toml parses as a table")
}

/// Validate every configured threshold key once the full config is known: a key
/// is legal if it is a registry per-file metric OR a project `[metrics.<key>]`.
/// Deferred here (not in the deserializer) so a custom metric — invisible to the
/// `MetricThresholds` deserializer — is accepted while a typo still fails fast.
fn validate_thresholds(cfg: &Config) -> Result<()> {
    for key in cfg.rules.thresholds.file.limits.keys() {
        if super::metrics::is_threshold_metric(key) || cfg.metrics.contains_key(key) {
            continue;
        }
        anyhow::bail!(
            "unknown threshold metric {key:?}; expected a per-file metric (e.g. sloc, loc, \
             cyclomatic, cognitive, hk, fan_in, fan_out, mi, volume, bugs) or a custom \
             [metrics.{key}] defined in this config"
        );
    }
    Ok(())
}

/// Discover the user's config as a raw [`Table`] (NOT yet deserialized into
/// [`Config`]) so the caller can deep-merge it over the built-in defaults.
/// The config layers to merge over the built-in defaults, in apply order (later
/// wins), plus a human label of the source(s) for the log line.
///
/// With explicit `--config FILE` paths, every file is read and returned in
/// command-line order (auto-discovery is skipped). Otherwise discovery returns a
/// single layer: `./code-ranker.toml` > `<workspace>/code-ranker.toml` >
/// `Cargo.toml [*.metadata.code-ranker]`. Returns `(vec![], None)` when nothing is
/// found (→ pure built-in defaults).
fn discover_user_tables(
    workspace: &Path,
    explicit: &[&Path],
) -> Result<(Vec<Table>, Option<String>)> {
    if !explicit.is_empty() {
        let mut layers = Vec::with_capacity(explicit.len());
        let mut labels = Vec::with_capacity(explicit.len());
        for path in explicit {
            layers.push(read_table(path)?);
            labels.push(path.display().to_string());
        }
        // Joined left-to-right so the log shows the merge order at a glance.
        return Ok((layers, Some(labels.join(" ⊕ "))));
    }

    let cwd = std::env::current_dir().unwrap_or_default();

    for dir in [cwd.as_path(), workspace] {
        let p = dir.join("code-ranker.toml");
        if p.exists() {
            let table = read_table(&p)?;
            let canonical = p.canonicalize().unwrap_or(p);
            return Ok((vec![table], Some(canonical.display().to_string())));
        }
    }

    for dir in [cwd.as_path(), workspace] {
        if let Some((table, src)) = table_from_cargo_toml(dir)? {
            return Ok((vec![table], Some(src)));
        }
    }

    Ok((Vec::new(), None))
}

/// Read and parse a `code-ranker.toml` file into a raw [`Table`] (the
/// suffixed-threshold pre-pass runs first, so `hk = 300K` parses).
fn read_table(path: &Path) -> Result<Table> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&quote_suffixed_thresholds(&text))
        .with_context(|| format!("parsing {}", path.display()))
}

fn table_from_cargo_toml(dir: &Path) -> Result<Option<(Table, String)>> {
    let cargo = dir.join("Cargo.toml");
    if !cargo.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&cargo).with_context(|| format!("reading {}", cargo.display()))?;
    let val: toml::Value = toml::from_str(&quote_suffixed_thresholds(&text))
        .with_context(|| format!("parsing {}", cargo.display()))?;

    let section = val
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("code-ranker"))
        .or_else(|| {
            val.get("package")
                .and_then(|p| p.get("metadata"))
                .and_then(|m| m.get("code-ranker"))
        });

    if let Some(v) = section {
        let table = v.as_table().cloned().with_context(|| {
            format!(
                "[*.metadata.code-ranker] in {} must be a table",
                cargo.display()
            )
        })?;
        let canonical = cargo.canonicalize().unwrap_or(cargo);
        return Ok(Some((
            table,
            format!("{}#metadata.code-ranker", canonical.display()),
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
            "ignore.gitignore" => cfg.ignore.gitignore = parse_on_off(value)?,
            "ignore.ignore_files" => cfg.ignore.ignore_files = parse_on_off(value)?,
            "ignore.hidden" => cfg.ignore.hidden = parse_on_off(value)?,
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
        "mutual" => cfg.rules.cycles.mutual = rule,
        "chain" => cfg.rules.cycles.chain = rule,
        other => anyhow::bail!("unknown cycle kind {other:?}; expected mutual|chain"),
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
    // Validity (registry metric ∪ custom `[metrics]`) is checked centrally in
    // `validate_thresholds`, once the whole config — including `[metrics]` — is
    // known; a CLI/inline override only records the limit here.
    bucket.set(metric.to_string(), val);
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

#[cfg(test)]
#[path = "load_test.rs"]
mod tests;
