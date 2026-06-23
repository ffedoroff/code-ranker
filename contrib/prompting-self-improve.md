# Prompt self-improvement loop

## Goal

> **Self-improving prompts — and a playbook that improves itself.**

code-ranker hands an AI agent a generated fix-prompt for every structural problem it
finds. How good the resulting fix is comes down to two things: the model, and the
prompt. We can't make every user run the most capable model, so the lever we own is
the **prompt**. This loop drives every prompt to the point where the *cheapest* model
produces the same fix the *most capable* one would — in fewer turns, because the
prompt told the agent exactly what it needed and nothing it didn't.

Three objectives, optimized together:

- **Quality** — a real structural fix, behaviour preserved, tests green — equal to the
  reference model's.
- **Cost** — the agent reaches that fix in as few calls and tokens as possible.
- **Clarity** — the agent never guesses: it reads the prompt once and knows the plan,
  which doc to read, and what "done" means.

The loop is **closed on itself**. Each pass runs a real fix, measures the gap to the
reference, changes the smallest prompt lever that would have closed it, rebuilds, and
re-runs — until the cheapest tier matches the bar. And when the *process itself*
proves clumsy — a run that teaches nothing, a score that doesn't discriminate, a
lever that's hard to find — we edit **this file too**, the algorithm of
self-improvement. Both layers improve: the prompts agents read, and the procedure
that improves those prompts.

Progress is **measured** ([Metrics](#metrics-metricscsv)), not felt: "better or
worse" between two prompt versions is a row-to-row comparison. End state: across every
`FOCUS`, the cheapest model matches the reference at minimum cost and maximum clarity,
and the playbook gets there with no manual babysitting.

---

A repeatable way to **empirically tune the AI fix-prompts** so that *cheaper*
models still produce reference-quality fixes. The reference is the most capable
model; the goal is to lift each cheaper tier up to it by improving the prompt —
not by relying on the model.

Think of it as a function:

```
improve(PROJECT, FOCUS)        # sweeps models, iterates the prompt
```

## Inputs (the variables)

| Variable | Meaning | Examples |
|---|---|---|
| `MODEL` | the agent model under test, ordered **most → least** capable | `opus` → `sonnet` → `haiku` |
| `FOCUS` | what to fix — a principle **or** a metric, passed to `--focus` | `cycle` (ADP), `hk`, `sloc`, `cognitive`, `SRP`, … |
| `PROJECT` | an **external** repo (not code-ranker) with real, non-trivial instances of `FOCUS` | any sample/work repo |

`MODEL_REF` = the first (most capable) model — the quality bar every cheaper model
is measured against.

## What we tune (the levers)

The prompt an agent sees is assembled from **embedded data**. To change it, edit one
of these and rebuild (see Setup) — all are baked into the binary:

- **principle framing** — the `[[principles]]` `prompt` in
  `crates/code-ranker-plugins/src/defaults.toml` (+ per-language overrides in
  `crates/code-ranker-plugins/src/languages/<lang>/config.toml`).
- **scaffolding** (intro / doc-note / task / focus prose) —
  `crates/code-ranker-graph/metrics/prompt.md`.
- **the full reference doc** the agent reads via `--doc <FOCUS>` —
  `languages/<lang>/<FOCUS>.md` (e.g. `ADP.md`), and the offline entry point
  `languages/base/AI.md` (`--doc AI`).

Change the **smallest** lever that fixes the observed failure.

## Setup (once per prompt version)

- **S1 — fresh build on PATH.** Release-build and install locally so the
  `code-ranker` invoked by the agent is the current build:
  `cargo build --release` (then `cargo install --path crates/code-ranker-cli`).
- **S2 — provenance commit + run id.** Commit code-ranker, so every report this
  build generates carries the current version + commit + date. Then capture the
  **short hash** — `CR_SHA=$(git -C <code-ranker> rev-parse --short HEAD)`. It names
  the artifact directory for this build (next section): every chat, report and JSON
  is traceable to the exact build — i.e. the exact **prompt version** — that
  produced it.

Every prompt edit (a lever above) re-runs S1–S2 before the next sweep, yielding a
fresh `CR_SHA` → a fresh artifact directory.

## The algorithm

Two nested loops. The **inner** loop improves the prompts; the **outer, meta** loop
([below](#the-meta-loop--improving-this-playbook)) improves *this playbook* when the
process itself gets in the way. Both are driven by the same measured signals.

```
for MODEL in models (most → least capable):        # opus, then sonnet, then haiku…
  loop (≤ 3 times):
    R = run(PROJECT, FOCUS, MODEL)                 # one clean-context fix (below)
    save artifacts(R)
    measure R → metrics.csv                        # quality + cost + clarity (objective)
    score R against MODEL_REF's best run for FOCUS
    if R meets the bar on all three axes:          # ref-quality AND few calls AND no guessing
      break                                        # this tier is good — lock it
    else:
      pick the SMALLEST prompt lever that explains the gap, by axis:
        quality  bad / shallow fix              → principle framing, then the FOCUS doc
        cost     wasted turns: re-reads, dead   → state up front what the prompt now
                 ends, rediscovered facts          makes the agent discover; cut noise
        clarity  agent asked / back-tracked /   → reword, reorder; put the decision
                 misread / read a doc twice        first, name "done" explicitly
      edit that lever, rebuild (S1–S2), re-run
      # the edit is a HYPOTHESIS: the next run's metrics must show the targeted gap
      # shrink vs the previous iteration — not just vs the reference — else revert it.
    # META — when the LOOP itself misbehaved (a run that taught nothing, a signal that
    # didn't discriminate, a lever you couldn't locate, an artifact you couldn't trace)
    # fix the PROCESS: edit THIS file, commit it (→ new CR_SHA), continue. It's a lever too.
  # descend to the next cheaper model and re-verify with the improved prompt
```

End state: across every `FOCUS`, the **cheapest** tier produces reference-quality
fixes at **minimum calls** and **maximum clarity** — and the playbook itself needed no
manual fixing to get there. Then repeat `improve(...)` for the next `FOCUS`.

## A single run — `run(PROJECT, FOCUS, MODEL)`

Let `RUN=<code-ranker>/.code-ranker/prompt-eval/<timestamp>_<CR_SHA>/<MODEL>-<FOCUS>-<N>`
— an **absolute** path into *this* repo's `.code-ranker/` (create it first). The
agent runs `code-ranker report .` inside `PROJECT`, but every `--output.*.path`
points at `$RUN`, so the evidence lands in code-ranker, not `PROJECT`. The agent's
**own** file writes (its plan file, any `report` it runs without an `--output`
override) still land in `PROJECT/.code-ranker/` — step 7 sweeps those into `$RUN`, so
nothing eval-related is left in `PROJECT`.

1. **Clean start.** `PROJECT` on `main`, working tree clean.
2. **Fresh agent session**, model = `MODEL`, **empty context**. Bootstrap it with the
   offline playbook only — no extra hints: have it read
   `code-ranker report --doc AI` (overview + catalog) and `--doc <FOCUS>` (the deep
   doc). This is what a real user would do, so it tests the *prompt*, not your
   coaching.
3. **BEFORE.** `code-ranker report . --output.html.path=$RUN/before.html --output.json.path=$RUN/before.json`.
4. **Save the focused prompt** (orchestrator, for the record):
   `code-ranker report . --output.prompt.path=$RUN/prompt.md --focus <FOCUS> --top 1`
   — captures the exact fix-prompt this run used into `$RUN/prompt.md`, so prompt ↔
   behaviour stays correlatable across models.
5. **Fix** (agent). Ask the agent to fix the single worst (`--top 1`) cycle and **let it
   work out how on its own** — which command to run, which doc to read, which refactor to
   choose. Don't hand it the command: the run tests whether the prompt and docs lead it
   there. The agent proposes the plan, applies the fix, and runs the project's tests.
6. **AFTER + DIFF.** `code-ranker report . --baseline $RUN/before.json --output.html.path=$RUN/diff.html --output.json.path=$RUN/after.json` (+ an `after.html`).
7. **Collect the agent's own writes into `$RUN`.** The generated prompt tells the agent
   to save a plan to `<PROJECT>/.code-ranker/<ts>-<FOCUS>.md`, and any `report` it runs
   without an `--output` override also lands in `<PROJECT>/.code-ranker/` — which is
   **not** gitignored in a typical project. Move them into `$RUN/` (e.g.
   `$RUN/agent-plan.md`) and clear `PROJECT/.code-ranker/`, so **all** eval evidence sits
   under code-ranker's `prompt-eval/` and the `PROJECT` branch carries only the code
   change. (This is also why the orchestrator must stage explicit paths, never
   `git add -A`, when committing the fix.)
8. **Save the transcript** to `$RUN/chat.md` (see "Saving the chat"), commit the code
   change to branch `<MODEL>-<FOCUS>-<N>` in `PROJECT`, return to `main`.
9. **Measure.** Append one row to `prompt-eval/metrics.csv` with the collector —
   don't hand-compute it (see [Metrics](#metrics-metricscsv) → Collecting a row):

   ```sh
   contrib/prompt-eval-metrics.py $RUN --focus <FOCUS> --project <name> \
     --project-path PROJECT --quality <1-5> --clarity <1-5> --verdict improved
   ```

## Artifacts: layout & naming

Everything lives under the **code-ranker repo's own `.code-ranker/`** (this repo,
not `PROJECT`) — it's gitignored and is the project's keep-forever run area, so all
prompt-eval evidence is collected in one place across every `PROJECT` and model. The
external `PROJECT` only carries the **code change**, on its branch. All evidence for
one **build / prompt version** sits in a single dated folder; **keep everything —
never delete, the runs are the comparison corpus.**

Layout (one build → one `<timestamp>_<CR_SHA>` folder → one subfolder per run):

```
<code-ranker>/.code-ranker/          # THIS repo's dir, not PROJECT's
└─ prompt-eval/
   ├─ metrics.csv                       csv    append-only — ONE row per run, ALL builds (comparison corpus)
   └─ 20260623T1412Z_a660e36/          dir   — <UTC timestamp>_<CR_SHA from S2>
      ├─ run.md                         md     ~1 KB   inputs: project, FOCUS, models, cr version+commit
      ├─ results.md                     md     ~2 KB   the results-log rows for this build
      ├─ opus-cycle-1/                  dir            one run = <model>-<focus>-<n> (matches the PROJECT branch)
      │  ├─ before.json                 json   ~150 KB baseline snapshot
      │  ├─ before.html                 html   ~1.5 MB self-contained viewer (inlined WASM/assets)
      │  ├─ after.json                  json   ~150 KB post-fix snapshot
      │  ├─ after.html                  html   ~1.5 MB
      │  ├─ diff.html                   html   ~1.6 MB baseline↔current diff report
      │  ├─ prompt.md                   md     ~3 KB   the exact `--focus` fix-prompt the agent got
      │  ├─ chat.jsonl                  jsonl ~0.5–3 MB raw session record (Claude Code; verbatim)
      │  └─ chat.md                     md   ~50–300 KB readable transcript (the tuning data)
      ├─ sonnet-cycle-1/                dir            same shape
      └─ haiku-cycle-2/                 dir            same shape
```

- folder/run id = `<model>-<focus>-<n>`, identical to the PROJECT branch holding
  that run's code change — so evidence ↔ code line up by name.
- the code-ranker version/commit is also embedded *inside* each report (from S2), so
  a file stays self-describing even if moved out of its folder.
- HTML reports are large (self-contained, WASM inlined); JSON snapshots scale with
  the project; `chat.md` is the biggest signal-per-byte and the smallest to diff.

### Launching a clean-context agent

Each run is a **fresh session** of `MODEL` with **no carried context** — start a new
one, never `--continue`/`--resume`. Keep `PROJECT` free of a code-ranker-specific
`CLAUDE.md`/memory so only `--doc AI` primes the agent; otherwise you're testing the
priming, not the prompt.

- **Claude Code** (Opus / Sonnet / Haiku), interactive — what the fix loop wants
  (multi-turn: run code-ranker, edit, run tests):

  ```sh
  cd PROJECT                                  # external repo, on main, clean tree
  claude --model opus                         # or sonnet / haiku — pins the tier; fresh = no context
  ```

  Then give it **one** opening message (the bootstrap), nothing else:

  > Read `code-ranker report --doc AI`, then fix the worst `<FOCUS>` in this
  > project. Show me the plan before changing code.

  Headless one-shot (scriptable, but weaker for the multi-step loop):

  ```sh
  cd PROJECT && claude -p "Read \`code-ranker report --doc AI\`, then fix the worst <FOCUS>…" --model haiku
  ```

- **Other agents** (Cursor, …): open a **New Chat** (not a continued thread), select
  the model, paste the same one-message bootstrap.

### Saving the chat

The transcript is the **primary tuning data** — it shows *where* a cheaper model
diverged (skipped `--doc`, picked the wrong cycle, hacked the metric). Save it raw,
**verbatim, no summary**, into `$RUN/chat.*`. It must include the bootstrap
(`--doc AI` / `--doc <FOCUS>` reads), the task, and **every** assistant turn — its
reasoning **and** the tool calls (the `code-ranker` commands + their output), through
the final fix and the test run.

- **Claude Code** — the canonical record is the session **JSONL** at
  `~/.claude/projects/<cwd-slug>/<session-id>.jsonl` (cwd-slug = `PROJECT`'s path with
  `/`→`-`; one file per session, newest by mtime = the run you just did). Copy it to
  `$RUN/chat.jsonl` (verbatim turns + tool calls) and/or render it to `$RUN/chat.md`
  for reading.
- **Other agents**: export / copy the conversation as Markdown into `$RUN/chat.md`.
- Also save the exact fix-prompt the agent received as `$RUN/prompt.md`, so prompt →
  behaviour is correlatable across models. Markdown stays readable and diffable.

## Metrics (`metrics.csv`)

"Better or worse" is decided by numbers, not memory. Every run appends one row to a
single append-only file, **`<code-ranker>/.code-ranker/prompt-eval/metrics.csv`** —
the cross-build comparison corpus. To compare two prompt versions, filter the rows to
the same `(project, focus, model)` and read down the columns: a newer `cr_sha` is
**better** when `quality_1_5` and `clarity_1_5` are ≥ and `focus_delta` is ≥ (more
negative or equal) **while** `tool_calls` / `commands` / `output_tokens` go **down**. A gain on one axis
paid for by a loss on another is not a win — name the trade in `notes`.

Columns, grouped by objective (most are extractable from the run's artifacts; the two
`*_1_5` are judged from the transcript + diff):

| Column | Axis | Source | Meaning (↑/↓ = better) |
|---|---|---|---|
| `ts`,`cr_sha`,`project`,`focus`,`model`,`iter`,`run` | id | run.md | identity — `cr_sha` is the prompt version |
| `tests_pass` | quality | project tests | 1/0 — tests green, behaviour preserved |
| `focus_before` / `focus_after` | quality | before/after `.json` scorecard | FOCUS violation count (e.g. ADP warnings) |
| `focus_delta` | quality | `after − before` | ↓ (negative) = fewer violations |
| `worst_before` / `worst_after` | quality | before/after `.json` | size of the worst instance (e.g. SCC node count) |
| `new_cycles` | quality | after vs before `.json` | ↓ cycles present in `after` but **not** `before` — regression guard (a fix that breaks one cycle and creates another scores 0 here) |
| `collateral_delta` | quality | full scorecard at main vs branch | Δ in **non-FOCUS** principle violations (run `report --output.scorecard --top 0` at each git state, sum all rows except FOCUS). ↓ = a fix that also cleared other principles; ↑ = collateral damage |
| `quality_1_5` | quality | transcript + diff | ↑ real fix (extract/invert/split) vs metric-hack |
| `tool_calls` | cost | transcript | ↓ total tool invocations (Read/Edit/Bash/Grep/…) |
| `commands` | cost | transcript | ↓ shell/CLI commands run (the `Bash` subset — code-ranker, cargo, grep) |
| `input_tokens` | cost | transcript | ↓ input tokens **incl. cache reads** — noisy (turn-/cache-dominated); compare only on the same extraction basis |
| `output_tokens` | cost | transcript | ↓ output tokens — the clean cost signal (session `result.usage.output_tokens`, or summed over assistant turns for a subagent log) |
| `cache_read_tokens` | cost | transcript | input tokens served from cache (context — explains the gap between `input_tokens` and fresh input) |
| `cost_usd` | cost | derived | ↓ **pure-API, no-cache, no-discount** cost = `input_tokens × $5/MTok + output_tokens × $25/MTok` (Opus standard rates; **not** the billed cost, which is far lower with caching). Comparable only when `input_tokens` shares an extraction basis |
| `wall_s` | cost | transcript | ↓ **total duration** — the whole wall-clock time waited end-to-end (thinking + API + local tool runs like `cargo test`/`code-ranker` + queue/rate-limit waits). Session `result.duration_ms`, or first→last event timestamp for a subagent log |
| `api_duration_s` | cost | transcript | ↓ the **API-only subset** of `wall_s` (active model time, `result.duration_api_ms`). `wall_s − api_duration_s` ≈ local tool execution + queueing. Blank when there's no session `result` event (subagent log) |
| `files_changed` | cost | diff | context — edit footprint (not better/worse alone) |
| `loc_added` / `loc_removed` | cost | PROJECT branch `git diff --shortstat` | precise edit footprint; a fix far larger than the reference's is a smell (also catches committed litter) |
| `read_doc_ai` / `read_doc_focus` | clarity | transcript | 1/0 — read `--doc AI` / `--doc <FOCUS>` |
| `doc_reread` | clarity | transcript | ↓ times a doc was read more than once (a re-read signals the prompt/doc wasn't clear the first time) |
| `planned_before_edit` | clarity | transcript | 1/0 — proposed a plan before editing |
| `used_generated_prompt` | adherence | transcript | 1/0 — actually fetched the tool's fix-prompt (`--output.prompt` / `--prompt`) vs improvising |
| `focus_framing` | adherence | transcript | which lens the agent chose — `ADP` (principle) or `cycle` (metric); reveals how it read the task |
| `first_edit_turn` | clarity | transcript | tool-call index of the first `Edit`/`Write` — very high = lots of exploration before acting (thoroughness, or an unclear prompt) |
| `clarifying_qs` | clarity | transcript | ↓ questions the prompt should have pre-answered |
| `discovery_retries` | clarity | transcript | ↓ failed tool calls (`is_error`) — dead ends the prompt could have prevented |
| `clarity_1_5` | clarity | transcript | ↑ read once, planned, no guessing/back-tracking |
| `verdict` | — | diff verdict | `improved` / `neutral` / `regressed` |
| `notes` | — | you | failure class, the lever changed, residual gap |

The objective columns (`focus_*`, `new_cycles`, `collateral_delta`, `tool_calls`, `commands`,
`output_tokens`, `loc_*`, retries, doc reads) are the hard signal; the two `*_1_5` judgments
are the qualitative "why" that drives the next prompt edit. `cost_usd` is a normalized
**no-cache** figure for cross-version comparison, deliberately *not* the billed amount —
caching/discounts are real-world noise that would make two prompt versions incomparable.
`results.md` stays the human narrative per build; `metrics.csv` is the machine-diffable
history across builds.

### Collecting a row

Don't hand-compute the objective columns — run the collector, which extracts them from
the run's artifacts and appends a row:

```sh
contrib/prompt-eval-metrics.py <prompt-eval>/<build>/<model>-<focus>-<n> \
  --focus <FOCUS> --project <name> --project-path <PROJECT> --base-branch main \
  --quality <1-5> --clarity <1-5> --collateral <Δ> --verdict improved --notes "…"
```

It reads `chat.jsonl` (tokens, durations, tool/command counts, doc reads + rereads,
`first_edit_turn`, `focus_framing`, `used_generated_prompt`, retries, and heuristic
`tests_pass` / `planned_before_edit`) and `before/after.json` (`focus_*`, `worst_*`,
`new_cycles`); with `--project-path` it adds `files_changed` / `loc_*` from the branch
diff; it derives `ts` / `cr_sha` / `model` / `iter` / `run` from the path and computes
`cost_usd`. Token extraction is **format-aware**: a full session log uses its
authoritative `result` usage; a subagent log sums per-turn (so its `input_tokens` /
`cost_usd` are cache-inflated and `api_duration_s` is blank). The **judged** columns —
`quality_1_5`, `clarity_1_5`, `collateral_delta`, `verdict`, `notes` — are flags (blank
if omitted; `collateral_delta` isn't auto-computed — it needs scorecards at two git
states, so compute it once and pass `--collateral`). `--dry-run` prints the row without
writing.

> **Run one mechanism per sweep.** `cost_usd` / `input_tokens` are only comparable when
> every run in the sweep was launched the same way (all interactive `claude`, or all
> subagent) — the two extraction bases don't line up. Don't mix them within a `FOCUS`.

### Scoring rubric — `quality_1_5` / `clarity_1_5`

The `*_1_5` columns are the only subjective signal, so pin them to a rubric or they
drift between sessions (an identical fix has already been scored 5 in one run and 4 in
another). Score against `MODEL_REF`'s run for the same `FOCUS`:

**`quality_1_5`** — is the fix real, and as good as the reference's?

- **5** — real structural fix (extract / invert / split, or the *correct minimal* fix
  for this violation); behaviour preserved, `new_cycles` 0, `collateral_delta` ≤ 0.
- **3–4** — correct and tests pass, but narrower/weaker than the reference, or leaves an
  obvious residual.
- **1–2** — silences the metric without fixing the structure, or needs follow-up to be
  correct.
- **0** — wrong, tests fail, or introduced a new cycle.

**`clarity_1_5`** — did the agent go straight to the fix, or grope?

- **5** — read each doc once, planned before editing, zero clarifying questions, zero
  failed/abandoned commands.
- subtract ~1 each for a `doc_reread`, a `discovery_retries` dead-end, a `clarifying_qs`,
  or a skipped plan — each is something a clearer prompt could have prevented.

When the rubric forces a judgement the columns can't capture, that's a signal to **add a
column** (the meta-loop), not to fudge the score.

## Tuning rule

A prompt change is justified when a cheaper model misses on **any** of the three
objectives in a way the prompt *could* have prevented:

- **quality** — it skipped the reference doc, picked the wrong cycle, or hacked the
  metric instead of extracting an abstraction;
- **cost** — it spent turns rediscovering what the prompt could have stated, or chased
  a dead end the prompt could have ruled out (`tool_calls` / `discovery_retries` high);
- **clarity** — it asked, back-tracked, or misread because the prompt buried the
  decision or ordered it confusingly (`clarifying_qs` high, `planned_before_edit` 0).

Map the miss to the **smallest** lever (principle `prompt` ⊂ scaffolding ⊂ the
`<FOCUS>` doc ⊂ — when the *process* is the problem — **this file**), change only
that, rebuild, re-sweep. Each edit is a hypothesis: the next run's `metrics.csv` row
must show the targeted column move, or the edit is reverted. Avoid over-fitting to one
project: a change should help the failure **class**, not memorise the repo.

Stop a tier after **3 iterations** even if not perfect — record the residual gap (the
row stays in `metrics.csv`) so it's a decision on record, not a silent failure.

## The meta-loop — improving this playbook

The prompts are levers; so is this file. After a sweep, ask whether the *process*
helped or fought you, and edit the playbook when it fought:

- a **run that taught nothing** (you couldn't tell *why* the fix scored as it did) →
  fix what a run captures, or add a metric column that would have shown it;
- a **signal that didn't discriminate** quality, cost, or clarity → sharpen the
  metric / its source;
- a **lever you couldn't locate**, or a change that helped but had no home above → fix
  "What we tune";
- a **missing or untraceable artifact** → fix the layout / naming.

Treat a playbook edit exactly like a prompt edit: it changes behaviour, so it gets its
own **S1–S2** (commit → new `CR_SHA`) and the next sweep runs under it. Log it in
`metrics.csv` / `results.md` with `focus = meta` so process changes are auditable
alongside prompt changes. The loop is done not when one prompt is perfect, but when
**neither the prompts nor this procedure** need another hand-correction.

## Results log

Track one row per run so the sweep is auditable:

| date | cr version+commit | PROJECT | FOCUS | MODEL | iter | branch | verdict (Δ) | tests | quality 1–5 | tokens | time (s) | notes / failure class |
|------|-------------------|---------|-------|-------|------|--------|-------------|-------|-------------|--------|----------|----------------------|
| … | 4.0.0-alpha.1 @abc123 | … | cycle | opus | 1 | opus-cycle-1 | improved (−2 cycles) | pass | 5 | 49.7k | 196 | reference |
| … | 4.0.0-alpha.1 @abc123 | … | cycle | sonnet | 1 | sonnet-cycle-1 | neutral (0) | pass | 2 | 88k | 310 | skipped `--doc`, hacked one edge |

`tokens` and `time (s)` are the cost axis at a glance (full breakdown —
`tool_calls`, `commands`, `input_tokens`, `output_tokens`, `wall_s` — lives in
`metrics.csv`); lower is better at equal quality.
