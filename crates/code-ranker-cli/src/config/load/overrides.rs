//! Apply the transient per-run flag overrides — inline `--config KEY=VALUE`,
//! `--cycle-rule`, `--threshold` — onto an already-deserialized [`Config`].
//!
//! These helpers depend only on their arguments and the config-model types from
//! [`super::super::model`]; they never reference items defined in the parent
//! `load` module, so the parent can import them back without forming a cycle.

use crate::config::model::{Config, CycleRule, MetricThresholds, parse_number};
use anyhow::{Context, Result};

pub(crate) fn apply_cli_overrides(
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

pub(crate) fn apply_inline_overrides(cfg: &mut Config, entries: &[&str]) -> Result<()> {
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
            "levels.functions" => cfg.levels.functions = parse_on_off(value)?,
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
            "templates.prompt" => cfg.templates.prompt = Some(value.to_string()),
            _ if key.strip_prefix("templates.languages.").is_some() => {
                // `templates.languages.<lang>.<ID>=path` — override one doc fragment.
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
            other => anyhow::bail!("unknown config key {other:?}"),
        }
    }
    Ok(())
}

pub(crate) fn set_cycle(cfg: &mut Config, kind: &str, rule: CycleRule) -> Result<()> {
    match kind {
        "mutual" => cfg.rules.cycles.mutual = rule,
        "chain" => cfg.rules.cycles.chain = rule,
        other => anyhow::bail!("unknown cycle kind {other:?}; expected mutual|chain"),
    }
    Ok(())
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
        [scope, metric] => Ok((scope, metric)),
        _ => anyhow::bail!("threshold must be file.METRIC, got: {path}"),
    }
}

pub(crate) fn set_threshold(cfg: &mut Config, scope: &str, metric: &str, val: f64) -> Result<()> {
    let st = match scope {
        "file" => &mut cfg.rules.thresholds.file,
        other => {
            anyhow::bail!("unknown threshold scope {other:?}; the only scope is `file`")
        }
    };
    set_metric(st, metric, val)
}

pub(crate) fn set_metric(bucket: &mut MetricThresholds, metric: &str, val: f64) -> Result<()> {
    // Validity (registry metric ∪ custom `[metrics]`) is checked centrally in
    // `validate_thresholds`, once the whole config — including `[metrics]` — is
    // known; a CLI/inline override only records the limit here.
    bucket.set(metric.to_string(), val);
    Ok(())
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
