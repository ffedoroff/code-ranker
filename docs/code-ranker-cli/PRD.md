# PRD — Code Ranker CLI (`code-ranker-cli`)

The command-line interface: the `code-ranker` binary, its two subcommands
(`check` / `report`), the layered configuration system, the machine-readable
comparison verdict, and CI integration. This is a component slice of the
product PRD — for the product overview, actors, plugin/extraction layer, graph
model and JSON schema, see the main [PRD](../PRD.md); for the viewer / HTML
report requirements see [`code-ranker-viewer/PRD.md`](../code-ranker-viewer/PRD.md).

<!-- toc -->

- [1. Unified CLI — Functional Requirements](#1-unified-cli--functional-requirements)
  - [Unified Entry-Point Command](#unified-entry-point-command)
  - [Configuration System](#configuration-system)
- [2. Baseline Comparison — CLI surface](#2-baseline-comparison--cli-surface)
  - [Machine-Readable Comparison Verdict](#machine-readable-comparison-verdict)
  - [Text Change Report](#text-change-report)
  - [CI Diff Integration (P2)](#ci-diff-integration-p2)
- [3. Public Interface — Code Ranker Unified CLI](#3-public-interface--code-ranker-unified-cli)

<!-- /toc -->

## 1. Unified CLI — Functional Requirements

### Unified Entry-Point Command

- [x] `p1` - **ID**: `cpt-code-ranker-fr-unified-cli`

All user-facing operations MUST be accessible through a single binary
`code-ranker`. Running it with no command prints help — every action goes
through an explicit subcommand; there is no default command. There are
**two** analysis subcommands, split by *what they emit* — `check` produces
an exit code (a CI gate), `report` produces files (a snapshot and a
viewer) — plus a small project-free `docs` command (below):

```
code-ranker check  [input] [--plugins <a,b,…>] [--baseline <snapshot>] [options]
code-ranker report [input] [--plugins <a,b,…>] [--language <name>] [--baseline <snapshot>] [--output.<fmt>.path <path>] [options]
code-ranker docs   [<lang> [<subject>]] [--config <PATH|KEY=VALUE>]   # reference docs (no analysis; language is first positional)
```

The single positional `[input]` (default `.`) is **polymorphic**: a
**directory** is analyzed in-process (run every active plugin, build the graphs,
compute metrics), while a **`.json` snapshot** or **`.html` report** is
read for its embedded snapshot — no analysis, source tree, or toolchain
required. Analysis-only flags (`--plugins`, `--ignore`) are rejected with a
snapshot input. A single run analyzes **all** relevant languages and produces one
report covering every language.

- `check` is the linter: it evaluates cycle rules, thresholds, and custom
  `[plugins.<lang>.rules.checks]` predicates, prints diagnostics, exits non-zero on any
  violation, and writes **no files**.
  With `--baseline <snapshot>` it switches to a **relative gate** that
  fails only on *new* violations versus the baseline (pre-existing ones
  tolerated) and emits a verdict (`improved` / `degraded` / `neutral`); a
  machine-readable verdict is produced with `--output-format json`.
- `report` writes artifacts (a JSON snapshot and/or an HTML viewer) and
  always exits `0`. Without `--baseline` the HTML is a single-snapshot
  viewer; with `--baseline <snapshot>` it becomes a baseline↔current diff
  view with a verdict, named `…-diff.html`.
- `docs [<lang> [<subject>]]` prints reference docs to stdout. It runs **no analysis**
  and takes **no `[input]`** — there is **no `--plugin` flag**; the language is the
  **first positional argument**. Bare `docs` lists every language (project-detected ones
  annotated) plus `base`. `docs <lang>` prints that language's full subject catalog.
  `docs <lang> <subject>` prints the subject doc for that language. `base` is a valid
  language for language-agnostic docs. A `<subject>` given without a language (e.g.
  `docs hk`, `docs ai`) is an **error** — the command exits non-zero and lists the
  project's languages with a hint to use `docs <lang> <subject>`. Subject matching is
  separator/case-insensitive (`fan_in` = `Fan-in` = `FAN in`). The `<subject>` selects:
  `metrics` / `principles` (index), a metric **category** (`loc`, `complexity`,
  `halstead`, `maintainability`, `coupling`), a **metric** key (`sloc`, `hk`, the
  language's own `unsafe`/`items`, …), or a **principle** id (`SRP`, `ADP`, …).
  `docs <lang> ai` prints the full AI-agent playbook + catalog for that language.

`report` selects artifacts and their destinations through one flag family,
`--output.<fmt>.path <path>` (`<fmt>` is `json`, `html`, `sarif`, `codequality`,
or `scorecard`; `sarif` and `codequality` write the same findings
documents `check --output-format sarif`/`codequality` emit but as artifacts —
SARIF 2.1.0 for GitHub code scanning / GitLab ≥18.11, CodeClimate JSON for the
GitLab Code Quality MR widget — see `cpt-code-ranker-fr-diagnostics`;
`scorecard` is the refactoring-guidance triage, and the matching AI fix-prompt is
`--prompt <ID>` to stdout, see `cpt-code-ranker-fr-ai-prompts`). When no
`--output.*` flag is given it writes
**both** `json` and `html` with default names into `.code-ranker/`:
`{ts}-{git-hash-3}.json` and `{ts}-{git-hash-3}.html`, e.g.
`.code-ranker/20260526-114144-a3f.json` (`{ts}` is the run's `generated_at` as a
local `YYYYMMDD-HHMMSS` timestamp — one value shared by every artifact a run
writes and identical to the embedded `generated_at`; `{git-hash-3}` the first
three chars of the commit); `sarif` / `codequality` /
`scorecard` are never in the default set and are emitted only when explicitly
named (`sarif` default `{ts}-{git-hash-3}.sarif`, `codequality` default
`{ts}-{git-hash-3}.codequality.json`). When one or more `--output.<fmt>.path` are given, **exactly** the
listed formats are written. The `.path` value is a file path (or a name
template, or `stdout`/`-` to stream the artifact); it supports placeholders
`{project-dir}` (slugified workspace name), `{ts}`, `{git-hash}` (the
12-char short commit), and `{git-hash-N}` (its first N chars). The
destination resolves as **`--output.<fmt>.path` flag › `[output.<fmt>]
path` in `code-ranker.toml` › built-in default**, so a project can pin its
own naming while a flag still wins for named states (e.g., `pr.json`). With
`--baseline`, the HTML default gains a `-diff` marker
(`{ts}-{git-hash-3}-diff.html`); the JSON artifact is always the current
snapshot, never a diff. The `scorecard` default is `stdout`. No additional registry
is created.

Each snapshot is a **single self-contained `.json` file** combining top-level
metadata (`schema_version = "5.0"`, command, `plugins`, versions, git state) and a
`languages` map — one entry per analyzed language, each carrying its
`graphs.files` graph (and `graphs.functions` when that level is on), `principles`,
and `prompt`. See `cpt-code-ranker-fr-snapshot-meta` (main [PRD](../PRD.md)) for the
full schema.

The snapshot is written as **canonical JSON**: every object key is emitted
in alphabetical order and the `nodes` / `edges` arrays are sorted by a
stable key (node `id`; edge `source`/`target`/`kind`). Re-analyzing unchanged
code therefore yields byte-identical graph data — no churn from map
iteration order — which keeps committed snapshots (e.g. the per-plugin
`sample/` goldens) diff-clean and makes a baseline comparison reflect only
real changes.

A `--baseline` comparison consumes snapshot files produced by `report` and
is plugin-agnostic. Splitting into separate binaries is forbidden at
P1; the separation of concerns lives inside the binary.

**Rationale**: One file per snapshot is easier to copy, archive, attach
to CI artifacts, and pass as a `--baseline`. A timestamped, commit-stamped
filename (`{ts}-{git-hash-3}`) means users never have to think about naming
for routine snapshots while keeping per-commit runs distinct; the
`[output.<fmt>]` config sets a project-wide template and an explicit
`--output.<fmt>.path` is available for named states (e.g.,
`snap-before-refactor.json`).

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-ci`

### Configuration System

- [x] `p1` - **ID**: `cpt-code-ranker-fr-config`

The analyzing commands (`check` / `report`) MUST load a layered
configuration from multiple sources. Priority order (highest wins for
scalars; `ignore.paths` is merged):

| Priority | Source |
|---|---|
| 1 | CLI flags (`--ignore`, `--cycle-rule`, `--threshold`, `--plugins`, `--output.<fmt>.path`) |
| 2 | `--config KEY=VALUE` inline overrides (dotted key into the config schema, e.g. `languages.<lang>.<key>=value`) |
| 3 | `--config <file>` |
| 4 | `code-ranker.toml` in cwd, then in target directory |
| 5 | `Cargo.toml` `[workspace.metadata.code-ranker]` / `[package.metadata.code-ranker]` |
| 6 | Built-in defaults |

**Config file keys** (`code-ranker.toml` or `Cargo.toml` metadata section):

```toml
version = "5.0"          # required config-schema version

[plugins]
enabled = ["rust", "markdown"]   # active languages; omit/empty ⇒ auto-detect all, overridden by --plugins

[plugins.base]           # virtual base: overrides applied to EVERY active language
skip_dirs = ["vendor"]

[plugins.base.ignore]
paths        = ["**/generated/**"]  # glob patterns matched against node path
tests        = true      # skip the language's test files; ON BY DEFAULT (legacy alias: test_modules)
dev_only_crates = true   # strip crates reachable only via [dev-dependencies]
                         # (uses `cargo metadata` for transitive accuracy)

[plugins.base.rules.cycles]
mutual     = true        # default: on
chain      = true        # default: on

[plugins.base.rules.thresholds.file]      # a single file (files graph)
loc        = 800
sloc       = 600             # any per-file metric the engine emits is accepted
hk         = 500_000
cyclomatic = 10

[plugins.base.rules.checks.de1101]        # custom check: a CEL bool predicate per file node →
when    = "tloc > 100"                     #   a `check.<id>` violation (config-only linter)
message = "{tloc} lines of inline test code in a production file"

[plugins.rust]           # per-language overrides: ANY plugin-config key, deep-merged
extensions = ["rs"]      # base wins under it; this wins over [plugins.base]

[output.json]                # default JSON snapshot destination (report command) — GLOBAL
path    = "{project-dir}-{ts}.json"   # placeholders: {project-dir} {ts} {git-hash} {git-hash-N}
enabled = true               # whether to write this format by default

[output.html]                # default HTML viewer destination (report command) — GLOBAL
path    = "{project-dir}-{ts}.html"   # a --output.html.path flag still overrides
enabled = true
```

**CLI flags**:

- `--plugins <a,b,…>` — set the active languages (comma list / repeatable); overrides
  the `[plugins].enabled` list. Omitted everywhere ⇒ auto-detect every language present
- `--language <name>` (`report`) — focus the `scorecard` / `--prompt <ID>` on one language;
  required only when a `--prompt`/`--focus` selector resolves across several languages
- `--config plugins.<lang>.<key>=value` — inline override of any plugin-config key
  (scalars / comma-lists); `plugins.base.*` targets the shared base language;
  `plugins.enabled=a,b` overrides the active language list
- `--output.<fmt>.path <PATH>` (`report`; `<fmt>` is `json`, `html`, or
  `scorecard`) — select
  that artifact format and set its destination (a path, a name template with
  placeholders `{project-dir}`, `{ts}`, `{git-hash}`, `{git-hash-N}`, or
  `stdout`/`-`); wins over `[output.<fmt>] path` (config sections exist for
  `json`/`html` only); built-in default
  `{ts}-{git-hash-3}`. Presence of any `--output.*` flag selects exactly the
  listed formats; with none, both `json` and `html` are written
  (`scorecard` is flag-only and never default)
- `--baseline <SNAPSHOT>` (`check` / `report`) — compare the current `[input]`
  against this baseline snapshot (`.json` or `.html`); on `check` it switches
  to a relative gate (fail only on new violations), on `report` it turns the
  HTML into a baseline↔current diff with a verdict
- `--git.<field> <VALUE>` (`check` / `report`) — override a snapshot git field
  (`--git.branch`, `--git.commit`, `--git.dirty-files`, `--git.origin`) instead
  of reading it from `git`; for CI, mapped from the platform's variables (e.g.
  `--git.branch="$CI_COMMIT_REF_NAME"`). Per field: a flag wins, the rest fall
  back to `git`; with `branch`+`commit`+`dirty-files` all set, `git` is not
  invoked. Applies only to a directory input
- `--config <PATH | KEY=VALUE>` — load config from an explicit file path, or
  override a single setting inline via a dotted key (repeatable; inline wins)
- `--ignore <GLOB>` — add a path glob (repeatable, merged with file)
- `--cycle-rule <KIND=on|off|N>` — configure a cycle check: `on` (any cycle of
  that kind fails), `off` (ignored), or an integer `N` (allow up to `N`, fail on
  the `N+1`-th — e.g. `chain=7` to pin today's count and forbid new ones)
- `--threshold <file.METRIC=N>` — set a per-file threshold (e.g.
  `file.loc=800`, `file.cyclomatic=10`); a breach fails the check (`check`
  only). The scope is always `file` (a single source file). `N` accepts `_`
  separators and `K`/`M`/`G` suffixes (e.g. `file.hk=5M`)
- `--top <N>` — report only the `N` worst violations (`check` only); reporting
  limit, does not change the exit code
- `--exit-zero` — exit 0 even when violations are found (`check` only,
  collect-only mode)
- `--suggest-config` — also print the current values as a ready-to-paste
  `code-ranker.toml` baseline (`check` only; off by default)

**No severity levels**: there is no warning tier — `check` either passes or fails.
A threshold is set or unset; a cycle kind is off, strict (`on`/`0`), or carries a
count budget `N` (up to `N` cycles of that kind allowed). A budget lets teams pin
today's cycle count and fail only on regressions, without fixing the backlog first.

**Rule ids and self-contained diagnostics**: every violation is identified by its
dotted rule id — the same string used as the config key and CLI flag (e.g.
`threshold.file.loc`) — and tagged with a concern group: `CYC` (dependency
cycles), `CPX` (complexity), `CPL` (coupling), `SIZ` (size). The full reference is
documented in [ERRORS.md](ERRORS.md). The default `human` output renders each
finding as a self-contained block — rule id, group, location (`id — path:line`),
measurement, rationale, fix, and the flag/config key that tunes the rule — so a
single block copied from the terminal is a complete prompt for an AI assistant.
The rule id and group are carried in every `--output-format` (block header,
`json` `rule`/`group` fields, `github` annotation title, `sarif` `ruleId` plus a
fired-rules `tool.driver.rules` catalog).

The `sarif` format additionally emits a `partialFingerprints` entry on every
result, keyed `codeRankerRuleLocation/v1` and valued `<rule>:<location>` — the same
`(rule, location)` signature `check --baseline` matches on internally. It
deliberately omits the line number, so a SARIF consumer (e.g. GitHub code scanning,
IDE SARIF viewers) keeps a finding identified as the *same* result across runs even
when surrounding edits shift it up or down the file, instead of reopening it as new.
The key is versioned (`/v1`) so the fingerprint basis can change later without
colliding with history.

**Current-values config block (`--suggest-config`)**: with `--suggest-config`,
`human` output prints — after the findings — the project's current measured values
as ready-to-paste `code-ranker.toml` blocks: the `[plugins.base.rules.cycles]` counts per kind,
and the `[plugins.base.rules.thresholds.file]` per-file thresholds (the worst single unit). A team copies the block to pin today's numbers as a baseline that passes
now and fails on regression. Off by default; the machine formats
(`json`/`github`/`sarif`) omit it.

The path of the config file actually used is recorded in the snapshot as `config_file`.

**Invalid configuration is fatal**: a malformed config file, an **unknown key or
section** in `code-ranker.toml` / `Cargo.toml` metadata (the schema is strict —
`deny_unknown_fields` — so a typo or stale key like `json-name` is rejected, not
silently ignored), an unknown threshold scope/metric, or a bad inline `--config`
/ `--threshold` / `--cycle-rule` value aborts the command with a non-zero exit
and a clear message (naming the offending field) — the tool never silently falls
back to defaults, which would drop the user's rules and let `check` pass when it
should fail (a false green for a CI gate).

**Rationale**: Teams need to suppress expected patterns (e.g. a budget of
allowed `chain` cycles, dev-only crate noise) and enforce structural budgets in
CI without modifying source code.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-ci`

See [config.md](config.md) for the full `code-ranker.toml` schema and
[CLI.md](CLI.md) for the complete flag reference.

## 2. Baseline Comparison — CLI surface

These are the `check --baseline` (machine gate) requirements of Step 4. The
human-facing HTML diff (`report --baseline`) is specified in
[`code-ranker-viewer/PRD.md`](../code-ranker-viewer/PRD.md) (`cpt-code-ranker-fr-graph-diff`,
`cpt-code-ranker-fr-diff-html-report`).

### Machine-Readable Comparison Verdict

- [x] `p1` - **ID**: `cpt-code-ranker-fr-compare`

`code-ranker check --baseline <snapshot> --output-format json` MUST compare
the current `[input]` against the baseline snapshot and emit a
machine-readable verdict and new-violation summary to stdout. The verdict is
`improved` (some violations resolved, none added), `degraded` (new violations),
or `neutral`; the gate is **relative** — it fails only on violations not already
present in the baseline (matched by `(rule, location)` signature). It is
implemented by re-evaluating the configured rules against the baseline snapshot
— **not** by the (deferred) structured graph diff — so it needs no
`compare_snapshots` engine.

JSON summary — a `verdict` wrapper around the new-violations list:

```json
{
  "verdict": "degraded",
  "violations": [
    { "rule": "threshold.file.hk", "group": "CPL", "language": "rust", "graph": "files",
      "location": "{target}/src/a.rs", "message": "…", "weight": 2.1 }
  ]
}
```

> A count-based summary (node/edge added/removed/affected, SCC counts) is **not**
> emitted in the JSON; the visual diff is computed browser-side from the two
> embedded snapshots (see `cpt-code-ranker-fr-graph-diff` in
> [`code-ranker-viewer/PRD.md`](../code-ranker-viewer/PRD.md)).

The human-facing counterpart is `code-ranker report --baseline`
(`cpt-code-ranker-fr-diff-html-report`), the interactive self-contained diff HTML
viewer — the same comparison surfaced two ways.

**Rationale**: `check --baseline` is the machine gate (an exit code and a
JSON verdict for CI); `report --baseline` is the shareable human diff viewer.

**Actors**: `cpt-code-ranker-actor-developer`, `cpt-code-ranker-actor-ci`,
`cpt-code-ranker-actor-pr-reviewer`

### Text Change Report

- [x] `p1` - **ID**: `cpt-code-ranker-fr-diff-text-report`

`code-ranker check --baseline <snapshot> --output-format json` emits a
structured JSON summary (see `cpt-code-ranker-fr-compare`) embeddable in CI
logs and PR comments. The JSON contains the `verdict` and the list of new
`violations` — **not** node/edge counts or SCC summaries (the visual diff is
computed browser-side from the two embedded snapshots).

**Actors**: `cpt-code-ranker-actor-ci`, `cpt-code-ranker-actor-pr-reviewer`

### CI Diff Integration (P2)

- [x] `p2` - **ID**: `cpt-code-ranker-fr-ci-diff`

`code-ranker check --baseline <snapshot>` SHOULD act as a CI regression
gate: exit non-zero when the current tree introduces *new* violations
versus the baseline (e.g. new cycles added, HK degraded beyond a limit).
The base-branch snapshot is fetched from a stored CI artifact; the verdict
JSON (`--output-format json`) and the `report --baseline` diff HTML are
attached to the pull request automatically.

**Actors**: `cpt-code-ranker-actor-ci`, `cpt-code-ranker-actor-pr-reviewer`

## 3. Public Interface — Code Ranker Unified CLI

- [x] `p1` - **ID**: `cpt-code-ranker-interface-cli`

**Type**: Single CLI binary (`code-ranker`)

**Stability**: unstable (pre-1.0)

**Subcommands**: bare `code-ranker` prints help — there is no default
command; every action is an explicit subcommand.

```
# Lint — gate on cycle rules & thresholds; writes no files
code-ranker check  [input] [--plugins <a,b,…>] [--threshold ...] [--cycle-rule ...] [--baseline <snapshot>] [--output-format <human|json|github|sarif|codequality>] [--exit-zero]

# Steps 1+2 — analyze (or read) the input and write a snapshot and/or HTML viewer
# (also the AI prompt via --prompt <ID> / console scorecard via --output.scorecard)
code-ranker report [input] [--plugins <a,b,…>] [--language <name>] [--output.<fmt>.path <path>] [--baseline <snapshot>] [--prompt <ID>] [--focus <NAME>] [--focus-path <PATH>] [--severity <tier>] [--top <N>]

# Reference docs (no analysis; language is first positional, no --plugin flag)
code-ranker docs [<lang> [<subject>]]
```

The positional `[input]` (default `.`) is polymorphic: a directory is
analyzed, while a `.json` snapshot or `.html` report is read for its
embedded snapshot (no analysis). Step 4 is `--baseline <snapshot>`, accepted
by both commands: `report --baseline` writes a baseline↔current diff HTML
viewer with a verdict, and `check --baseline` is a relative CI gate (fail
only on new violations) whose verdict is machine-readable with
`--output-format json`.

Global options accepted by every command: `--config <PATH | KEY=VALUE>`
(repeatable; inline wins), `--output.mode <quiet|summary|verbose>` (verbosity of
the **stderr** diagnostic stream; default `summary` — errors, warnings,
written-artifact paths and the closing `✓ … — <time>` line; `quiet` = errors only;
`verbose` = also the `▶`/`config:` startup lines and every external command's
`↳` timing — stdout/artifacts are unaffected), `-h/--help`, `-V/--version`.

**Exit codes**: 0 = `check` passed (or `--exit-zero`), `report`
completed; non-zero = generic failure, or `check` found a violation;
failures emit a structured JSON error on stderr.

**Breaking Change Policy**: Adding flags or subcommands is minor;
renaming or removing flags, changing JSON artifact schema, or changing
exit-code semantics requires a major-version bump.

---

**Related docs**: [CLI.md](CLI.md) (full flag reference) ·
[config.md](config.md) (`code-ranker.toml` schema) ·
[ERRORS.md](ERRORS.md) (rule reference) ·
[DESIGN.md](DESIGN.md) (CLI technical design) ·
main [PRD](../PRD.md) · [`code-ranker-viewer/PRD.md`](../code-ranker-viewer/PRD.md)
