//! C [`Dialect`] for the generic metric engine.
//!
//! The walk logic lives in `crate::engine`; this is the thin C-specific layer:
//! the grammar, the resolved [`Roles`] (from `c/config.toml`), the cognitive
//! state machine (if / loops / switch / `?:` nesting, `else`, `&&`/`||` runs),
//! and the `args` override — C nests a function's parameters under a
//! `function_declarator`, not on a `parameters` field of the unit node.

use crate::engine::{CogCtx, CogState, Dialect, Roles, UnitKind};
use code_ranker_plugin_api::metrics::{FunctionUnit, MetricInputs};
use std::collections::HashSet;
use std::sync::LazyLock;
use tree_sitter::{Language, Node};

// Inheritance chain `defaults.toml ⊕ cfamily/config.toml ⊕ c/config.toml` — same
// as `mod.rs`; the metric engine's `[roles]`/`[halstead]`/`[loc]` and `[units]`/
// `[fields]` draw on the shared C-family base plus C's own overrides.
static CONFIG: LazyLock<toml::Table> = LazyLock::new(|| {
    crate::config::load_chain(&[
        include_str!("../cfamily/config.toml"),
        include_str!("config.toml"),
    ])
});

static ROLE_CFG: LazyLock<crate::engine::RoleCfg> = LazyLock::new(|| {
    CONFIG
        .clone()
        .try_into()
        .expect("cfamily ⊕ c/config.toml [roles]/[halstead]/[loc] parse")
});

struct CDialect {
    lang: Language,
    roles: Roles,
    unit_default: String,
    field_declarator: String,
    function_definition: u16,
    function_declarator: u16,
    if_statement: u16,
    for_statement: u16,
    while_statement: u16,
    do_statement: u16,
    switch_statement: u16,
    conditional_expression: u16,
    else_clause: u16,
    binary_expression: HashSet<u16>,
    parameter_list: u16,
    parameter_declaration: u16,
    variadic_parameter: u16,
    amp_amp: u16,
    pipe_pipe: u16,
}

impl CDialect {
    fn new() -> Self {
        let lang: Language = tree_sitter_c::LANGUAGE.into();
        let roles = Roles::resolve(&lang, &ROLE_CFG);
        let one = |k: &str| roles.one(k);
        let g = |k: &str| roles.group(k).clone();
        let units = crate::config::units(&CONFIG);
        let fields = crate::config::string_table(&CONFIG, "fields");
        CDialect {
            unit_default: units.get("default").cloned().expect("[units].default"),
            field_declarator: fields
                .get("declarator")
                .cloned()
                .expect("[fields].declarator"),
            function_definition: one("function_definition"),
            function_declarator: one("function_declarator"),
            if_statement: one("if_statement"),
            for_statement: one("for_statement"),
            while_statement: one("while_statement"),
            do_statement: one("do_statement"),
            switch_statement: one("switch_statement"),
            conditional_expression: one("conditional_expression"),
            else_clause: one("else_clause"),
            binary_expression: g("binary_expression"),
            parameter_list: one("parameter_list"),
            parameter_declaration: one("parameter_declaration"),
            variadic_parameter: one("variadic_parameter"),
            amp_amp: one("amp_amp"),
            pipe_pipe: one("pipe_pipe"),
            lang,
            roles,
        }
    }

    /// The first `parameter_list` under `node`'s declarator subtree (params nest
    /// under `function_declarator`, possibly inside a `pointer_declarator`).
    fn find_param_list<'t>(&self, node: Node<'t>) -> Option<Node<'t>> {
        let start = node
            .child_by_field_name(&self.field_declarator)
            .unwrap_or(node);
        fn rec<'t>(n: Node<'t>, pl: u16) -> Option<Node<'t>> {
            if n.kind_id() == pl {
                return Some(n);
            }
            let mut cur = n.walk();
            for c in n.children(&mut cur) {
                if let Some(f) = rec(c, pl) {
                    return Some(f);
                }
            }
            None
        }
        rec(start, self.parameter_list)
    }
}

static DIALECT: LazyLock<CDialect> = LazyLock::new(CDialect::new);

impl Dialect for CDialect {
    fn language(&self) -> &Language {
        &self.lang
    }
    fn roles(&self) -> &Roles {
        &self.roles
    }

    fn file_initial_spaces(&self) -> u32 {
        1 // the translation_unit space (not in space_kinds)
    }

    fn classify_unit(&self, node: Node) -> Option<UnitKind> {
        (node.kind_id() == self.function_definition).then_some(UnitKind::Func)
    }

    fn args(&self, node: Node) -> u32 {
        let Some(params) = self.find_param_list(node) else {
            // COVERAGE: defensive — a real C `function_definition` always carries a
            // `function_declarator` with a `parameter_list`, so this None arm is
            // unreachable for well-formed input.
            return 0;
        };
        let mut cur = params.walk();
        params
            .children(&mut cur)
            .filter(|c| {
                c.kind_id() == self.parameter_declaration || c.kind_id() == self.variadic_parameter
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
        let mut cn = nesting;

        if id == self.if_statement
            || id == self.for_statement
            || id == self.while_statement
            || id == self.do_statement
            || id == self.switch_statement
            || id == self.conditional_expression
        {
            st.structural += nesting + depth + lambda + 1;
            cn = nesting + 1;
            st.boolean_op = None;
        } else if id == self.else_clause {
            // `else if` is an else_clause whose child is an if_statement — the
            // nested if already increments, so don't double-count; a plain `else`
            // (compound_statement child) adds +1 with no nesting.
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
        }

        CogCtx {
            nesting: cn,
            depth,
            lambda,
        }
    }

    fn is_function_unit(&self, node: Node) -> bool {
        node.kind_id() == self.function_definition
    }

    fn fn_kind(&self, _node: Node) -> &str {
        &self.unit_default
    }

    fn unit_name(&self, node: Node, src: &[u8]) -> Option<String> {
        // function_definition → (pointer/paren…) function_declarator →
        // declarator: identifier (the function name).
        fn find<'t>(n: Node<'t>, fd: u16) -> Option<Node<'t>> {
            if n.kind_id() == fd {
                return Some(n);
            }
            let mut cur = n.walk();
            for c in n.children(&mut cur) {
                if let Some(f) = find(c, fd) {
                    return Some(f);
                }
            }
            None
        }
        let decl = node.child_by_field_name(&self.field_declarator)?;
        let fdecl = find(decl, self.function_declarator)?;
        fdecl
            .child_by_field_name(&self.field_declarator)
            .and_then(|n| n.utf8_text(src).ok())
            .map(str::to_string)
    }
}

/// Parse `src` with tree-sitter-c and compute the file-level metrics.
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
