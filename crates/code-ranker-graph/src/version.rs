//! Project format-version constants — the single home for the versions every
//! compatibility check shares. There are **three** independent versions, each
//! guarding one surface and bumped on its own criterion (see `docs/versions.md`):
//!
//! 1. **app** — the release version, Cargo's `[workspace.package] version`
//!    (`env!("CARGO_PKG_VERSION")`). Not defined here.
//! 2. **config + CLI** — [`CONFIG_VERSION`].
//! 3. **JSON snapshot + viewer** — [`SCHEMA_VERSION`].
//!
//! (2) and (3) are `major.minor` of the app release that last changed that surface.
//! They may share a value (both `"4.0"` today) but move independently. The number
//! lives ONLY here — every consumer imports it, never hardcodes it.

/// The **config + CLI** format version. A `code-ranker.toml` must declare a
/// matching `version` (checked in `config::load`); the CLI surface (flags /
/// subcommands / output) is documented against the same number.
///
/// **Bump when** the TOML config schema or the CLI surface changes — a **minor**
/// for an additive/back-compatible change (new optional key or flag), a **major**
/// for a breaking one (renamed/removed key, flag or section). Set it to the app
/// `major.minor` of the release that ships the change.
pub const CONFIG_VERSION: &str = "5.0";

/// The **JSON snapshot + viewer** format version. Written as the snapshot's
/// `schema_version`, rejected on mismatch when a snapshot is read back
/// (`analyze.rs`), and checked in the browser on a snapshot swap (injected as
/// `window.SCHEMA_VERSION`).
///
/// **Bump when** the snapshot JSON shape changes (a field added/renamed/removed,
/// or the viewer's read contract changes) — same minor/major rule as
/// [`CONFIG_VERSION`], set to the app `major.minor` of the shipping release.
pub const SCHEMA_VERSION: &str = "5.0";
