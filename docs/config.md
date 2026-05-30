# code-split configuration

## Priority order

Settings are merged from multiple sources. **Higher priority wins** for the same key.

| Priority | Source | Example |
|---|---|---|
| 1 | CLI flags | `--ignore '**/tests/**'` |
| 2 | `--config KEY=VALUE` inline override | `--config rules.thresholds.module.hk=200000` |
| 3 | `--config <file>` | `--config ci/code-split.toml` |
| 4 | `code-split.toml` in cwd | `./code-split.toml` |
| 5 | `code-split.toml` in workspace root | `<workspace>/code-split.toml` |
| 6 | `Cargo.toml` metadata | `[workspace.metadata.code-split]` |
| 7 | Built-in defaults | `test-embed` off, `mutual` / `chain` on |

For `ignore.paths` and CLI `--ignore`: lists are **merged** (union), not replaced.  
For cycle rules and thresholds: CLI **overrides** the file value.

---

## Config file: `code-split.toml`

```toml
# Default plugin. Overridden by --plugin.
plugin = "rust"

[ignore]
paths = [
  "**/tests/**",
  "**/generated/**",
  "crates/*/benches/**",
]

[rules.cycles]
# each kind: false = off, true = strict (any cycle fails, same as 0),
# or an integer N = allow up to N cycles of that kind (the N+1-th fails).
test-embed = false   # default — off (Rust #[cfg(test)] back-edge, not a smell)
mutual     = true    # default — strict
chain      = 7       # allow up to 7 chain cycles; pin today's count as a baseline

[rules.thresholds.file]      # a single file (files graph only)
loc        = 800

[rules.thresholds.file.avg]  # the files-graph average
loc        = 300

[rules.thresholds.module]    # a single module/crate (modules graph only)
hk         = 500_000         # `_` separators; or a quoted suffix: hk = "5M"
fan_out     = 50

[rules.thresholds.function]  # a single function (functions graph only)
cognitive  = 25
loc        = 120

[rules.thresholds.function.avg]  # the functions-graph average
cyclomatic  = 8
```

Threshold **scopes** — `file` / `module` / `function` — each apply to a single
unit on that one graph (the scope *is* the graph), so a file and a function can
carry different budgets. **Every scope also has an `.avg` sub-table** for that
scope's graph-wide average. The single and `avg` buckets are independent.

**Values** accept `_` digit separators and `K`/`M`/`G` suffixes (×10³/10⁶/10⁹):
`5_123_000`, or a quoted `"5M"` in TOML (bare `5M` is invalid TOML), or bare on the
CLI (`--threshold module.hk=5M`). See [ERRORS.md](ERRORS.md#threshold-scopes).

---

## Config in `Cargo.toml`

Useful when you don't want an extra file. Supports the same keys under
`[workspace.metadata.code-split]` (monorepo) or `[package.metadata.code-split]`
(single crate).

```toml
[workspace.metadata.code-split.ignore]
paths = ["**/tests/**"]

[workspace.metadata.code-split.rules.cycles]
test-embed = false
mutual     = true

[workspace.metadata.code-split.rules.thresholds.module]
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
code-split check .                   # auto-detect (or config.plugin)
code-split check . --plugin python   # always uses python
```

### `--config <FILE>`

Load config from an explicit path instead of auto-discovery.

```bash
code-split check . --config ci/strict.toml
```

### `--ignore <GLOB>`

Add a path glob to the ignore list. Repeatable.

```bash
code-split check . --ignore '**/tests/**' --ignore '**/generated/**'
```

### `--cycle-rule <KIND=on|off|N>`

Configure a cycle check. `KIND`: `test-embed` | `mutual` | `chain`. Value: `on`
(strict — any cycle fails), `off` (ignored), or an integer `N` (allow up to `N`
cycles of that kind, fail on the `N+1`-th). Defaults: `test-embed` off, `mutual`
and `chain` on (= strict). Repeatable.

```bash
# flag test-embed cycles; allow up to 7 chain cycles (forbid an 8th)
code-split check . --cycle-rule test-embed=on --cycle-rule chain=7
```

### `--threshold <SCOPE[.avg].METRIC=N>`

Set a threshold — a breach fails the check. `SCOPE`: `file` | `module` |
`function` (a single unit on that graph). Add `.avg` for that scope's graph-wide
average. `METRIC`: `hk` | `cyclomatic` | `cognitive` | `fan_in` | `fan_out` |
`loc`. `N` accepts `_` separators and `K`/`M`/`G` suffixes (e.g. `5M`, `1_500`).
Repeatable.

```bash
code-split check . --threshold file.loc=800 --threshold function.cognitive=25 \
  --threshold function.avg.cyclomatic=10
```

### `--exit-zero`

Exit 0 even when violations are found. Useful in CI when you want to
collect the snapshot as an artifact without blocking the pipeline.

```bash
code-split check . --exit-zero
```

Without this flag, `code-split check` exits 1 whenever at least one violation
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
- run: code-split check . --exit-zero

# linter mode (blocks on any violation)
- run: code-split check .
```

Or with inline overrides to tighten rules in CI without changing `code-split.toml`:

```bash
code-split check . --cycle-rule test-embed=on --threshold module.hk=200000
```
