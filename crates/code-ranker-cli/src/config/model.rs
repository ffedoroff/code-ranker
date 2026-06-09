//! Config data model: the serde-deserialized `Config` tree and threshold
//! number parsing (`_` separators, K/M/G suffixes).

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};
use std::fmt;

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct OutputConfig {
    pub json: OutputArtifact,
    pub html: OutputArtifact,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OutputArtifact {
    pub path: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IgnoreConfig {
    pub paths: Vec<String>,
    /// Skip the language's test files during analysis. **On by default** —
    /// metrics and cycles describe production code unless you opt back in with
    /// `tests = false`. The plugin decides what counts as a test (see
    /// `LanguagePlugin::is_test_path`).
    #[serde(alias = "test_modules", alias = "test-modules")]
    pub tests: bool,
    /// Strip crates that appear only in [dev-dependencies].
    pub dev_only_crates: bool,
}

impl Default for IgnoreConfig {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            tests: true,
            dev_only_crates: false,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct CycleRules {
    pub mutual: CycleRule,
    pub chain: CycleRule,
}

impl Default for CycleRules {
    fn default() -> Self {
        Self {
            mutual: CycleRule::Max(0),
            chain: CycleRule::Max(0),
        }
    }
}

impl CycleRules {
    /// Budget for a cycle kind string (`"mutual"`/`"chain"`):
    /// `Some(max)` if enabled, `None` if disabled.
    pub fn budget_for(self, kind: &str) -> Option<u32> {
        match kind {
            "mutual" => self.mutual,
            "chain" => self.chain,
            _ => CycleRule::Off,
        }
        .budget()
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ThresholdRules {
    pub file: MetricThresholds,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
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
pub(crate) fn parse_number(s: &str) -> Result<f64> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_rules_default_strict() {
        let d = CycleRules::default();
        assert_eq!(d.mutual, CycleRule::Max(0));
        assert_eq!(d.chain, CycleRule::Max(0));
        assert_eq!(d.budget_for("mutual"), Some(0));
        assert_eq!(d.budget_for("chain"), Some(0));
        assert_eq!(d.budget_for("unknown"), None);
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
mutual = true
chain = 7
[rules.thresholds.file]
loc = 800
";
        let cfg: Config = toml::from_str(src).unwrap();
        assert_eq!(cfg.rules.cycles.mutual, CycleRule::Max(0));
        assert_eq!(cfg.rules.cycles.chain, CycleRule::Max(7));
        assert_eq!(cfg.rules.thresholds.file.loc, Some(800.0));
    }

    #[test]
    fn config_rejects_unknown_keys() {
        // A stale/mistyped key must be a hard error, not silently ignored.
        let top = toml::from_str::<Config>("oops = 1")
            .unwrap_err()
            .to_string();
        assert!(top.contains("unknown field"), "top-level: {top}");

        let nested = toml::from_str::<Config>("[output]\njson-name = \"x\"\n")
            .unwrap_err()
            .to_string();
        assert!(nested.contains("unknown field"), "nested: {nested}");
        assert!(nested.contains("json-name"), "names the bad key: {nested}");
    }
}
