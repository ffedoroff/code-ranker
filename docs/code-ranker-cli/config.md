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
cognitive  = 25
hk         = 500_000         # `_` separators; or a quoted suffix: hk = "5M"
fan_out    = 50

[output.json]                # `report` JSON snapshot artifact
path = "{project-dir}-{ts}.json"   # default if unset: .code-ranker/{ts}-{git-hash-3}.json
# enabled = false            # keep the path but don't write JSON unless re-selected

[output.html]                # `report` HTML viewer artifact
path = "{project-dir}-{ts}.html"   # default if unset: .code-ranker/{ts}-{git-hash-3}.html
```

The threshold scope is always `file` — a single source file on the one graph
code-ranker builds.

### `[output.json]` / `[output.html]` — report artifacts

Each table configures one `code-ranker report` artifact: `path` is the destination
(a filename template, or `stdout`/`-`), and `enabled` (a bool) forces the format on
or off. `--output.<fmt>.path` / `--output.<fmt>` on the CLI override these; when no
artifact is selected anywhere, both are written to `.code-ranker/` under the built-in
default `{ts}-{git-hash-3}`. `path` accepts these placeholders:

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
(a single source file). `METRIC`: `hk`
| `cyclomatic` | `cognitive` | `fan_in` | `fan_out` | `loc`. `N` accepts `_`
separators and `K`/`M`/`G` suffixes (e.g. `5M`, `1_500`). Repeatable.

```bash
code-ranker check . --threshold file.loc=800 --threshold file.cognitive=25 \
  --threshold file.cyclomatic=10
```

### `--baseline <SNAPSHOT>`

Compare the input against a baseline snapshot (`.json`/`.html`). On `check` it makes
the gate **relative** — fail only on *new* violations vs the baseline, tolerating
pre-existing ones; on `report` it turns the HTML into a baseline↔current diff.

```bash
code-ranker check . --baseline .code-ranker/main.json
```

### `--output.json` / `--output.html` / `--output.<fmt>.path` (report)

Select which artifacts `report` writes and where. `--output.json` / `--output.html`
select a format (path from config/default); `--output.json.path=…` /
`--output.html.path=…` select it and set the destination (a template, or `stdout`/`-`).
With none given, both are written to `.code-ranker/`.

```bash
code-ranker report .                              # both, default names
code-ranker report . --output.html                # only HTML, default path
code-ranker report . --output.json.path=stdout    # JSON to stdout, no HTML
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
