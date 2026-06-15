//! Cycle-rule application (strip disabled kinds) and the rule documentation
//! catalog used by diagnostics.

use super::model::CycleRules;
use code_ranker_graph::level_graph::CycleGroup;
use code_ranker_plugin_api::{attrs::AttrValue, node::Node};
use std::collections::HashSet;

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
        why: "Henry-Kafura — sloc × (fan_in × fan_out)² — flags units that are both highly connected \
              and large: change-amplifiers whose edits ripple widely across the system.",
        fix: "Cut fan-in or fan-out: narrow the public surface, split the unit by responsibility, or \
              route dependencies through a smaller interface. Shrinking the file (sloc) also lowers hk.",
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

/// The concern group for any rule id — a `cycle.*` kind or a
/// `threshold.<scope>.<metric>` — falling back to `?` for an unknown id. The
/// threshold vocabulary lives in the leaf [`super::metrics`] module.
pub fn rule_group(id: &str) -> &'static str {
    if id.starts_with("cycle.") {
        return rule_doc(id).map(|d| d.group).unwrap_or("CYC");
    }
    let metric = id.rsplit('.').next().unwrap_or(id);
    super::metrics::threshold_metric(metric)
        .map(|m| m.group)
        .or_else(|| rule_doc(id).map(|d| d.group))
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
}
