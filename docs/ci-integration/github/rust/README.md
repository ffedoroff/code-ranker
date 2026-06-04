# Running code-split in GitHub Actions — Rust projects

This guide shows two ways to wire `code-split` into a GitHub Actions workflow.
The examples target **Rust** projects (the binary is pulled in via
`cargo install`); other languages follow the same shape in their sibling folders
under `ci-integration/github/`. It mirrors the
[GitLab guide](../../gitlab/rust/README.md) — same two modes, same artifacts,
adapted to Actions' triggers and token model.

| Mode | What it does | Needs a secret? | Reference file |
|---|---|---|---|
| **Minimal** | Generates a JSON snapshot + HTML viewer on every run, kept as artifacts. Runs as an advisory linter. | No | [`minimal.example.yml`](./minimal.example.yml) |
| **Diff** | On a PR, compares the current code against the **base branch** and renders an HTML diff with a verdict. | No (built-in `GITHUB_TOKEN`) | [`diff.example.yml`](./diff.example.yml) |

Both modes upload the same artifacts (`code-split-<hash>.json` and
`code-split-<hash>.html`) and both run the job as **advisory**
(`continue-on-error: true`) so a failed analysis never fails the workflow. Pick
Minimal to start; add the diff wiring once you want per-PR regression diffs. The
two reference files are drop-in workflows — copy one into `.github/workflows/`
and adjust the `runs-on` / install step.

---

## Prerequisite: get the binary onto PATH

`code-split` is a single binary. Make it available to the job in whichever way
fits your setup:

- **Install it per-job** — add `cargo install code-split --locked` to a step.
  `ubuntu-latest` runners already ship a Rust toolchain, so this works out of the
  box (it's what the reference files do).
- **Bake it into a container image** (faster for repeated runs) — add
  `RUN cargo install code-split --locked` to the image you run the job in
  (`container:` / a self-hosted runner image), or copy a prebuilt binary from a
  GitHub Release.

`code-split` makes no network calls of its own. The only network access it
*initiates* is the optional baseline download in diff mode (via `gh`, below).

**Rust is the exception** — see [the cargo dependency cache](#rust-the-cargo-dependency-cache)
below. The Rust plugin shells out to `cargo metadata`, which resolves the full
dependency graph and can hit the network on a cold cache. This caveat is
Rust-only; other languages (e.g. Python) run no such sub-command and need none of
this.

---

## Rust: the cargo dependency cache

> **Rust projects only.** Skip this section entirely for Python, JS/TS, or any
> other language — they invoke no cargo sub-command and have no such dependency.

The Rust plugin analyzes the workspace by running **`cargo metadata`** under the
hood. That command resolves the project's *full transitive dependency graph* —
which means cargo must have, locally:

- the **registry index** (the catalogue of crate versions), and
- every dependency's **source**, fetched into `$CARGO_HOME/registry/` —
  including any **private git dependencies** (cloned via your credentials).

On a warm cache this is instant. On a **cold runner** cargo has to download all of
the above first, which can turn a sub-5-second analysis into **minutes** — the
time is spent entirely in the dependency fetch, not in code-split itself.

### What this means for your workflow

- **The job needs a working `cargo` and network/credentials to resolve deps**,
  exactly like a build or test job does. If `cargo metadata` can't resolve the
  graph (missing token for a private git dep, no registry access), the Rust
  analysis fails — so make sure cargo works in the job first.
- **Cache `$CARGO_HOME`.** Add a Rust cache step before the analysis — the
  de-facto standard is [`Swatinem/rust-cache@v2`](https://github.com/Swatinem/rust-cache),
  or roll your own with `actions/cache` over `~/.cargo` and `target/`. Run
  code-split **in the same job (or with the same cache key) as your build/test
  jobs** so `cargo metadata` reads everything from disk and the analysis stays
  fast.
- **Private git deps** need credentials in the job, the same as a build:
  configure `git config --global url."https://x-access-token:${TOKEN}@github.com/".insteadOf "https://github.com/"`
  (or an SSH deploy key) before the analysis runs.

If you run code-split on a bare runner with no cargo cache, expect the first run
to be slow while it populates `$CARGO_HOME` — that cost is cargo's, not
code-split's, and it disappears once the cache is reused.

---

## Mode 1 — Minimal (advisory linter + artifacts)

**Reference:** [`minimal.example.yml`](./minimal.example.yml)

The job does exactly two things:

1. `code-split report . --output.json.path=… --output.html.path=…` — analyze the
   workspace and emit both a JSON snapshot and a self-contained HTML viewer.
2. Upload both as an artifact (`if: always()`, so they survive even if a later
   step fails).

Artifacts inside the upload are named by commit hash
(`code-split-<hash>.json/.html`) so every run is identifiable and never collides.
The job is `continue-on-error: true` — it surfaces structure for reviewers but
never fails the workflow.

**Want a hard gate?** Use `code-split check .` instead — it evaluates thresholds
and cycle rules and **exits non-zero** on violation (it writes no files). Drop
`continue-on-error` on that job to make it actually block. The minimal reference
file includes a commented-out `code-split-gate` job showing this.

---

## Mode 2 — Diff (for pull requests)

**Reference:** [`diff.example.yml`](./diff.example.yml)

On a pull request this mode renders the HTML as a **baseline ↔ current diff**:
baseline = the code-split snapshot from the **base branch**, current = the PR's
code. The report then carries a verdict (improved / degraded / neutral) and
highlights added/removed/affected nodes. On a push to the default branch, or
before any baseline exists, it falls back to a plain review report.

The flow inside the job:

1. Analyze → `code-split-<hash>.json` (same as minimal mode).
2. **Download the baseline** from the latest successful run on the base branch
   (best-effort, see below).
3. Render HTML: `--baseline <downloaded.json>` if found, otherwise a review report.

### How the baseline download works — and why GitHub makes it easy

`actions/download-artifact` only sees artifacts from the **current** workflow run.
The baseline lives in a **previous** run (the last successful run on the base
branch), so the job downloads it with the pre-installed **`gh` CLI**:

```sh
gh run list --branch <base> --workflow <name> --status success --limit 1 --json databaseId
gh run download <run-id> --name code-split-report --dir base
```

The key difference from GitLab: the **built-in `GITHUB_TOKEN` can already read
this repo's workflow runs and download their artifacts** — you just grant the
workflow `permissions: actions: read`. **No personal or bot access token is
needed** for a same-repo diff (contrast GitLab, where `CI_JOB_TOKEN` can't reach
the pipelines API and you must provision a separate `read_api` token).

For the diff to find anything, the **base branch must have run this workflow and
uploaded the artifact at least once** — that's why the reference file also
triggers on `push` to the default branch (which produces a review-report run
whose artifact becomes the baseline for later PRs). Both modes upload under the
same artifact name (`code-split-report`) so `gh run download --name` matches.

Everything is best-effort and logged: a missing run, a 404, or no matching
artifact only downgrades to a review report — the job never fails (it is
`continue-on-error: true`).

### The fork-PR caveat

Pull requests opened **from a fork** run with a **read-only `GITHUB_TOKEN` and no
access to secrets**. The baseline download may therefore be restricted depending
on your org's Actions settings, and artifact upload from a fork PR can be
disabled. When that happens the diff simply falls back to a **review report** —
it never fails. (Do **not** reach for `pull_request_target` to work around this;
it runs untrusted PR code with a writable token and is a well-known security
footgun.)

---

## How a successful diff run looks in the log

When a baseline is found, the render step's last lines are:

```
baseline from <base>: base/code-split-<hash>.json
html-report=code-split-<hash>-diff.html
```

The `-diff` suffix on the HTML name confirms code-split built a comparison. When
no baseline is pulled you'll instead see `no baseline on <base> -> review report`
and a plain `code-split-<hash>.html` — the job still succeeds.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| HTML is a review report, not a diff (`no baseline on <base> -> review report` in the log) | No successful run on the base branch yet, **or** its artifact has no `code-split-*.json`, **or** the workflow lacks `actions: read` | Add `permissions: { contents: read, actions: read }`; make sure the base branch has run this workflow successfully at least once (the `push` trigger does this) |
| Diff never renders on PRs from forks | Fork PRs get a read-only token / no secrets, so the baseline download is restricted | Expected — it degrades to a review report. Do not use `pull_request_target` to force it |
| Snapshot shows `"branch": "HEAD"`, a merge-commit hash, an inflated `dirty_files`, or no/odd `origin` | Actions' detached checkout (and the merge ref on PRs) mangle the raw `git` view | Map Actions variables onto the `--git.*` flags (already wired in both reference files) — see [`docs/code-split-cli/CLI.md` → Git metadata overrides](../../../code-split-cli/CLI.md#git-metadata-overrides) |
| `cargo metadata` / analysis errors on a cold runner | Missing dependencies or credentials for `cargo` | Run code-split where cargo already works — cache `$CARGO_HOME` and wire private-git-dep credentials, same as your build/test jobs (see the cargo cache section) |
