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
number still looks plausible (the root-vs-sum bug reported a constant `1` / `0`
for years while every test stayed green). The workflow below exists to make that
class of silent error impossible to ship.

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
three homes, and a test lives in the same crate as the computation it checks:

| metric family | crate | language scope |
|---|---|---|
| `cyclomatic` `cognitive` `exits` `args` `closures`; Halstead (`volume` `effort` `time` `bugs` `length` `vocabulary`); LOC (`sloc` `lloc` `cloc` `blank` `tloc`); `mi` `mi_sei` | per-language engines (one in-tree `tree-sitter` engine per crate: `rust_ts` / `python_ts` / `ecmascript_ts`), called from each plugin's `metrics()`; shared scaffolding (`FileMetrics` / `write_metrics` / `metric_specs`) in `code-ranker-graph` | **shared, multi-language** |
| dependency edges (`uses` `external` `reexports` `super` `contains`); `unsafe`; `items`; `loc` | each `code-ranker-plugin-<lang>` (its own `syn` / tree-sitter walk) | **per-language** |
| coupling (`fan_in` `fan_out` `fan_out_external` `hk`); `cycle` | `code-ranker-graph` (operates on the abstract graph) | language-agnostic |

So a complexity metric is added / fixed / tested in the per-language engines
(scaffolding shared via `code-ranker-graph`); an edge / `unsafe` / `items` metric in the
relevant plugin crate; coupling / cycle in `code-ranker-graph`. There is no
single "metric tests" crate — tests follow the computation.

For the machine-usable identifier catalog — every metric keyed by tier and source
as a dotted id (`t1code.eta1`, `t2code.volume`, `t1graph.fan_in`, `t3.hk`,
`t4.<metric>.mean`, `t5.ranker_score`), with an emitted/intermediate/planned
status on each — see [`metric-tiers.md`](metric-tiers.md).

## Runbook A — add a new metric

Two computation paths, depending on the home above.

### A1. A shared (complexity-engine) metric

Computed by the per-language `tree-sitter` engines — `rust_ts` in
`code-ranker-plugin-rust`, `python_ts` in `code-ranker-plugin-python`,
`ecmascript_ts` in `code-ranker-ecmascript-core` — each a port of
`rust-code-analysis`'s rules. Each engine builds a
`code_ranker_graph::FileMetrics`, which the plugin's `metrics()` writes onto
file nodes via `code_ranker_graph::write_metrics`. Add a metric across:

- `code_ranker_graph::FileMetrics` — add the field (the canonical key set).
- each per-language engine (`rust_ts` / `python_ts` / `ecmascript_ts`) — compute
  the new count during the tree walk and set the field. Per-function metrics are
  **summed over the file's child function spaces**, never read from the vacuous
  file-root value (that is the root-vs-sum bug).
- `code_ranker_graph::write_metrics` — `put("<key>", m.<field>)` (gated at its
  `omit_at`); one writer now serves every language.
- `code_ranker_graph::metric_specs` — a `SpecRow` declaring label / name / short /
  description / formula / direction, and (if its no-signal value is not `0`)
  `omit_at` via `metric_omit_at`, so emission and the published spec never drift.
  Keep the **description language-neutral** here. If a language needs a different
  wording (e.g. Rust noting that `sloc`/`lloc`/`cloc`/`blank` exclude inline
  `#[cfg(test)]` items), override it in that plugin's `LanguagePlugin::metric_specs`
  hook — the neutral default must never carry one language's nuance (it would leak
  into every other language's snapshot).

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

**2. Count during the walk** — `module_graph.rs`, `walk_file`. Add a
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

Goldens **freeze** values; they do not **verify** them (the root-vs-sum value was
frozen as "expected" for years). Real verification needs an independent source of
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
| `code-ranker-plugin-rust` / `-python` / `-ecmascript-core` (one per engine) | each crate's `#[cfg(test)]` (e.g. `metrics_tests` / `lib.rs` tests) | 1, 2, 3 | `cyclomatic` `cognitive` `exits` `args` `closures`, Halstead, LOC, `mi`/`mi_sei` — driven via a local `metric_of` (the crate's own engine) on per-language snippets: keyword-injection invariance (FP), +1-construct increment (FN / magnitude), hand-labelled exact-count anchors | 1 ✅ (FP matrix 9 positions × the per-language **trigger set** from the spec, branch-form FN, cross-language); 2 ✅ (deterministic generator: construct count = ground truth over a grid); 3 ✅ (`complexity_absolute_anchors_hand_derived`: exact integer counts hand-derived from the spec; `complexity_frozen_scale_anchors`: cognitive/Halstead/MI frozen scale anchors). **By design:** the whole-file `cyclomatic` and `exits` exceed a naive textbook reading — we match `rust-code-analysis` (the **algorithm of record** our engines port), and the spec states this explicitly. (a) `cyclomatic` = the file unit's base path (1) + the per-function McCabe sum. Textbook McCabe over functions (`V(G)=E−N+2P`) carries no container term, but the analyzer counts the file unit and — crucially — its `mi` is computed from the same `cyclomatic_sum()`, so emitting that value verbatim keeps `cyclomatic` and `mi` coherent. (b) `exits` has **no canonical theory**, so the analyzer's rule (each `return`/`?` + a value-returning `-> T` exit) is the source of truth. Both documented in §cyclomatic / §exits with citations; code emits the analyzer's values unchanged, goldens unchanged. |
| `code-ranker-plugin-rust` | `src/module_graph.rs` `#[cfg(test)]` | 1, 3 | `unsafe` `items` `loc` + edge detection: a keyword in an identifier / comment / string / macro body → no count / edge; a real construct or `use` → exact | 1 ✅ (`unsafe` + bare-path FP); 2 ✅ (collector scaling: N real paths + noise → N); broader positions ⏳ |
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
otherwise report a vacuous `1`). Gate emission on the **same** value the spec
publishes (for central metrics via `metric_omit_at`; for a plugin metric, the
`> 0` check above) — never hardcode a bespoke threshold that the spec does not
declare.

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
