# Unit testing guide

Philosophy, conventions, and patterns for unit tests in `code-ranker`.

## Philosophy

### What a unit test is here

A unit test calls a Rust function directly — a parser, a validation rule, a rule
evaluator, a value transform — and verifies the result. Pure, deterministic, and fast;
the only I/O a test ever touches is an occasional throwaway temp dir.

This is the project's line of defense for correctness. Every test is a synchronous
`#[test]` that exercises a single function and asserts its output.

### Three questions before adding a test

Every test must pass all three:

1. **Does it verify deterministic logic?** Parsing, rule evaluation, plugin resolution
   (which languages a workspace yields),
   name templating — all deterministic, all testable.
2. **Is it atomic and fast?** One `#[test]` = one behavior. No `sleep`, no `timeout`.
   The whole suite runs in well under 5 seconds.
3. **Does removing it reduce confidence?** If not, it is redundant. Every test guards a
   specific behavior that, if broken, would let a wrong snapshot, a missed violation, or
   a silent misconfiguration through.

## Reliability principles

- **Atomic** — one `#[test]` = one behavior. No compound "test everything" functions.
- **Fast** — no `sleep`, no `timeout`, no async. Target: full suite < 5s.
- **Independent** — no shared state. A test that needs a workspace creates its own temp
  dir. `cargo test` runs them in parallel, in any order.
- **Synchronous** — pure logic is `#[test]`, never `async`.
- **No new *external* crate dependencies for testing** — use
  `assert!(matches!(...))`, not `assert_matches!`; manual `vec![] + loop` for
  table-driven cases, not `rstest`. `tempfile` (already a workspace dependency) is
  the allowed external helper, for temp-dir isolation. The one internal helper is
  **`code-ranker-test-support`** (a `publish = false`, path-only dev-dependency —
  cargo strips it from published manifests — shared by the language-plugin tests
  for `write_file` and graph-assertion boilerplate): internal shared test code, not
  a third-party test framework.

## What belongs in unit tests

| Area | What to cover | Where |
|---|---|---|
| Config parsing | `--cycle-rule KIND=on\|off`, `--threshold SCOPE.METRIC=N`, defaults, rejection of bad input | `code-ranker-cli/src/config.rs` |
| Rule evaluation | `check_violations_all(languages, rules)` (cycles + thresholds across every language); `apply_cycle_rules` strips disabled kinds | `code-ranker-cli/src/config.rs` |
| Plugin resolution | `resolve_plugins` precedence; `detect_all` markers / multi-detect / none | `code-ranker-cli/src/main.rs` |
| Name templating | `render_name` — `{project-dir}` slug, `{ts}` stamp, `{git-hash}` / `{git-hash-N}`; `[output]` name resolution | `code-ranker-cli/src/main.rs`, `config.rs` |
| Snapshot & graph types | serde round-trip of the snapshot (the public artifact); builder / projection invariants; cycle and HK annotation | `code-ranker-core/src/*` |
| Graph extraction | module / file graph shape on small in-source inputs | `code-ranker-syn/src/*` |

## What does NOT belong

- **HTML report rendering** — visual and cosmetic. Verify the data that feeds the report,
  not the markup.
- **rust-analyzer call-graph accuracy on real crates** — depends on an external
  toolchain and a real workspace; not deterministic enough for a unit test.

## What to assert

A test that only checks `is_ok()` provides almost no value. Cover every dimension that
applies:

1. **Primary outcome** — success, or the *specific* error.
2. **Returned values** — the actual fields, not just the status.
3. **Side effects** — anything mutated beyond the return value (a graph stripped, only
   the affected node changed).
4. **Error context** — the error message contains the offending token or field, not just
   "failed".

### The lazy-assert trap

```rust
// BAD — only checks it didn't error
let v = check_violations_all(&languages, &rules);
assert!(!v.is_empty());

// GOOD — checks the count, which language + graph, the message, AND that
// the in-budget node did NOT contribute a violation
assert_eq!(v.len(), 1, "only the over-budget node violates");
assert_eq!(v[0].language, "rust");
assert_eq!(v[0].graph, "functions");
assert!(v[0].message.contains("cognitive"), "got {:?}", v[0].message);
```

## Patterns

**Error context** — assert the message, not just `is_err()`:

```rust
let err = apply_cli_overrides(&mut cfg, &[], &["mutual=loud".into()], &[]).unwrap_err();
assert!(format!("{err:#}").contains("loud"), "got {err:#}");
```

**Table-driven** — manual `vec![] + loop` with a descriptive message per case:

```rust
let cases = vec![("on", Some(true)), ("off", Some(false)), ("maybe", None)];
for (input, expected) in cases {
    match expected {
        Some(b) => assert_eq!(parse_on_off(input).unwrap(), b, "for {input:?}"),
        None => assert!(parse_on_off(input).is_err(), "should reject {input:?}"),
    }
}
```

**Minimal fixtures** — build the smallest graph or node that exercises the rule; a
helper keeps it readable:

```rust
fn node_with_cognitive(id: &str, cognitive: f64) -> Node { /* … */ }
```

**Temp workspaces** — for path-marker logic, a throwaway directory:

```rust
let d = tempfile::tempdir().unwrap();
std::fs::write(d.path().join("Cargo.toml"), "").unwrap();
assert_eq!(detect_all(d.path()).unwrap(), ["rust"]);
```

## Naming

`{area}_{scenario}` in snake_case:

```text
parse_on_off_accepts_on_off_true_false
cycle_rules_default_test_embed_off_others_on
check_reports_enabled_cycle_group
apply_cycle_rules_strips_disabled_kind
detect_all_returns_every_matching_language
detect_all_errors_on_zero_detect
resolve_plugins_precedence_explicit_then_config_then_auto
```

## Organization

Tests live in-source, next to the code they cover, in a `#[cfg(test)] mod tests` block:

```text
crates/code-ranker-cli/src/config.rs             # parsing, rule evaluation
crates/code-ranker-cli/src/main.rs               # plugin resolution, name templating
crates/code-ranker-cli/src/plugin/python.rs      # Python extraction + import/call graph
crates/code-ranker-cli/src/plugin/javascript.rs  # JS/TS extraction + import/call graph
crates/code-ranker-core/src/builder.rs           # graph builder invariants
crates/code-ranker-core/src/cycles.rs            # SCC detection / cycle classification
crates/code-ranker-core/src/diff.rs              # snapshot comparison
crates/code-ranker-core/src/graph.rs             # graph / projection / serde
crates/code-ranker-core/src/snapshot.rs          # snapshot serde, path / id rewriting
crates/code-ranker-core/src/stats.rs             # metric averaging
crates/code-ranker-syn/src/module_graph.rs       # module / file extraction
```

## Priority

- **P1 — invariants:** rule evaluation (cycle on/off → violation or strip, threshold
  breach), config parsing and rejection, snapshot serde round-trip, plugin resolution.
- **P2 — secondary paths:** error context, name-template edges, graph projection edges.
- **P3 — nice to have:** boundary values, cosmetic defaults.

## Acceptance criteria

- `cargo test --workspace` — 0 failed.
- Full suite completes in under 5 seconds.
- Zero `sleep`, `timeout`, or async usage in tests.
- `make all` (build + test + lint) passes with zero errors.
- Every rule invariant is covered by at least one test.

## Raising coverage without crutches

The whole-workspace floor (`make coverage`, ≥ 90% lines) is the blunt backstop;
`make diff-coverage` is the surgical view for one branch — it intersects the
`cargo llvm-cov` zero-hit lines with the lines your branch changed vs `origin/main`
and prints exactly the new, untested lines per file. Work that list, but treat each
line as a **decision**, not a quota.

### First ask: is the uncovered line a missing test, or a bug?

An uncovered branch is sometimes telling you the branch is **unreachable as
written** — a real defect hiding behind a green build. Before writing a test,
confirm the line *can* execute and that exercising it gives the *right* answer.

> Worked example (this is why the rule exists). `cargo llvm-cov` flagged the
> `else if id == self.binary_expression { … eval_boolean … }` arm in
> `c/dialect.rs` as uncovered, even though a test fed it `a > b && a > 0`. The arm
> was **dead**: `tree-sitter-c` (and `-cpp`) declare the name `binary_expression`
> on **two** symbols — the ordinary expression and the preprocessor-context one
> used in `#if A && B` — both *named*, both *visible*, neither a *supertype*. The
> single-id `roles.one("binary_expression")` resolved to whichever symbol comes
> first, which is **not** the one that appears in normal-code trees, so the `==`
> never matched and `&&`/`||` silently dropped out of C/C++ cognitive complexity.
> The fix was not a test — it was switching the lookup to a **set**
> (`[roles.group] binary_expression.named = ["binary_expression"]`, matched with
> `.contains(&id)`, which `resolve_set` fills with *all* matching ids). The test
> came after, as a regression guard: `&&` must raise `cognitive` over the same
> code without it. **Lesson: when a single-id node-kind lookup silently no-ops,
> suspect a duplicate-named grammar symbol and use a group/set, not `[roles.one]`.**

A fast way to confirm a suspicion like this: drop a temporary `#[test]` that prints
(`eprintln!`) the resolved id and walks the parsed tree printing `node.kind()` /
`kind_id()`, run it with `-- --nocapture`, then delete it (`git checkout` the test
file). Never leave a probe behind.

### Cheap, organic tests for the "skip / error" branches

Most remaining gaps after the happy path are guard clauses. These are reachable
with tiny fixtures — no mocks:

- **`fs::read … else continue` (unreadable source).** Write a file with invalid
  UTF-8 bytes (`[0xFF, 0xFE, 0x00]`) and the right extension. `read_to_string`
  fails on it, so the analyzer's skip arm runs while the rest of the walk
  proceeds. Assert the bad file is absent from the graph and a good sibling is
  present.
- **Plugin `metrics` / `function_units` node guards.** Build a `Graph` by hand
  with one `EXTERNAL` node (trips the `kind != FILE` guard) and one `FILE` node
  whose path doesn't exist (trips the read-skip). Assert both `metrics(&g)` and
  `function_units(&g)` come back empty. (See the per-language `tests/mod_rs.rs`.)
- **Scanner edge forms.** Feed the line/text scanner its malformed inputs
  directly — an unterminated `#include "x`, a macro `#include FOO`, a `](` with no
  closing `)`. One assertion per arm.
- **Resolution fallbacks.** A subdir include resolved by repo-relative path; an
  on-disk neighbour with an uncollected extension resolved by the `is_file()`
  fallback — each is a few lines in a temp dir.

### Lines worth leaving uncovered (state them, don't paper over them)

Some lines are honest gaps. Skip them deliberately — a forced test is worse than a
documented gap — but **mark the gap in the code, right next to the line**, with a
short comment saying it is intentionally uncovered and why (rare case, defensive
guard, region artifact). The note belongs at the call site, not only in a PR
description, so the next reader (and the next coverage diff) sees the decision in
place. Use a consistent prefix so the gaps are greppable, e.g.:

```rust
// COVERAGE: defensive — tree-sitter `parse()` never returns None for a valid
// grammar, so this `?` is unreachable in practice.
let tree = parser.parse(src, None)?;
```

- **Defensive `?`/`None` propagation that can't fire in practice** — e.g. a
  tree-sitter `parse()` returning `None` (it effectively never does for a valid
  grammar), or `utf8_text()` failing on a node whose bytes were already required to
  parse. Reaching these needs contradictory inputs or brittle scaffolding.
- **`unwrap`-region closing braces.** `llvm-cov` sometimes attributes a `}` that
  closes an `if let … { return … }` to a region the happy-path test doesn't mark,
  even when every reachable statement is covered. Don't contort a fixture to color
  a brace.
- **Paths that need escaping the workspace / hidden dirs** — e.g. a Markdown link
  to an `.md` that exists on disk but was never collected. Constructing it is
  brittle and tests an effectively-impossible state.

Rule of thumb: if covering the line requires a mock, a contrived input that could
never occur in real source, or a production-side hook added only to be tested,
leave it — and drop a `// COVERAGE:` note on the line saying why.
