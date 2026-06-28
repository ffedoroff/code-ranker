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
principle, by that design principle.

## Principles & metrics

Each entry summarizes one principle or metric; run `code-ranker docs <lang> <ID>`
to print its full doc (offline, straight to the terminal).

<!-- doc:tldr-index -->
