# code-ranker — Use Cases

A cookbook of real scenarios. **One scenario per entry, one exact command per entry** —
copy, adjust the path, run.

Two commands underlie everything:

- **`check`** — a **gate**. Evaluates the configured rules (`rules.thresholds`,
  `rules.cycles`), prints diagnostics, and **exits non-zero** on a violation. Writes no
  files. Thresholds come from `code-ranker.toml` (or `--threshold` overrides).
- **`report`** — produces **artifacts** (JSON snapshot, HTML viewer) and the **advisory**
  outputs (`scorecard` triage, `prompt` for an AI). The advisory tiers (`warn` / `info`)
  are driven by the **same `[plugins.<lang>.rules.thresholds.file]` limits the gate enforces** — `warn` is the
  gate line, `info` an optional softer line below it — so the report shows what fails (or is
  about to fail) `check`. Always exits `0`.

`[input]` is polymorphic: a directory is analyzed; a `.json`/`.html` snapshot is read back
with no re-analysis.

Ranking metrics used below (the `--focus` metric or principle that narrows the scorecard): `hk`
(Henry-Kafura coupling), `cycle` (dependency cycles — the ADP view), `sloc` (module size),
`cognitive` / `cyclomatic` (complexity), `fan_in` / `fan_out` (coupling direction),
`items` (interface size).

The **`--prompt <ID>`** fix-prompt is **name-it-yourself**: you pass the principle or
metric (picked from the scorecard) and it prints the fix-prompt to stdout.

---

## 1. Triage — where do I start?

**See the full triage across every principle.**

```sh
code-ranker report . --output.scorecard
```

**See only the 5 worst modules overall.**

```sh
code-ranker report . --output.scorecard --top 5
```

**Triage one metric — Henry-Kafura coupling.**

```sh
code-ranker report . --output.scorecard --focus hk
```

**Find the single worst HK module to fix first.**

```sh
code-ranker report . --output.scorecard --focus hk --top 1
```

**Find the single worst dependency cycle.**

```sh
code-ranker report . --output.scorecard --focus cycle --top 1
```

**Triage the biggest files (module size).**

```sh
code-ranker report . --output.scorecard --focus sloc --top 5
```

**Triage the most cognitively complex files.**

```sh
code-ranker report . --output.scorecard --focus cognitive --top 5
```

**Triage one subtree — scope the ranking to a folder.**

```sh
code-ranker report . --output.scorecard --focus hk --focus-path crates/code-ranker-cli/src/
```

**Show only warning-tier breaches (hide info-tier noise).**

```sh
code-ranker report . --output.scorecard --severity warning
```

---

## 2. Fix one thing at a time

**Snapshot the BEFORE state, so you can diff after the fix.**

```sh
code-ranker report . --output.json.path=.code-ranker/before.json
```

**After the fix, render a BEFORE→AFTER diff as an HTML viewer.**

```sh
code-ranker report . --baseline .code-ranker/before.json --output.html.path=.code-ranker/after.html
```

**Get a copy-paste AI fix-prompt for a named principle/metric (pick it from the scorecard).**

```sh
code-ranker report . --prompt HK --top 1
```

---

## 3. CI gate (pass/fail)

**Fail the build on any configured violation.**

```sh
code-ranker check .
```

**Gate a single coupling budget (Henry-Kafura).**

```sh
code-ranker check . --threshold file.hk=200000
```

**Gate maximum file size.**

```sh
code-ranker check . --threshold file.loc=800
```

**Gate cognitive complexity per file.**

```sh
code-ranker check . --threshold file.cognitive=25
```

**Freeze the cycle count: forbid an 8th chain cycle (allow today's 7).**

```sh
code-ranker check . --cycle-rule chain=7
```

**Report violations but keep the exit code 0 (non-blocking job).**

```sh
code-ranker check . --exit-zero
```

**Print only the single worst violation (hand one fix to a human or AI).**

```sh
code-ranker check . --top 1
```

---

## 4. Focused checks — gate a subset of files or rules

> The whole project is always analyzed (the dependency graph needs it); `--focus-path`
> / `--focus` only restrict what is reported and counted toward the exit code.

**Gate only the file you are refactoring.**

```sh
code-ranker check . --focus-path crates/code-ranker-plugin-api/src/plugin.rs
```

**Gate only one subsystem/folder.**

```sh
code-ranker check . --focus-path crates/code-ranker-cli/src/
```

**Gate two specific paths at once.**

```sh
code-ranker check . --focus-path crates/a/src/lib.rs --focus-path crates/b/src/
```

**Gate only the files changed in this PR (vs `main`).**

```sh
code-ranker check . $(git diff --name-only origin/main | sed 's/^/--focus-path /')
```

**List only one rule's / group's violations.**

```sh
code-ranker check . --focus check.inline_tests_too_large   # or: --focus TST
```

**Intersect a rule with a folder.**

```sh
code-ranker check . --focus-path crates/code-ranker-graph --focus TST
```

**Combine a focused scope with a metric budget.**

```sh
code-ranker check . --focus-path crates/code-ranker-plugin-api/src/plugin.rs --threshold file.hk=200000
```

---

## 5. Baselines & regressions

**Fail only on NEW violations vs a committed baseline (tolerate pre-existing ones).**

```sh
code-ranker check . --baseline .code-ranker/baseline.json
```

**Produce the baseline snapshot the gate above compares against.**

```sh
code-ranker report . --output.json.path=.code-ranker/baseline.json
```

**Pin today's measured numbers as a ready-to-paste passing config.**

```sh
code-ranker check . --suggest-config
```

---

## 6. Machine-readable output

**Emit violations as JSON for custom tooling.**

```sh
code-ranker check . --output-format json
```

**Emit GitHub Actions PR annotations.**

```sh
code-ranker check . --output-format github
```

**Emit SARIF to stdout for GitHub code scanning.**

```sh
code-ranker check . --output-format sarif
```

**Emit a GitLab Code Quality report to stdout.**

```sh
code-ranker check . --output-format codequality
```

**Gate and, on failure, print a Markdown AI fix-prompt for the violations (agent loop).**

```sh
code-ranker check . --output-format prompt
```

**Write SARIF as a file artifact (instead of stdout).**

```sh
code-ranker report . --output.sarif.path=.code-ranker/report.sarif
```

**Write the GitLab Code Quality artifact for `artifacts:reports:codequality`.**

```sh
code-ranker report . --output.codequality.path=gl-code-quality.json
```

---

## 7. Artifacts (snapshot & viewer)

**Generate the default artifacts (JSON snapshot + HTML viewer).**

```sh
code-ranker report .
```

**Write only the HTML viewer to a fixed path.**

```sh
code-ranker report . --output.html.path=.code-ranker/viewer.html
```

**Write only the JSON snapshot to a fixed path.**

```sh
code-ranker report . --output.json.path=.code-ranker/snap.json
```

**Render a diff viewer between two states.**

```sh
code-ranker report . --baseline .code-ranker/before.json --output.html
```

---

## 8. AI prompts

Name the principle or metric with `--prompt <ID>` (pick it from the scorecard). The
prompt is printed to stdout.

**Emit the fix-prompt to stdout (for an agent to read on a failed gate).**

```sh
code-ranker report . --prompt HK --top 1
```

**Save the fix-prompt to a file (redirect stdout).**

```sh
code-ranker report . --prompt HK > prompt.md
```

---

## 9. Configuration & thresholds

**Override one threshold inline without editing the config file.**

```sh
code-ranker check . --threshold file.cognitive=25
```

**Override a config value inline via `--config KEY=VALUE`.**

```sh
code-ranker check . --config plugins.base.rules.thresholds.file.hk=200000
```

**Layer an extra config file on top of the project's (last wins).**

```sh
code-ranker check . --config ci/strict.toml
```

**Ignore generated or vendored paths.**

```sh
code-ranker check . --ignore "**/generated/**"
```

**Dump the full effective configuration (defaults ⊕ overrides) and exit.**

```sh
code-ranker report . --export-full-config .code-ranker/effective.toml
```

---

## 10. Other languages & reading snapshots

**Analyze a Python project with a complexity budget.**

```sh
code-ranker check ./api --plugins python --threshold file.cognitive=25
```

**Triage a JavaScript/TypeScript project.**

```sh
code-ranker report ./web --plugins js --output.scorecard
```

**Analyze several languages in one run (every language present is auto-detected).**

```sh
code-ranker report .
```

**Restrict a mixed repo to exactly two languages.**

```sh
code-ranker check . --plugins rust,markdown
```

**Triage one language in a multi-language report.**

```sh
code-ranker report . --language rust --output.scorecard
```

**Re-check an existing snapshot with no re-analysis (fast, offline).**

```sh
code-ranker check .code-ranker/snap.json --threshold file.hk=200000
```

**Triage straight from an existing HTML report (no re-analysis).**

```sh
code-ranker report .code-ranker/viewer.html --output.scorecard
```

---

## 11. CI metadata overrides (detached checkout)

**Map a clean branch name (a detached CI checkout reports `HEAD`).**

```sh
code-ranker report . --git.branch="$CI_COMMIT_REF_NAME"
```

**Use the CI project URL for source links (avoid a token-bearing clone URL).**

```sh
code-ranker report . --git.origin="$CI_PROJECT_URL"
```

**Ignore the untracked files a CI job created before analysis.**

```sh
code-ranker check . --git.dirty-files=0
```
