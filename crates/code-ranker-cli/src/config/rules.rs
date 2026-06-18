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
    node_attributes: &BTreeMap<String, AttributeSpec>,
    cycle_kinds: &BTreeMap<String, CycleKindSpec>,
) -> Option<RuleDoc> {
    if let Some(kind) = id.strip_prefix("cycle.") {
        let c = cycle_kinds.get(kind)?;
        return Some(RuleDoc {
            title: c.label.clone(),
            why: c.description.clone(),
            fix: c.remediation.clone(),
        });
    }
    let metric = id.rsplit('.').next().unwrap_or(id);
    let s = node_attributes.get(metric)?;
    Some(RuleDoc {
        title: s.name.clone().or_else(|| s.label.clone()),
        why: s.description.clone(),
        fix: s.remediation.clone(),
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

pub fn rule_tuning(id: &str) -> String {
    if let Some(kind) = id.strip_prefix("cycle.") {
        format!(
            "disable with --cycle-rule {kind}=off   ·   rules.cycles.{kind} in code-ranker.toml"
        )
    } else if let Some(rest) = id.strip_prefix("threshold.") {
        format!("set with --threshold {rest}=N   ·   rules.thresholds.{rest} in code-ranker.toml")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_doc_resolves_why_fix_from_specs_and_cycle_kinds() {
        use code_ranker_plugin_api::attrs::ValueType;
        let mut na = BTreeMap::new();
        let mut hk = AttributeSpec::new(ValueType::Float, "HK");
        hk.name = Some("Henry–Kafura".into());
        hk.description = Some("why-hk".into());
        hk.remediation = Some("fix-hk".into());
        na.insert("hk".to_string(), hk);
        let mut ck = BTreeMap::new();
        ck.insert(
            "mutual".to_string(),
            CycleKindSpec {
                label: Some("Mutual".into()),
                description: Some("why-cyc".into()),
                remediation: Some("fix-cyc".into()),
            },
        );

        // A threshold id resolves to its metric's node-attribute spec.
        let m = rule_doc("threshold.file.hk", &na, &ck).expect("metric doc");
        assert_eq!(m.title.as_deref(), Some("Henry–Kafura"));
        assert_eq!(m.why.as_deref(), Some("why-hk"));
        assert_eq!(m.fix.as_deref(), Some("fix-hk"));
        // A cycle id resolves to the cycle-kind spec.
        let c = rule_doc("cycle.mutual", &na, &ck).expect("cycle doc");
        assert_eq!(c.why.as_deref(), Some("why-cyc"));
        assert_eq!(c.fix.as_deref(), Some("fix-cyc"));
        // An unknown metric has no spec → no doc.
        assert!(rule_doc("threshold.file.bogus", &na, &ck).is_none());
    }

    #[test]
    fn rule_group_resolves_threshold_and_cycle_ids() {
        assert_eq!(rule_group("threshold.file.sloc"), "SIZ");
        assert_eq!(rule_group("threshold.file.cyclomatic"), "CPX");
        assert_eq!(rule_group("threshold.file.hk"), "CPL");
        assert_eq!(rule_group("cycle.mutual"), "CYC");
        assert_eq!(rule_group("threshold.file.bogus"), "?");
    }

    #[test]
    fn apply_cycle_rules_strips_disabled_kind() {
        use crate::config::model::CycleRule;
        let mut cycles = vec![CycleGroup {
            kind: "mutual".into(),
            nodes: vec!["a".into(), "b".into()],
        }];
        let mut nodes: Vec<Node> = vec![];
        // A kind whose budget is disabled is stripped from the groups.
        let rules = CycleRules {
            mutual: CycleRule::Off,
            chain: CycleRule::Max(0),
        };
        apply_cycle_rules(&mut cycles, &mut nodes, &rules);
        assert!(cycles.is_empty(), "disabled kind -> stripped");
    }

    #[test]
    fn apply_cycle_rules_clears_disabled_cycle_attr_on_nodes() {
        use crate::config::model::CycleRule;
        let mut cycles: Vec<CycleGroup> = vec![];
        let node = |id: &str, kind: &str| Node {
            id: id.into(),
            kind: "file".into(),
            name: id.into(),
            parent: None,
            attrs: [("cycle".to_string(), AttrValue::Str(kind.into()))]
                .into_iter()
                .collect(),
        };
        // `mutual` is disabled, `chain` keeps a budget — only the mutual node's
        // `cycle` attribute is cleared.
        let mut nodes = vec![node("a", "mutual"), node("b", "chain")];
        let rules = CycleRules {
            mutual: CycleRule::Off,
            chain: CycleRule::Max(3),
        };
        apply_cycle_rules(&mut cycles, &mut nodes, &rules);
        assert!(
            !nodes[0].attrs.contains_key("cycle"),
            "disabled-kind cycle attr cleared"
        );
        assert!(
            nodes[1].attrs.contains_key("cycle"),
            "an enabled kind's attr is kept"
        );
    }

    #[test]
    fn rule_tuning_emits_cli_and_config_hints() {
        assert!(rule_tuning("cycle.mutual").contains("--cycle-rule mutual=off"));
        assert!(rule_tuning("threshold.file.hk").contains("--threshold file.hk=N"));
        assert_eq!(rule_tuning("bogus.id"), "");
    }
}
