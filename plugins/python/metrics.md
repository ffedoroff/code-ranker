# How metrics are counted (in Python)

Python support is **beta**. The complexity metrics use the same in-tree
`tree-sitter` engine approach as Rust (and the shared `code-ranker-graph` metric
scaffolding) — here the in-tree `tree-sitter-python`
engine (`python_ts`, in the `python` module of `code-ranker-plugins`, a port of `rust-code-analysis`'s rules), not `syn`, invoked
by the Python plugin's `metrics()` step; this file is the Python-specific
normative spec. For the shared conceptual definitions of each
metric (what `cyclomatic` / `cognitive` / Halstead / `mi` mean) see
[`../rust/metrics.md`](../rust/metrics.md); this file only states what differs
for Python.

## What "correct" means (normative)

This is the **source of truth** for *what each metric counts* in Python — the
definition the **Metric Accuracy** goal (`cpt-code-ranker-nfr-metric-accuracy`)
and its tests assert against (see [`../../docs/metric-correctness.md`](../../docs/metric-correctness.md)).
Three rules hold for **every** metric:

- **Counted from the parsed AST, never from text.** A keyword that appears only
  as a look-alike — inside an identifier, a comment, a string / docstring, or an
  f-string — **does not count**. No false positives from text.
- **Per-function metrics are summed over the file's functions** and **omitted at
  their no-signal value** (`omit_at`; `1` for `cyclomatic`, `0` for the rest).
  `cyclomatic` is the analyzer's whole-file value — the per-function McCabe sum
  plus the file unit's own base path; see the
  [Rust spec §cyclomatic](../rust/metrics.md) for the definition and citations.
- **Dynamic forms are not resolved.** `importlib.import_module`, `__import__`,
  `eval` take string arguments and are *not* analyzed — a deliberate blind spot,
  not a missed count (mirrors how Rust does not expand macros).

**Keyword look-alike guard set.** The construct keywords a complexity metric can
key on; the FP tests inject each only as a look-alike (comment / string /
docstring / f-string / identifier) and assert no metric moves. A superset of the
analyzer's exact triggers is fine — guarding extra is harmless, missing one is
not: `if`, `elif`, `else`, `while`, `for`, `and`, `or`, `return`, `try`,
`except`, `with`, `assert`, `raise`. (The FP matrix iterates this exact list and
a test asserts it is documented here, so the two cannot drift.)

## Per-language metric scope

The `python_ts` engine does not emit every metric for Python. Within the central
catalog, the Python analyzer emits:

| metric | Python |
|---|---|
| `cyclomatic` `cognitive` | ✅ computed |
| `exits` | ✅ computed |
| LOC (`sloc` `lloc` `cloc` `blank`), Halstead, `mi` / `mi_sei` | ✅ computed |
| `args` | ❌ **not emitted for Python** (analyzer scope) |
| `closures` | ❌ **not emitted for Python** (analyzer scope) |
| `tloc` | ❌ not produced — only the Rust analysis strips `#[cfg(test)]`; Python test files are counted as ordinary production lines |

These gaps are an analyzer-scope limit, not fixture or detector bugs, and are
pinned per language in [`../../docs/e2e.md`](../../docs/e2e.md). A construct whose
metric the analyzer does not emit (a multi-argument `def`, a `lambda`) simply
yields no value — that is the documented contract for Python, not a false
negative.

## Dependency edges

File→file edges come from real `import` statements: `import pkg`, `from pkg
import x`, relative `from .mod import y`, and imports inside a function body (the
walk is whole-tree, not top-level only). **Not** detected: dynamic imports
(`importlib` / `__import__` / `eval` with a string argument) — there is no static
path to resolve, so no edge is produced.
