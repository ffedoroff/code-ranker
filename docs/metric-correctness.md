# Working on a metric — add, fix, and prove it correct

This is the **single entry point** for any change to a code-ranker metric:
adding a new one, or fixing a bug in an existing one. It ties together the
*goal* (what "correct" means), *where* a metric is computed, the *runbook* for
each task, and *how we prove* correctness inside the test budget. Read it before
touching metric code, and link new tests / PRs back to it.

## The goal (why this doc exists)

Every metric value MUST equal the true count of what it measures — **no false
positives** (a keyword appearing in an identifier / comment / string / macro
body never counts) and **no false negatives** (a real construct is never
missed). This is the **Metric Accuracy** requirement
`cpt-code-ranker-nfr-metric-accuracy` (PRD §6.1), made attainable by the
**AST-Accurate Metrics** principle `cpt-code-ranker-principle-metric-accuracy`
(DESIGN §2.1): metrics are read from the parsed syntax tree, never matched as
text.

The product output is an anomaly shortlist a human or AI agent acts on, so a
silently miscounted metric is a silently wrong ranking — and it hides because the
number still looks plausible: a green golden freezes whatever value it was given,
right or wrong. The workflow below exists to make that class of silent error
impossible to ship.

## The source of truth: the metric spec

"Count correctly" is undefined without a written rule for *what counts*. Each
metric's counting rules live in the normative spec `principles/<lang>/metrics.md`
(per language — the semantics differ; e.g. whether `?` is a branch, whether a
tail expression is an exit).

- **Adding a metric:** write its rule there first — that rule *is* the definition
  of correct.
- **Fixing a metric:** check behaviour against the rule. If the rule was never
  written, writing it is the first half of the fix — you cannot call something a
  bug without a definition it violates.

Everything else (tests, code) asserts conformance to this spec.

## Where a metric is computed (so you know where to code and test)

A metric's home is **where it is computed**, not where it is shown. There are
four homes, and a test lives in the same crate as the computation it checks:

| metric family | home | language scope |
|---|---|---|
| **measured counting**: `cognitive` `exits` `args` `closures`; Halstead base counts (η₁/η₂/N₁/N₂); LOC (`sloc` `lloc` `cloc` `blank` `tloc`); `spaces`/`branches`/`span_sloc` | per-language engines (one in-tree `tree-sitter` engine per crate: `rust_ts` / `python_ts` / `ecmascript_ts`), called from each plugin's `metrics()`, returning a `MetricInputs` | **shared, multi-language** |
| **derived formulas** (`cyclomatic`, Halstead `volume`/`effort`/`time`/`bugs`/`length`/`vocabulary`, `mi`/`mi_sei`) and **aggregates** | **data** — CEL formulas + specs in `code-ranker-graph/metrics/builtin.toml`, evaluated by the registry engine (`registry.rs`): derived metrics per node (written by `write_metrics`), aggregates over the whole node set (graph-scope, into the `stats` block). No derived-metric name is hardcoded in Rust. | language-agnostic |
| dependency edges (`uses` `external` `reexports` `super` `contains`); `unsafe`; `items`; `loc` | each `code-ranker-plugin-<lang>` (its own `syn` / tree-sitter walk) | **per-language** |
| coupling (`fan_in` `fan_out` `fan_out_external` `hk`); `cycle` | `code-ranker-graph` (operates on the abstract graph) | language-agnostic |

So a **measured count** is added / fixed / tested in the per-language engines; a
**derived formula** (or aggregate) is added by editing `metrics/builtin.toml` (or,
for a user metric, `[metrics.<key>]` in config) — no Rust change — and tested in
`code-ranker-graph` (`registry.rs` / `metrics.rs`); an edge / `unsafe` / `items`
metric in the relevant plugin crate; coupling / cycle in `code-ranker-graph`.
There is no single "metric tests" crate — tests follow the computation. The same
registry runs per **unit**, so file nodes and (with `[levels] functions` on)
function nodes share one definition.

For the full catalog — every metric grouped by how it is produced (measured /
derived / aggregated) — see [`metric-tiers.md`](metric-tiers.md). Keys are flat:
the catalog name is the bare node attribute key.

## How derived metrics are computed (the CEL registry engine)

Derived metrics and aggregates are **declarative data**, not Rust: a CEL
`formula` plus a display spec, in `code-ranker-graph/metrics/builtin.toml` (the
built-ins) or under `[metrics.<key>]` in `code-ranker.toml` (user metrics). The
registry engine — `code-ranker-graph/src/registry.rs` — turns them into node
values. No derived-metric name is hardcoded in Rust.

### What a metric definition is

A `MetricDef` carries:

- `formula` — a CEL expression over **measured inputs** and **other metric keys**,
  plus a small host standard library (`log2` / `ln` / `pow` / `sqrt` / `sin` / …)
  the language lacks. Each host function is the exact `f64` operation the engines
  used before, so a transcribed formula is **bit-identical**.
- `scope` — `node` (computed per unit) or `graph` (an aggregate; see below).
- spec fields (`label` / `name` / `short` / `description` / `direction` / `group`
  / `value_type` / `omit_at`) — what makes it a first-class, sortable,
  delta-coloured column. The viewer hardcodes no metric by name.

The measured inputs a formula may read come from `code_ranker_graph::MetricInputs`
(what the engines measure): `eta1` / `eta2` / `n1` / `n2`, `spaces` / `branches`,
`cognitive` / `exits` / `args` / `closures`, `sloc` / `lloc` / `cloc` / `blank` /
`tloc`, `span_sloc`.

### Ordering — a compile-time topological sort

A formula references other metrics by key (e.g. `mi` reads `volume` and
`cyclomatic`). `Engine::compile`:

1. compiles each formula once (`Program::compile`) — never per node;
2. builds a dependency graph by scanning each formula's text for **whole-word**
   occurrences of other metric keys (`references()`, the same `\bkey\b` rule the
   viewer uses, so `mi` does not match inside `mi_sei`);
3. Kahn topological sort → the compiled programs are stored **in dependency
   order** (inputs before dependents);
4. a definition cycle (`a` ← `b` ← `a`) is rejected here as
   `RegistryError::Cycle`; a formula that does not parse is `RegistryError::Parse`.

Measured inputs (`span_sloc`, `n1`, …) and host functions (`ln`) are **not**
metric keys, so they are never dependencies — they are simply present from the
start.

### Evaluation — one context per unit, results fed forward

`eval_node` (run once per file unit, and per function unit when the `functions`
level is on):

1. builds **one** CEL `Context`, registers the host stdlib **once**, and adds the
   unit's measured inputs as variables;
2. runs the ordered programs and — **after each one — adds its result back into
   the same context**, so a later (dependent) formula reads it.

Worked example, `mi = 171.0 - 5.2*ln(volume) - 0.23*cyclomatic - 16.2*ln(span_sloc)`:

- `span_sloc` is already in the context (a measured input);
- `volume` runs first (topo order) → added to the context;
- `cyclomatic` runs → added;
- `mi` runs against a context that already holds `volume`, `cyclomatic`,
  `span_sloc`.

Dependents read **full-precision** upstream values — the context holds unrounded
`f64`; the 3-significant-digit rounding happens only when a value is written onto
the node (`num_attr`). That is why the server-computed values match the former
hardcoded Rust derivation bit-for-bit, which the e2e goldens prove.

### Aggregates (graph scope)

A metric with `scope = "graph"` is evaluated **once over the whole node set**
(`eval_graph`). Its formula calls the host reducer `agg(key, reducer, population)`:

- `reducer` ∈ `sum` / `avg` / `min` / `max` / `count` / `median` / `p<q>`
  (percentile by **numpy R-7** linear interpolation);
- `population` ∈ `not_empty` (only nodes whose value ≠ `omit_at`) / `all` (every
  applicable node, a missing value counted at the `omit_at` floor);
- an empty population → omitted.

The result lands in the level's `stats` block under the metric's key. The
built-in `stats` means (a fixed key set) are computed directly in `stats.rs`.

### Failure modes — and how we react

- **Definition errors** (a formula that will not parse, or a dependency cycle) are
  caught at load (`Engine::compile`) and **abort the run** with a clear config
  error — a broken metric definition cannot ship silently.
- **Per-node evaluation errors** (an undefined reference, a type error, division
  by zero, or a non-finite / non-numeric result) make `exec_f64` return `None`:
  the metric is simply **omitted for that node** and the run continues — the same
  graceful semantic as the viewer's `evalCalc` (`catch → null`).
- **Project-wide empty** — if a declared metric produced **no value on any node**
  (e.g. a misspelled input key resolves to nothing everywhere), a warning is
  printed to stderr, so the otherwise-silent typo is visible.

### Performance

Programs are compiled once (built-ins via a process-wide `LazyLock`; user metrics
once per analysis run) and reused for every unit. Per unit, only the `Context`
is set up (host stdlib + inputs) and the programs executed — no recompilation.

## Runbook A — add a new metric

Two paths, depending on the home above.

### A1. A derived metric — edit data, not Rust

A derived metric is a CEL `formula` plus its display spec. Add it in
`code-ranker-graph/metrics/builtin.toml` (built-in) or under `[metrics.<key>]` in
`code-ranker.toml` (user metric):

```toml
[specs.<key>]            # display spec (label / name / short / description /
value_type  = "float"   # direction / group / omit_at)
label       = "…"
direction   = "lower_better"
group       = "halstead"

[formulas]
<key> = "…"             # CEL over measured inputs + other metrics; host math:
                        # log2 / ln / pow / sqrt / sin / …
```

- The formula reads measured inputs (`eta1`/`eta2`/`n1`/`n2`/`spaces`/`branches`/
  `sloc`/`cloc`/`span_sloc`/…) and earlier metrics by key; the registry engine
  (`registry.rs`) topologically orders by dependency and rejects a cycle at load.
- `omit_at` on the spec is the no-signal value (`0` for most; `1` for
  `cyclomatic`); emission and the published spec read the same value.
- A value not expressible as a formula over one unit's inputs is **not** derived —
  if it needs a new AST count, add that count in the engines first (A1b), then a
  formula that consumes it.

**A1b. A new measured count** (when a formula needs a number the engines don't
measure): add the field to `code_ranker_graph::MetricInputs` and count it during
the tree walk in each per-language engine (`rust_ts` / `python_ts` /
`ecmascript_ts`). The same engines feed both the file unit and (with the
`functions` level on) each function unit, so a count is measured once and the
formula applies per unit.

### A2. A plugin-computed metric (worked example: `unsafe`)

Computed by a language plugin during its source walk. The plugin already ships
two such per-file integers — **`loc`** and **`items`** — so a new one is *not*
new machinery; it rides the same four touchpoints. All paths below are in
`crates/code-ranker-plugin-rust/src/`. The split to keep in mind:

- the attribute **value** reaches JSON purely because it sits in `node.attrs`
  (touchpoints 1–3);
- the attribute **spec** (touchpoint 4) is what makes the viewer render it as a
  named, sortable, delta-coloured metric — the viewer hardcodes no metric by name.

**1. Carry the count on the internal node model** — `internal.rs`, `struct Node`.
Add a field next to `loc` / `item_count` and default it (`None`) at every
construction site (the compiler lists them):

```rust
pub item_count: Option<u32>,
pub unsafe_count: Option<u32>,   // NEW: count of `unsafe` usages (production only)
```

**2. Count during the walk** — `module_graph/walk.rs`, `walk_file`. Add a
`syn::visit::Visit` collector and run it over the **same test-filtered items** as
the existing collector, so `unsafe` inside `#[cfg(test)]` / `#[test]` never counts
(consistent with how `sloc` / complexity already exclude tests):

```rust
/// Counts `unsafe` usages: `unsafe { }` blocks plus `unsafe fn`/`impl`/`trait`.
#[derive(Default)]
struct UnsafeCounter { count: u32 }

impl<'ast> syn::visit::Visit<'ast> for UnsafeCounter {
    fn visit_expr_unsafe(&mut self, n: &'ast syn::ExprUnsafe) {
        self.count += 1;
        syn::visit::visit_expr_unsafe(self, n);
    }
    fn visit_item_fn(&mut self, n: &'ast syn::ItemFn) {
        if n.sig.unsafety.is_some() { self.count += 1; }
        syn::visit::visit_item_fn(self, n);
    }
    fn visit_item_impl(&mut self, n: &'ast syn::ItemImpl) {
        if n.unsafety.is_some() { self.count += 1; }
        syn::visit::visit_item_impl(self, n);
    }
    fn visit_item_trait(&mut self, n: &'ast syn::ItemTrait) {
        if n.unsafety.is_some() { self.count += 1; }
        syn::visit::visit_item_trait(self, n);
    }
}
```

Then write it onto the owning module node where `loc` / `item_count` are set:
`node.unsafe_count = Some(unsafe_counter.count);`. Counting from these AST nodes
(not a text scan) is exactly what keeps `super_unsafe_fn`, a `// unsafe` comment,
or a `"unsafe"` string from counting — see the AST-Accurate principle.

**3. Carry it through the module→file collapse** — `lib.rs`. `loc` / `items` are
written in **two** branches (the *insert* branch that first creates the file node
and the *update* branch that touches an existing one). Mirror the new field in
**both**, gated at its no-signal value:

```rust
if let Some(u) = node.unsafe_count {
    if u > 0 {                          // omit the no-signal value — see omit_at
        attrs.insert("unsafe".to_string(), AttrValue::Int(u as i64));
    }
}
```

**4. Declare the attribute spec** — `lib.rs`, `fn levels()`, in the
`node_attributes` block next to `loc` / `items`:

```rust
let mut unsafe_spec = aspec(ValueType::Int, "Unsafe");
unsafe_spec.short = Some("Unsafe".into());
unsafe_spec.description = Some("Count of `unsafe` blocks and \
    `unsafe fn`/`impl`/`trait` in production code (tests excluded).".into());
unsafe_spec.direction = Direction::LowerBetter;   // higher = worse → red delta
node_attributes.insert("unsafe".into(), unsafe_spec);
```

The orchestrator merges the spec into the level dictionary and prunes it to keys
actually present, so no further wiring is needed.

## Runbook B — fix a bug in an existing metric

1. **Reproduce against the spec.** In the metric's home crate, write a failing
   test that pins the bug: a metamorphic case (inject the keyword in a comment /
   string / identifier and assert the count does **not** change → false positive;
   or add one real construct and assert the exact increment → false negative /
   magnitude). If `principles/<lang>/metrics.md` lacks the rule, write it first.
2. **Fix the detector** in that crate.
3. **Lock the regression** — the new test now fails on the old behaviour and
   passes on the new.
4. **Update fixtures / goldens** (below) if the emitted JSON changed, and update
   the spec / `node_schema.md` if the rule's wording was unclear.

## How we prove correctness (the test strategy)

Goldens **freeze** values; they do not **verify** them — a wrong value frozen as
"expected" stays green forever. Real verification needs an independent source of
truth. Five layers, each catching a **distinct** error class. **Layers 1–3 are
the implemented per-PR strategy; layers 4–5 are NOT planned** — they are recorded
below only as escalation options (see "Escalation").

| layer | catches uniquely | type | where | status |
|---|---|---|---|---|
| 1 metamorphic | FP / FN / magnitude vs our spec | unit | metric's home crate | **implemented**, per-PR |
| 2 generative | same + parser edge cases (nesting, raw strings, unicode) | unit (deterministic generator) | home crate | **implemented**, per-PR |
| 3 asserted anchors | absolute scale (uniform offset / scale bug) | unit + e2e | home crate + `cli` | **implemented** (generator relative anchors + e2e golden absolute pins) |
| 4 differential vs external tools / compiler | **our spec itself is wrong** + unknown-unknowns | e2e / integration | `cli` / `xtask` | **not planned** (escalation) |
| 5 detector mutation (`cargo-mutants`) | the tests have teeth | tooling job | over the crates' suites | **not planned** (escalation) |

Layers 1 / 2 / 3 all verify "matches **our** spec" — if the spec itself is wrong
they would happily confirm the wrong thing. That residual risk is what layers 4
and 5 would address, and it is why they are kept on record — but they are **not on
the roadmap** (see below).

### Escalation — layers 4 & 5 (documented, not planned)

These are deliberately **not implemented**. Reach for them only if a
metric-correctness problem surfaces that layers 1–3 do **not** catch — i.e. the
implemented suite is green but a metric is still wrong:

- **Layer 4 — differential vs an independent oracle.** If our *spec itself* is
  wrong (we defined "correct" incorrectly and 1–3 dutifully confirm it), compare
  against an outside definition: `unsafe` ↔ `cargo-geiger`, LOC ↔ `tokei` / `scc`,
  cyclomatic ↔ `lizard`, edges ↔ `rustc` / `rust-analyzer`. Cost: external tool
  dependencies and definitional drift (each tool defines metrics slightly
  differently) — and it conflicts with the offline-first NFR, so it could only be
  a manual / nightly job, never per-PR. Justified only if a real wrong-spec bug
  is found.
- **Layer 5 — detector mutation (`cargo-mutants`).** If a regression slips through
  green tests, mutation testing proves whether the suite actually has teeth (would
  a broken detector fail a test?). Cost: per-mutant recompiles (minutes–hours) and
  a dedicated CI job. Justified only if the suite is suspected of being toothless.

If neither failure mode shows up in practice, layers 1–3 are the whole strategy.

Metamorphic invariance is the backbone: for every metric, injecting its keyword
into each lexical position — identifier, line / block / doc comment, string / raw
string / char, attribute, format string, macro name, **unexpanded macro body**,
raw identifier `r#kw` — must leave the value unchanged; adding one real construct
must change it by exactly its defined increment.

### Budget: the full suite (unit + e2e) MUST pass in ≤ 20 s after compilation

After compilation, test cost is **process spawns + I/O, not CPU**: a small-snippet
parse is sub-millisecond, so thousands run in-process per second, while each
binary spawn is ~100–600 ms. Today the whole suite runs in ~3.5 s — and **the only
real cost is the e2e binary spawns (~2.3 s); every unit-test binary is ~0 s.**

Consequences for the layers:

- **1 / 2 / 3 are essentially free.** The analysis crates (the plugins,
  `code-ranker-ecmascript-core`, `code-ranker-graph`) are library-callable in-process — no binary,
  no `cli` — so the FP / FN core costs ~nothing. Spend the budget here: thousands
  of metamorphic / generated cases. Cap per-PR property-case counts; let nightly
  run the same generators far deeper.
- **4 and 5 cannot fit** (external processes / compilation / per-mutant rebuilds)
  and run nightly / manual — the 20 s ceiling enforces this, it is not optional.
- **e2e stays small** (golden match + the `every_central_metric_is_exercised_per_language`
  coverage invariant + a few format checks + one binary smoke test). `cli` is a
  binary-only crate, so e2e spawns the binary; at ~2.3 s of a 20 s budget that is
  fine. If e2e ever grows into the ceiling, extract a `cli` `lib.rs` and run the
  pipeline in-process to collapse those spawns.

So: per-PR proves "implementation matches our spec" broadly and fast; nightly
proves "spec matches reality" (4) and "tests have teeth" (5). A wrong-spec or
toothless-test gap lives ≤ a day, not forever.

### Test placement plan — which tests live where

The concrete map from the layers above to files (✅ exists today, ⏳ planned).
The home crate is always the one that **computes** the metric.

| home | test location | layers | covers | status |
|---|---|---|---|---|
| `code-ranker-plugin-rust` / `-python` / `-ecmascript-core` (one per engine) | each crate's `#[cfg(test)]` (e.g. `metrics_tests` / `lib.rs` tests) | 1, 2, 3 | `cyclomatic` `cognitive` `exits` `args` `closures`, Halstead, LOC, `mi`/`mi_sei` — driven via a local `metric_of` (the crate's own engine) on per-language snippets: keyword-injection invariance (FP), +1-construct increment (FN / magnitude), hand-labelled exact-count anchors | 1 ✅ (FP matrix 9 positions × the per-language **trigger set** from the spec, branch-form FN, cross-language); 2 ✅ (deterministic generator: construct count = ground truth over a grid); 3 ✅ (`complexity_absolute_anchors_hand_derived`: exact integer counts hand-derived from the spec; `complexity_frozen_scale_anchors`: cognitive/Halstead/MI frozen scale anchors). **By design:** the whole-file `cyclomatic` and `exits` exceed a naive textbook reading — we match `rust-code-analysis` (the **algorithm of record** our engines port), and the spec states this explicitly. (a) `cyclomatic` = `spaces + branches` — the file unit's base path (1) plus each function space and its decision points. Textbook McCabe over functions (`V(G)=E−N+2P`) carries no container term, but the analyzer counts the file unit, and `mi`'s formula consumes the same `cyclomatic` value, so the two stay coherent. (b) `exits` has **no canonical theory**, so the analyzer's rule (each `return`/`?` + a value-returning `-> T` exit) is the source of truth. Both documented in §cyclomatic / §exits with citations; code emits the analyzer's values unchanged, goldens unchanged. |
| `code-ranker-plugin-rust` | `src/module_graph/walk.rs` + `resolve.rs` `#[cfg(test)]` | 1, 3 | `unsafe` `items` `loc` + edge detection: a keyword in an identifier / comment / string / macro body → no count / edge; a real construct or `use` → exact | 1 ✅ (`unsafe` + bare-path FP); 2 ✅ (collector scaling: N real paths + noise → N); broader positions ⏳ |
| `code-ranker-plugin-python` / `-javascript` / `-typescript` | each plugin's `#[cfg(test)]` | 1, 2, 3 | per-language edge detection + FP invariance (a path inside a string / comment → no edge); edge scaling (n imports → n edges) | 1 ✅ (import-path FP); 2 ✅ (edge scaling) |
| `code-ranker-graph` | `src/hk.rs` / `src/cycles.rs` `#[cfg(test)]` | algorithmic | `fan_in` / `fan_out` / `hk` aggregation, `cycle` classification (mutual / chain) — graph maths, not a text-FP class | ✅ |
| `code-ranker-cli` | `tests/e2e.rs` | 3 (realistic), golden, coverage | full-pipeline JSON pinned per language; the `every_central_metric_is_exercised_per_language` coverage invariant; one binary smoke test | ✅ |
| `code-ranker-cli` or a new `xtask` | `tests/differential.rs`, feature-gated / `#[ignore]` | 4 | vs `cargo-geiger` (`unsafe`), `tokei` / `scc` (LOC), `rustc` / `rust-analyzer` (edges) over a small corpus | **not planned** (escalation only) |
| repo tooling | `cargo-mutants` config + CI job | 5 | mutate the per-language metric engines / plugin detectors, assert a test fails | **not planned** (escalation only) |

How to read it: the **top five rows are the implemented per-PR suite** (in-process
unit + lean e2e, ≤ 20 s); the **bottom two are not planned** — escalation options
only (see "Escalation"). Adding a metric means adding layer-1/2 cases in its home
crate — and the `every_central_metric_is_exercised_per_language` coverage
invariant will then *require* a new central metric to appear in every golden or
fail.

## Zero-omission / `omit_at`

A metric is omitted at its **no-signal value** — absent from the JSON, blank in
the viewer — so a present key always carries a meaningful value (see
`node_schema.md`). That value is `omit_at` on the `AttributeSpec`: `0` for almost
everything, `1` for `cyclomatic` (McCabe's floor — a function-less file would
otherwise report a vacuous `1`). Emission is gated on that same spec value — the
registry writer reads each metric's `omit_at` from its spec, so the emitted JSON
and the declared spec never drift; never hardcode a bespoke threshold the spec
does not declare.

## Snapshot / golden tests will change

Adding or changing a metric changes the JSON the e2e / sample goldens assert
against. Regenerate the goldens per `e2e.md` (re-run `report` on each sample, then
freeze the volatile header). The `every_central_metric_is_exercised_per_language`
test will additionally **require** a new central metric to appear non-zero in
every language's golden — add a fixture that produces it, or a documented
per-language exception. Never delete prior `.code-ranker/` run snapshots when
regenerating.

## Known limitations (deliberate non-goals, not bugs)

- **Purely syntactic.** Metrics count syntactic appearance, with no type /
  semantic analysis (this is why rust-analyzer is intentionally absent). Defined
  scope, documented in the spec — not a false negative.
- **Macros are not expanded.** A construct produced *inside* a macro body is
  invisible — `syn` / tree-sitter do not parse macro-invocation tokens.
- **Test exclusion is top-level.** `#[cfg(test)]` / `#[test]` / `#[bench]` items
  are filtered where they are walked; a test attribute on a deeply nested item is
  out of scope.

These are part of the spec's definition of "correct"; layer 4 (differential) is
where we confirm they are deliberate and consistent rather than accidental.

## Generalizing later

Once one per-file integer works end-to-end, the same touchpoints take any further
marker (`unwrap` / `expect`, `panic!` / `todo!`, …) — each is another counter and
another spec. Project normalization (per-100KLOC), `stats` rollup, cross-language
parity, and a composite 0–100 score are separate later layers, intentionally not
part of a single metric slice.

## Cross-references

- **Goal:** PRD §6.1 `cpt-code-ranker-nfr-metric-accuracy`, §9 acceptance;
  DESIGN §2.1 `cpt-code-ranker-principle-metric-accuracy`.
- **Spec (what "correct" means):** `principles/<lang>/metrics.md`.
- **Schema, attribute specs, `omit_at`:** `node_schema.md`.
- **Fixtures, goldens, coverage invariant, regeneration:** `e2e.md`.
