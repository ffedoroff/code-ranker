//! Go [`Dialect`] for the generic metric engine.
//!
//! The walk logic lives in `crate::engine`; this is the thin Go-specific layer:
//! the grammar, the resolved [`Roles`] (from `go/config.toml`), and the few
//! predicates that differ for Go — the cognitive state machine (if / for /
//! switch / select nesting, `else`, short-circuit `&&` / `||` runs, closure and
//! nested-function nesting) and the function-unit classification.

use crate::engine::{self, CogCtx, CogState, Dialect, Roles, UnitKind};
use code_ranker_plugin_api::metrics::{FunctionUnit, MetricInputs};
use std::sync::LazyLock;
use tree_sitter::{Language, Node};

static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));

static ROLE_CFG: LazyLock<crate::engine::RoleCfg> = LazyLock::new(|| {
    CONFIG
        .clone()
        .try_into()
        .expect("go/config.toml [roles]/[halstead]/[loc] parse")
});

struct GoDialect {
    lang: Language,
    roles: Roles,
    unit_method: String,
    unit_default: String,
    field_alternative: String,
    // function / closure units
    function_declaration: u16,
    method_declaration: u16,
    func_literal: u16,
    // cognitive nesting nodes
    if_statement: u16,
    for_statement: u16,
    expression_switch_statement: u16,
    type_switch_statement: u16,
    select_statement: u16,
    expression_statement: u16,
    binary_expression: u16,
    kw_else: u16,
    amp_amp: u16,
    pipe_pipe: u16,
}

impl GoDialect {
    fn new() -> Self {
        let lang: Language = tree_sitter_go::LANGUAGE.into();
        let roles = Roles::resolve(&lang, &ROLE_CFG);
        let one = |k: &str| roles.one(k);
        let units = crate::config::units(&CONFIG);
        let unit = |k: &str| units.get(k).cloned().expect("[units] key");
        let fields = crate::config::string_table(&CONFIG, "fields");
        GoDialect {
            unit_method: unit("method"),
            unit_default: unit("default"),
            field_alternative: fields
                .get("alternative")
                .cloned()
                .expect("[fields].alternative"),
            function_declaration: one("function_declaration"),
            method_declaration: one("method_declaration"),
            func_literal: one("func_literal"),
            if_statement: one("if_statement"),
            for_statement: one("for_statement"),
            expression_switch_statement: one("expression_switch_statement"),
            type_switch_statement: one("type_switch_statement"),
            select_statement: one("select_statement"),
            expression_statement: one("expression_statement"),
            binary_expression: one("binary_expression"),
            kw_else: one("kw_else"),
            amp_amp: one("amp_amp"),
            pipe_pipe: one("pipe_pipe"),
            lang,
            roles,
        }
    }
}

static DIALECT: LazyLock<GoDialect> = LazyLock::new(GoDialect::new);

impl Dialect for GoDialect {
    fn language(&self) -> &Language {
        &self.lang
    }
    fn roles(&self) -> &Roles {
        &self.roles
    }

    fn file_initial_spaces(&self) -> u32 {
        1 // the source_file unit space (source_file is not in space_kinds)
    }

    fn classify_unit(&self, node: Node) -> Option<UnitKind> {
        let id = node.kind_id();
        if id == self.function_declaration || id == self.method_declaration {
            Some(UnitKind::Func)
        } else if id == self.func_literal {
            Some(UnitKind::Closure)
        } else {
            None
        }
    }

    fn cog_node(&self, node: Node, ctx: CogCtx, st: &mut CogState) -> CogCtx {
        let id = node.kind_id();
        let CogCtx {
            nesting,
            depth,
            lambda,
        } = ctx;
        // Go has no nested named functions (only func literals → `lambda`), so
        // `depth` never increments here; it stays as inherited.
        let (mut cn, cd, mut cl) = (nesting, depth, lambda);

        if id == self.if_statement
            || id == self.for_statement
            || id == self.expression_switch_statement
            || id == self.type_switch_statement
            || id == self.select_statement
        {
            st.structural += nesting + depth + lambda + 1;
            cn = nesting + 1;
            st.boolean_op = None;
        } else if id == self.kw_else {
            // `else { … }` adds +1 (no nesting); `else if …` does not double-count
            // (the nested if_statement above already increments).
            let is_else_if = node
                .parent()
                .and_then(|p| p.child_by_field_name(&self.field_alternative))
                .is_some_and(|a| a.kind_id() == self.if_statement);
            if !is_else_if {
                st.structural += 1;
            }
        } else if id == self.expression_statement {
            st.boolean_op = None;
        } else if id == self.binary_expression {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                let cid = child.kind_id();
                if cid == self.amp_amp || cid == self.pipe_pipe {
                    st.eval_boolean(cid);
                }
            }
        } else if id == self.func_literal {
            cl = lambda + 1;
        }

        CogCtx {
            nesting: cn,
            depth: cd,
            lambda: cl,
        }
    }

    fn is_function_unit(&self, node: Node) -> bool {
        let id = node.kind_id();
        id == self.function_declaration || id == self.method_declaration
    }

    fn fn_kind(&self, node: Node) -> &str {
        if node.kind_id() == self.method_declaration {
            &self.unit_method
        } else {
            &self.unit_default
        }
    }
}

/// Parse `src` with tree-sitter-go and compute the file-level metrics.
pub fn compute(src: &[u8]) -> Option<MetricInputs> {
    engine::compute(src, &*DIALECT)
}

/// Per-function metric units over each function / method declaration subtree.
pub fn compute_functions(src: &[u8]) -> Vec<FunctionUnit> {
    engine::compute_functions(src, &*DIALECT)
}

#[cfg(test)]
#[path = "tests/dialect.rs"]
mod dialect_tests;
