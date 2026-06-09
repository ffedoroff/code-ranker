//! The generic (language-neutral) Prompt-Generator preset catalog. The
//! orchestrator builds these defaults and hands them to
//! `LanguagePlugin::presets`, which may pass them through, edit, drop or extend
//! per language. The catalog lives here (the assembler) so `code-ranker-plugin-api`
//! stays a thin contract.

use code_ranker_plugin_api::plugin::Preset;

const PRINCIPLES_URL: &str = "https://github.com/ffedoroff/code-ranker/blob/main/principles";

/// The principle-corpus language for a plugin (JS/TS share `typescript`).
pub fn principle_lang(plugin: &str) -> String {
    if plugin == "javascript" {
        "typescript".to_string()
    } else {
        plugin.to_string()
    }
}

/// One catalog entry: (id, title, sort_metric, connections, doc_slug, prompt body).
type CatalogEntry = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static str,
    &'static str,
);

const CATALOG: &[CatalogEntry] = &[
    (
        "CPX",
        "CPX — Reduce Complexity",
        "cognitive",
        &[],
        "reduce-complexity",
        "These modules are too complex and I want to reduce their complexity.\n\
         Reduce it by splitting large units into smaller single-responsibility ones,\n\
         extracting repeated patterns into shared helpers, flattening deeply nested\n\
         control flow, and breaking large functions into focused helpers.",
    ),
    (
        "ADP",
        "ADP — Acyclic Dependencies Principle",
        "cycle",
        &["common"],
        "acyclic-dependencies-principle",
        "The dependency graph between modules must form a DAG. When module A depends\n\
         on module B, no chain of dependencies should bring B back to A.\n\n\
         Identify any cycles in the modules below. For each cycle, propose a concrete\n\
         refactoring (extract a shared abstraction, invert a dependency, split a module)\n\
         that makes the graph acyclic without breaking existing functionality.\n\n\
         When splitting a module to break a cycle, the new structure should:\n\
         - Preserve existing API contracts\n\
         - Minimise coupling in the new structure\n\
         - Follow the Single Responsibility Principle\n\
         - Not introduce new dependency cycles",
    ),
    (
        "SRP",
        "SRP — Single Responsibility Principle",
        "sloc",
        &["in", "out"],
        "solid-single-responsibility",
        "A module should have one reason to change — it should serve one actor\n\
         and encapsulate one coherent set of decisions.\n\n\
         For each module below, identify whether it has more than one responsibility.\n\
         Propose how to split responsibilities so each module changes for only one reason,\n\
         and specify the new module boundaries.",
    ),
    (
        "OCP",
        "OCP — Open/Closed Principle",
        "cyclomatic",
        &[],
        "solid-open-closed",
        "A module should be open for extension but closed for modification: new behaviour\n\
         should be addable without editing existing, working code.\n\n\
         For each module below, identify extension points that currently require editing\n\
         existing code (e.g. growing match/switch/if-else chains). Propose an extension\n\
         mechanism (polymorphism, strategy, plug-in registration) so new cases can be added\n\
         without modifying these modules.",
    ),
    (
        "LSP",
        "LSP — Liskov Substitution Principle",
        "hk",
        &[],
        "solid-liskov-substitution",
        "Every implementation of an interface must honour its full contract — return-value\n\
         invariants, error/exception behaviour, side effects, and resource ownership — not\n\
         just the method signatures. A subtype must be substitutable for its base without\n\
         surprising callers.\n\n\
         Identify the interface implementations in the modules below. For each, check it can\n\
         replace any other implementation of the same interface without breaking callers.\n\
         Flag violations and propose fixes.",
    ),
    (
        "ISP",
        "ISP — Interface Segregation Principle",
        "items",
        &["in"],
        "solid-interface-segregation",
        "Clients should not be forced to depend on methods they do not use. Prefer several\n\
         small, focused interfaces over one wide interface.\n\n\
         Identify interfaces in the modules below that are wider than their consumers need.\n\
         Propose how to split them into narrower interfaces so each consumer depends only on\n\
         what it actually uses.",
    ),
    (
        "DIP",
        "DIP — Dependency Inversion Principle",
        "fan_out",
        &["common", "out"],
        "solid-dependency-inversion",
        "High-level modules should not depend on low-level modules; both should depend on\n\
         abstractions, and abstractions should not depend on details.\n\n\
         Find places in the modules below where a high-level module depends directly on a\n\
         concrete low-level type. Propose an abstraction (interface) to invert each such\n\
         dependency, and specify where the concrete implementation should be wired in.",
    ),
    (
        "DRY",
        "DRY — Don't Repeat Yourself",
        "sloc",
        &[],
        "dry",
        "Every piece of knowledge must have a single authoritative representation.\n\
         DRY is about knowledge duplication, not just code duplication.\n\n\
         Identify concepts, rules, or policies that are duplicated across the modules\n\
         below. For each duplication, propose a canonical location and the refactoring\n\
         needed to consolidate it.",
    ),
    (
        "KISS",
        "KISS — Keep It Simple",
        "cognitive",
        &[],
        "kiss",
        "When two designs solve the same problem, prefer the simpler one — fewer\n\
         abstractions, fewer indirection layers, fewer moving parts.\n\n\
         Identify over-engineered or needlessly complex constructs in the modules below.\n\
         For each, describe the simpler alternative and estimate the risk of simplifying.",
    ),
    (
        "LoD",
        "Law of Demeter — Principle of Least Knowledge",
        "fan_out",
        &["common", "out"],
        "law-of-demeter",
        "A method should only call methods on: itself, its direct fields,\n\
         its parameters, and objects it constructs locally.\n\
         Avoid `x.foo().bar().baz()` chains that traverse object graphs.\n\n\
         Identify method chains or deep field traversals in the modules below that\n\
         violate LoD. For each, propose a narrow accessor or a facade that exposes only\n\
         what the caller needs, reducing coupling.",
    ),
    (
        "MISU",
        "MISU — Make Invalid States Unrepresentable",
        "cyclomatic",
        &[],
        "make-invalid-states-unrepresentable",
        "Move correctness from runtime checks into the type system, so invalid states\n\
         cannot be constructed and fail at compile time rather than at runtime.\n\n\
         Identify data structures or function signatures in the modules below where invalid\n\
         states are representable at runtime. For each, propose a type-level encoding\n\
         (sum type / enum, newtype, typestate) that makes the invalid state unrepresentable\n\
         by construction.",
    ),
    (
        "CoI",
        "CoI — Composition Over Inheritance",
        "items",
        &["common"],
        "composition-over-inheritance",
        "Build behaviour by composing small, focused pieces rather than through deep\n\
         inheritance hierarchies.\n\n\
         Identify large types that accumulate behaviour in the modules below. Propose how to\n\
         decompose them into smaller composable parts, and show how consumers would assemble\n\
         the behaviour they need.",
    ),
    (
        "YAGNI",
        "YAGNI — You Aren't Gonna Need It",
        "sloc",
        &["out"],
        "yagni",
        "Build for the problem you have now, not one you imagine you might have later.\n\
         Don't add an abstraction, a generic parameter, or a public API for a hypothetical\n\
         future use.\n\n\
         Identify abstractions, generics, or public APIs in the modules below that were\n\
         added speculatively. For each, assess whether multiple real callers use it today,\n\
         and propose simplification if not.",
    ),
];

/// The generic preset set for a principle-corpus language. `doc_url`s point at
/// `principles/<lang>/<slug>.md`.
pub fn default_presets(lang: &str) -> Vec<Preset> {
    CATALOG
        .iter()
        .map(
            |(id, title, sort_metric, connections, slug, prompt)| Preset {
                id: (*id).to_string(),
                label: (*id).to_string(),
                title: (*title).to_string(),
                prompt: (*prompt).to_string(),
                doc_url: Some(format!("{PRINCIPLES_URL}/{lang}/{slug}.md")),
                sort_metric: (*sort_metric).to_string(),
                connections: connections.iter().map(|s| s.to_string()).collect(),
            },
        )
        .collect()
}
