//! Cycle-rule application (strip disabled kinds) and the rule documentation
//! catalog used by diagnostics.

use super::model::CycleRules;
use code_ranker_graph::level_graph::CycleGroup;
use code_ranker_plugin_api::{
    attrs::AttrValue,
    level::{AttributeSpec, CycleKindSpec},
    node::Node,
};
use std::collections::{BTreeMap, HashSet};

/// Strip disabled cycle kinds from the cycle groups and clear the matching
/// `cycle` node attributes.
pub fn apply_cycle_rules(cycles: &mut Vec<CycleGroup>, nodes: &mut [Node], rules: &CycleRules) {
    let disabled: HashSet<&str> = ["mutual", "chain"]
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

/// Resolved diagnostic copy for one rule, pulled from the active snapshot's
/// specs (**data, not Rust**): `why` is the metric/cycle `description`, `fix`
/// its `remediation`, `title` its display name. No prose lives here — it comes
/// from `builtin.toml` (metrics / coupling / cycles) and the language configs
/// (structural metrics).
pub struct RuleDoc {
    pub title: Option<String>,
    pub why: Option<String>,
    pub fix: Option<String>,
}

/// Resolve the diagnostic copy for a rule id from the active level's specs: a
/// `cycle.<kind>` reads the level's `cycle_kinds` spec; any other id (a
/// `threshold.<scope>.<metric>` or a bare metric key) reads the `node_attributes`
/// spec for its trailing metric key. `None` when no spec matches.
pub fn rule_doc(
    id: &str,
    lang: &str,
    node_attributes: &BTreeMap<String, AttributeSpec>,
    cycle_kinds: &BTreeMap<String, CycleKindSpec>,
) -> Option<RuleDoc> {
    if let Some(kind) = id.strip_prefix("cycle.") {
        let c = cycle_kinds.get(kind)?;
        return Some(RuleDoc {
            title: c.label.clone(),
            why: c.description.clone(),
            // `{lang}` in an authored remediation → the resolved language, so a
            // `code-ranker docs {lang} ADP` pointer is runnable as printed.
            fix: c.remediation.clone().map(|r| r.replace("{lang}", lang)),
        });
    }
    let metric = id.rsplit('.').next().unwrap_or(id);
    let s = node_attributes.get(metric)?;
    // A metric's `fix` is its own `remediation` when one is authored (a project
    // `[metrics.<key>]` may set a custom fix); otherwise auto-derive a command that
    // generates the AI fix-prompt for this metric, so the built-in catalog carries no
    // duplicated boilerplate and the command always names the correct subject
    // (`report --plugins <lang> --prompt <key>`).
    let fix = s
        .remediation
        .clone()
        .map(|r| r.replace("{lang}", lang))
        .or_else(|| {
            Some(format!(
                "Run `code-ranker report --plugins {lang} --prompt {metric}` to generate an AI fix-prompt."
            ))
        });
    Some(RuleDoc {
        title: s.name.clone().or_else(|| s.label.clone()),
        why: s.description.clone(),
        fix,
    })
}

/// The concern group for any rule id — `CYC` for a `cycle.*` kind, else the
/// `threshold.<scope>.<metric>` metric's group from the leaf [`super::metrics`]
/// vocabulary, falling back to `?` for an unknown id.
pub fn rule_group(id: &str) -> &'static str {
    if id.starts_with("cycle.") {
        return "CYC";
    }
    let metric = id.rsplit('.').next().unwrap_or(id);
    super::metrics::threshold_metric(metric)
        .map(|m| m.group)
        .unwrap_or("?")
}

pub fn rule_tuning(id: &str, lang: &str) -> String {
    // Rules are per-language: tune one language under `[plugins.<lang>]`, or the
    // shared `[plugins.base]` layer to affect every language at once.
    if let Some(kind) = id.strip_prefix("cycle.") {
        format!(
            "set with --config plugins.{lang}.rules.cycles.{kind}=off   ·   \
             plugins.{lang}.rules.cycles.{kind} in code-ranker.toml (or plugins.base for all)"
        )
    } else if let Some(rest) = id.strip_prefix("threshold.") {
        format!(
            "set with --config plugins.{lang}.rules.thresholds.{rest}=N   ·   \
             plugins.{lang}.rules.thresholds.{rest} in code-ranker.toml (or plugins.base for all)"
        )
    } else {
        String::new()
    }
}

#[cfg(test)]
#[path = "rules_test.rs"]
mod tests;
