//! Role-keyed node-kind tables: the data the generic engine keys on.
//!
//! A language's `<lang>.toml` declares, under `[roles]` / `[halstead]` / `[loc]`,
//! which tree-sitter `kind` strings fill each engine *role* (branch kinds, exit
//! kinds, operators, comment kinds, …). [`RoleCfg`] is the deserialized shape;
//! [`Roles`] is it resolved against a concrete grammar into id sets.
//!
//! Resolution collects ALL ids whose `(name, is_named)` matches a requested
//! `(name, flag)` pair. For Rust/Python every name maps to a single id, so the
//! set is a singleton and `set.contains(id)` is equivalent to the old
//! `id == id_for_node_kind(name)` check. ECMAScript's grammar has duplicate-name
//! variants (`Identifier2`/`String2`/…) that share a name but differ by id; the
//! set-scan collects them all, matching rca.

use std::collections::HashSet;
use std::sync::LazyLock;
use tree_sitter::Language;

/// A list of node-kind names tagged with whether they are named nodes or
/// anonymous tokens. Most config entries are homogeneous, so two list fields
/// (`named` / `anon`) keep the TOML readable; both are optional.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct NameSet {
    #[serde(default)]
    pub named: Vec<String>,
    #[serde(default)]
    pub anon: Vec<String>,
}

/// The full role config as read from a merged `<lang>.toml`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RoleCfg {
    pub roles: RolesCfg,
    pub halstead: HalsteadCfg,
    pub loc: LocCfg,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RolesCfg {
    // structural
    #[serde(default)]
    pub space_kinds: NameSet,
    /// Extra cognitive-only spaces (boolean-run boundaries that are NOT counted
    /// as structural spaces): python's `lambda`. Empty when closures already
    /// live in `space_kinds` (rust/ecmascript).
    #[serde(default)]
    pub closure_space_kinds: NameSet,
    #[serde(default)]
    pub branch_kinds: NameSet,
    #[serde(default)]
    pub exit_kinds: NameSet,
    #[serde(default)]
    pub non_arg_kinds: NameSet,
    // cognitive shared
    #[serde(default)]
    pub cog_if: NameSet,
    #[serde(default)]
    pub cog_nest: NameSet,
    #[serde(default)]
    pub cog_else: NameSet,
    #[serde(default)]
    pub unary: NameSet,
    #[serde(default)]
    pub binary: NameSet,
    #[serde(default)]
    pub and_or: NameSet,
    // closure / function-unit / fn_kind / cognitive function-nesting kinds —
    // resolved as singleton ids by canonical key (see `Roles::one`).
    #[serde(default)]
    pub one: std::collections::BTreeMap<String, OneEntry>,
    // free-form extra named/anon groups a dialect needs (e.g. ecmascript's
    // ancestor-walk sets, python's reset/loop-else kinds). Keyed by name.
    #[serde(default)]
    pub group: std::collections::BTreeMap<String, NameSet>,
}

/// One `[roles.one.<key>]` entry: a single kind name + whether it is named.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OneEntry {
    pub kind: String,
    #[serde(default = "default_one_named")]
    pub named: bool,
}

/// The `[roles.one]` `named` default, sourced from `defaults.toml`'s
/// `[defaults].one_named` (the single source — no literal in Rust). Parsed once;
/// reads only that key, independent of any language's full config merge.
static ONE_NAMED_DEFAULT: LazyLock<bool> = LazyLock::new(|| {
    #[derive(serde::Deserialize)]
    struct Wrap {
        defaults: Defs,
    }
    #[derive(serde::Deserialize)]
    struct Defs {
        one_named: bool,
    }
    toml::from_str::<Wrap>(crate::config::DEFAULTS)
        .expect("defaults.toml [defaults].one_named")
        .defaults
        .one_named
});

fn default_one_named() -> bool {
    *ONE_NAMED_DEFAULT
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct HalsteadCfg {
    #[serde(default)]
    pub operators: NameSet,
    #[serde(default)]
    pub operands: NameSet,
    /// Special context kinds a dialect's `hal_classify` keys on, by name.
    #[serde(default)]
    pub special: std::collections::BTreeMap<String, OneEntry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LocCfg {
    #[serde(default)]
    pub noop_kinds: NameSet,
    #[serde(default)]
    pub comment_kinds: NameSet,
    #[serde(default)]
    pub statement_kinds: NameSet,
    /// Special LOC context kinds a dialect's `loc_node` keys on, by name.
    #[serde(default)]
    pub special: std::collections::BTreeMap<String, OneEntry>,
}

/// The resolved role sets for a concrete grammar.
pub struct Roles {
    /// The grammar, kept so [`Roles::one`] can resolve an identity role
    /// (canonical name == named grammar kind) without a `[roles.one]` entry.
    lang: Language,
    pub space_kinds: HashSet<u16>,
    pub closure_space_kinds: HashSet<u16>,
    pub branch_kinds: HashSet<u16>,
    pub exit_kinds: HashSet<u16>,
    pub non_arg_kinds: HashSet<u16>,
    pub cog_if: HashSet<u16>,
    pub cog_nest: HashSet<u16>,
    pub cog_else: HashSet<u16>,
    pub unary: HashSet<u16>,
    pub binary: HashSet<u16>,
    pub and_or: HashSet<u16>,
    pub operators: HashSet<u16>,
    pub operands: HashSet<u16>,
    pub noop_kinds: HashSet<u16>,
    pub comment_kinds: HashSet<u16>,
    pub statement_kinds: HashSet<u16>,
    /// Singleton ids resolved from `[roles.one]`, by canonical key.
    pub one: std::collections::BTreeMap<String, u16>,
    /// Free-form group sets from `[roles.group]`, by name.
    pub group: std::collections::BTreeMap<String, HashSet<u16>>,
    /// Special Halstead / LOC singleton ids, by name.
    pub special: std::collections::BTreeMap<String, u16>,
}

impl Roles {
    /// Resolve `cfg` against `lang` into id sets.
    pub fn resolve(lang: &Language, cfg: &RoleCfg) -> Self {
        let s = |ns: &NameSet| resolve_set(lang, ns);
        let r = &cfg.roles;

        let mut one = std::collections::BTreeMap::new();
        for (k, e) in &r.one {
            one.insert(k.clone(), resolve_one(lang, e));
        }
        let mut group = std::collections::BTreeMap::new();
        for (k, ns) in &r.group {
            group.insert(k.clone(), s(ns));
        }
        let mut special = std::collections::BTreeMap::new();
        for (k, e) in &cfg.halstead.special {
            special.insert(k.clone(), resolve_one(lang, e));
        }
        for (k, e) in &cfg.loc.special {
            special.insert(k.clone(), resolve_one(lang, e));
        }

        Roles {
            lang: lang.clone(),
            space_kinds: s(&r.space_kinds),
            closure_space_kinds: s(&r.closure_space_kinds),
            branch_kinds: s(&r.branch_kinds),
            exit_kinds: s(&r.exit_kinds),
            non_arg_kinds: s(&r.non_arg_kinds),
            cog_if: s(&r.cog_if),
            cog_nest: s(&r.cog_nest),
            cog_else: s(&r.cog_else),
            unary: s(&r.unary),
            binary: s(&r.binary),
            and_or: s(&r.and_or),
            operators: s(&cfg.halstead.operators),
            operands: s(&cfg.halstead.operands),
            noop_kinds: s(&cfg.loc.noop_kinds),
            comment_kinds: s(&cfg.loc.comment_kinds),
            statement_kinds: s(&cfg.loc.statement_kinds),
            one,
            group,
            special,
        }
    }

    /// A singleton id for a role. Anon tokens / aliases (kind != key, or
    /// `named = false`) are spelled out in `[roles.one]`; an **identity** role
    /// (its canonical name IS its named grammar kind) needs NO config entry —
    /// it is resolved directly here. `u16::MAX` if the name is not in the grammar.
    pub fn one(&self, key: &str) -> u16 {
        self.one.get(key).copied().unwrap_or_else(|| {
            resolve_one(
                &self.lang,
                &OneEntry {
                    kind: key.to_string(),
                    named: true,
                },
            )
        })
    }

    /// A `[roles.group]` set (empty if absent).
    pub fn group(&self, key: &str) -> &HashSet<u16> {
        static EMPTY: std::sync::OnceLock<HashSet<u16>> = std::sync::OnceLock::new();
        self.group
            .get(key)
            .unwrap_or_else(|| EMPTY.get_or_init(HashSet::new))
    }

    /// A special Halstead/LOC singleton id (or `u16::MAX` if absent).
    pub fn special(&self, key: &str) -> u16 {
        self.special.get(key).copied().unwrap_or(u16::MAX)
    }
}

/// Collect ALL ids whose `(name, is_named)` matches any requested pair.
fn resolve_set(lang: &Language, ns: &NameSet) -> HashSet<u16> {
    let wanted: Vec<(&str, bool)> = ns
        .named
        .iter()
        .map(|n| (n.as_str(), true))
        .chain(ns.anon.iter().map(|n| (n.as_str(), false)))
        .collect();
    if wanted.is_empty() {
        return HashSet::new();
    }
    let mut out = HashSet::new();
    for id in 0..lang.node_kind_count() as u16 {
        if let Some(name) = lang.node_kind_for_id(id) {
            let named = lang.node_kind_is_named(id);
            if wanted.iter().any(|(n, b)| *n == name && *b == named) {
                out.insert(id);
            }
        }
    }
    out
}

/// Resolve a single `(kind, named)` entry to the first matching id.
fn resolve_one(lang: &Language, e: &OneEntry) -> u16 {
    (0..lang.node_kind_count() as u16)
        .find(|&id| {
            lang.node_kind_for_id(id) == Some(e.kind.as_str())
                && lang.node_kind_is_named(id) == e.named
        })
        .unwrap_or(u16::MAX)
}

#[cfg(test)]
#[path = "tests/roles.rs"]
mod roles_tests;
