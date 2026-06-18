//! C# [`Dialect`] for the generic metric engine.
//!
//! C# exposes standard `name` / `parameters` fields, so the default `unit_name`
//! and `args` work; this layer adds the cognitive state machine (if / loops /
//! switch / `?:` / `catch` nesting, `else`, `&&`/`||` runs, lambda closures) and
//! the function/closure classification.

use crate::engine::{CogCtx, CogState, Dialect, Roles, UnitKind};
use code_ranker_plugin_api::metrics::{FunctionUnit, MetricInputs};
use std::sync::LazyLock;
use tree_sitter::{Language, Node};

static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));

static ROLE_CFG: LazyLock<crate::engine::RoleCfg> = LazyLock::new(|| {
    CONFIG
        .clone()
        .try_into()
        .expect("csharp/config.toml [roles]/[halstead]/[loc] parse")
});

struct CsDialect {
    lang: Language,
    roles: Roles,
    unit_method: String,
    unit_default: String,
    field_alternative: String,
    method_declaration: u16,
    constructor_declaration: u16,
    local_function_statement: u16,
    lambda_expression: u16,
    anonymous_method_expression: u16,
    if_statement: u16,
    for_statement: u16,
    foreach_statement: u16,
    while_statement: u16,
    do_statement: u16,
    switch_statement: u16,
    conditional_expression: u16,
    catch_clause: u16,
    binary_expression: u16,
    kw_else: u16,
    amp_amp: u16,
    pipe_pipe: u16,
}

impl CsDialect {
    fn new() -> Self {
        let lang: Language = tree_sitter_c_sharp::LANGUAGE.into();
        let roles = Roles::resolve(&lang, &ROLE_CFG);
        let one = |k: &str| roles.one(k);
        let units = crate::config::units(&CONFIG);
        let fields = crate::config::string_table(&CONFIG, "fields");
        CsDialect {
            unit_method: units.get("method").cloned().expect("[units].method"),
            unit_default: units.get("default").cloned().expect("[units].default"),
            field_alternative: fields
                .get("alternative")
                .cloned()
                .expect("[fields].alternative"),
            method_declaration: one("method_declaration"),
            constructor_declaration: one("constructor_declaration"),
            local_function_statement: one("local_function_statement"),
            lambda_expression: one("lambda_expression"),
            anonymous_method_expression: one("anonymous_method_expression"),
            if_statement: one("if_statement"),
            for_statement: one("for_statement"),
            foreach_statement: one("foreach_statement"),
            while_statement: one("while_statement"),
            do_statement: one("do_statement"),
            switch_statement: one("switch_statement"),
            conditional_expression: one("conditional_expression"),
            catch_clause: one("catch_clause"),
            binary_expression: one("binary_expression"),
            kw_else: one("kw_else"),
            amp_amp: one("amp_amp"),
            pipe_pipe: one("pipe_pipe"),
            lang,
            roles,
        }
    }
}

static DIALECT: LazyLock<CsDialect> = LazyLock::new(CsDialect::new);

impl Dialect for CsDialect {
    fn language(&self) -> &Language {
        &self.lang
    }
    fn roles(&self) -> &Roles {
        &self.roles
    }

    fn file_initial_spaces(&self) -> u32 {
        1
    }

    fn classify_unit(&self, node: Node) -> Option<UnitKind> {
        let id = node.kind_id();
        if id == self.method_declaration
            || id == self.constructor_declaration
            || id == self.local_function_statement
        {
            Some(UnitKind::Func)
        } else if id == self.lambda_expression || id == self.anonymous_method_expression {
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
        let (mut cn, mut cl) = (nesting, lambda);

        if id == self.if_statement
            || id == self.for_statement
            || id == self.foreach_statement
            || id == self.while_statement
            || id == self.do_statement
            || id == self.switch_statement
            || id == self.conditional_expression
            || id == self.catch_clause
        {
            st.structural += nesting + depth + lambda + 1;
            cn = nesting + 1;
            st.boolean_op = None;
        } else if id == self.kw_else {
            let is_else_if = node
                .parent()
                .and_then(|p| p.child_by_field_name(&self.field_alternative))
                .is_some_and(|a| a.kind_id() == self.if_statement);
            if !is_else_if {
                st.structural += 1;
            }
        } else if id == self.binary_expression {
            let mut cur = node.walk();
            for c in node.children(&mut cur) {
                let cid = c.kind_id();
                if cid == self.amp_amp || cid == self.pipe_pipe {
                    st.eval_boolean(cid);
                }
            }
        } else if id == self.lambda_expression || id == self.anonymous_method_expression {
            cl = lambda + 1;
        }

        CogCtx {
            nesting: cn,
            depth,
            lambda: cl,
        }
    }

    fn is_function_unit(&self, node: Node) -> bool {
        let id = node.kind_id();
        id == self.method_declaration
            || id == self.constructor_declaration
            || id == self.local_function_statement
    }

    fn fn_kind(&self, node: Node) -> &str {
        let id = node.kind_id();
        if id == self.method_declaration || id == self.constructor_declaration {
            &self.unit_method
        } else {
            &self.unit_default
        }
    }
}

/// Parse `src` with tree-sitter-c-sharp and compute the file-level metrics.
pub fn compute(src: &[u8]) -> Option<MetricInputs> {
    crate::engine::compute(src, &*DIALECT)
}

/// Per-function metric units over each method / constructor / local-function subtree.
pub fn compute_functions(src: &[u8]) -> Vec<FunctionUnit> {
    crate::engine::compute_functions(src, &*DIALECT)
}

#[cfg(test)]
#[path = "tests/dialect.rs"]
mod dialect_tests;
