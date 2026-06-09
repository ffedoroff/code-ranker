//! Config loading: discover `code-ranker.toml` (or `Cargo.toml` metadata),
//! apply inline `KEY=VALUE` and `--cycle-rule` / `--threshold` CLI overrides.

use super::model::{Config, CycleRule, MetricThresholds, parse_number};
use anyhow::{Context, Result};
use code_ranker_plugin_api::log;
use std::path::Path;

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
    match &source_file {
        Some(p) => log::line(&format!("config: {p}")),
        None => log::line("config: built-in defaults (no config file found)"),
    }
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
        let p = dir.join("code-ranker.toml");
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
        .and_then(|m| m.get("code-ranker"))
        .or_else(|| {
            val.get("package")
                .and_then(|p| p.get("metadata"))
                .and_then(|m| m.get("code-ranker"))
        });

    if let Some(v) = section {
        let cfg: Config = v
            .clone()
            .try_into()
            .with_context(|| format!("parsing [*.metadata.code-ranker] in {}", cargo.display()))?;
        let canonical = cargo.canonicalize().unwrap_or(cargo);
        return Ok(Some((
            cfg,
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn cli_override_sets_cycle_and_threshold() {
        let mut cfg = Config::default();
        apply_cli_overrides(
            &mut cfg,
            &[],
            &["chain=on".into(), "mutual=off".into()],
            &["file.cognitive=25".into(), "file.hk=1000".into()],
        )
        .unwrap();
        assert_eq!(cfg.rules.cycles.chain, CycleRule::Max(0));
        assert_eq!(cfg.rules.cycles.mutual, CycleRule::Off);
        assert_eq!(cfg.rules.thresholds.file.cognitive, Some(25.0));
        assert_eq!(cfg.rules.thresholds.file.hk, Some(1000.0));
    }

    #[test]
    fn inline_overrides_set_each_key() {
        let mut cfg = Config::default();
        apply_inline_overrides(
            &mut cfg,
            &[
                "plugin=rust",
                "ignore.tests=on",
                "ignore.dev_only_crates=true",
                "ignore.paths=a/**, b/**",
                "output.json.path=out.json",
                "output.html.path=out.html",
                "output.json.enabled=off",
                "output.html.enabled=true",
                "rules.cycles.chain=7",
                "rules.thresholds.file.loc=800",
            ],
        )
        .unwrap();
        assert_eq!(cfg.plugin.as_deref(), Some("rust"));
        assert!(cfg.ignore.tests && cfg.ignore.dev_only_crates);
        assert_eq!(cfg.ignore.paths, ["a/**", "b/**"]);
        assert_eq!(cfg.output.json.path.as_deref(), Some("out.json"));
        assert_eq!(cfg.output.html.path.as_deref(), Some("out.html"));
        assert_eq!(cfg.output.json.enabled, Some(false));
        assert_eq!(cfg.output.html.enabled, Some(true));
        assert_eq!(cfg.rules.cycles.chain, CycleRule::Max(7));
        assert_eq!(cfg.rules.thresholds.file.loc, Some(800.0));
    }

    #[test]
    fn inline_overrides_reject_bad_input() {
        let mut cfg = Config::default();
        assert!(apply_inline_overrides(&mut cfg, &["no_equals_sign"]).is_err());
        assert!(apply_inline_overrides(&mut cfg, &["totally.unknown=1"]).is_err());
    }

    #[test]
    fn parse_cycle_rule_variants() {
        assert_eq!(parse_cycle_rule("on").unwrap(), CycleRule::Max(0));
        assert_eq!(parse_cycle_rule("true").unwrap(), CycleRule::Max(0));
        assert_eq!(parse_cycle_rule("off").unwrap(), CycleRule::Off);
        assert_eq!(parse_cycle_rule("false").unwrap(), CycleRule::Off);
        assert_eq!(parse_cycle_rule("7").unwrap(), CycleRule::Max(7));
        assert!(parse_cycle_rule("-1").is_err());
        assert!(parse_cycle_rule("nope").is_err());
    }

    #[test]
    fn parse_threshold_path_shape() {
        assert_eq!(parse_threshold_path("file.loc").unwrap(), ("file", "loc"));
        assert!(parse_threshold_path("loc").is_err());
        assert!(parse_threshold_path("a.b.c").is_err());
    }

    #[test]
    fn set_metric_each_then_unknown() {
        let mut b = MetricThresholds::default();
        for m in ["hk", "cyclomatic", "cognitive", "fan_in", "fan_out", "loc"] {
            set_metric(&mut b, m, 1.0).unwrap();
        }
        assert!(set_metric(&mut b, "bogus", 1.0).is_err());
    }

    #[test]
    fn set_threshold_and_cycle_reject_unknowns() {
        let mut cfg = Config::default();
        assert!(set_threshold(&mut cfg, "function", "loc", 1.0).is_err());
        set_threshold(&mut cfg, "file", "hk", 5.0).unwrap();
        assert_eq!(cfg.rules.thresholds.file.hk, Some(5.0));
        assert!(set_cycle(&mut cfg, "weird", CycleRule::Off).is_err());
    }

    #[test]
    fn split_kv_requires_equals() {
        assert_eq!(split_kv("a=b", "x").unwrap(), ("a", "b"));
        assert!(split_kv("noeq", "x").is_err());
    }
}
