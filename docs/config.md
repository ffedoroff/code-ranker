# code-split configuration

## Priority order

Settings are merged from multiple sources. **Higher priority wins** for the same key.

| Priority | Source | Example |
|---|---|---|
| 1 | CLI flags | `--ignore '**/tests/**'` |
| 2 | `--config KEY=VALUE` inline override | `--config rules.thresholds.node.hk=200000` |
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
test-embed = false   # default — off (Rust #[cfg(test)] back-edge, not a smell)
mutual     = true    # default — on
chain      = true    # default — on

[rules.thresholds.node]   # any single node exceeds → violation
hk         = 500_000
cyclomatic  = 25
cognitive   = 30
fan_out     = 50

[rules.thresholds.avg]    # graph-wide average exceeds → violation
hk         = 50_000
cyclomatic  = 10
```

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

[workspace.metadata.code-split.rules.thresholds.node]
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

### `--cycle-rule <KIND=on|off>`

Enable or disable a cycle check. `KIND`: `test-embed` | `mutual` | `chain`.
Defaults: `test-embed` off, `mutual` and `chain` on. Repeatable.

```bash
# also flag test-embed cycles; stop flagging chain cycles
code-split check . --cycle-rule test-embed=on --cycle-rule chain=off
```

### `--threshold <SCOPE.METRIC=N>`

Set a threshold — a breach fails the check. `SCOPE`: `node` | `avg`. `METRIC`:
`hk` | `cyclomatic` | `cognitive` | `fan_in` | `fan_out` | `loc`. Repeatable.

```bash
code-split check . --threshold node.hk=500000 --threshold avg.cyclomatic=10
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
code-split check . --cycle-rule test-embed=on --threshold node.hk=200000
```
