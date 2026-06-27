//! Config loading: discover `code-ranker.toml` (or `Cargo.toml` metadata),
//! apply inline `KEY=VALUE` and `--cycle-rule` / `--threshold` CLI overrides.

use super::model::{Config, DEFAULTS, quote_suffixed_thresholds};
use anyhow::{Context, Result};
use code_ranker_plugin_api::log;
use code_ranker_plugin_api::toml_merge::deep_merge;
use std::path::Path;
use toml::Table;

mod overrides;
// Re-export the flag-override helpers so the `load` body and `load_test.rs`
// (which pulls them in via `super::*`) keep compiling unchanged. The helpers
// live in a sibling module and depend only on the `model` types, so this
// import does not form a parent↔child cycle.
use overrides::{apply_cli_overrides, apply_inline_overrides};
// The remaining override helpers are referenced only by `load_test.rs` (via
// `super::*`); import them under `#[cfg(test)]` so normal builds stay warning-free.
#[cfg(test)]
use overrides::{parse_cycle_rule, parse_on_off, parse_threshold_path, split_kv};

#[derive(Debug)]
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
        Some(p) => log::verbose(&format!("config: {p}")),
        None => log::verbose("config: built-in defaults (no config file found)"),
    }
    let merged = layers.into_iter().fold(builtin_table(), deep_merge);

    // Hard-error on the legacy singular `plugin` key before serde gets a chance to
    // reject it with a cryptic `unknown field`. This lets us give a directed
    // migration message instead.
    if merged.contains_key("plugin") {
        anyhow::bail!(
            "`plugin = \"x\"` is no longer supported; use `plugins = [\"x\"]` instead \
             (version 5.0 schema)"
        );
    }

    let mut config: Config = merged
        .clone()
        .try_into()
        .context("applying project config over the built-in defaults")?;

    apply_inline_overrides(&mut config, &inline)?;
    apply_cli_overrides(&mut config, ignore_paths, cycle_rules, thresholds)?;
    validate_thresholds(&config)?;
    validate_schema_version(&config, &source_file)?;
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
/// is legal if it is a registry per-file metric, a project `[metrics.<key>]`, OR
/// a metric key declared under any `[languages.*].metrics` table (a per-language
/// custom metric is a valid global-threshold target).
/// Deferred here (not in the deserializer) so custom metrics — invisible to the
/// `MetricThresholds` deserializer — are accepted while a typo still fails fast.
fn validate_thresholds(cfg: &Config) -> Result<()> {
    // Thresholds are per-language now: validate each configured language's effective
    // `[rules.thresholds.file]` (base ⊕ <lang>) against that language's metric
    // vocabulary (registry metrics ∪ its own `[metrics.<key>]`).
    for lang in cfg.plugins.languages.keys() {
        let lc = cfg.language_config(lang)?;
        for key in lc.rules.thresholds.file.limits.keys() {
            if super::metrics::is_threshold_metric(key) || lc.metrics.contains_key(key) {
                continue;
            }
            anyhow::bail!(
                "unknown threshold metric {key:?} under [plugins.{lang}]; expected a per-file \
                 metric (e.g. sloc, loc, cyclomatic, cognitive, hk, fan_in, fan_out, mi, volume, \
                 bugs) or a custom [plugins.{lang}.metrics.{key}] / [plugins.base.metrics.{key}]"
            );
        }
    }
    Ok(())
}

/// Require a discovered config file to declare a compatible `version`. Pure
/// built-in defaults (no file found) have nothing to version, so the check is
/// skipped there. An exact `major.minor` mismatch fails with a directional hint
/// (migrate the config vs upgrade the tool) — far clearer than the `unknown field`
/// error a stale schema would otherwise raise under `deny_unknown_fields`.
fn validate_schema_version(cfg: &Config, source_file: &Option<String>) -> Result<()> {
    let Some(src) = source_file else {
        return Ok(());
    };
    let want = super::model::CONFIG_SCHEMA_VERSION;
    match cfg.version.as_deref() {
        Some(v) if v == want => Ok(()),
        None => anyhow::bail!(
            "config {src} is missing the required `version`. Add `version = \"{want}\"` \
             (the config-schema version this code-ranker supports)."
        ),
        Some(v) => {
            let hint = match (parse_major_minor(v), parse_major_minor(want)) {
                (Some(got), Some(exp)) if got < exp => "migrate the config to the new schema",
                (Some(got), Some(exp)) if got > exp => {
                    "upgrade code-ranker to a version that supports it"
                }
                _ => "migrate the config, or upgrade code-ranker",
            };
            anyhow::bail!(
                "config {src} declares schema `version = {v:?}`, but this code-ranker expects \
                 {want:?} — {hint}."
            )
        }
    }
}

/// Parse a `major.minor` (ignoring any patch/pre-release tail) for ordering.
fn parse_major_minor(s: &str) -> Option<(u32, u32)> {
    let mut it = s.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
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

#[cfg(test)]
#[path = "load_test.rs"]
mod tests;
