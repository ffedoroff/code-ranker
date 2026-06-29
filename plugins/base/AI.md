# code-ranker — AI agent skill

`code-ranker` is a multi-language **structural analysis platform** an AI
assistant can drive. It builds a project's dependency graph, finds the structural
problems that make code hard to change — dependency **cycles** (ADP), heavy
**coupling** (Henry–Kafura), and complexity hotspots — ranks them worst-first, and
scores them against design principles (SOLID, DRY, KISS, …). It gates CI on your
thresholds, renders a self-contained HTML viewer of the graph, and emits
ready-to-use **AI fix-prompts**. One binary; a language plugin (Rust, Python,
JavaScript / TypeScript, Go, C / C++, C#, Markdown) is selected per project.

This is the short guide for driving it — the commands below operate the tool.

## Commands

- **`check [input]`** — the **gate**. Evaluates cycle rules and metric thresholds
  (with `--baseline`, only regressions), prints diagnostics, and **exits non-zero**
  on a violation. Writes no files — the CI entry point.
- **`report [input]`** — produces **artifacts**: a JSON snapshot, a self-contained
  HTML viewer, and the advisory **`scorecard`** (console triage) / **`prompt`** (an
  LLM fix-prompt). Always exits `0` — the analysis + refactoring entry point.
- **`docs <lang> <subject>`** — print a reference doc to stdout (no analysis). The
  language comes first (`rust`, `python`, …, or `base` for the language-agnostic
  catalog). `docs <lang> ai` prints this playbook plus the full principle/metric
  catalog; `docs <lang> metrics` / `docs <lang> principles` index every metric /
  principle; `docs <lang> <category>` (`loc`, `complexity`, …) lists a category;
  `docs <lang> <ID>` prints one metric or principle (`docs rust hk`,
  `docs rust SRP`). Always exits `0`.
- **`help`** — usage for the binary or any command (`code-ranker --help`,
  `code-ranker <command> --help`, or `-h <command>`). Lists every flag.

`[input]` (default `.`) is polymorphic: a directory is analyzed; a `.json` / `.html`
snapshot is read back with no re-analysis. Keep old `.code-ranker/` snapshots — they
are baselines for a before/after diff (`--baseline <snapshot>`).

## Read the tool's output — don't parse its files, don't truncate it

code-ranker **is** the query layer. You never need to parse the JSON snapshot
(`jq`, ad-hoc greps over the report) — everything is available directly from the
console, already ranked and labelled:

- `--output.scorecard` — the worst-first ranking.
- `--focus <metric|principle>` (e.g. `--focus hk`, `--focus ADP`) — frame the ranking by one metric/principle.
- `--focus-path <dir>` (repeatable) — restrict the ranking / `--prompt` to modules **under a path**. The whole project is still analyzed (the graph needs it), but only that subtree is listed. **This is how you scope to one crate / package / folder** — reach for it instead of post-filtering output yourself.
- `--prompt <ID>` — the fix-prompt, with each hotspot's incoming/outgoing connections (the `fan_in` / `fan_out` edges) already listed.
- `--top N` — how many rows the scorecard / prompt shows (`--top 1` = the single worst).

**Always name the language on the same line.** Those are all `report` flags, and
every `report` / `check` run must resolve a plugin first — so pass
`--plugins <lang>` alongside `--prompt` /
`--output.scorecard` / `--focus`. A run that omits it can't pick a language and
stops at the language picker instead of producing the prompt:
`code-ranker report --plugins <lang> --prompt HK --top 1`.

The `.json` / `.html` artifacts exist for the HTML viewer and for `--baseline`
diffs — not for you to read or parse. If you find yourself writing `jq` against a
report, stop and use a flag above instead.

**Never pipe code-ranker output through `head` / `tail` / `sed` / `cut` /
`awk`.** The scorecard puts the metric value in a right-hand column and the
prompt emits multi-line connection blocks — line-truncating tools cut off
exactly the numbers and edges you need and silently drop rows, which leads you
to wrong conclusions. Bound the output **at the source** with `--focus-path` and
`--top N`, then read it whole.

<!-- ai:select-start -->
## Select a language

`code-ranker` analyzes **one** language per run, selected by a plugin — and none
could be resolved here:

> {reason}

Pick one of: **{plugins}**. Either name it per run (applies to `check` / `report`
too):

```sh
code-ranker check . --plugins <name>
```

…or set it once in a `code-ranker.toml` at the project root, so every command picks
it up:

```toml
version = "{config_version}"
[plugins]
enabled = ["<name>"]
```

Then re-run `code-ranker docs <name> ai` for the full playbook and the principle/metric catalog.
<!-- ai:select-end -->

## The two that matter most

Fix one thing at a time, worst-first. Cycles (**ADP**) are structural — clear them
first; then coupling (**HK**). Focus on one metric or principle with `--focus` and
inspect the worst tier with `--severity warning`.

- **ADP** — dependency cycles; the module graph should be acyclic.
- **HK** — Henry–Kafura coupling, `HK = sloc × (fan_in × fan_out)²`: a large module
  on a busy crossroads of incoming/outgoing dependencies.

## The fix loop

```sh
code-ranker check . --plugins <lang>                          # 1. the gate verdict
code-ranker report . --plugins <lang> --output.scorecard --focus ADP --top 1   # 2. focus one metric/principle, worst-first
code-ranker docs <lang> <principle>                           # 3. READ the deep doc — before you touch code
```

**Step 3 is not optional — read the `docs <lang> <principle>` page before proposing a
fix.** It names the *language-specific cause* of this violation and the *smallest
correct remedy* for it, often with a worked example. Agents that skip it reach for a
heavier, wrong-shaped refactor that can leave the real cycle intact, introduce a new
one, or drop tests. Read it first; then fix.

`--focus` takes any catalog id below (a principle like `ADP`, or a metric like
`hk` / `loc`): focusing on a metric frames the output by that metric; on a
principle, by that design principle. In a large repo or workspace, add
`--focus-path <dir>` to rank only the crate/package/folder you are working on
(the graph is still built from the whole project, so cross-module edges stay
correct).

## Principles & metrics

Each entry summarizes one principle or metric; run `code-ranker docs <lang> <ID>`
to print its full doc (offline, straight to the terminal).

<!-- doc:tldr-index -->
