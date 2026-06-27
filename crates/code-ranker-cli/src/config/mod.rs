//! Configuration: data model, loading, graph filtering, rule catalog, and rule
//! evaluation — one submodule per concern. This module re-exports the surface the
//! CLI consumes as `config::…`. Unlike the foundational crates this is a safe
//! place for a re-export facade: its fan-in is small, so the squared-coupling HK
//! term stays negligible.

pub mod ignore;
pub mod load;
pub mod metrics;
pub mod model;
pub mod rules;
pub mod violations;

pub use ignore::apply_ignore;
pub use load::load;
pub(crate) use model::merge_project_principles;
pub use model::{CycleRules, OutputArtifact, OutputConfig, RulesConfig, TemplatesConfig};
pub use rules::{apply_cycle_rules, rule_doc, rule_tuning};
pub use violations::{Violation, check_violations_all};
