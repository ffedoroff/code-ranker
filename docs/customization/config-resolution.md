# Config resolution & overrides — the full precedence ladder

Where does a setting come from, and who wins when two layers disagree? This page
documents the **complete inheritance order** of every config layer code-ranker
reads, plus the **console (CLI flag) overrides** that ride on top of them.

There are two independent resolution chains:

- **Project config** — the `code-ranker.toml` that tunes *your* run (thresholds,
  cycles, custom metrics, checks, report views, output paths). Resolved at runtime.
- **Language config** — the `<lang>.toml` that defines a *plugin's* vocabulary and
  principles. Resolved at compile time, inside the binary.

Both layer with the **same deep-merge primitive**
([`code_ranker_plugin_api::toml_merge::deep_merge`](../../crates/code-ranker-plugin-api/src/toml_merge.rs)),
so the per-key rules below are identical for both. For the merge mechanics
(table-vs-table recursion, `[[principles]]`-by-`id`, the list-op DSL) see
[the merge semantics](#merge-semantics) section. For *what each key means*, see
the [customization guide](README.md).

---

## 1. Project config — the precedence ladder

When `check` / `report` analyzes a source tree, the effective project config is
built bottom-up. **Later layers override earlier ones, key by key** — a layer only
needs to spell out what it changes; everything else is inherited.

```
  ┌─────────────────────────────────────────────────────────────────┐
  │ 5. CLI value-flags        --cycle-rule / --threshold / --ignore   │  highest
  │                           --git.* / --output.*                    │
  ├─────────────────────────────────────────────────────────────────┤
  │ 4. Inline --config KEY=VALUE   (allowlisted keys only — see §3)   │
  ├─────────────────────────────────────────────────────────────────┤
  │ 3. Discovered / explicit config file(s)   (deep-merged, §2)       │
  ├─────────────────────────────────────────────────────────────────┤
  │ 2. (no separate layer — the file layer IS the user's input)       │
  ├─────────────────────────────────────────────────────────────────┤
  │ 1. Built-in defaults.toml   (embedded in the binary)              │  base
  └─────────────────────────────────────────────────────────────────┘
```

Implemented in
[`config/load.rs::load`](../../crates/code-ranker-cli/src/config/load.rs).

### Layer 1 — built-in defaults (the merge base)

[`crates/code-ranker-cli/src/config/defaults.toml`](../../crates/code-ranker-cli/src/config/defaults.toml)
is **embedded into the binary** (`include_str!`) and is the **single source of
every default value** — no default is hardcoded in Rust. It is always complete,
so even with no config file at all the run has a full config.

What it carries today:

| Section | Default |
|---|---|
| `[plugins]` | `enabled` unset / empty → auto-detect every language present |
| `[plugins.base.ignore]` | `paths = []`, `tests = true`, `dev_only_crates = false`, `gitignore/ignore_files/hidden = true` |
| `[plugins.base.rules.cycles]` | `mutual = true`, `chain = true` (strict) |
| `[plugins.base.rules.thresholds.file]` | empty — **no per-file limits by default** |
| `[output.json]` / `[output.html]` | on, path `.code-ranker/{ts}-{git-hash-3}.{ext}` |
| `[output.sarif]` / `[output.codequality]` | off (written only on demand) |
| `[output.scorecard]` | path default `stdout`; off unless requested |
| `[plugins.base.levels]` | `functions = false` (only the `files` level is emitted) |

### Layer 3 — the discovered or explicit config file(s)

The user's `code-ranker.toml` is read as a raw table and **deep-merged over the
built-in defaults**. How the file(s) are located:

**A. Explicit `--config FILE` (one or more).** Every file is read and merged
**in command-line order, later wins**. Auto-discovery is skipped entirely.

```bash
# base.toml first, then strict.toml overrides only the keys it names
code-ranker check --config base.toml --config strict.toml
```

**B. Auto-discovery (no `--config FILE`).** The **first** of these that exists
becomes the single file layer (it stops at the first hit):

1. `./code-ranker.toml` (current working directory)
2. `<workspace>/code-ranker.toml`
3. `./Cargo.toml` → `[workspace.metadata.code-ranker]` or `[package.metadata.code-ranker]`
4. `<workspace>/Cargo.toml` → same metadata sections

**C. Nothing found** → pure built-in defaults.

The resolved source is printed at startup (`config: <path>`, or
`config: built-in defaults (no config file found)`). With multiple `--config`
files the log shows the merge order: `a.toml ⊕ b.toml`.

> Because the file layer goes through `deep_merge`, the [list-op DSL](#merge-semantics)
> (`{add,remove,replace,clear,prepend}`) composes across file layers too — a later
> `--config` file can *patch* a list an earlier one set, not just replace it.

### The per-language overlay — `[plugins.<lang>]` and `[plugins.base]`

A project's `code-ranker.toml` can override **any** key of a language's built-in
config — not just `[metrics]` — through two per-language layers:

- **`[plugins.base]`** — a **virtual** base language. It is not a real plugin;
  its overrides apply to **every** active language as a shared base.
- **`[plugins.<lang>]`** — overrides for one specific language. It wins over
  `[plugins.base]`.

Either block overrides any key the language's TOML carries (`extensions`,
`detect_markers`, `skip_dirs`, `edge_kinds`, `node_attributes`, `[[principles]]`,
`metrics`, `levels`, `ignore`, `rules`, `report`, …), deep-merged onto that
language's effective config with the [same merge semantics](#merge-semantics) (the
list-op DSL composes here too). The **effective per-language plugin config**
therefore resolves low→high:

```
  defaults.toml  ⊕  [family base.toml]  ⊕  <lang>.toml      (built-in, embedded)
      ⊕  [plugins.base]  ⊕  [plugins.<lang>]                (user, project config)
      ⊕  --config plugins.base.*  ⊕  --config plugins.<lang>.*   (CLI)
```

So the built-in language chain (§4) is the base, the user's `[plugins.base]`
then `[plugins.<lang>]` overlay it, and the matching inline `--config` flags ride
highest. An overridden `detect_markers` / `extensions` feeds back into
auto-detection — a language is auto-detected against its **effective** config.

### Layers 4 & 5 — the transient per-run flag overrides

After the file layers are merged and deserialized into the `Config`, two more
override passes run **on the live config object** (not on the TOML table):

- **Layer 4 — inline `--config KEY=VALUE`** (applied after *all* files).
- **Layer 5 — the dedicated value-flags** `--cycle-rule`, `--threshold`,
  `--ignore`, `--git.*`, `--output.*`.

These are **transient**: they affect this run only and are **not** part of the
merged table that `--export-full-config` dumps (that shows layers 1–3 only).

---

## 2. Console overrides — flag ↔ config-key map

Every CLI flag below overrides the corresponding TOML key for the current run.
`--ignore` and `--config ignore.paths=` **extend** the list; everything else
**replaces** the value.

| Console flag | Overrides TOML key | Notes |
|---|---|---|
| `--plugins <a,b,…>` (comma-separated / repeatable) | `[plugins] enabled` | the active-language list; replaces config `[plugins].enabled` and beats auto-detection |
| `--language <name>` | *(no TOML key)* | `report` / `recommend` only — picks which single language the scorecard + prompt focus on; required only when a `--prompt <ID>` / `--focus` resolves in 2+ languages |
| `--config FILE` (repeatable) | *(whole file layer)* | layered in CLI order, later wins; skips auto-discovery |
| `--config KEY=VALUE` (repeatable) | the named key | allowlisted keys + `plugins.<lang>.<key>` — see §3 |
| `--ignore GLOB` (repeatable) | `[plugins.base.ignore] paths` | **appends** to the configured globs |
| `--cycle-rule KIND=on\|off\|N` | `[plugins.base.rules.cycles] mutual\|chain` | `on`/`0` = strict, `off` = disabled, `N` = allow up to N |
| `--threshold file.METRIC=N` | `[plugins.base.rules.thresholds.file] METRIC` | `N` accepts `_` separators + `K/M/G` suffixes |
| `--output.json` / `--output.html` / `--output.sarif` / `--output.codequality` | `[output.<fmt>] enabled` | forces that format on (`report` only) |
| `--output.<fmt>.path PATH` | `[output.<fmt>] path` | overrides the filename template |
| `--output.scorecard` | `[output.scorecard]` | turns the recommendation output on |
| `--output.scorecard.path` | `[output.scorecard] path` | defaults to `stdout` |
| `--git.branch` / `--git.commit` / `--git.dirty-files` / `--git.origin` | *(snapshot metadata — no TOML key)* | CI escape hatch; replaces what `git` would report |

Flags with **no TOML equivalent** (they shape this run's output, not the config):
`--baseline`, `--prompt`, `--focus-path`, `--focus`, `--output-format`, `--top`,
`--exit-zero`, `--suggest-config`, `--severity`, `--export-full-config`.

Full flag reference: [CLI.md](../code-ranker-cli/CLI.md).

---

## 3. Inline `--config KEY=VALUE` — the allowlist

Inline overrides are **not** arbitrary TOML paths — they are matched against a
fixed allowlist in
[`config/load/overrides.rs`](../../crates/code-ranker-cli/src/config/load/overrides.rs).
An unrecognized key is a hard error (`unknown config key …`). Anything outside
this list (e.g. a custom `[plugins.base.metrics.<key>]`, a `[plugins.base.report]`
view, a `[plugins.base.rules.checks]`)
**must go in a file** — there is no inline form for it.

| Inline key | Type | Effect |
|---|---|---|
| `plugins.enabled` | csv | set the active-language list (same as `--plugins`) |
| `plugins.<lang>.<key>` | scalar / csv | override any plugin key for one language (or `plugins.base.<key>` for the shared base); scalars and comma-lists only — deep nested tables / arrays-of-tables need the `[plugins.<lang>]` TOML block |
| `plugins.base.ignore.tests` (alias `plugins.base.ignore.test_modules`) | on/off | drop test files |
| `plugins.base.ignore.dev_only_crates` | on/off | (rust) drop dev-only dependency nodes |
| `plugins.base.ignore.gitignore` / `plugins.base.ignore.ignore_files` / `plugins.base.ignore.hidden` | on/off | walk filters |
| `plugins.base.ignore.paths` | csv | **append** comma-separated globs |
| `plugins.base.rules.cycles.<kind>` | on/off/N | same as `--cycle-rule <kind>=…` |
| `plugins.base.rules.thresholds.file.<metric>` | number | same as `--threshold file.<metric>=…` |
| `output.json.path` / `output.html.path` | string | output template |
| `output.json.enabled` / `output.html.enabled` | on/off | force format on/off |

```bash
# inline equivalents — no file needed
code-ranker check --config plugins.base.rules.thresholds.file.sloc=800 \
                  --config plugins.base.rules.cycles.chain=7 \
                  --config plugins.base.ignore.tests=off
```

A threshold key is validated *after* the full config (including any file
`[metrics]`) is known, so a custom metric is accepted while a typo still fails
fast (`unknown threshold metric …`).

---

## 4. Language config — the inheritance chain

Each language plugin's config is assembled at **compile time** from `include_str!`'d
TOML, deep-merged in this order (later overrides earlier), in
[`plugins/src/config/parse.rs`](../../crates/code-ranker-plugins/src/config/parse.rs):

```
  defaults.toml  ⊕  [family base.toml]  ⊕  <lang>.toml
  (common base)     (optional)             (the language)
```

- **`defaults.toml`** —
  [`crates/code-ranker-plugins/src/defaults.toml`](../../crates/code-ranker-plugins/src/defaults.toml).
  The language-neutral base: the common `[[principles]]` catalog, `doc_base`, the
  field-omission `[defaults]`, and the one-value-each `[ids]` / `[visibility]` /
  `[edges]` vocab every language shares.
- **Family base (optional)** — a language in a family inherits one extra layer:
  JavaScript & TypeScript inherit `ecmascript/config.toml`; C & C++ inherit
  `cfamily/config.toml`. So `js/config.toml` carries only what differs from the
  shared engine vocab.
- **`<lang>.toml`** — the language's own node-kind tables (`[kinds]`, `[halstead]`,
  `[loc]`), its `doc_lang` / `doc_overrides`, and any principle additions/overrides.

A standalone language passes `&[lang_toml]`; a family member passes
`&[base_lang_toml, lang_toml]`. The `[[principles]]` array merges **by `id`** (a
language principle replaces a same-`id` base principle, a new `id` appends) — see below.

### Principle-doc resolution — the `base/` corpus fallback

A principle's `doc_url` inherits from a shared corpus the same way config inherits
`defaults.toml`. It resolves (in [`specs.rs`](../../crates/code-ranker-plugins/src/config/specs.rs))
to `{doc_base}/{doc_lang}/{id}.md` for the ids a language **overrides**, and to
`{doc_base}/base/{id}.md` otherwise — `base/` is the language-neutral fallback
corpus under [`languages/`](../../languages/). Which ids a language overrides is
declared by `doc_overrides` in its `<lang>.toml`:

- `doc_overrides = "*"` — a full own corpus; every principle routes to the
  language's own folder (rust / python / typescript; javascript shares typescript).
- `doc_overrides = ["SRP", …]` — a partial corpus; only those ids route to the
  language's folder, the rest fall back to `base/`.
- absent — no own corpus; every `doc_url` resolves to `base/` (e.g. go, c, cpp,
  csharp, markdown).

So adding a language needs no doc authoring to get working links (it inherits
`base/`); a language ships its own docs incrementally by dropping files in
`languages/<lang>/` and listing their ids in `doc_overrides`.

Authoring a `<lang>.toml`, including the report list overrides, is covered in
[§2 of the customization guide](README.md#2-language-config-langtoml--for-plugin-authors).

---

## 5. Merge semantics

Both chains use the same `deep_merge(base, overlay)`. For each key of `overlay`:

| Case | Result |
|---|---|
| table vs table | **recurse** — per-key deep merge |
| `[[principles]]` arrays | merge **by `id`**: same `id` replaces in place, new `id` appends |
| array + op-table `{add,remove,replace,clear,prepend}` | inherited list **patched in place** (the [list-op DSL](README.md#21-the-list-override-dsl)) |
| array + plain array | overlay **replaces** wholesale |
| any scalar / type mismatch | overlay **replaces** the base value |
| key in one side only | kept as-is |

So the default for a list is **replace**; opt into patching with an op-table.
This is what lets a project's `--config strict.toml` add to a list set by
`base.toml`, and a `<lang>.toml` extend (rather than restate) the inherited
principle catalog or edge-kind vocab.

---

## 6. Inspecting the resolved config

Don't guess what won — dump it:

```bash
# layers 1–3 for [project] + the merged [plugins.<lang>] config (every language), to a file
code-ranker report . --export-full-config effective.toml
```

`--export-full-config` writes `[project]` (built-in defaults ⊕ `--config` files)
plus a `[plugins.<lang>]` block for **every registered language** (not only the
active ones — the merged effective config for each, including its
`[plugins.<lang>]` / `[plugins.base]` overlay). It does
**not** include the transient layer-4/5 flag overrides — those are per-run only.

See also: [customization guide](README.md) · [CLI reference](../code-ranker-cli/CLI.md).
