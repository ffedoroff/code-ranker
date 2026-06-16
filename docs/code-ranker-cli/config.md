# code-ranker configuration

## Priority order

Settings are merged from multiple sources. **Higher priority wins** for the same key.

| Priority | Source | Example |
|---|---|---|
| 1 | CLI flags | `--ignore '**/tests/**'` |
| 2 | `--config KEY=VALUE` inline override | `--config rules.thresholds.file.hk=200000` |
| 3 | `--config <file>` | `--config ci/code-ranker.toml` |
| 4 | `code-ranker.toml` in cwd | `./code-ranker.toml` |
| 5 | `code-ranker.toml` in the analyzed target directory | `<target>/code-ranker.toml` |
| 6 | `Cargo.toml` metadata | `[workspace.metadata.code-ranker]` |
| 7 | Built-in defaults | `mutual` / `chain` on |

For `ignore.paths` and CLI `--ignore`: lists are **merged** (union), not replaced.  
For cycle rules and thresholds: CLI **overrides** the file value.

---

## Config file: `code-ranker.toml`

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
  by numpy R-7). Populations: `not_empty` (value ≠ `omit_at`) / `all` (missing
  counted at the floor).
- `label` / `name` / `short` / `description` / `direction` / `group` / `value_type`
  / `omit_at` — display spec (rendered like any built-in metric).

A node-scope metric is computed for every file (and function, when that level is
on) and is usable as a `[rules.thresholds.file]` limit like any built-in.

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

Load config from an explicit path instead of auto-discovery.

```bash
code-ranker check . --config ci/strict.toml
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
