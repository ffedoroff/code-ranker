# code-split configuration

## Priority order

Settings are merged from multiple sources. **Higher priority wins** for the same key.

| Priority | Source | Example |
|---|---|---|
| 1 | CLI flags | `--ignore '**/tests/**'` |
| 2 | `--config KEY=VALUE` inline override | `--config rules.thresholds.file.hk=200000` |
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
  "**/generated/**",
  "crates/*/benches/**",
]
tests = true             # strip test files from the graph (legacy alias: test_modules)

[rules.cycles]
# each kind: false = off, true = strict (any cycle fails, same as 0),
# or an integer N = allow up to N cycles of that kind (the N+1-th fails).
test-embed = false   # default — off (Rust #[cfg(test)] back-edge, not a smell)
mutual     = true    # default — strict
chain      = 7       # allow up to 7 chain cycles; pin today's count as a baseline

[rules.thresholds.file]      # a single file (files graph)
loc        = 800
cognitive  = 25
hk         = 500_000         # `_` separators; or a quoted suffix: hk = "5M"
fan_out    = 50

[output]                     # default artifact names for `report`
json-name = "{project-dir}-{ts}.json"   # default if unset: {ts}-{git-hash-3}.json
html-name = "{project-dir}-{ts}.html"   # default if unset: {ts}-{git-hash-3}.html
```

The threshold scope is always `file` — a single source file on the one graph
code-split builds.

### `[output]` — report artifact names

`[output] json-name` / `html-name` set the default filename templates for
`code-split report`. A `--json-name` / `--html-name` flag still overrides them;
when neither is set the built-in default `{ts}-{git-hash-3}` is used. Both keys
accept `_`/kebab spelling (`json-name` or `json_name`) and these placeholders:

| Placeholder | Expands to |
|---|---|
| `{project-dir}` | slugified workspace directory name |
| `{ts}` | local `YYYYMMDD-HHMMSS` timestamp |
| `{git-hash}` | 12-char short commit hash (zeros outside a git repo) |
| `{git-hash-N}` | first `N` chars of the commit hash |

**Values** accept `_` digit separators and `K`/`M`/`G` suffixes (×10³/10⁶/10⁹):
`5_123_000`, or a quoted `"5M"` in TOML (bare `5M` is invalid TOML), or bare on the
CLI (`--threshold file.hk=5M`). See [ERRORS.md](ERRORS.md#threshold-scopes).

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

[workspace.metadata.code-split.rules.thresholds.file]
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

### `--threshold <file.METRIC=N>`

Set a per-file threshold — a breach fails the check. The scope is always `file`
(a single source file). `METRIC`: `hk`
| `cyclomatic` | `cognitive` | `fan_in` | `fan_out` | `loc`. `N` accepts `_`
separators and `K`/`M`/`G` suffixes (e.g. `5M`, `1_500`). Repeatable.

```bash
code-split check . --threshold file.loc=800 --threshold file.cognitive=25 \
  --threshold file.cyclomatic=10
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
code-split check . --cycle-rule test-embed=on --threshold file.hk=200000
```
