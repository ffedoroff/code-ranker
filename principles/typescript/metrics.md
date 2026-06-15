# How metrics are counted (in TypeScript)

TypeScript support is **beta**. The complexity metrics use the same in-tree
`tree-sitter` engine approach as Rust (and the shared `code-ranker-graph` metric
scaffolding) — here the in-tree `tree-sitter` engine
(`ecmascript_ts`, in `code-ranker-ecmascript-core`, shared with JavaScript/TSX, a port of `rust-code-analysis`'s
rules), not `syn`, invoked by the TypeScript plugin's `metrics()` step; this file
is the TypeScript-specific normative spec. For the shared conceptual definitions of
each metric (what `cyclomatic` / `cognitive` / Halstead / `mi` mean) see
[`../rust/metrics.md`](../rust/metrics.md); this file only states what differs
for TypeScript.

## What "correct" means (normative)

This is the **source of truth** for *what each metric counts* in TypeScript — the
definition the **Metric Accuracy** goal (`cpt-code-ranker-nfr-metric-accuracy`)
and its tests assert against (see [`../../docs/metric-correctness.md`](../../docs/metric-correctness.md)).
Three rules hold for **every** metric:

- **Counted from the parsed AST, never from text.** A keyword that appears only
  as a look-alike — inside an identifier, a comment, a string, a template
  literal, or a type annotation — **does not count**. No false positives.
- **Per-function metrics are summed over the file's functions** and **omitted at
  their no-signal value** (`omit_at`; `1` for `cyclomatic`, `0` for the rest).
  `cyclomatic` is the analyzer's whole-file value — the per-function McCabe sum
  plus the file unit's own base path; see the
  [Rust spec §cyclomatic](../rust/metrics.md) for the definition and citations.
- **Dynamic forms are not resolved.** A dynamic `import()` expression is a call,
  not an import statement, and is *not* analyzed — a deliberate blind spot, not a
  missed count.

**Keyword look-alike guard set.** The construct keywords / operators a complexity
metric can key on; the FP tests inject each only as a look-alike (comment /
string / template literal / identifier) and assert no metric moves. A superset of
the analyzer's exact triggers is fine — guarding extra is harmless, missing one
is not: `if`, `else`, `while`, `for`, `do`, `switch`, `case`, `catch`, `return`,
`throw`, `&&`, `||`, `??`, `?`. (The FP matrix iterates this exact list and a test
asserts it is documented here, so the two cannot drift.)

## Per-language metric scope

Within the central catalog the TypeScript analyzer emits **every** metric except
one:

| metric | TypeScript |
|---|---|
| `cyclomatic` `cognitive` `exits` `args` `closures` | ✅ computed |
| LOC (`sloc` `lloc` `cloc` `blank`), Halstead, `mi` / `mi_sei` | ✅ computed |
| `tloc` | ❌ not produced — only the Rust analysis strips `#[cfg(test)]`; TS test files are counted as ordinary production lines |

This gap is an analyzer-scope limit, not a fixture or detector bug, and is pinned
per language in [`../../docs/e2e.md`](../../docs/e2e.md).

## Dependency edges

File→file edges come from `import` / `export` statements: named imports, the
**extension-less** `from "./b"` form (the resolver tries `.ts`), type-only
`import type`, and `export * from`. Alias resolution (`tsconfig` `paths` /
`baseUrl`) is honored. Namespace (`import * as ns`) and aliased
(`import { a as b }`) forms are transparent — the edge keys on the module
specifier, not the binding. **Not** detected: dynamic `import()` expressions — a
runtime call with no static path to resolve — and the legacy
`import x = require("./b")` form, whose specifier sits inside a `require` call
the import walker does not descend into. Both are deliberate syntactic blind
spots (pinned by `dynamic_import_and_import_equals_are_non_goals`), not bugs.
