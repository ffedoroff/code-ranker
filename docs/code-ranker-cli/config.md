# code-ranker configuration

## Priority order

Settings are merged from multiple sources. **Higher priority wins** for the same key.

| Priority | Source | Example |
|---|---|---|
| 1 | CLI flags | `--ignore '**/tests/**'` |
| 2 | `--config KEY=VALUE` inline override | `--config rules.thresholds.file.hk=200000` |
| 3 | `--config <file>` (**repeatable** — see below) | `--config base.toml --config over.toml` |
| 4 | `code-ranker.toml` in cwd | `./code-ranker.toml` |
| 5 | `code-ranker.toml` in the analyzed target directory | `<target>/code-ranker.toml` |
| 6 | `Cargo.toml` metadata | `[workspace.metadata.code-ranker]` |
| 7 | Built-in defaults | `mutual` / `chain` on |

For `ignore.paths` and CLI `--ignore`: lists are **merged** (union), not replaced.  
For cycle rules and thresholds: CLI **overrides** the file value.

### Multiple `--config` files

`--config <file>` is **repeatable**. Files are deep-merged over the built-in
defaults **in command-line order, last wins**:

```sh
code-ranker check . --config base.toml --config team.toml --config rules.thresholds.file.hk=200000
#                    └── built-in defaults ⊕ base.toml ⊕ team.toml ⊕ inline KEY=VALUE ──┘
```

So a shared `base.toml` can set the house rules and a `team.toml` (or a CI-only
file) layer tweaks on top — `team.toml` wins per key, and inline `KEY=VALUE`
overrides win over every file. Because the merge is the same `deep_merge` used
everywhere, list keys can be **patched** across layers, not just replaced
(`paths = { add = [...] }` in a later file extends an earlier list). **Passing any
`--config` file disables auto-discovery** of `code-ranker.toml` (rows 4–6); inline
`KEY=VALUE` alone does not — discovery still runs and the override applies on top.

The **built-in defaults are the merge base, always** — they ship inside the binary
(`config/defaults.toml`) and are **deep-merged** with your config, so a partial
`code-ranker.toml` need only spell out what it changes and inherits every other key
(e.g. omit `[rules.cycles]` and you still get strict mutual/chain). A discovered file
overrides per key; arrays can also be patched in place with an op-table
(`paths = { add = ["x/**"], remove = ["y/**"] }`) instead of replaced wholesale.
Run [`--export-full-config`](#--export-full-config-path) to see every effective value.

### The full layer stack (with sources)

Config arrives in **two independent stacks**, both compiled into the binary as the
base and then overridable. `--export-full-config` dumps the effective result of
both (`[plugin]` = the language stack, `[project]` = the project stack).

**1. Language stack** — the node-kind vocabulary, per-language metric specs,
default thresholds, and `[report]` view overrides. Selected by `--plugin` (or
auto-detected). Built by `config::load_chain` as
`base ⊕ [family] ⊕ <lang>`, each layer deep-merged over the last:

| Layer | Applies to | Source (GitHub `main`) |
|---|---|---|
| **base** — common vocabulary & defaults | every language | [`crates/code-ranker-plugins/src/defaults.toml`](https://github.com/ffedoroff/code-ranker/blob/main/crates/code-ranker-plugins/src/defaults.toml) |
| **family** (optional middle layer) | JS/TS → [`ecmascript/config.toml`](https://github.com/ffedoroff/code-ranker/blob/main/crates/code-ranker-plugins/src/languages/ecmascript/config.toml) · C/C++ → [`cfamily/config.toml`](https://github.com/ffedoroff/code-ranker/blob/main/crates/code-ranker-plugins/src/languages/cfamily/config.toml) | the shared engine vocab |
| **`<lang>`** — only what differs | the chosen plugin | [`languages/<lang>/config.toml`](https://github.com/ffedoroff/code-ranker/tree/main/crates/code-ranker-plugins/src/languages) — e.g. [`rust`](https://github.com/ffedoroff/code-ranker/blob/main/crates/code-ranker-plugins/src/languages/rust/config.toml), [`python`](https://github.com/ffedoroff/code-ranker/blob/main/crates/code-ranker-plugins/src/languages/python/config.toml), [`go`](https://github.com/ffedoroff/code-ranker/blob/main/crates/code-ranker-plugins/src/languages/go/config.toml) |

The shared **metric catalog** (built-in derived metrics, aggregates, the default
`[report]` columns/card/size/filter) is layered under the language `[report]`:
[`crates/code-ranker-graph/metrics/builtin.toml`](https://github.com/ffedoroff/code-ranker/blob/main/crates/code-ranker-graph/metrics/builtin.toml).

**2. Project stack** — `[rules]`, `[metrics]`, `[presets]`, `[ignore]`,
`[levels]`, output, and a project `[report]` patch. This is what `--config` and
`code-ranker.toml` set:

| Order | Layer | Source |
|---|---|---|
| base | built-in project defaults | [`crates/code-ranker-cli/src/config/defaults.toml`](https://github.com/ffedoroff/code-ranker/blob/main/crates/code-ranker-cli/src/config/defaults.toml) |
| ⊕ | auto-discovered `code-ranker.toml` *(skipped if any `--config` file is passed)* | your repo |
| ⊕ | `--config file1` ⊕ `file2` ⊕ … | **in command-line order, last wins** |
| ⊕ | `--config KEY=VALUE` inline | after all files |
| ⊕ | CLI flags (`--threshold`, `--cycle-rule`, `--ignore`) | last |

So the **complete order**, base → most-specific: language `base` ⊕ `[family]` ⊕
`<lang>` for the plugin side; project `defaults.toml` ⊕ discovered/`--config`
files (in order) ⊕ inline ⊕ CLI flags for the rules side. A project `[report]`
patch layers on top of the language `[report]`, which layers on the catalog — so
the three `[report]` sources compose (catalog → language → project).

---

## Config file: `code-ranker-rust-example.toml`

> **This is an annotated EXAMPLE, not a required file.** It shows the available
> keys (with Rust-flavoured values) so you can copy the bits you need. Two things
> to keep in mind:
>
> - **Filename.** For auto-discovery the file must be named **`code-ranker.toml`**
>   (in the cwd or the analyzed target). Any other name — like this
>   `code-ranker-rust-example.toml` — only works when passed explicitly with
>   `--config <path>`.
> - **Partial is fine.** Every key has a built-in default (see
>   [the layer stack](#the-full-layer-stack-with-sources)), so a real config spells
>   out **only what it changes**. The block below is exhaustive purely to document
>   the options — you would never write all of it.

```toml
# Default plugin. Overridden by --plugin.
plugin = "rust"

[ignore]
paths = [
  "**/generated/**",
  "crates/*/benches/**",
]
tests = true             # skip the language's test files — ON BY DEFAULT; set false to keep them
                         # (the plugin decides what is a test: Rust #[cfg(test)] modules,
                         #  Python test_*.py / tests/, JS/TS *.test.* …; legacy alias: test_modules)
gitignore = true         # honour .gitignore (+ global gitignore + .git/info/exclude) while a
                         # directory-walking plugin collects files — ON BY DEFAULT; scoped to the
                         # analyzed root (an enclosing repo's rules never leak in). Git-faithful:
                         # applies only inside a git repo. The Rust plugin uses cargo metadata
                         # (not a walk), so it is unaffected.
ignore_files = true      # honour .ignore files — ON BY DEFAULT
hidden = true            # skip hidden files/dirs (dotfiles) — ON BY DEFAULT; set false to include them

[rules.cycles]
# each kind: false = off, true = strict (any cycle fails, same as 0),
# or an integer N = allow up to N cycles of that kind (the N+1-th fails).
mutual     = true    # default — strict
chain      = 7       # allow up to 7 chain cycles; pin today's count as a baseline

[rules.thresholds.file]      # a single file (files graph)
loc        = 800
sloc       = 600             # any per-file metric the engine emits is accepted
cognitive  = 25
hk         = 500_000         # `_` separators; or a suffix: hk = 5M (bare or "5M")
fan_out    = 50

[rules.defs]                 # reusable named CEL helpers (expanded into checks)
is_test_file = 'name.endsWith("_tests.rs") || path.contains("/tests/")'

[rules.checks.de1101]        # a custom linter: CEL bool predicate per file node
when    = "tloc > 100 && !is_test_file"   # node values + path/deps/files + helpers
message = "{tloc} lines of inline test code in a production file"
group   = "TST"              # free-form concern label (default "LNT")
# why / fix / title          # optional diagnostic copy; {key} interpolated

[output.json]                # `report` JSON snapshot artifact
path = "{project-dir}-{ts}.json"   # default if unset: .code-ranker/{ts}-{git-hash-3}.json
# enabled = false            # keep the path but don't write JSON unless re-selected

[output.html]                # `report` HTML viewer artifact
path = "{project-dir}-{ts}.html"   # default if unset: .code-ranker/{ts}-{git-hash-3}.html

[output.sarif]               # `report` SARIF 2.1.0 artifact (GitHub code scanning / GitLab >=18.11)
path = "{project-dir}-{ts}.sarif"  # default if unset: .code-ranker/{ts}-{git-hash-3}.sarif
# enabled = true             # write SARIF on every report run (opt-in; not in the default set)

[output.codequality]         # `report` GitLab Code Quality (CodeClimate) artifact
path = "gl-code-quality-report.json"  # default if unset: .code-ranker/{ts}-{git-hash-3}.codequality.json
# enabled = true             # write Code Quality on every report run (opt-in)

[levels]                     # opt-in extra graph levels beyond `files`
# functions = true           # emit a `functions` level with per-function metrics

[metrics.comment_ratio]      # user-defined metric (CEL formula + spec)
formula   = "sloc > 0.0 ? cloc / sloc * 100.0 : 0.0"
label     = "Comments %"
direction = "higher_better"  # lower_better | higher_better
group     = "loc"
# scope   = "node"           # node (per file/function, default) | graph (aggregate)

[metrics.cyclomatic_p90]     # a graph-scope aggregate → lands in the `stats` block
scope     = "graph"
formula   = "agg('cyclomatic', 'p90', 'not_empty')"
```

The threshold scope is always `file` — a single source file on the one graph
code-ranker builds.

### `[rules.checks.<id>]` — custom checks (config-only linters)

A custom check is the general form of a `check` rule: a CEL **boolean** `when`
predicate evaluated per file node (a second pass over the fully-built graph). When
it is true, `check` reports a `check.<id>` violation pinned to that file — like a
threshold or cycle violation, in every output format, counting toward the exit
code. Where `[rules.thresholds.file]` only does `metric > limit`, a check is any
boolean expression over a rich context:

- **node values** — any attribute key (`tloc`, `sloc`, `loc`, `unsafe`, `cyclomatic`, …);
  the Rust plugin also emits syntactic-fact strings (production code only) usable
  via `contains`/`matches`: `derives` / `macros` / `attrs` / `imports` (comma-joined
  "uses X" sets) and `types` / `traits` (names defined in the file);
- **path fields** — `path` / `name` / `stem` / `ext` / `dir`;
- **edges (lists)** — `deps` / `rdeps` (dependency neighbour labels; an external
  crate is `ext:<name>`), plus `depends_on(s)` / `depended_on_by(s)`;
- **file collections** — `files` / `siblings`, plus `file_exists(p)`;
- **string fns** — CEL's own `contains` / `startsWith` / `endsWith` / `matches`
  (regex), callable as methods (`path.endsWith("_tests.rs")`) or functions;
- **CEL list macros** — `.size()` / `.exists(x, …)` / `.all(x, …)` / `.filter(x, …)`;
- `n.double()` to take a real ratio (CEL `/` is integer division on ints).

Fields: `when` (required), `message` (required), and optional `group` (free-form
concern label, default `LNT`) / `why` / `fix` / `title` diagnostic copy — all of
which interpolate `{key}` from the node's values. A `when` that fails to compile
becomes a loud `check.<id>` violation (a typo can't pass silently).

### `[rules.defs]` — reusable named helpers

`name = "<cel expr>"` entries expanded into a check's `when` before compilation (a
helper may reference an earlier one; a reference cycle is a hard error). They add
reuse/readability, not new power. See **`docs/customization/README.md` §1.8** for
the full walkthrough and **`docs/customization/cel-reference.md`** for the complete
CEL reference (language, built-in functions, what is in scope in checks vs metrics).

### `[levels]` — opt-in graph levels

`functions = true` adds a second graph level, `functions`, with one node per
sub-file unit (function / method / closure / …) carrying the same per-unit
metrics. **Off by default**, so the default output (and goldens) is unchanged;
the `files` level is always emitted. Function nodes have `parent` = their file
node id and a per-language `kind` (e.g. `fn`/`method`/`closure`,
`function`/`arrow`/`generator`). No call graph is built (no `Calls` edges).

### `[metrics.<key>]` — declarative metrics

Every tier-2 metric is **data**: a CEL `formula` plus display spec. The built-in
set ships in `code-ranker-graph/metrics/builtin.toml`; you add or override metrics
here with no code change. Fields:

- `formula` (required) — a CEL expression over other metric keys and the tier-1
  inputs (`eta1`/`eta2`/`n1`/`n2`/`spaces`/`branches`/`sloc`/`cloc`/`span_sloc`/…),
  plus host math (`log2`/`ln`/`pow`/`sqrt`/`sin`/…). A bad formula or a definition
  cycle is a hard config error.
- `scope` — `node` (per file/function; default) or `graph` (an aggregate computed
  once over all nodes via `agg(key, reducer, population)` and emitted into
  `stats`). Reducers: `sum`/`avg`/`min`/`max`/`count`/`median`/`p<q>` (percentile
  by numpy R-7), and `top<N>`/`top<N>_<reducer>` (keep the N largest, then reduce —
  default `avg`). Populations: `not_empty` (value ≠ `omit_at`) / `all` (missing
  counted at the floor).
- `label` / `name` / `short` / `description` / `formula_pretty` / `calc` /
  `direction` / `group` / `value_type` / `omit_at` — display spec (rendered like
  any built-in metric). `name` is the tooltip title, `short` the table header,
  `description` the tooltip body, `formula_pretty` the readable formula's first
  tooltip line. `calc` is the JS the viewer re-runs with the node's values for the
  second line (the formula filled with numbers, like `hk`); it defaults to the CEL
  `formula`, so plain-arithmetic metrics get the line for free.
- `warning` / `info` — optional two-tier severity thresholds the scorecard and
  viewer badge against (a missing tier mirrors the other). Distinct from the
  single-tier `[rules.thresholds.file]` `check` gate.

A node-scope metric is computed for every file (and function, when that level is
on) and is usable as a `[rules.thresholds.file]` limit like any built-in (the key
is validated at load — a typo, or a metric you never defined, is a hard error).

### `[presets.<ID>]` — project Prompt-Generator presets

A preset is a refactoring lens: it ranks files by one metric and ships a
ready-to-paste AI prompt, surfaced by `--preset` / the `scorecard` / the viewer's
Prompt-Generator buttons. The plugin catalog ships the SOLID/complexity presets;
a project adds its own (e.g. over a custom metric) here. The table key is the id;
a same-id project preset overrides the plugin's, a new id appends.

```toml
[presets.TSR]
title       = "TSR — Trim inline test bulk"  # prompt heading (defaults to id)
sort_metric = "tsr"                          # the metric the worst-first list ranks by
prompt      = "Move inline test modules into sibling test files…"
# optional: label (button text, defaults to id), doc_url, connections = ["in","out","common"]
```

Only `sort_metric` is essential. See the worked example in
[`docs/customization/`](../customization/README.md#17-prompt-generator-presets--presetsid).

### `[output.json]` / `[output.html]` / `[output.sarif]` / `[output.codequality]` — report artifacts

Each table configures one `code-ranker report` artifact: `path` is the destination
(a filename template, or `stdout`/`-`), and `enabled` (a bool) forces the format on
or off. `--output.<fmt>.path` / `--output.<fmt>` on the CLI override these; when no
artifact is selected anywhere, `json` + `html` are written to `.code-ranker/` under
the built-in default `{ts}-{git-hash-3}` (`sarif` is opt-in — never in the default
set). `path` accepts these placeholders:

| Placeholder | Expands to |
|---|---|
| `{project-dir}` | slugified workspace directory name |
| `{ts}` | local `YYYYMMDD-HHMMSS` timestamp |
| `{git-hash}` | 12-char short commit hash (zeros outside a git repo) |
| `{git-hash-N}` | first `N` chars of the commit hash |

**Values** accept `_` digit separators and `K`/`M`/`G` suffixes (×10³/10⁶/10⁹):
`5_123_000`, a bare `5M`, or a quoted `"5M"`. The bare suffix works both on the
CLI (`--threshold file.hk=5M`) and inside a `[rules.thresholds.*]` table
(`hk = 5M`) — code-ranker quotes the value before parsing, since raw TOML would
otherwise reject it. See [ERRORS.md](ERRORS.md#threshold-scopes).

---

## Config in `Cargo.toml`

Useful when you don't want an extra file. Supports the same keys under
`[workspace.metadata.code-ranker]` (monorepo) or `[package.metadata.code-ranker]`
(single crate).

```toml
[workspace.metadata.code-ranker.ignore]
paths = ["**/tests/**"]

[workspace.metadata.code-ranker.rules.cycles]
mutual     = true

[workspace.metadata.code-ranker.rules.thresholds.file]
hk = 500_000
```

---

## CLI flags

All config values can be set or overridden from the command line.

### `--plugin <NAME|auto>`

Select the built-in plugin (`rust`, `python`, or `javascript`).
Default is `auto`: resolved from `plugin` in the config file, then by project
markers (`Cargo.toml`→rust, `pyproject.toml`/`setup.py`→python,
`package.json`/`tsconfig.json`→javascript). Ambiguous or no marker → error.

```bash
code-ranker check .                   # auto-detect (or config.plugin)
code-ranker check . --plugin python   # always uses python
```

### `--config <FILE>`

Load config from an explicit path instead of auto-discovery. It is still
**deep-merged over the built-in defaults**, so it need only list overrides.

```bash
code-ranker check . --config ci/strict.toml
```

### `--export-full-config <PATH>`

A `report` flag: instead of analyzing, write the **full effective configuration**
to `PATH` and exit. The file has two sections — `[project]` (built-in defaults ⊕
your `--config`) and `[plugin]` (the `--plugin` language's merged config: presets,
thresholds, vocab). A diagnostic view of every value you can override.

```bash
code-ranker report . --plugin python --config ci/strict.toml \
  --export-full-config /tmp/effective.toml
```

### `--ignore <GLOB>`

Add a path glob to the ignore list. Repeatable.

```bash
code-ranker check . --ignore '**/tests/**' --ignore '**/generated/**'
```

### `--cycle-rule <KIND=on|off|N>`

Configure a cycle check. `KIND`: `mutual` | `chain`. Value: `on`
(strict — any cycle fails), `off` (ignored), or an integer `N` (allow up to `N`
cycles of that kind, fail on the `N+1`-th). Defaults: `mutual` and `chain` on
(= strict). Repeatable.

```bash
# allow up to 7 chain cycles (forbid an 8th); keep mutual strict
code-ranker check . --cycle-rule chain=7
```

### `--threshold <file.METRIC=N>`

Set a per-file threshold — a breach fails the check. The scope is always `file`
(a single source file). `METRIC` is any per-file metric the engine emits —
`loc` / `sloc` / `cyclomatic` / `cognitive` / `mi` / `volume` / `bugs` / `hk` /
`fan_in` / `fan_out` / … (an unknown name errors). `N` accepts `_` separators and
`K`/`M`/`G` suffixes (e.g. `5M`, `1_500`). Repeatable.

```bash
code-ranker check . --threshold file.loc=800 --threshold file.sloc=600 \
  --threshold file.cyclomatic=10
```

### `--baseline <SNAPSHOT>`

Compare the input against a baseline snapshot (`.json`/`.html`). On `check` it makes
the gate **relative** — fail only on *new* violations vs the baseline, tolerating
pre-existing ones; on `report` it turns the HTML into a baseline↔current diff.

```bash
code-ranker check . --baseline .code-ranker/main.json
```

### `--output.json` / `--output.html` / `--output.sarif` / `--output.codequality` / `--output.<fmt>.path` (report)

Select which artifacts `report` writes and where. `--output.<fmt>` selects a format
(path from config/default); `--output.<fmt>.path=…` selects it and sets the
destination (a template, or `stdout`/`-`). With none given, `json` + `html` are
written to `.code-ranker/` (`sarif` and `codequality` are opt-in).

```bash
code-ranker report .                                    # json + html, default names
code-ranker report . --output.html                      # only HTML, default path
code-ranker report . --output.json.path=stdout          # JSON to stdout, no HTML
code-ranker report . --output.sarif.path=stdout         # SARIF to stdout, nothing else
code-ranker report . --output.codequality.path=stdout   # GitLab Code Quality to stdout
```

### `--exit-zero`

Exit 0 even when violations are found. Useful in CI when you want to
collect the snapshot as an artifact without blocking the pipeline.

```bash
code-ranker check . --exit-zero
```

Without this flag, `code-ranker check` exits 1 whenever at least one violation
is found — matching the default behaviour of tools like `ruff check`.

---

## Enabled vs disabled

There are no severity levels. Every rule is binary:

| State | Effect |
|---|---|
| enabled (`true` / threshold set) | Violations are reported; `check` exits non-zero (unless `--exit-zero`) |
| disabled (`false` / threshold unset) | Not checked |

---

## Typical CI setup

```yaml
# collect-only (never blocks the pipeline)
- run: code-ranker check . --exit-zero

# linter mode (blocks on any violation)
- run: code-ranker check .
```

Or with inline overrides to tighten rules in CI without changing `code-ranker.toml`:

```bash
code-ranker check . --cycle-rule chain=7 --threshold file.hk=200000
```
