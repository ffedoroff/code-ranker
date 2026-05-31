# samples/ — fixed e2e fixtures

Four tiny projects (one per language) that exercise the `code-split` analyzer in
the **files-only** model: a single file graph with `File` + `External` nodes and
`Uses`/`Reexports` edges.

Each project deliberately contains **both the dependency forms we DO detect and
the known blind spots** (which we do not detect yet). The intent is documented in
the source comments and pinned in the `code-split-report.json` at the root of
each project.

## How it works

- `samples/<lang>/code-split.toml` — a self-contained config (plugin pinned,
  `ignore.tests = false` so test files stay in the graph).
- `samples/<lang>/code-split-report.json` — the **golden** JSON report. The graph
  is already relativized to the `{target}` placeholder (machine-independent). The
  header (`generated_at`, `command`, `git`, versions, absolute paths, `timings`)
  is kept as-is, with absolute paths anonymized to `/home/user/…`, and is
  normalized only at comparison time.
- `samples/regen.sh` — regenerate every golden after an intentional change (it
  also anonymizes machine-specific absolute paths in the header).
- `crates/code-split-cli/tests/e2e.rs` — the test: runs the binary on each
  sample, asserts the volatile header fields changed, normalizes them to a
  canonical value on both sides, and compares the whole structure
  **character-for-character** (100% match required).

```sh
bash samples/regen.sh                  # refresh the goldens
cargo test -p code-split --test e2e    # verify
```

## Coverage matrix

Every project contains a file-to-file dependency cycle (`a ⇄ b`), an external
dependency, and a test file.

### Rust (`rust/`)

Detected: `use crate::`, groups `{}`, glob `*`, `as` rename, `super::`, inline
modules, `mod foo;` declaration → a file→file `uses` edge (`lib.rs → foo.rs`),
`pub use` → `Reexports` edge, external crate via `use serde::` → `External` node,
and **crate-qualified bare paths** in expressions/types (`once_cell::sync::Lazy`
with no `use`) → the crate's `External` node (and, across workspace members, a
file→file edge to that crate's root). A `std::`/`core::` path is recognized but
is NOT emitted as an External node.

Not detected: `extern crate serde;` (old syntax, no edge); a `use` **inside a
macro body** (`macros.rs` has incoming fan-in from `mod macros;`, but the
`use crate::c::gamma` hidden in the `pull_in_c!` body is invisible, so it gets no
outgoing edge to `c.rs`); macro invocations (`make_answer!`, `pull_in_c!`) — no
nodes or edges; and integration tests under `tests/` — a separate target kind
that is not analyzed at all.

### Python (`python/`)

Detected: `import`, dotted (`import os.path`), `as`, `from … import`, relative
(`from .`, `from .c`), grouped, star `*`, and — importantly — an **import inside a
function** (`base64`).

Not detected: dynamic/string-based imports — `importlib.import_module("…")`,
`__import__("…")`, `eval("…")` (the `xml`/`csv`/`hashlib` modules are absent).

### JavaScript (`javascript/`)

Detected: `import` (named/namespace/default/side-effect), `export … from`
(re-export), `require()` both local and external, extension and `index.*`
resolution.

Not detected: dynamic `import("./dynamic.js")` (`dynamic.js` is an orphan);
`require(variable)` with a computed argument.

### TypeScript (`typescript/`)

Detected: import without extension, `import type` (deduped with the value import
into a single edge), the `@/` alias → source root, `export * from`, external
`axios`, scoped `@scope/util`.

Not detected: dynamic `import("./lazy")` (`lazy.ts` is an orphan); a tsconfig
alias other than `@/` — `~utils/*` is **misclassified** as an external package
`~utils` instead of an edge to `util.ts`.
