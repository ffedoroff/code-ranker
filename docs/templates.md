# Templates — the docs corpus, prompt scaffolding & rendering

Everything code-ranker emits as **prose** — the principle/metric docs it links from
findings, and the AI fix-prompts the Prompt Generator builds — is **data, not
code**, assembled from a small set of templates. This page is the single reference
for that system: where the source lives, how layers compose, how the binary embeds
and overrides them, and the CLI surface that prints a prompt or a doc directly.

> **Status legend.** Each section is tagged:
> **✅ implemented** — shipped, verifiable in the current tree.
> **🔜 planned** — designed and agreed, not built yet.
> Don't treat 🔜 behaviour as available; it documents the target so the pieces
> land coherently.

---

## 1. The two template families

| Family | What it is | Source today | Rendered by |
|---|---|---|---|
| **Docs corpus** | per-principle / per-metric Markdown (`SRP.md`, `HK.md`, …) | `plugins/<lang>/<ID>.md` | embedded in the binary (§3) and addressed by `doc_url`; no longer served live (§5) |
| **Prompt scaffolding** | the framing prose around a principle in an AI prompt (intro, task protocol, …) | `code-ranker-graph/metrics/prompt.md` → `PromptTemplate` | `compose_prompt` (CLI), `composePrompt` (viewer) |

They are converging on **one composition engine** (§4) and one override mechanism
(§6), so this page covers both.

---

## 2. Docs corpus — layout & inheritance ✅/🔜

### 2.1 Resolution & the `base/` fallback ✅

A finding links its principle/metric doc via a principle's `doc_url`, resolved in
[`specs.rs`](../crates/code-ranker-plugins/src/config/specs.rs):

```
doc_url = {doc_base}/{doc_lang}/{id}.md   for the ids a language OVERRIDES
        = {doc_base}/base/{id}.md         otherwise  (the shared fallback corpus)
```

- `doc_base` — common, in plugins [`defaults.toml`](../crates/code-ranker-plugins/src/defaults.toml).
- `doc_lang` + `doc_overrides` — per `<lang>.toml`. `doc_overrides = "*"` → full own
  corpus (rust / python / typescript); `["SRP", …]` → only those ids; absent → every
  doc falls back to `base/` (go / c / cpp / csharp / markdown).

This is the doc analogue of the `defaults.toml ⊕ <lang>.toml` config inheritance.
See [config-resolution.md](customization/config-resolution.md#principle-doc-resolution--the-base-corpus-fallback).

### 2.2 Source split: `base/` + per-language manifests ◐ (mechanism live; migration 🔜)

A language doc avoids duplicating `base/`'s language-neutral content (theory,
algorithm, history, references) by being a **manifest** rather than a full copy:

```
plugins/
  base/<ID>.md     SOURCE — language-neutral content as `## ` sections; also the
                   served fallback for languages with no own corpus.
  <lang>/<ID>.md   Either a MANIFEST (assembled from base) or a full standalone doc.
```

A `<lang>/<ID>.md` is a **manifest** when it carries at least one `<!-- doc:base … -->`
include: it lists, in order, the sections of the final doc — each either pulled from
base by reference or written inline. It may write its own `#` H1 + TL;DR head (used
verbatim, so a language can word its own intro); without one the base head is
inherited, auto-suffixed `(in <Lang>)`. A file with no `doc:base` includes is a full
standalone doc, served verbatim (the migration is gradual: un-migrated full copies
keep working).

### 2.3 The manifest directives ✅

A manifest is an ordered sequence of base-section includes and inline sections:

| Form | Effect |
|---|---|
| `<!-- doc:base "Heading" -->` | include that whole `## Heading` section from base |
| `<!-- doc:base "Heading" from "P1" -->` | include the section text from phrase `P1` onward |
| `<!-- doc:base "Heading" to "P2" -->` | include it up to and including phrase `P2` |
| `<!-- doc:base "Heading" from "P1" to "P2" -->` | the slice `P1`..=`P2` |
| `## Inline heading` + body | a section written here verbatim (new, or a rewrite) |

Rules: **output order = manifest order** (a base section not referenced is simply
absent — the language owns the full structure); the H1 + preamble is inherited from
`base/` and auto-suffixed `(in <Lang>)`; a `\n` in a `from`/`to` phrase anchors it
to a line start; a `doc:base` naming a missing section — or a `from`/`to` phrase not
found — is a hard error.

Worked example — `plugins/rust/ADP.md`:

```markdown
<!-- doc:base "The principle" -->
<!-- doc:base "Why it matters" -->

## In Rust
Cargo models crates as a DAG; a dependency cycle between crates does not compile…

<!-- doc:base "Common cycle shapes" -->
<!-- doc:base "References" -->
```

`compose(rust/ADP.md manifest, base/ADP.md)` → the served `plugins/rust/ADP.md`.

---

## 3. Embedding in the binary ✅

The corpus is embedded into the binary at build time (dependency-free — a
[`build.rs`](../crates/code-ranker-cli/build.rs) walks `plugins/**/*.md` and
generates an `include_str!`-backed `CORPUS` slice; see
[`templates.rs`](../crates/code-ranker-cli/src/templates.rs)), so the tool can
**use the doc text itself**, not only link a URL:

- inline a principle / its `remediation` text into an AI prompt instead of
  "Download from `<url>`";
- print a doc directly from the CLI (§7);
- render a principle inline in the HTML viewer (it already bundles a Markdown
  renderer, `snarkdown`).

Because the fragments are embedded, composition (and any override, §6) happens at
**runtime** — which is also why the composer is Rust, not a build-time shell script.

---

## 4. The composition engine ✅

One Rust composer implements `compose(manifest, base)`, used in three places so the
logic exists exactly once:

1. **Runtime** — embed fragments, compose on demand for CLI/viewer output (§3).
2. **Prompt scaffolding** — the same section/`{key}` machinery renders the
   `PromptTemplate` (§8).

It builds on the existing `{key}` interpolation in
[`checks/text.rs`](../crates/code-ranker-graph/src/checks/text.rs) (`render_message`:
`{path}`, `{name}`, `{stem}`, plus any node attribute) — the most developed
substitution primitive already in the tree.

---

## 5. Publishing to GitHub Pages — removed

Corpus publishing to GitHub Pages (and the `code-ranker docs` subcommand that
composed the corpus to disk) has been **removed**. The corpus is no longer served
over a URL; it lives only **embedded in the binary** (§3) and is reached through
the `docs <lang> <ID>` command / inline prompt text. The Pages workflow still publishes the HTML
*report* (`report . → site/index.html`), but not the doc corpus, so a finding's
`doc_url` no longer resolves to a live page.

---

## 6. Per-file override — `[templates.…]` ✅

A user can substitute any single corpus fragment with their own file, and the binary
treats it **as if it were that file in `plugins/`**:

```toml
[templates.languages.base]
HK = "custom-hk.md"          # use ./custom-hk.md as plugins/base/HK.md
```

or inline on the command line:

```bash
code-ranker report . --config templates.languages.base.HK=custom-hk.md
```

- The key namespace is `[templates.languages.<lang>.<ID>]`; the value is a path to
  the user's file (relative to the config file / cwd).
- The override is at the **fragment** level (`base` or a `<lang>` manifest), then the
  normal composition (§2.3) runs on top — so overriding `base.HK` flows through into
  every language that inherits it.
- It layers through the same `deep_merge` as the rest of the config (built-in
  defaults ⊕ project `code-ranker.toml` ⊕ inline `--config`); see
  [config-resolution.md](customization/config-resolution.md). The CLI-inline form
  adds a `templates.` prefix branch alongside the existing `rules.thresholds.` one in
  [`config/load/overrides.rs`](../crates/code-ranker-cli/src/config/load/overrides.rs).
- Resolution per fragment: a `templates.languages.<lang>.<ID>` override is the
  **final doc** (served verbatim, no compose); a `templates.languages.base.<ID>`
  override substitutes the base fragment and composition (§2.3) runs on top, so it
  flows into every language that inherits it.

Because the override is applied **before** the snapshot is serialized, the HTML
viewer reflects it automatically (the viewer reads everything from the snapshot — it
needs no override logic of its own).

---

## 7. CLI — print a prompt or a doc directly

### 7.1 `--prompt <ID>` — print one principle's prompt ✅

A direct shortcut on `report`: compose the AI prompt for one principle/metric and
write it to **stdout**, then exit — no HTML/JSON artifacts.

```bash
# "give me the HK prompt" → prints it immediately
code-ranker report . --prompt HK

# narrow the ranked modules it lists
code-ranker report . --prompt HK --top 5 --focus-path src/engine
```

- `<ID>` is a principle id (`HK`, `ADP`, `SRP`, …) or a metric key; unknown ids fail
  with the known list (same validation as `compose_prompt`).
- It runs the normal analysis (the prompt lists the offending modules ranked by the
  principle's `sort_metric`), composes via the shared engine, and prints. `--top` and
  `--focus-path` refine the module list.
- With embedded docs (§3) it can inline the full principle text rather than only the
  link.
- It is the explicit, name-it-yourself, print-to-stdout path — the quick "show me HK"
  path — and (being a standalone dump) accepts any `--top N` to widen the ranked module
  list. Redirect to a file when you need an artifact: `code-ranker report . --prompt HK > prompt.md`.

### 7.2 `docs <lang> <subject>` — print the raw principle doc ✅

The `docs` command dumps the embedded principle/metric Markdown (composed for one
language, with any `[templates.…]` override applied), no analysis and no `[input]`. The
language is the **first positional argument** — there is no `--plugin` flag. Bare `docs`
lists available languages; `docs <lang>` lists that language's subject catalog:

```bash
code-ranker docs rust HK   # the resolved plugins/rust/HK.md
code-ranker docs base HK   # the language-agnostic base doc
```

### 7.3 Existing prompt surfaces ✅

- `report --prompt <ID>` — print the named principle/metric prompt to stdout.
- `check --output-format prompt` — build a prompt from the gate's own violations
  (`render_prompt`).
- HTML viewer Prompt Generator — interactive, builds the prompt around the user's
  live node selection (`composePrompt` in
  [`export-popup.js`](../crates/code-ranker-viewer/src/assets/export-popup.js)).

See the full flag reference in [code-ranker-cli/CLI.md](code-ranker-cli/CLI.md).

---

## 8. Prompt scaffolding (`PromptTemplate`)

### 8.1 The data ✅

The framing prose lives in [`metrics/prompt.md`](../crates/code-ranker-graph/metrics/prompt.md)
as Markdown `## <field>` sections (parsed by `prompt_template()` in `builtin.rs`; a
project may substitute its own via `prompt_template_from()`), and is carried in the
snapshot as [`PromptTemplate`](../crates/code-ranker-plugin-api/src/principle.rs) so the
CLI and the viewer render identical text from one source. Unlike the principle/metric
corpus, `prompt.md` is **internal template prose**: it sits next to `builtin.toml`
(not under `plugins/`) and is not a `<lang>/<ID>` doc.

| Field | Role |
|---|---|
| `intro` | one-line intent under the title |
| `doc_note` | how to read the full principle — points at the offline `code-ranker docs <lang> <id>` command (`{id}` substituted), not a network URL |
| `task` | the task-protocol bullets (`{id}` → active principle id) |
| `focus` | closing emphasis line |
| `cycle_note` | note prepended to a single dependency-cycle's module list |

### 8.2 The three render sites & the duplication problem ✅ → 🔜

The prompt's Markdown skeleton (`# title` / `## Summary` / `## Task` /
`## Modules ordered by` / `## Connections`) is currently hand-assembled in **three**
places that must stay in sync:

- `compose_prompt` — [`recommend/prompt.rs`](../crates/code-ranker-cli/src/recommend/prompt.rs) (CLI `prompt`)
- `render_prompt` — [`check.rs`](../crates/code-ranker-cli/src/check.rs) (gate `--output-format prompt`)
- `composePrompt` — [`export-popup.js`](../crates/code-ranker-viewer/src/assets/export-popup.js) (viewer)

🔜 **Plan:** the static **principle head** (title / intro / summary / doc-link / task
/ focus) is pre-rendered once by the shared engine (§4) and embedded in the snapshot;
the CLI and the viewer read that pre-rendered head and only append their own module /
edge lists. The viewer's lists must stay client-side (they depend on the user's live
selection in the export popup), but the skeleton stops being duplicated across Rust
and JS.

---

## 9. Status summary

| Piece | State |
|---|---|
| `doc_url` base-fallback resolution (`doc_overrides`) | ✅ |
| `base/` corpus + `doc_base = .../plugins` | ✅ |
| `PromptTemplate` prose in `metrics/prompt.md` (internal; parsed by `prompt_template`) | ✅ |
| `compose_prompt` / `render_prompt` / viewer `composePrompt` | ✅ |
| `check --output-format prompt` | ✅ |
| Embedding the corpus in the binary (`build.rs` → `CORPUS`) | ✅ |
| `[templates.languages.<lang>.<ID>]` per-file override | ✅ |
| `report --prompt <ID>` | ✅ |
| `docs <lang> <subject>` | ✅ |
| Manifest composer (`compose.rs`: `doc:base` + `from`/`to`) + `resolve_doc` wiring | ✅ |
| `code-ranker docs` build subcommand + corpus Pages publishing (Variant B) | ✗ removed — corpus is binary-embedded only, not served over a URL |
| `base/` + per-language manifest migration | ◐ all `rust/` docs migrated; `python`/`typescript` 🔜 |
| `doc_base` → Pages URL (activation) | ✗ dropped with corpus Pages publishing |
| Pre-render prompt head into the snapshot (de-dup Rust↔JS) | ✗ deferred — net-negative (bloats the snapshot to remove ~20 stable JS lines) |

See also: [customization/config-resolution.md](customization/config-resolution.md) ·
[customization/README.md](customization/README.md) · [code-ranker-cli/CLI.md](code-ranker-cli/CLI.md).
