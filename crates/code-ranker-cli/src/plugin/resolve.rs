//! Plugin **selection policy**: building each language's effective config,
//! auto-detecting languages, resolving the active set (console > config >
//! auto-detect), and the one-file-one-language extension-uniqueness guard. This is
//! orchestration over the registry, kept apart from the thin per-plugin method
//! dispatch in [`super`] so neither concern carries the other's dependencies.

use super::registry;
use anyhow::{Result, anyhow, bail};
use code_ranker_graph::version::CONFIG_VERSION;
use code_ranker_plugin_api::{plugin::PluginInput, toml_merge::deep_merge};
use std::collections::BTreeMap;
use std::path::Path;

/// Resolve a user-supplied language token — a canonical plugin `name()` **or** one
/// of a plugin's declared `aliases` (e.g. `js` → `javascript`) — to the canonical
/// name. `None` when it matches neither. Aliases are read from each plugin's static
/// config (`aliases = [...]` in its `config.toml`); a canonical name wins outright.
pub fn canonical_name(token: &str) -> Option<String> {
    let reg = registry();
    if let Some(p) = reg.iter().find(|p| p.name() == token) {
        return Some(p.name().to_string());
    }
    reg.iter()
        .find(|p| {
            toml_string_list(&p.config(), "aliases")
                .iter()
                .any(|a| a == token)
        })
        .map(|p| p.name().to_string())
}

/// Resolve an alias to its canonical name, leaving an unknown token untouched so a
/// downstream lookup reports it as unknown with the proper hint. Idempotent on an
/// already-canonical name.
pub fn to_canonical(token: &str) -> String {
    canonical_name(token).unwrap_or_else(|| token.to_string())
}

/// Canonical names with their aliases, for "unknown language" error hints —
/// e.g. `c, cpp (c++, cxx), … , javascript (js), …` (sorted).
pub fn names_with_aliases() -> String {
    let mut entries: Vec<String> = registry()
        .iter()
        .map(|p| {
            let aliases = toml_string_list(&p.config(), "aliases");
            if aliases.is_empty() {
                p.name().to_string()
            } else {
                format!("{} ({})", p.name(), aliases.join(", "))
            }
        })
        .collect();
    entries.sort_unstable();
    entries.join(", ")
}

/// Read a top-level string array from a TOML table (e.g. `extensions = ["rs"]`).
/// Returns an empty `Vec` when the key is absent or is not a string array.
fn toml_string_list(cfg: &toml::Table, key: &str) -> Vec<String> {
    cfg.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Build the effective plugin config for `name`:
///   static plugin base (`plugin.config()`)
///   ⊕ user `[languages.base]`
///   ⊕ user `[languages.<name>]`
///
/// `lang_overrides` is `Config.languages` — the raw per-language tables already
/// carrying any `--config languages.*` edits. The merge uses `deep_merge` from
/// `code_ranker_plugin_api`, matching the rest of the config pipeline.
pub fn effective_plugin_config(
    name: &str,
    lang_overrides: &BTreeMap<String, toml::Table>,
) -> toml::Table {
    let base_cfg = registry()
        .iter()
        .find(|p| p.name() == name)
        .map(|p| p.config())
        .unwrap_or_default();

    // A `[plugins.<lang>]` block carries BOTH plugin-config keys (extensions,
    // detect_markers, node_attributes, …) and orchestrator sections
    // (ignore/rules/metrics/levels/report/principles). Only the former belong in the
    // plugin's effective config; the orchestrator sections are read separately via
    // `Config::language_config`, and some (`principles`) even have a conflicting
    // shape here (the plugin's own `[[principles]]` is an array, the project's
    // `[principles.<ID>]` a table), so merging them in would corrupt the plugin config.
    let plugin_keys = |block: &toml::Table| -> toml::Table {
        block
            .iter()
            .filter(|(k, _)| !crate::config::model::LANG_SECTION_KEYS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };

    let mut acc = base_cfg;
    for key in ["base", name] {
        if let Some(block) = lang_overrides.get(key) {
            acc = deep_merge(acc, plugin_keys(block));
        }
    }
    acc
}

/// All plugins whose `detect()` returns `true` under their effective config;
/// sorted alphabetically. Multiple matches are NORMAL (e.g. Rust + Markdown).
///
/// `eff_cfgs` maps each registered plugin name to its pre-built effective config
/// (call `effective_plugin_config` for each registered plugin beforehand).
pub fn detect_all(
    eff_cfgs: &BTreeMap<String, toml::Table>,
    workspace: &Path,
    input: &PluginInput,
) -> Vec<String> {
    let reg = registry();
    let mut found: Vec<String> = reg
        .iter()
        .filter(|p| {
            let cfg = eff_cfgs
                .get(p.name())
                .map(|t| t as &toml::Table)
                .unwrap_or(&EMPTY_TABLE);
            p.detect(cfg, workspace, input)
        })
        .map(|p| p.name().to_string())
        .collect();
    found.sort_unstable();
    found
}

/// An empty TOML table used as the fallback effective config when none is present.
static EMPTY_TABLE: std::sync::LazyLock<toml::Table> = std::sync::LazyLock::new(toml::Table::new);

/// Resolve the active plugins.
///
/// Precedence (low → high; each level fully REPLACES the one below it):
///   1. auto-detect (`detect_all`) — used only when neither config nor console pin the list.
///   2. config `plugins` — replaces auto-detect.
///   3. console `--plugins` (`arg`) — replaces config.
///
/// An empty `detect_all` result (no markers found) → `Err` with a zero-detect message.
pub fn resolve_plugins(
    arg: &[String],
    cfg_plugins: &[String],
    eff_cfgs: &BTreeMap<String, toml::Table>,
    workspace: &Path,
    input: &PluginInput,
    config_file: Option<&str>,
) -> Result<Vec<String>> {
    // Console wins outright. Resolve aliases (`js` → `javascript`) so the active
    // set — and therefore the snapshot keys — are always canonical.
    if !arg.is_empty() {
        return Ok(arg.iter().map(|t| to_canonical(t)).collect());
    }
    // Config wins over auto-detect.
    if !cfg_plugins.is_empty() {
        return Ok(cfg_plugins.iter().map(|t| to_canonical(t)).collect());
    }
    // Auto-detect: error on empty result.
    let detected = detect_all(eff_cfgs, workspace, input);
    if detected.is_empty() {
        let e = anyhow!(
            "could not auto-detect any language in {}: no project markers found \
             (for C/C++ projects, no source files with the expected extensions were found)",
            workspace.display()
        );
        return Err(with_config_hint(e, config_file));
    }
    Ok(detected)
}

/// Startup guard: for the ACTIVE plugins, build a map `extension → [plugin names]`
/// from their effective configs; any extension claimed by more than one plugin is
/// an error (a file would be analysed twice, breaking the one-file-one-language
/// invariant).
pub fn validate_extension_uniqueness(
    active: &[String],
    eff_cfgs: &BTreeMap<String, toml::Table>,
) -> Result<()> {
    let mut ext_owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in active {
        let cfg = eff_cfgs
            .get(name)
            .map(|t| t as &toml::Table)
            .unwrap_or(&EMPTY_TABLE);
        // Read the `extensions` list from the effective config (same key the
        // plugins use in their own TOML).
        let extensions = toml_string_list(cfg, "extensions");
        for ext in extensions {
            ext_owners.entry(ext).or_default().push(name.clone());
        }
    }
    let conflicts: Vec<String> = ext_owners
        .iter()
        .filter(|(_, owners)| owners.len() > 1)
        .map(|(ext, owners)| format!(".{ext} claimed by: {}", owners.join(", ")))
        .collect();
    if !conflicts.is_empty() {
        bail!(
            "extension conflict between active plugins — a file would be analysed by multiple \
             languages (breaking the one-file-one-language invariant):\n  {}\n\
             Fix: adjust `extensions` in `[languages.<lang>]` or restrict `plugins = [\"...\"]`.",
            conflicts.join("\n  ")
        );
    }
    Ok(())
}

/// Augment a failed-detection error with how to pin the languages in config.
fn with_config_hint(e: anyhow::Error, config_file: Option<&str>) -> anyhow::Error {
    let how = match config_file {
        Some(path) => format!(
            "add `plugins = [\"<name>\"]` to {path} \
             (run `code-ranker docs` for a list of built-in plugins)"
        ),
        None => format!(
            "create a `code-ranker.toml` at the project root with:\n\
             \tversion = \"{CONFIG_VERSION}\"\n\
             \tplugins = [\"<name>\"]"
        ),
    };
    anyhow!("{e}\n  → or pin the language in config: {how}")
}
