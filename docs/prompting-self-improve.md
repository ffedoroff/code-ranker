# Prompt self-improvement loop

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

```
for MODEL in models (most → least capable):        # opus, then sonnet, then haiku…
  loop (≤ 3 times):
    R = run(PROJECT, FOCUS, MODEL)                 # one clean-context fix (below)
    save artifacts(R)
    score R against MODEL_REF's best run for FOCUS
    if R ≈ reference quality:
      break                                        # this tier is good — lock it
    else:
      tune a prompt lever to address R's failure
      rebuild (S1–S2)
  # descend to the next cheaper model and re-verify with the improved prompt
```

End state: the **cheapest** tier still produces reference-quality fixes for `FOCUS`.
Then repeat `improve(...)` for the next `FOCUS`.

## A single run — `run(PROJECT, FOCUS, MODEL)`

Let `RUN=<code-ranker>/.code-ranker/prompt-eval/<timestamp>_<CR_SHA>/<MODEL>-<FOCUS>-<N>`
— an **absolute** path into *this* repo's `.code-ranker/` (create it first). The
agent runs `code-ranker report .` inside `PROJECT`, but every `--output.*.path`
points at `$RUN`, so the evidence lands in code-ranker, not `PROJECT`.

1. **Clean start.** `PROJECT` on `main`, working tree clean.
2. **Fresh agent session**, model = `MODEL`, **empty context**. Bootstrap it with the
   offline playbook only — no extra hints: have it read
   `code-ranker report --doc AI` (overview + catalog) and `--doc <FOCUS>` (the deep
   doc). This is what a real user would do, so it tests the *prompt*, not your
   coaching.
3. **BEFORE.** `code-ranker report . --output.html.path=$RUN/before.html --output.json.path=$RUN/before.json`.
4. **Fix.** Save the focused prompt and hand it to the agent:
   `code-ranker report . --output.prompt.path=$RUN/prompt.md --focus <FOCUS> --top 1`.
   The agent proposes the plan, applies the fix, runs the project's tests.
5. **AFTER + DIFF.** `code-ranker report . --baseline $RUN/before.json --output.html.path=$RUN/diff.html --output.json.path=$RUN/after.json` (+ an `after.html`).
6. **Save the transcript** to `$RUN/chat.md` (see "Saving the chat"), commit the code
   change to branch `<MODEL>-<FOCUS>-<N>` in `PROJECT`, return to `main`.

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

## Comparison & scoring

Score each cheaper-model run against `MODEL_REF`'s run for the same `FOCUS`:

| Signal | Source | Question |
|---|---|---|
| **Correctness** | project tests | Tests pass, behaviour preserved? |
| **FOCUS reduced** | `diff.json` verdict + metric delta | Fewer cycles / lower HK / …? (objective) |
| **Structural quality** | transcript + diff | A real fix (extract / invert / split), not a hack to silence the metric? |
| **Followed the prompt** | transcript | Read the doc, proposed before changing, took before/after reports? |
| **Cost** | transcript | Turns / tokens to get there. |

The diff verdict + delta are the **objective** signal; the transcript is the
**qualitative** "why" that drives the prompt change.

## Tuning rule

A prompt change is justified only when a cheaper model fails in a way the prompt
*could* have prevented — e.g. it skipped the reference doc, picked the wrong cycle,
or hacked the metric instead of extracting an abstraction. Map the failure to the
**smallest** lever (principle `prompt` ⊂ scaffolding ⊂ the `<FOCUS>` doc), change
only that, rebuild, re-sweep. Avoid over-fitting to one project: a change should
help the failure class, not memorise the repo.

Stop a tier after **3 iterations** even if not perfect — record the residual gap so
it's a decision on record, not a silent failure.

## Results log

Track one row per run so the sweep is auditable:

| date | cr version+commit | PROJECT | FOCUS | MODEL | iter | branch | verdict (Δ) | tests | quality 1–5 | notes / failure class |
|------|-------------------|---------|-------|-------|------|--------|-------------|-------|-------------|----------------------|
| … | 4.0.0-alpha.1 @abc123 | … | cycle | opus | 1 | opus-cycle-1 | improved (−2 cycles) | pass | 5 | reference |
| … | 4.0.0-alpha.1 @abc123 | … | cycle | sonnet | 1 | sonnet-cycle-1 | neutral (0) | pass | 2 | skipped `--doc`, hacked one edge |
