//! C++ [`Dialect`] for the generic metric engine.
//!
//! Like C (`../c/dialect.rs`) plus the C++ constructs: lambdas (closures),
//! methods (`fn_kind` keys on the enclosing class/struct), range-for and `catch`
//! as cognitive nesting. Parameters and the function name nest under a
//! `function_declarator`, so `args` / `unit_name` are overridden as in C.

use crate::engine::{CogCtx, CogState, Dialect, Roles, UnitKind};
use code_ranker_plugin_api::metrics::{FunctionUnit, MetricInputs};
use std::collections::HashSet;
use std::sync::LazyLock;
use tree_sitter::{Language, Node};

static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));

static ROLE_CFG: LazyLock<crate::engine::RoleCfg> = LazyLock::new(|| {
    CONFIG
        .clone()
        .try_into()
        .expect("cpp/config.toml [roles]/[halstead]/[loc] parse")
});

struct CppDialect {
    lang: Language,
    roles: Roles,
    unit_method: String,
    unit_default: String,
    field_declarator: String,
    function_definition: u16,
    function_declarator: u16,
    lambda_expression: u16,
    class_specifier: u16,
    struct_specifier: u16,
    if_statement: u16,
    for_statement: u16,
    for_range_loop: u16,
    while_statement: u16,
    do_statement: u16,
    switch_statement: u16,
    conditional_expression: u16,
    catch_clause: u16,
    else_clause: u16,
    binary_expression: HashSet<u16>,
    parameter_list: u16,
    parameter_declaration: u16,
    optional_parameter_declaration: u16,
    variadic_parameter_declaration: u16,
    amp_amp: u16,
    pipe_pipe: u16,
}

impl CppDialect {
    fn new() -> Self {
        let lang: Language = tree_sitter_cpp::LANGUAGE.into();
        let roles = Roles::resolve(&lang, &ROLE_CFG);
        let one = |k: &str| roles.one(k);
        let g = |k: &str| roles.group(k).clone();
        let units = crate::config::units(&CONFIG);
        let fields = crate::config::string_table(&CONFIG, "fields");
        CppDialect {
            unit_method: units.get("method").cloned().expect("[units].method"),
            unit_default: units.get("default").cloned().expect("[units].default"),
            field_declarator: fields
                .get("declarator")
                .cloned()
                .expect("[fields].declarator"),
            function_definition: one("function_definition"),
            function_declarator: one("function_declarator"),
            lambda_expression: one("lambda_expression"),
            class_specifier: one("class_specifier"),
            struct_specifier: one("struct_specifier"),
            if_statement: one("if_statement"),
            for_statement: one("for_statement"),
            for_range_loop: one("for_range_loop"),
            while_statement: one("while_statement"),
            do_statement: one("do_statement"),
            switch_statement: one("switch_statement"),
            conditional_expression: one("conditional_expression"),
            catch_clause: one("catch_clause"),
            else_clause: one("else_clause"),
            binary_expression: g("binary_expression"),
            parameter_list: one("parameter_list"),
            parameter_declaration: one("parameter_declaration"),
            optional_parameter_declaration: one("optional_parameter_declaration"),
            variadic_parameter_declaration: one("variadic_parameter_declaration"),
            amp_amp: one("amp_amp"),
            pipe_pipe: one("pipe_pipe"),
            lang,
            roles,
        }
    }

    fn find_kind<'t>(start: Node<'t>, k: u16) -> Option<Node<'t>> {
        if start.kind_id() == k {
            return Some(start);
        }
        let mut cur = start.walk();
        for c in start.children(&mut cur) {
            if let Some(f) = Self::find_kind(c, k) {
                return Some(f);
            }
        }
        None
    }
}

static DIALECT: LazyLock<CppDialect> = LazyLock::new(CppDialect::new);

impl Dialect for CppDialect {
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
        if id == self.function_definition {
            Some(UnitKind::Func)
        } else if id == self.lambda_expression {
            Some(UnitKind::Closure)
        } else {
            None
        }
    }

    fn args(&self, node: Node) -> u32 {
        let start = node
            .child_by_field_name(&self.field_declarator)
            .unwrap_or(node);
        let Some(params) = Self::find_kind(start, self.parameter_list) else {
            // COVERAGE: defensive — a real C++ `function_definition` always has a
            // `parameter_list` under its declarator, so this None arm is
            // unreachable for well-formed input.
            return 0;
        };
        let mut cur = params.walk();
        params
            .children(&mut cur)
            .filter(|c| {
                let k = c.kind_id();
                k == self.parameter_declaration
                    || k == self.optional_parameter_declaration
                    || k == self.variadic_parameter_declaration
            })
            .count() as u32
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
            || id == self.for_range_loop
            || id == self.while_statement
            || id == self.do_statement
            || id == self.switch_statement
            || id == self.conditional_expression
            || id == self.catch_clause
        {
            st.structural += nesting + depth + lambda + 1;
            cn = nesting + 1;
            st.boolean_op = None;
        } else if id == self.else_clause {
            let is_else_if = {
                let mut cur = node.walk();
                node.children(&mut cur)
                    .any(|c| c.kind_id() == self.if_statement)
            };
            if !is_else_if {
                st.structural += 1;
            }
        } else if self.binary_expression.contains(&id) {
            let mut cur = node.walk();
            for c in node.children(&mut cur) {
                let cid = c.kind_id();
                if cid == self.amp_amp || cid == self.pipe_pipe {
                    st.eval_boolean(cid);
                }
            }
        } else if id == self.lambda_expression {
            cl = lambda + 1;
        }

        CogCtx {
            nesting: cn,
            depth,
            lambda: cl,
        }
    }

    fn is_function_unit(&self, node: Node) -> bool {
        node.kind_id() == self.function_definition
    }

    fn fn_kind(&self, node: Node) -> &str {
        // `method` when the nearest enclosing scope is a class/struct body.
        let mut p = node.parent();
        while let Some(n) = p {
            let id = n.kind_id();
            if id == self.class_specifier || id == self.struct_specifier {
                return &self.unit_method;
            }
            if id == self.function_definition {
                // COVERAGE: defensive early-out for a function nested directly in
                // another function body with no class/struct between — not valid
                // C++, so unreachable; the same `unit_default` fallthrough below
                // handles every real free function.
                return &self.unit_default;
            }
            p = n.parent();
        }
        &self.unit_default
    }

    fn unit_name(&self, node: Node, src: &[u8]) -> Option<String> {
        let decl = node.child_by_field_name(&self.field_declarator)?;
        let fdecl = Self::find_kind(decl, self.function_declarator)?;
        fdecl
            .child_by_field_name(&self.field_declarator)
            .and_then(|n| n.utf8_text(src).ok())
            .map(str::to_string)
    }
}

/// Parse `src` with tree-sitter-cpp and compute the file-level metrics.
pub fn compute(src: &[u8]) -> Option<MetricInputs> {
    crate::engine::compute(src, &*DIALECT)
}

/// Per-function metric units over each `function_definition` subtree.
pub fn compute_functions(src: &[u8]) -> Vec<FunctionUnit> {
    crate::engine::compute_functions(src, &*DIALECT)
}

#[cfg(test)]
#[path = "tests/dialect.rs"]
mod dialect_tests;
