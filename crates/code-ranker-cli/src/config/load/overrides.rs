//! Apply the transient per-run flag overrides — inline `--config KEY=VALUE`,
//! `--cycle-rule`, `--threshold` — onto an already-deserialized [`Config`].
//!
//! The per-language sections (`ignore`/`rules`/`metrics`/`levels`/`report`/
//! `principles`) live in raw `[plugins.<lang>]` tables (read later via
//! `Config::language_config`), so their overrides are written into those raw tables
//! here rather than into typed fields. A bare top-level key (e.g. `ignore.tests`,
//! `rules.thresholds.file.hk`) targets the shared `[plugins.base]` layer; a
//! `plugins.<lang>.<path>` key targets one language. Only `[output]` / `[templates]`
//! remain typed (global), so those keep their explicit arms.

use crate::config::model::{Config, CycleRule, parse_number};
use anyhow::{Context, Result};

pub(crate) fn apply_cli_overrides(
    cfg: &mut Config,
    ignore_paths: &[String],
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<()> {
    // `--ignore` paths extend the base-language ignore globs.
    if !ignore_paths.is_empty() {
        let base = base_table(cfg);
        let arr = ensure_array(base, &["ignore", "paths"]);
        arr.extend(ignore_paths.iter().map(|p| toml::Value::String(p.clone())));
    }

    for raw in cycle_rules {
        let (kind, state) = split_kv(raw, "cycle-rule")?;
        // Validate kind / state, then store as a raw cycle value on the base layer.
        let value = cycle_value(parse_cycle_rule(state)?);
        if kind != "mutual" && kind != "chain" {
            anyhow::bail!("unknown cycle kind {kind:?}; expected mutual|chain");
        }
        set_path(base_table(cfg), &["rules", "cycles", kind], value);
    }

    for raw in thresholds {
        let (path, val_str) = split_kv(raw, "threshold")?;
        let val = parse_number(val_str).with_context(|| format!("in --threshold {raw}"))?;
        let (scope, metric) = parse_threshold_path(path)?;
        set_path(
            base_table(cfg),
            &["rules", "thresholds", scope, metric],
            number_value(val),
        );
    }

    Ok(())
}

pub(crate) fn apply_inline_overrides(cfg: &mut Config, entries: &[&str]) -> Result<()> {
    for raw in entries {
        let (key, value) = raw
            .split_once('=')
            .with_context(|| format!("--config override must be KEY=VALUE, got: {raw}"))?;
        // Normalize the `ignore.tests` aliases to the canonical key so a raw-table
        // write overwrites the default `tests` rather than adding a duplicate
        // alias key (which the alias-aware deserializer would reject).
        let key = match key {
            "ignore.test_modules" | "ignore.test-modules" => "ignore.tests",
            other => other,
        };
        match key {
            // The active-language list.
            "plugins" | "plugins.enabled" => {
                cfg.plugins.enabled = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            // Global, typed output config.
            "output.json.path" => cfg.output.json.path = Some(value.to_string()),
            "output.html.path" => cfg.output.html.path = Some(value.to_string()),
            "output.json.enabled" => cfg.output.json.enabled = Some(parse_on_off(value)?),
            "output.html.enabled" => cfg.output.html.enabled = Some(parse_on_off(value)?),
            // Global, typed templates config.
            "templates.prompt" => cfg.templates.prompt = Some(value.to_string()),
            _ if key.strip_prefix("templates.languages.").is_some() => {
                let rest = key.strip_prefix("templates.languages.").unwrap();
                let (lang, id) = rest.split_once('.').with_context(|| {
                    format!(
                        "--config templates key must be templates.languages.<lang>.<ID>, got: {key}"
                    )
                })?;
                cfg.templates
                    .languages
                    .entry(lang.to_string())
                    .or_default()
                    .insert(id.to_string(), value.to_string());
            }
            // `plugins.<lang>.<path>=value` — a leaf override into one language's
            // raw block (scalars / comma-lists only; deep tables need a TOML block).
            _ if key.strip_prefix("plugins.").is_some() => {
                let rest = key.strip_prefix("plugins.").unwrap();
                let (lang, path) = rest.split_once('.').with_context(|| {
                    format!("--config plugins key must be plugins.<lang>.<path>, got: {key}")
                })?;
                let segs: Vec<&str> = path.split('.').collect();
                let table = cfg.plugins.languages.entry(lang.to_string()).or_default();
                set_path(table, &segs, parse_leaf_value(value));
            }
            // A known per-language section key (e.g. `ignore.tests`,
            // `rules.thresholds.file.hk`, `levels.functions`, `metrics.*`) applies to
            // every language via the shared `[plugins.base]` layer.
            _ if key.split_once('.').is_some_and(|(head, _)| {
                crate::config::model::LANG_SECTION_KEYS.contains(&head)
            }) =>
            {
                let segs: Vec<&str> = key.split('.').collect();
                set_path(base_table(cfg), &segs, parse_leaf_value(value));
            }
            other => anyhow::bail!("unknown config key {other:?}"),
        }
    }
    Ok(())
}

/// The raw `[plugins.base]` override table, created on first use.
fn base_table(cfg: &mut Config) -> &mut toml::Table {
    cfg.plugins.languages.entry("base".to_string()).or_default()
}

/// Insert `value` at a dotted key path within a raw table, creating intermediate
/// tables (replacing a non-table value in the way, which can only be a misuse).
fn set_path(table: &mut toml::Table, path: &[&str], value: toml::Value) {
    match path {
        [] => {}
        [last] => {
            table.insert((*last).to_string(), value);
        }
        [head, rest @ ..] => {
            let entry = table
                .entry((*head).to_string())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if !entry.is_table() {
                *entry = toml::Value::Table(toml::Table::new());
            }
            set_path(
                entry.as_table_mut().expect("just ensured table"),
                rest,
                value,
            );
        }
    }
}

/// Get (creating if needed) a mutable array at a dotted path.
fn ensure_array<'a>(table: &'a mut toml::Table, path: &[&str]) -> &'a mut Vec<toml::Value> {
    // Walk/create intermediate tables, then ensure the leaf is an array.
    let (head, rest) = path.split_first().expect("non-empty path");
    if rest.is_empty() {
        let entry = table
            .entry((*head).to_string())
            .or_insert_with(|| toml::Value::Array(Vec::new()));
        if !entry.is_array() {
            *entry = toml::Value::Array(Vec::new());
        }
        return entry.as_array_mut().expect("just ensured array");
    }
    let entry = table
        .entry((*head).to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !entry.is_table() {
        *entry = toml::Value::Table(toml::Table::new());
    }
    ensure_array(entry.as_table_mut().expect("just ensured table"), rest)
}

/// The raw TOML value for a cycle rule: `true` (strict / `Max(0)`), `false`
/// (off), or an integer budget.
fn cycle_value(rule: CycleRule) -> toml::Value {
    match rule {
        CycleRule::Off => toml::Value::Boolean(false),
        CycleRule::Max(0) => toml::Value::Boolean(true),
        CycleRule::Max(n) => toml::Value::Integer(n as i64),
    }
}

/// A numeric threshold as a TOML value (integer when whole, else float).
fn number_value(v: f64) -> toml::Value {
    if v.fract() == 0.0 && v.abs() < i64::MAX as f64 {
        toml::Value::Integer(v as i64)
    } else {
        toml::Value::Float(v)
    }
}

pub(crate) fn parse_cycle_rule(s: &str) -> Result<CycleRule> {
    match s {
        "on" | "true" => Ok(CycleRule::Max(0)),
        "off" | "false" => Ok(CycleRule::Off),
        other => other.parse::<u32>().map(CycleRule::Max).with_context(|| {
            format!("cycle rule must be on|off or a non-negative integer, got {other:?}")
        }),
    }
}

pub(crate) fn parse_threshold_path(path: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = path.split('.').collect();
    match parts.as_slice() {
        [scope, metric] if *scope == "file" => Ok((scope, metric)),
        [scope, _] => anyhow::bail!("unknown threshold scope {scope:?}; the only scope is `file`"),
        _ => anyhow::bail!("threshold must be file.METRIC, got: {path}"),
    }
}

pub(crate) fn split_kv<'a>(s: &'a str, flag: &str) -> Result<(&'a str, &'a str)> {
    s.split_once('=')
        .with_context(|| format!("--{flag} must be key=value, got: {s}"))
}

pub(crate) fn parse_on_off(s: &str) -> Result<bool> {
    match s {
        "on" | "true" => Ok(true),
        "off" | "false" => Ok(false),
        other => anyhow::bail!("expected on|off, got {:?}", other),
    }
}

/// Parse a leaf CLI value for a raw-table override.
///
/// Supported forms (scalars + comma-lists only — deep nested tables must use a full
/// `[plugins.<lang>]` TOML block):
/// - `"on"` / `"true"` → `true`; `"off"` / `"false"` → `false`
/// - a bare integer (no decimal) → TOML integer
/// - a bare float (has `.`) → TOML float
/// - a comma-separated list (`a,b,c`) → TOML array of strings
/// - anything else → TOML string (suffixed numbers like `8K` are parsed later by
///   the threshold deserializer)
pub(crate) fn parse_leaf_value(s: &str) -> toml::Value {
    match s {
        "true" | "on" => toml::Value::Boolean(true),
        "false" | "off" => toml::Value::Boolean(false),
        _ if s.contains(',') => {
            let arr: Vec<toml::Value> = s
                .split(',')
                .map(|item| toml::Value::String(item.trim().to_string()))
                .collect();
            toml::Value::Array(arr)
        }
        _ => {
            if !s.contains('.') {
                if let Ok(i) = s.parse::<i64>() {
                    return toml::Value::Integer(i);
                }
            } else if let Ok(f) = s.parse::<f64>() {
                return toml::Value::Float(f);
            }
            toml::Value::String(s.to_string())
        }
    }
}
