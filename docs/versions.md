# Versioning

Code Ranker tracks **three independent versions**, each guarding one compatibility
surface and bumped on its own criterion. They are deliberately separate: the app
ships often, the on-disk formats rarely. Keeping them apart means upgrading the app
does **not** force a config migration unless the config format itself changed.

| # | Surface | Constant | Lives in | Current |
|---|---------|----------|----------|---------|
| 1 | **app** — the release | `[workspace.package] version` (`env!("CARGO_PKG_VERSION")`) | root `Cargo.toml` | `4.0.0-alpha.1` |
| 2 | **config + CLI** — the user-facing input interface | `CONFIG_VERSION` | `crates/code-ranker-graph/src/version.rs` | `4.0` |
| 3 | **JSON snapshot + viewer** — the data format and its consumer | `SCHEMA_VERSION` | `crates/code-ranker-graph/src/version.rs` | `4.0` |

Versions 2 and 3 are the app **`major.minor`** of the release that last changed
that surface (so a reader can tell which app generation a config/snapshot targets).
They may share a value — both `4.0` today — but move independently. The number
lives **only** at its constant; every consumer imports it, never hardcodes it
(fixtures and data files included — fixtures `format!` it from the constant).

## 1. app version

Plain SemVer of the release (`4.0.0-alpha.1`). Bumped with `make bump VERSION=…`,
which rewrites `Cargo.toml`, `README.md` and the `code-ranker`/`--version` doc
mentions. Every normal release bumps it; it does **not** imply a format change.

## 2. config + CLI version — `CONFIG_VERSION`

Governs the **TOML config schema** and the **CLI surface** (flags / subcommands /
output). A `code-ranker.toml` **must** declare a matching `version`
(`config::CONFIG_SCHEMA_VERSION` aliases `CONFIG_VERSION`); `config::load` rejects a
mismatch with a directional hint (older → migrate the config, newer → upgrade the
tool) instead of a cryptic `unknown field` error.

**Bump when** the config schema or CLI surface changes incompatibly:

- **minor** — additive / backward-compatible (a new optional config key, a new flag);
- **major** — breaking (a renamed/removed config key, flag, subcommand, or a changed
  output contract).

Set it to the app `major.minor` of the shipping release. Then update `version = "…"`
in the root `code-ranker.toml`, every sample (`crates/.../tests/sample/code-ranker.toml`),
and doc examples.

## 3. JSON snapshot + viewer version — `SCHEMA_VERSION`

Governs the **JSON snapshot shape** and the **viewer** that reads it. Written as the
snapshot's `schema_version`; a snapshot read back as `--baseline`/input is rejected
on mismatch (`analyze.rs`), and the viewer rejects an incompatible swapped-in
snapshot (the renderer injects `window.SCHEMA_VERSION`, checked in
`snap-controls.js`).

**Bump when** the snapshot JSON changes incompatibly (a field added/renamed/removed,
or the viewer's read contract changes) — same **minor / major** rule as
`CONFIG_VERSION`, set to the app `major.minor` of the shipping release. Then
regenerate the e2e goldens (their `schema_version`).

## When to bump — branch discipline

A format version represents "the format as of release X". A single unmerged branch
becomes a single release, so it must move each format version **at most once** —
regardless of how many commits touch that format. The branch's net effect on a
surface is what matters, not each step along the way.

### Procedure (per surface, per branch)

1. **Detect a change.** Compare the branch against `main` for that surface:
   - **config + CLI** (`CONFIG_VERSION`) — did the `code-ranker.toml` schema or the
     clap flags / subcommands / output shape change? (`git diff main...HEAD` over
     `crates/code-ranker-cli/src/cli.rs`, `config/`, and the config docs.)
   - **JSON snapshot + viewer** (`SCHEMA_VERSION`) — did the snapshot JSON shape or
     the viewer's read contract change? (diff over `crates/code-ranker-graph/src/{snapshot,serialize}.rs`,
     the `node_attributes`/`edge_kinds`/… emitted, and `crates/code-ranker-viewer/`.)
   - No change to a surface → **do not** touch its version, even if the app bumped.
2. **Classify severity** (the *net* change vs `main`):
   - **minor** — additive / backward-compatible: a new **optional** config key, a
     new flag, a new optional JSON field. Old configs/snapshots still load.
   - **major** — breaking: a renamed/removed config key, flag, subcommand, JSON
     field, or a changed meaning/output contract. Old inputs no longer load.
3. **Bump once.** Set the constant to the app `major.minor` of the release this
   branch will ship as. Then propagate (see each surface's section above:
   `version = …` in configs / samples / doc examples; regenerate goldens'
   `schema_version`).

### Don't stack — escalate

If the surface was **already bumped earlier in this same branch** (vs `main`):

- another change of the **same or lower** severity → **no** new bump; it rides the
  existing one (a second additive tweak is still just one minor step for the
  release).
- a **breaking** change after a **minor** bump → **escalate**: replace the minor
  with a **major** (e.g. `4.0 → 4.1` becomes `4.0 → 5.0`). Never end with two
  separate bumps in one branch.

### Worked examples

- Branch adds one optional `[rules]` key → `CONFIG_VERSION` minor (`4.0 → 4.1`).
  `SCHEMA_VERSION` untouched (JSON unchanged).
- Same branch later renames a snapshot field → `SCHEMA_VERSION` **major**
  (`4.0 → 5.0`); the earlier `CONFIG_VERSION` minor stays as-is (different surface).
- Branch first adds an optional flag (`CONFIG_VERSION` `4.0 → 4.1`), then removes a
  different flag → the removal is breaking, so escalate **the same** bump to
  `CONFIG_VERSION` `4.0 → 5.0` (not a separate second bump).
- Branch only refactors internals / fixes a bug with no format change → bump
  **nothing** here (the app version still moves on release, per §1).

The `/update-docs` checklist runs this procedure as its format-compatibility step.
