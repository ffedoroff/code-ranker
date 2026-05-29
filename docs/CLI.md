# `code-split` CLI reference

Pluggable multi-language structural analysis platform.

```
code-split <command> [flags]
```

Run `code-split --help` for the auto-generated synopsis, or `code-split <command> --help` for per-command flags. Versions: `code-split --version`.

The CLI has four commands:

| Command | What it does |
|---|---|
| [`analyze`](#analyze) | Extract dependency graphs from a workspace and write a snapshot JSON file. Doubles as the CI linter via threshold / cycle-rule flags. |
| [`report`](#report) | Render a self-contained, offline HTML report from a snapshot. |
| [`diff`](#diff) | Render a self-contained HTML *diff* report between two snapshots. |
| [`compare`](#compare) | Same comparison as `diff`, but emit JSON or HTML — useful as CI artifact. |

## `analyze`

Run a plugin against a project and capture its graphs (modules / files / functions) into one snapshot JSON.

```
code-split analyze <workspace> [flags] [-- <plugin-args>...]
```

### Required

| Argument | Meaning |
|---|---|
| `<workspace>` | Path to the project root (directory the plugin will analyze). |

### Flags

| Flag | Meaning |
|---|---|
| `--plugin <name>` | Plugin to use. Built-ins: `rust`, `python`, `javascript` (covers TypeScript). External: any `code-split-plugin-<name>` on PATH. Falls back to `plugin` in `code-split.toml`, then `rust`. |
| `-o, --output <path>` | Output snapshot path. Default: `.code-split/snap-<timestamp>.json`. |
| `--local-only` | Skip any network-dependent step (e.g. `cargo metadata --no-deps` style). |
| `--graph <kinds>` | Which graphs to build. Repeatable or comma-separated: `modules`, `files`, `functions`. Default: all three. |
| `--config <file>` | Explicit config file. Auto-discovered from `code-split.toml`, then `Cargo.toml#metadata.code-split`. |
| `--ignore <glob>` | Repeatable. Glob to exclude paths from analysis. Merged with config-file globs. |
| `--cycle-rule <KIND=SEVERITY>` | Override cycle severity. Kinds: `test-embed`, `mutual`, `chain`. Severities: `allow`, `warn`, `deny`. Example: `--cycle-rule test-embed=allow`. |
| `--threshold <SCOPE.METRIC=N>` | Set a hard limit on a metric. Scopes: `node` (per item) or `avg` (workspace average). Metrics: `cyclomatic`, `cognitive`, `hk`, `fan_in`, `fan_out`, `loc`. Example: `--threshold node.cognitive=25`. |
| `--exit-zero` | Collect-only mode. Even when violations exist, return exit code 0. Useful in non-blocking CI checks. |
| `-- <extra-args>` | Everything after `--` is forwarded verbatim to the plugin. |

### Lint behaviour (no separate `lint` subcommand)

`analyze` is the linter. The CLI exits **non-zero** when, across the analyzed graphs:

- a cycle of severity `deny` is detected, OR
- any node breaches a `node.<metric>` threshold, OR
- any workspace-average metric breaches an `avg.<metric>` threshold.

Pass `--exit-zero` to keep the snapshot but suppress the failure exit.

### Examples

```sh
# minimal: analyze the current Rust workspace, write to .code-split/snap-<ts>.json
code-split analyze . --plugin rust

# Python project, only function-level graph, with explicit output:
code-split analyze ./api --plugin python --graph functions -o snapshots/api.json

# CI gate: forbid cycles and cap per-function cognitive complexity at 25
code-split analyze . --plugin rust \
  --cycle-rule mutual=deny --cycle-rule chain=deny \
  --threshold node.cognitive=25 --threshold node.loc=800

# forward extra args to the plugin (after `--`)
code-split analyze . --plugin javascript -- --tsconfig ./tsconfig.json
```

## `report`

Render an interactive offline HTML report from one snapshot.

```
code-split report --input <snap.json> [-o <out.html>]
```

| Flag | Meaning |
|---|---|
| `--input <file>` | Snapshot JSON produced by `analyze`. |
| `-o, --output <file>` | Output HTML. Default `-` (stdout). |

The HTML is self-contained: graph layout (Dagre), pan/zoom, sortable node tables for each of the three levels (modules / files / functions), and the prompt-generator panel that copies ready-to-paste prompts for ADP / SRP / OCP / LSP / ISP / DIP / DRY / KISS / LoD / MISU / CoI / YAGNI plus *Reduce Complexity* and *Split Components* presets. No network, no telemetry.

### Examples

```sh
code-split report --input .code-split/snap-2026-05-29.json -o report.html
open report.html

# pipe to file directly via stdout
code-split report --input snap.json > report.html
```

## `diff`

Render an HTML diff report between two snapshots.

```
code-split diff --before <a.json> --after <b.json> [-o <out.html>]
```

| Flag | Meaning |
|---|---|
| `--before <file>` | Snapshot taken before the change. |
| `--after <file>` | Snapshot taken after the change. |
| `-o, --output <file>` | Output HTML. Default `-` (stdout). |

The diff report shows added / removed nodes and edges per level, per-node weight delta, and an overall verdict (`improved` / `degraded` / `neutral`).

### Examples

```sh
code-split diff --before main.json --after pr.json -o diff.html

# typical CI flow: build snapshots on both sides of a PR, diff them
code-split analyze .   --plugin rust -o pr.json
git checkout main
code-split analyze .   --plugin rust -o main.json
git checkout -
code-split diff --before main.json --after pr.json -o diff.html
```

## `compare`

Same data as `diff` but emitted as **JSON** (default) or HTML (with `--html`). Use JSON for CI artifacts, downstream tooling, programmatic checks.

```
code-split compare --before <a.json> --after <b.json> [-o <out>] [--html]
```

| Flag | Meaning |
|---|---|
| `--before <file>` | Snapshot taken before the change. |
| `--after <file>` | Snapshot taken after the change. |
| `-o, --output <file>` | Output path. Default `-` (stdout). |
| `--html` | Emit HTML instead of JSON (equivalent to `diff`). |

### Examples

```sh
# JSON to stdout for CI parsing
code-split compare --before main.json --after pr.json | jq '.verdict'

# attach JSON as CI artifact + HTML for humans
code-split compare --before main.json --after pr.json -o diff.json
code-split compare --before main.json --after pr.json --html -o diff.html
```

## Config file: `code-split.toml`

Most CLI flags can be set in `code-split.toml` at the workspace root (or under `[package.metadata.code-split]` in `Cargo.toml`). Auto-discovered. The `--config <file>` flag overrides discovery. See [`docs/config.md`](config.md) for the full schema.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success (or `--exit-zero` masking violations). |
| 1 | Generic error: parsing, IO, plugin failure. |
| Non-zero (other) | Lint violation (`deny` cycle or breached threshold) when `--exit-zero` is not passed. |

## Plugins

Built-in (no install needed):

- `rust` — `cargo metadata` + `syn` for module/file graphs, optional `rust-analyzer` (`ra_ap_*`) for the call graph when not in `--local-only` mode and `cargo` is on PATH.
- `python` — tree-sitter-python for module/file/function graphs, native parser.
- `javascript` — tree-sitter-javascript / tree-sitter-typescript; one plugin handles `.js`, `.jsx`, `.ts`, `.tsx`.

External plugins: any binary on PATH named `code-split-plugin-<name>`. The CLI invokes it as `code-split-plugin-<name> <workspace> --output <tmpfile> [--local-only] [-- <extra-args>]`; the binary must write a `{ "graphs": {...} }` JSON. See [DESIGN](DESIGN.md) §3.2 for the contract.
