# e2e fixtures & golden snapshots

Nine tiny projects (one per language) exercise the `code-ranker` analyzer in the
**files** level of the generic graph model: nodes of `kind` `"file"` /
`"external"`, connected by `uses` (flow) and `reexports` / `contains` / `super`
(non-flow, structural) edges — the last being the Rust `use super::*` /
`use crate::<ancestor>::*` namespace pull.

Each fixture lives **next to its plugin crate** so the sample and the parser that
produces it sit together:

```
crates/code-ranker-plugins/src/rust/tests/sample/
crates/code-ranker-plugins/src/python/tests/sample/
crates/code-ranker-plugins/src/javascript/tests/sample/
crates/code-ranker-plugins/src/typescript/tests/sample/
crates/code-ranker-plugins/src/go/tests/sample/
crates/code-ranker-plugins/src/c/tests/sample/
crates/code-ranker-plugins/src/cpp/tests/sample/
crates/code-ranker-plugins/src/csharp/tests/sample/
crates/code-ranker-plugins/src/markdown/tests/sample/
```

Each project deliberately contains **both the dependency forms we DO detect and
the known blind spots**, documented in the source comments and pinned in its
`code-ranker-report.json`.

## Why these fixtures exist

The goldens are the **regression net for metric correctness**, not just for graph
shape. A whole class of metric bug is *silent*: a per-function metric
(`cyclomatic`, `cognitive`, `exits`, `args`, `closures`) read from the file's
**root** code space instead of summed over its child function spaces comes out as
a constant — `1` for `cyclomatic`, `0` (and thus omitted) for the rest — on every
file. The e2e test stays green because the golden froze that broken output as the
"expected" value. The bug hid for exactly as long as no fixture exercised the
metric with a non-trivial value.

So the standing requirement is: **every metric and every case must appear with a
meaningful, non-trivial value in at least one golden.** Concretely the fixtures
must, between them, exercise:

- **Every node metric the analyzer computes for the language**, with a non-zero
  value on at least one file — including the per-function ones (a function with
  **nested branches** → `cognitive`; a `return` → `exits`; a **multi-argument**
  function → `args`; a **closure** → `closures`; an `unsafe` block → `unsafe`). A
  fixture of trivial stub functions is not enough: it leaves those metrics at
  their omit value and the golden asserts nothing about them.
- **Every edge kind** (`uses`, `contains`, `reexports`, `super`) and an external
  node.
- **Every cycle kind** — both `mutual` (2-node) and `chain` (3+-node) — in every
  language's fixture.

When a fixture is missing one of these, the metric/case is **unguarded**: a future
regression (or a re-introduction of the root-vs-sum read) changes nothing the test
can see. Adding coverage means changing a sample's source so the value becomes
non-trivial, then regenerating its golden (below).

**Per-language metric scope.** The shared generic metric engine
(`code-ranker-plugins/src/engine/`, parameterized per language by a `Dialect`) does not
emit every metric for every language, so "every metric" means *every metric the
engine emits for that language* — and every metric is still guarded by **at least
one** golden. Known gaps the fixtures cannot fill (the construct is present in each
`complex.*` but no value is emitted):

| metric | not emitted for | why |
|---|---|---|
| `tloc` | Python, JavaScript, TypeScript, Go | only the Rust analysis strips `#[cfg(test)]` items |
| `items`, `unsafe` | non-Rust | emitted only by the Rust plugin (not in the central catalog) |

This per-language scope is enforced by the `every_central_metric_is_exercised_per_language`
test: each central metric must be non-zero in every language's golden except the
rows above (encoded as `COVERAGE_EXCEPTIONS`), and a stale exception (the analyzer
started emitting it) also fails — so this table and the test stay in lock-step.

So per golden, **every metric the analyzer/plugin produces for that language
appears with a non-zero value on at least one file** (Rust covers all 26; the
others cover all minus the rows above). Verified by iterating each committed
golden, not by spot-check.

`cyclomatic` / `cognitive` are computed for all eight code languages (every
language but Markdown, which is documentation and has no complexity engine).

## One grammar version per language

A plugin parses each file for structure and the metric engine measures the same
file; both must use the **same** tree-sitter grammar, or one run could parse a
file two different ways (the version-skew class of bug). The whole workspace
therefore pins exactly **one** version of each grammar (`tree-sitter` core plus
`tree-sitter-{rust,python,javascript,typescript,go}`), shared by the plugins and the
in-tree metric engines. The `grammar_single_version` test
(`crates/code-ranker-cli/tests/grammar_single_version.rs`) reads `Cargo.lock` and
fails the build if any of those grammars resolves to more than one version — e.g.
a stray `=x.y.z` pin, or a new dependency bundling its own copy. The metric
engines resolve node kinds **by name**, so they stay correct across grammar
bumps; that is why a single shared version is safe.

## How it works

- `crates/code-ranker-plugins/src/<lang>/tests/sample/code-ranker.toml` — a self-contained
  config (plugin pinned, `ignore.tests = false` to override the **on-by-default**
  test skipping so test files stay in the graph and the fixture exercises them).
- `crates/code-ranker-plugins/src/<lang>/tests/sample/code-ranker-report.json` — the **golden**
  JSON report (`schema_version: "3"`). The graph is already relativized to the
  `{target}` placeholder (machine-independent). The header (`generated_at`,
  `command`, `git`, versions, absolute paths, `timings`) is kept frozen /
  anonymized in the committed file, and normalized only at comparison time.
- `crates/code-ranker-plugins/src/<lang>/tests/sample/code-ranker-check.sarif` — the **golden
  SARIF** for `check --output-format sarif` on that sample: the fired-rules catalog
  (`tool.driver.rules`), the results, and each result's stable `partialFingerprints`
  (`codeRankerRuleLocation/v1` = `<rule>:<location>`, line-independent). Everything
  is deterministic and `{target}`-relative; the only volatile field is
  `tool.driver.version` (the crate version), blanked on both sides at comparison
  time. The test asserts that field carries the live crate version, then compares
  the rest **character-for-character**.
- `crates/code-ranker-plugins/src/<lang>/tests/sample/code-ranker-check.codequality.json` —
  the **golden Code Quality** (CodeClimate) report for
  `check --output-format codequality`: a flat array of issues, each with a stable
  `fingerprint` (`<rule>:<location>`), `severity`, and `location.path` +
  `lines.begin`. No volatile fields, so it is compared verbatim.
- `crates/code-ranker-cli/tests/e2e.rs` — the test: runs the binary on each
  sample, asserts the volatile header fields changed, normalizes them to a
  canonical value on both sides, and compares the whole structure
  **character-for-character** (100% match required). The same file also holds the
  SARIF golden checks (`*_sample_check_sarif_matches_golden`) and the Code Quality
  golden checks (`*_sample_check_codequality_matches_golden`). It also holds the
  declarative-metric / level checks (no golden file, self-contained temp project):
  `user_defined_metric_is_computed_and_emitted` (a `[metrics.<key>]` CEL formula
  is computed and emitted with its spec), `user_defined_aggregate_lands_in_stats`
  (a graph-scope `agg(…)` lands in `stats`), and `functions_level_is_opt_in` (the
  `functions` level is absent by default and present with per-function nodes when
  `[levels] functions` is on).
- `crates/code-ranker-cli/src/plugin/mod.rs` — `every_registered_plugin_has_committed_goldens`:
  a guard unit test driven by the **plugin registry** (the single source of truth for
  which languages exist). It asserts every registered plugin ships *both* goldens
  (`code-ranker-report.json`, `code-ranker-check.sarif`, and
  `code-ranker-check.codequality.json`), so adding a new language
  fails the build until its fixtures are committed — the gap can't slip through by
  simply lacking an e2e case.

```sh
cargo test -p code-ranker --test e2e    # verify against the committed goldens
```

## Regenerating the goldens

After an intentional analyzer change, regenerate each language's golden by
running `code-ranker report` on its sample with the sample's own config. Build the
binary first; the Rust sample resolves its crates from the warm cargo cache, so
analysis stays offline:

```sh
cargo build -p code-ranker
export CARGO_NET_OFFLINE=true
bin=target/debug/code-ranker

for lang in rust python javascript typescript go; do
  dir="crates/code-ranker-plugins/src/$lang/tests/sample"
  "$bin" report "$dir" \
    --config "$dir/code-ranker.toml" \
    --output.json.path="$dir/code-ranker-report.json"
done
```

The e2e test normalizes the volatile header (timestamp, command, git, versions,
absolute paths, per-stage `ms`) at comparison time, so the regenerated goldens
will pass as-is. To keep the **committed** file machine-independent and
churn-free, freeze that header — anonymize your home dir and zero the volatile
fields — before committing:

```sh
for lang in rust python javascript typescript go; do
  f="crates/code-ranker-plugins/src/$lang/tests/sample/code-ranker-report.json"
  python3 - "$f" "$PWD" "$HOME" <<'PY'
import sys, json
path, repo, home = sys.argv[1:4]
text = open(path).read().replace(repo, "/home/user/code-ranker").replace(home, "/home/user")
d = json.loads(text)
d["generated_at"] = "1970-01-01T00:00:00Z"
if "git" in d:
    d["git"] = {"branch": "main", "commit": "000000000000",
                "dirty_files": 0, "origin": "git@example.com:org/repo.git"}
for t in d.get("timings", []):
    t["ms"] = 0
open(path, "w").write(json.dumps(d, indent=2, sort_keys=True, ensure_ascii=False) + "\n")
PY
done
```

### Regenerating the SARIF goldens

The `check --output-format sarif` goldens are fully deterministic (`{target}`-relative,
no machine-specific paths), so they need no anonymization — `check` exits non-zero when
the sample has violations, which is expected, so ignore the exit code:

```sh
cargo build -p code-ranker
export CARGO_NET_OFFLINE=true
bin=target/debug/code-ranker

for lang in rust python javascript typescript go; do
  dir="crates/code-ranker-plugins/src/$lang/tests/sample"
  "$bin" check "$dir" --config "$dir/code-ranker.toml" \
    --output-format sarif > "$dir/code-ranker-check.sarif" || true
done
```

The committed file keeps the real `tool.driver.version`; the test blanks it on both
sides, so a release bump never forces a regeneration here.

### Regenerating the Code Quality goldens

Same shape, fully deterministic (no volatile fields), so compared verbatim:

```sh
for lang in rust python javascript typescript go; do
  dir="crates/code-ranker-plugins/src/$lang/tests/sample"
  "$bin" check "$dir" --config "$dir/code-ranker.toml" \
    --output-format codequality > "$dir/code-ranker-check.codequality.json" || true
done
```

## Coverage matrix

Every project contains a file-to-file dependency cycle (`a ⇄ b`), an external
dependency, and a test file.

### Rust (`crates/code-ranker-plugins/src/rust/tests/sample/`)

**Metric coverage** (`src/complex.rs`): a dependency-free file built solely to
surface the per-function metrics with real values — nested branches
(`cyclomatic` / `cognitive`), early `return`s (`exits`), several arguments
(`args`), a closure (`closures`), and an `unsafe` block (`unsafe`). Without it
the stub functions elsewhere leave those metrics at their omit value and the
golden would assert nothing about them.

**Cycle kinds** — both are pinned: `mutual` (2-node, `a ⇄ b`) and `chain`
(3-node, `chain::one → two → three → one` under `src/chain/`).

Detected: `use crate::`, groups `{}`, glob `*`, `as` rename, `super::`, inline
modules, `pub use` → `Reexports` edge, external crate via `use serde::` →
`External` node, and **bare qualified paths** in expressions/types with no
`use` — both cross-crate (`once_cell::sync::Lazy` → the crate's `External` node)
and intra-crate (`foo::run()` → a `Uses` edge `lib.rs → foo.rs`). A
`std::`/`core::` path is recognized but is NOT emitted as an External node.

**Namespace pull → `super` edge** (`src/foo/bar.rs`): a glob `use super::*`
that reaches *up* the module tree is emitted as the non-flow `super` kind
(`foo/bar.rs → foo.rs`), not `uses` — kept in the JSON but excluded from
fan-in / fan-out / HK / cycles; on the map drawn dashed on a leaf-node hover
(like `contains` / `reexports`).
Contrast `b.rs`'s `use super::a::alpha`: a *named* import of a sibling item is a
real `Uses` edge — only the glob pull from an ancestor becomes `super`.

**Cycle semantics** (`src/cycle_examples/`): a dedicated module spelling out which
edge forms close a cycle and which do not — a `reexports` + back-`uses` pair
(`reex_hub` / `reex_spoke`), a `super` glob where the child really uses a parent
item (`sup_parent` — a genuine but deprioritized cycle), and one where it does not
(`sup_loose` — benign scope-sugar). None are cycles today (only `uses` is flow);
the full reasoning is in [cycles.md](cycles.md).

**Inline tests excluded from metrics** (`lib.rs`, `c.rs`, `derives.rs` carry
`#[cfg(test)] mod tests`): the Rust metric step strips test items first, so those
lines are excluded from `sloc` / `lloc` / `cloc` / `blank` (and HK) and counted
as `tloc` instead — production metrics only. The test bodies reference items by
their own `crate::<mod>::…` path, so they add no cross-file edges.

**Cross-crate, submodule-precise** (the `helper` workspace member): a
`use helper::widget::{Widget, make}` resolves through `helper`'s library module
index to the **owning submodule file** — `cross.rs → helper/src/widget.rs` and
`→ helper/src/gadget.rs`, not a single edge to `helper`'s crate root. A path
that stops at a crate-root item (`use helper::TOP`) has no deeper submodule to
match and falls back to the root (`→ helper/src/lib.rs`). Registry crates with
no local library index still collapse to one `External` node.

**Qualified derive macros** (`derives.rs`): `#[derive(serde::Serialize)]` names
a crate by a fully-qualified path *inside* the derive list. Derive arguments are
an opaque token stream, but the analyzer parses qualified derive paths, so this
yields `derives.rs → serde` even with no `use serde` in the file. (A bare
single-segment derive like `#[derive(Serialize)]` still relies on the `use` for
its edge.)

**`#[path = "..."]` modules** (`relocated/custom.rs`): a module whose backing
file is at a non-default location is resolved via its `#[path]` attribute
(relative to the declaring file's directory), walked, and its edges captured
(`custom.rs → c.rs`). Without `#[path]` support the file and its edges would be
silently dropped.

Each `mod foo;` becomes a `File` node and emits a `Contains` edge
(parent → child). `Contains` is kept in the JSON snapshot as structural
ownership, but is **not** drawn on the main map and **not** counted in
fan_in / HK / cycles (directory grouping shows ownership instead).

Not detected: `extern crate serde;` (old syntax, no edge); a `use` **inside a
macro body** (the `use crate::c::gamma` hidden in the `pull_in_c!` body is
invisible, so `b.rs` gets no edge to `c.rs`); macro invocations (`make_answer!`,
`pull_in_c!`) — no nodes or edges. `macros.rs` is the remaining blind spot: it
is reached only via `mod macros;` (a `Contains`, excluded from fan_in), so it
has no information-flow inbound edge. Integration tests under `tests/` are a
separate target kind that is not analyzed at all.

### Python (`crates/code-ranker-plugins/src/python/tests/sample/`)

Detected: `import`, dotted (`import os.path`), `as`, `from … import`, relative
(`from .`, `from .c`), grouped, star `*`, and — importantly — an **import inside a
function** (`base64`).

Not detected: dynamic/string-based imports — `importlib.import_module("…")`,
`__import__("…")`, `eval("…")` (the `xml`/`csv`/`hashlib` modules are absent).

### JavaScript (`crates/code-ranker-plugins/src/javascript/tests/sample/`)

Detected: `import` (named/namespace/default/side-effect), `export … from`
(re-export), `require()` both local and external, extension and `index.*`
resolution.

Not detected: dynamic `import("./dynamic.js")` (`dynamic.js` is an orphan);
`require(variable)` with a computed argument.

### TypeScript (`crates/code-ranker-plugins/src/typescript/tests/sample/`)

Detected: import without extension, `import type` (deduped with the value import
into a single edge), the `@/` alias → source root, `export * from`, external
`axios`, scoped `@scope/util`.

Not detected: dynamic `import("./lazy")` (`lazy.ts` is an orphan); a tsconfig
alias other than `@/` — `~utils/*` is **misclassified** as an external package
`~utils` instead of an edge to `util.ts`.
