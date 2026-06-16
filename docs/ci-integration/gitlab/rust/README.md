# Running code-ranker in GitLab CI — Rust projects

This guide shows two ways to wire `code-ranker` into a GitLab pipeline. The
examples target **Rust** projects (the binary is pulled in via `cargo install`);
other languages follow the same shape in their sibling folders under
`ci-integration/gitlab/`.

| Mode | What it does | Needs a token? | Reference file |
|---|---|---|---|
| **Minimal** | JSON snapshot + HTML viewer (artifacts) on every run, then `check`s the snapshot — emitting a Code Quality report GitLab shows inline in the MR, and gating on any violation (absolute). | No | [`minimal.example.yml`](./minimal.example.yml) |
| **Diff** | Same, but on an MR compares against the **target branch**: HTML diff + verdict, and the `check` gate is **relative** (only NEW violations count). | Yes (read-only) | [`diff.example.yml`](./diff.example.yml) |

Both modes keep the same downloadable artifacts (`code-ranker-<hash>.json` and
`code-ranker-<hash>.html`) plus a Code Quality report for native GitLab findings
(see [Native findings via Code Quality](#native-findings-via-code-quality)).
Both ship as a **soft gate** (`allow_failure: true`) — a violation marks the job
failed-but-allowed and the pipeline continues; delete `allow_failure` for a hard
gate. Pick Minimal to start; add the diff wiring once you want per-MR regression
diffs. The two reference files are drop-in jobs — copy one into your
`.gitlab-ci.yml` and
adjust the `image`.

---

## Prerequisite: get the binary onto PATH

`code-ranker` is a single binary. Make it available to the job in whichever way
fits your setup:

- **Bake it into your CI image** (recommended for repeated runs) — add
  `RUN cargo install code-ranker --locked` to your Dockerfile, or copy a prebuilt
  binary from a GitHub Release into the image.
- **Install it per-job** — add `cargo install code-ranker --locked` to a
  `before_script`.

`code-ranker` makes no network calls of its own. The only network access it
*initiates* is the optional baseline fetch from the GitLab API (diff mode only).

**Rust is the exception** — see [the cargo dependency cache](#rust-the-cargo-dependency-cache)
below. The Rust plugin shells out to `cargo metadata` under the hood, and that
command resolves the full dependency graph, which can hit the network on a cold
cache. This caveat is Rust-only; other languages (e.g. Python) run no such
sub-command and need none of this.

---

## Rust: the cargo dependency cache

> **Rust projects only.** Skip this section entirely for Python, JS/TS, or any
> other language — they invoke no cargo sub-command and have no such dependency.

The Rust plugin analyzes the workspace by running **`cargo metadata`** under the
hood. That command resolves the project's *full transitive dependency graph* —
which means cargo must have, locally:

- the **registry index** (the catalogue of crate versions), and
- every dependency's **source**, fetched into `$CARGO_HOME/registry/` —
  including any **private git dependencies** (cloned via your token).

On a warm cache (e.g. your own machine after a build) this is instant. On a
**cold CI runner** cargo has to download all of the above over the network
first, which can turn a sub-5-second analysis into **minutes** — the time is
spent entirely in the dependency fetch, not in code-ranker itself.

### What this means for your pipeline

- **The job needs a working `cargo` and network/credentials to resolve deps**,
  exactly like a build or test job does. If `cargo metadata` can't resolve the
  graph (missing token for a private git dep, no registry access), the Rust
  analysis fails — so make sure cargo works in the job before adding code-ranker.
- **Reuse the cache you already have.** Most Rust pipelines already warm
  `$CARGO_HOME` (or a mounted cache volume) for their **build and test** jobs.
  If you place the code-ranker job **right next to those jobs** — same base Rust
  image, same cache — `cargo metadata` reads everything from disk and the
  analysis stays fast. You almost never need to set up caching *for* code-ranker;
  you just need to run it where the cache already exists.
- **Recommended placement:** keep code-ranker in the `test` stage, alongside your
  other test/lint/validator jobs, on the same base Rust image that those jobs
  use. That image is what carries the dependency cache (and the
  `git config … insteadOf` token wiring for private git deps), so reusing it is
  what keeps the analysis cheap.

If you run code-ranker on a bare image with no cargo cache, expect the first run
to be slow while it populates `$CARGO_HOME` — that cost is cargo's, not
code-ranker's, and it disappears once the cache is reused.

---

## Mode 1 — Minimal (analyze + gate + native findings)

**Reference:** [`minimal.example.yml`](./minimal.example.yml)

The job:

1. `code-ranker report .` → a JSON snapshot + a self-contained HTML viewer (kept
   as downloadable artifacts, named by commit hash).
2. `code-ranker check "<snapshot>.json" --output-format codequality > gl-code-quality-report.json`
   — re-read the snapshot (no second analysis), evaluate the rules, and write the
   findings as a GitLab **Code Quality** report. This `check` is also the gate: it
   **exits non-zero** on a violation.
3. Hand the Code Quality report to GitLab as `artifacts:reports:codequality`
   (`when: always`, so it's uploaded even when the gate fails).

### Native findings via Code Quality

The JSON snapshot and HTML viewer are **downloadable** artifacts — a reviewer
opens the HTML. To surface the findings **inside GitLab** — inline on the MR diff
and in the pipeline **Code Quality** widget — the `check` step writes a Code
Quality (CodeClimate) report registered as `artifacts:reports:codequality`. Each
violation becomes an issue with a stable `fingerprint` (keyed on `rule:location`,
no line) so GitLab tracks the same finding across pipelines and shows only what a
merge request *adds*.

> **This is GA — it works on current GitLab with no feature flag.** Because the
> report comes from `check`, the one step both highlights the findings and gates.

**Soft vs hard gate.** The reference job sets `allow_failure: true`, so a
violation marks the job failed-but-allowed (yellow) and the pipeline continues —
findings still highlighted. **Delete `allow_failure`** to make a violation
**block** the pipeline. Either way the report is written (`artifacts: when:
always`). Tune what gates in `code-ranker.toml` (`[rules.thresholds.file]` /
`[rules.cycles]`) or with flags on the `check` line.

**SARIF alternative.** `check` also emits SARIF (`--output-format sarif`), which
GitLab ingests into its *security* views — but only on **GitLab ≥ 18.11 with the
`sarif_ingestion` feature flag enabled** (off by default; an admin turns it on).
On older instances prefer Code Quality. Check your instance version with
`echo "$CI_SERVER_VERSION"` in a job, or `glab api version`.

The Code Quality report uses a **fixed** filename (`gl-code-quality-report.json`,
not `-<hash>`-named) because `artifacts:reports:` is resolved when the pipeline
config is parsed, before the script computes the hash.

---

## Mode 2 — Diff (for merge requests)

**Reference:** [`diff.example.yml`](./diff.example.yml)

On a merge request this mode renders the HTML as a **baseline ↔ current diff**:
baseline = the code-ranker snapshot from the **target branch**, current = the MR's
code. The report then carries a verdict (improved / degraded / neutral) and
highlights added/removed/affected nodes. On the default branch, or before any
baseline exists, it falls back to a plain review report.

The flow inside the job:

1. Analyze → `code-ranker-<hash>.json` (same as minimal mode).
2. **Fetch the baseline** from the target branch (best-effort, see below).
3. Render HTML: `--baseline <fetched.json>` if found, otherwise a review report.
4. **Gate + Code Quality:** `check` the snapshot and write the
   `gl-code-quality-report.json` GitLab shows inline. With a baseline the gate is
   **relative** — only violations NEW vs the target branch count, so pre-existing
   debt never fails the MR and the report lists just the new ones; without one it
   falls back to an absolute check. Soft gate by default (`allow_failure: true`);
   delete that line to block on a new violation.

### Why the baseline fetch looks the way it does

The obvious endpoint — `GET /projects/:id/jobs/artifacts/:ref/download?job=…` —
is **not** used, for two reasons learned the hard way:

- **Detached MR pipelines are invisible to it.** Many projects run merge-request
  pipelines whose ref is `refs/merge-requests/N/head`, not the branch name.
  `artifacts/:ref/download` only matches **branch** pipelines, so it returns 404
  even when the MR pipeline succeeded and has the artifact.
- **Branch pipelines often never reach `success`.** A manually triggered branch
  pipeline tends to get `canceled` on later deploy stages, and the by-ref
  endpoint only serves artifacts from a fully **successful** pipeline.

So the job uses the **pipelines API** instead, which sees MR pipelines and only
cares that the `code-ranker` **job** is green:

```
GET /projects/:id/pipelines?ref=<target>&status=success&per_page=1   -> pipeline id
GET /projects/:id/pipelines/<pid>/jobs?per_page=100                  -> code-ranker job id
GET /projects/:id/jobs/<jid>/artifacts                               -> the artifact zip
```

Everything is logged (the URL, the HTTP code, the archive contents) and guarded
so a 404 or a no-match only downgrades to a review report — the job never fails.

### Why you need a token (`CODERANKER_API_TOKEN`)

The built-in `CI_JOB_TOKEN` **cannot reach the pipelines/jobs list API** — it
returns `404 Project Not Found`. This is by design (a job token is not an API
token) and is **not** fixable via the project's Token Access allowlist (that
setting governs cross-project token use, not access to the pipelines API).

So the baseline fetch needs a real `read_api` token, supplied via a CI/CD
variable named `CODERANKER_API_TOKEN`. If the variable is absent, the job falls
back to `CI_JOB_TOKEN` and simply produces a review report (no diff) — it still
won't fail.

### Setting up the token

You can scope the token to a **single project** or to a **whole group** of
projects. It's always a **bot token** (GitLab creates a bot user — it acts as
that bot, not as you), never your personal access token.

**One token for a group of projects — recommended if you have more than one Rust
service.** Create a **Group Access Token** and store it as a **group-level** CI/CD
variable: every project under that group inherits it automatically, so you set it
up **once** and all current and future services in the group just work. This
requires the **Owner** role on the group. If you don't have Owner, either ask a
group Owner to create it, or use a per-project token (below) — Maintainer on the
project is enough for that.

**One token for a single project.** Create a **Project Access Token** and store
it as a project CI/CD variable. Needs the **Maintainer** role on the project.

Steps (same for both — pick the group or project scope as you go):

1. **Create the token.**
   *Group (one for all projects):* `Group → Settings → Access Tokens → Add new token`.
   *Project (single):* `Project → Settings → Access Tokens → Add new token`.
   - **Role:** `Reporter` (enough to read pipelines/jobs/artifacts of a private project)
   - **Scopes:** `read_api` only
   - **Expiration:** your call (tokens can be rotated; pick a date and renew).
   - Copy the `glpat-…` value — it's shown **once**. GitLab provisions a bot user
     (`group_<id>_bot_…` or `project_<id>_bot_…`) that owns it.

2. **Store it as a CI/CD variable.**
   *Group-level* (`Group → Settings → CI/CD → Variables`) to share it across every
   project in the group, or *project-level* (`Project → Settings → CI/CD →
   Variables`) for a single project:
   - **Key:** `CODERANKER_API_TOKEN`
   - **Value:** the `glpat-…` token
   - **Masked:** ✅ on
   - **Protected:** ❌ **off** — this is critical. A *Protected* variable is only
     exposed to pipelines on protected branches/tags. MR pipelines run on feature
     branches, so a protected variable arrives **empty** there and the fetch
     silently falls back to 404 / review. Keep it Masked but not Protected.

The token lives only in the CI/CD variable — never commit it to the repository.
A group-level variable + group access token is the least-effort setup: one token,
one variable, every Rust project in the group covered.

---

## How a successful diff run looks in the log

When a baseline is found, the job's last lines are:

```
baseline from <target>: base/code-ranker-<hash>.json
html-report=code-ranker-<hash>-diff.html
Job succeeded
```

The `-diff` suffix on the HTML name confirms code-ranker built a comparison. When
no baseline is pulled you'll instead see `no baseline on <target> -> review
report` and a plain `code-ranker-<hash>.html` — the job still succeeds.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| HTML is a review report, not a diff (`no baseline on <target> -> review report` in the log) | No `read_api` token reached the job, **or** no successful pipeline on the target branch yet, **or** its artifact has no `code-ranker-*.json` | Set `CODERANKER_API_TOKEN` (see below); make sure the target branch has run code-ranker successfully at least once |
| Token is set but the report is still a review | The variable is **Protected** (MR pipelines on feature branches don't receive it) or isn't visible at this scope | Make the variable **Masked but not Protected**, at project or group scope |
| `cargo metadata` / analysis errors on a cold runner | Missing dependencies or credentials for `cargo` | Run code-ranker where cargo already works — same image/cache as your build/test jobs (see the cargo cache section) |
| Snapshot shows `"branch": "HEAD"`, a wrong commit, an inflated `dirty_files`, or a token-bearing `origin` | CI's detached checkout and job-written files mangle the raw `git` view | Map CI variables onto the `--git.*` flags (already wired in both reference files) — see [`docs/code-ranker-cli/CLI.md` → Git metadata overrides](../../../code-ranker-cli/CLI.md#git-metadata-overrides) |
