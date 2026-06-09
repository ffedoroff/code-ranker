# code-ranker rule reference

Every diagnostic emitted by `code-ranker check` is identified by a stable, dotted
**rule id** — the *same string* you use as the config key and the CLI flag — and
tagged with a concern **group** (`CYC` / `CPX` / `CPL` / `SIZ`). There are no
opaque numeric codes: the id *is* the documentation.

| Identifier | Example | Also used as |
|------------|---------|--------------|
| Rule id | `threshold.file.loc` | CLI flag `--threshold file.loc=N` · config key `rules.thresholds.file.loc` |
| Group | `SIZ` | filtering / the `ref` link into this page |

The prose below is what `check` prints in its console output, so a single finding
copied from the terminal is already a complete, self-contained prompt for an AI
assistant — it carries the rule id, the location, the measurement, the rationale,
and the fix.

## Severity model

code-ranker has **no severity levels**. A rule is either *active* or not:

- **Cycle rules** are on / off / a count budget. `mutual` and `chain` are on by
  default. A kind's value can be `on`/`true` (any cycle of
  that kind fails — same as `0`), `off`/`false` (ignored), or an integer `N` (up
  to `N` cycles of that kind allowed; the `N+1`-th fails). Use `N` to pin today's
  count and forbid adding more (e.g. `chain=7`).
- **Threshold rules** are inactive until you set a number. Once set, any file
  over the limit is a violation.

Any violation of any active rule fails `check` with a non-zero exit code. There
is no warning tier — if something should not fail the build, turn the rule off or
raise its threshold rather than downgrading it.

| Exit code | Meaning |
|-----------|---------|
| `0` | No violations — or violations found but `--exit-zero` was passed (collect-only). |
| `1` | One or more violations, **or** an operational error (bad config, plugin failure, snapshot not found). Operational errors are reported as a plain message, not a rule id. |

`--top N` limits only how many findings are *reported* (worst-first by breach
severity); it never changes the exit code.

## Threshold scopes

A threshold rule id is `threshold.file.<metric>`. There is a single graph
level — files — so the scope is always `file`:

| Scope | Applies to |
|-------|-----------|
| `file` | a single source file (files graph) |

The limit is checked **per file** — any single file over the limit is a violation:

| Form | Meaning | Example |
|------|---------|---------|
| `threshold.file.<metric>` | a single file exceeds | `--threshold file.cognitive=25` |

So `file.loc` caps any one file.

```bash
code-ranker check --threshold file.loc=400 --threshold file.cognitive=25
```

The **metric** is one of `cyclomatic`, `cognitive`, `hk`, `fan_in`, `fan_out`,
`loc` (see the group tables below). Every form of one metric shares the same
rationale and fix.

**Value syntax.** A threshold value accepts `_` digit separators and a `K` / `M` /
`G` multiplier suffix (×10³ / ×10⁶ / ×10⁹, case-insensitive): `5K` = 5 000,
`1.5M` = 1 500 000. On the CLI use it bare (`--threshold file.hk=5M`). In TOML a
suffix must be quoted (`hk = "5M"`) since bare `5M` is not valid TOML; underscored
integers are native (`hk = 5_000_000`).

> **All built-in plugins (Rust, Python, JavaScript/TypeScript) build a single file
> graph,** so the `file` scope applies to every language. `fan_in` / `fan_out` /
> `hk` are computed from internal file→file edges only; edges to external library
> nodes are excluded.

## Anatomy of a finding

In the default `human` output each violation is one block:

```text
threshold.file.cognitive  ·  CPX  ·  files graph
  where  {target}/src/handlers.rs
  issue  cognitive complexity 67 exceeds limit 25 (2.7× over budget)
  why    Cognitive complexity weights nested and interrupted control flow by how hard a human finds it to follow…
  fix    Extract nested blocks into named helpers, use early returns to cut nesting depth…
  tune   set with --threshold file.cognitive=N   ·   rules.thresholds.file.cognitive in code-ranker.toml
  ref    https://github.com/ffedoroff/code-ranker/blob/main/docs/code-ranker-cli/ERRORS.md#group-cpx
```

- **rule id + group + graph** — the rule, its concern group, and the graph (files) it fired on.
- **where** — `id — path`, a clickable location. Omitted for cycle rules.
- **issue** — the measurement: value, limit, and how far over budget.
- **why / fix** — the rationale and the concrete remedy.
- **tune** — the CLI flag and the `code-ranker.toml` key that adjust or disable the rule (identical to the rule id).
- **ref** — a link to this page's group section.

## Output formats

`check --output-format` controls how findings are serialized. The rule id and
group are present in every format.

| Format | Identifies the rule as | Notes |
|--------|------------------------|-------|
| `human` (default) | the block header | Rich, self-contained blocks as shown above. |
| `json` | `"rule"` + `"group"` fields | Array of `{rule, group, graph, location, message, weight}`. |
| `github` | annotation title (`code-ranker threshold.file.loc`) | GitHub Actions `::error` workflow commands. |
| `sarif` | `ruleId` | SARIF 2.1.0; the rules that fired are described under `tool.driver.rules` (id, group, rationale, helpUri). |

## Rule groups

<a id="group-cyc"></a>

### CYC — dependency cycles

Cycles are structural: they come from the import/dependency graph, not from a
metric threshold. `mutual` and `chain` are on by default.
Each kind takes `on` (strict — any cycle fails), `off` (ignored), or a count
budget `N` (allow up to `N`, fail on the next): `--cycle-rule chain=off`,
`--cycle-rule chain=7`, or `rules.cycles.chain = 7`. `check --suggest-config`
prints the current count per kind so you can paste it as a baseline.

| Rule id | What it flags | How to fix |
|---------|---------------|------------|
| `cycle.mutual` | Two units import each other (A ↔ B), so neither can be built, tested, or understood in isolation — the tightest possible coupling. | Move the shared types into a third, lower-level unit both depend on; invert one direction behind a trait/interface; or merge the two if they are really one concept. |
| `cycle.chain` | Three or more units form a strongly-connected component (A → B → C → A); the whole component must be loaded and changed together, defeating modular boundaries. | Find the edge that closes the loop — usually one "back" dependency pointing upward — and invert or remove it, or introduce an abstraction layer between the units. |

<a id="group-cpx"></a>

### CPX — control-flow complexity

Threshold rules: `threshold.file.<metric>` for the metrics below — per single
file (see [Threshold scopes](#threshold-scopes)). Inactive until a limit is set.

| Metric | What it flags | How to fix |
|--------|---------------|------------|
| `cyclomatic` | Cyclomatic complexity counts the independent paths through a unit; high values mean many branches, which demand many tests and are easy to get wrong. | Split the function, replace branching with polymorphism or a lookup table, and pull guard clauses to the top to flatten nesting. |
| `cognitive` | Cognitive complexity weights nested and interrupted control flow by how hard a human finds it to follow; a high score reads as "hard to hold in your head". | Extract nested blocks into named helpers, use early returns to cut nesting depth, and avoid mixing several control structures in one function. |

<a id="group-cpl"></a>

### CPL — coupling

Threshold rules over the dependency graph. Henry-Kafura combines size and
connectivity: `hk = sloc × (fan_in × fan_out)²`. Inactive until a limit is set.

| Metric | What it flags | How to fix |
|--------|---------------|------------|
| `hk` | Henry-Kafura coupling: the unit is both highly connected and large — a change-amplifier whose edits ripple widely across the system. | Cut fan-in or fan-out: narrow the public surface, split the unit by responsibility, or route dependencies through a smaller interface. Shrinking the file (sloc) also lowers hk. |
| `fan_in` | Too many other units depend on this one, making it risky to change and a single point of failure — though some hubs (shared types) carry high fan-in legitimately. | If unintended, split the unit so each caller depends only on the slice it uses; otherwise stabilize the interface so high fan-in is safe. |
| `fan_out` | This unit depends on too many others, so it breaks when any of them change and is hard to test in isolation. | Group related dependencies behind a facade, inject collaborators instead of reaching for them, or move logic closer to the data it uses. |

<a id="group-siz"></a>

### SIZ — size

Threshold rule over source lines of code. Inactive until a limit is set.

| Metric | What it flags | How to fix |
|--------|---------------|------------|
| `loc` | The unit has more source lines than allowed; large files/functions tend to hold several responsibilities and are harder to review, test, and reuse. | Split by responsibility into smaller units, extract helpers, and separate data definitions from behavior. |

## Tuning recap

```bash
# Disable a cycle kind, or pin today's count as a budget (forbid new ones)
code-ranker check --cycle-rule chain=off
code-ranker check --cycle-rule chain=7

# Single-file limits
code-ranker check --threshold file.loc=400 --threshold file.cognitive=25 \
  --threshold file.hk=500000

# Collect findings without failing the build
code-ranker check --threshold file.loc=120 --exit-zero
```

Equivalent `code-ranker.toml` (per-file metrics sit directly under
`[rules.thresholds.file]`):

```toml
[rules.cycles]
mutual = true        # strict — any mutual cycle fails (same as 0)
chain = 7            # allow up to 7 chain cycles; the 8th fails

[rules.thresholds.file]
loc = 400
cognitive = 25
hk = 500000
```

See [CLI.md](CLI.md) for the full `check` flag set and [config.md](config.md) for
the complete configuration schema.
