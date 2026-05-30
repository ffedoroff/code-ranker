# Unit testing guide

Philosophy, conventions, and patterns for unit tests in `code-split`.

## Philosophy

### What a unit test is here

A unit test calls a Rust function directly — a parser, a validation rule, a rule
evaluator, a value transform — and verifies the result. Pure, deterministic, and fast;
the only I/O a test ever touches is an occasional throwaway temp dir.

This is the project's line of defense for correctness. Every test is a synchronous
`#[test]` that exercises a single function and asserts its output.

### Three questions before adding a test

Every test must pass all three:

1. **Does it verify deterministic logic?** Parsing, rule evaluation, plugin resolution,
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
- **No new crate dependencies for testing** — use `assert!(matches!(...))`, not
  `assert_matches!`; manual `vec![] + loop` for table-driven cases, not `rstest`.
  `tempfile` (already a workspace dependency) is the one allowed helper, for temp-dir
  isolation.

## What belongs in unit tests

| Area | What to cover | Where |
|---|---|---|
| Config parsing | `--cycle-rule KIND=on\|off`, `--threshold SCOPE.METRIC=N`, defaults, rejection of bad input | `code-split-cli/src/config.rs` |
| Rule evaluation | `check_violations` (cycles + thresholds); `apply_cycle_rules` strips disabled kinds | `code-split-cli/src/config.rs` |
| Plugin resolution | `resolve_plugin` precedence; `detect_plugin` markers / ambiguity / none | `code-split-cli/src/main.rs` |
| Name templating | `render_name` — `{project-dir}` slug, `{ts}` stamp | `code-split-cli/src/main.rs` |
| Snapshot & graph types | serde round-trip of the snapshot (the public artifact); builder / projection invariants; cycle and HK annotation | `code-split-core/src/*` |
| Graph extraction | module / file graph shape on small in-source inputs | `code-split-syn/src/*` |

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
let v = check_violations(&graphs, &rules);
assert!(!v.is_empty());

// GOOD — checks the count, which graph, the message, AND that the
// in-budget node did NOT contribute a violation
assert_eq!(v.len(), 1, "only the over-budget node violates");
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
assert_eq!(detect_plugin(d.path()).unwrap(), "rust");
```

## Naming

`{area}_{scenario}` in snake_case:

```text
parse_on_off_accepts_on_off_true_false
cycle_rules_default_test_embed_off_others_on
check_reports_enabled_cycle_group
apply_cycle_rules_strips_disabled_kind
detect_plugin_errors_on_ambiguous_or_empty
resolve_plugin_precedence_explicit_then_config_then_auto
```

## Organization

Tests live in-source, next to the code they cover, in a `#[cfg(test)] mod tests` block:

```text
crates/code-split-cli/src/config.rs             # parsing, rule evaluation
crates/code-split-cli/src/main.rs               # plugin resolution, name templating
crates/code-split-cli/src/plugin/python.rs      # Python extraction + import/call graph
crates/code-split-cli/src/plugin/javascript.rs  # JS/TS extraction + import/call graph
crates/code-split-core/src/builder.rs           # graph builder invariants
crates/code-split-core/src/cycles.rs            # SCC detection / cycle classification
crates/code-split-core/src/diff.rs              # snapshot comparison
crates/code-split-core/src/graph.rs             # graph / projection / serde
crates/code-split-core/src/snapshot.rs          # snapshot serde, path / id rewriting
crates/code-split-core/src/stats.rs             # metric averaging
crates/code-split-syn/src/module_graph.rs       # module / file extraction
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
