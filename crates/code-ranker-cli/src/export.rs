//! `--export-full-config` — dump the fully-resolved configuration as one TOML
//! document:
//!   - `[project]` — the merged project config: the built-in defaults
//!     (`config/defaults.toml`) ⊕ the discovered / `--config` project config;
//!   - `[languages.<lang>]` — one section per EVERY registered language plugin,
//!     showing the full effective config for that language (its inheritance chain
//!     `defaults.toml ⊕ [base] ⊕ <lang>.toml`). A diagnostic reference dump of
//!     every parameter the user may override per-language.
//!
//! The file is human-facing (not strictly re-importable as a single `--config`).

use crate::cli::AnalyzeArgs;
use crate::{config, logger, plugin};
use anyhow::{Context, Result};
use std::path::Path;

/// Write the full effective configuration for `args` (`--plugins` / `--config` /
/// input) to `out`, then return — no analysis runs.
pub(crate) fn export_full_config(args: &AnalyzeArgs, out: &Path) -> Result<()> {
    // The workspace the config / plugins are resolved against. A best-effort
    // canonicalize keeps auto-detection working for a relative input.
    let workspace = args
        .input
        .canonicalize()
        .unwrap_or_else(|_| args.input.clone());

    // Project config exactly as analysis resolves it (built-in defaults ⊕ the
    // discovered / `--config` file). The merged raw table is `[project]`.
    let loaded = config::load(&workspace, &args.config, &args.ignore_paths, &[], &[])
        .context("configuration error")?;

    // Every registered language plugin's static base config → `[languages.<lang>]`.
    // This is the reference view of all overridable keys, not just the active plugins.
    let reg = plugin::registry();
    let mut languages = toml::Table::new();
    for p in &reg {
        let lang_table = p.config();
        languages.insert(p.name().to_string(), toml::Value::Table(lang_table));
    }

    let project_src = loaded
        .source_file
        .as_deref()
        .unwrap_or("built-in defaults (no project config file found)");

    let mut doc = toml::Table::new();
    doc.insert("project".into(), toml::Value::Table(loaded.merged));
    doc.insert("languages".into(), toml::Value::Table(languages));
    let body = toml::to_string_pretty(&doc).context("serializing full config")?;

    let header = format!(
        "# code-ranker — full effective configuration (diagnostic dump).\n\
         # [project]             = built-in defaults ⊕ {project_src}\n\
         # [languages.<lang>]    = static base config for each registered language\n\
         #                         (every key you can override in [languages.<lang>])\n\
         # Human-facing reference dump; not directly reusable as a single --config.\n\n"
    );

    std::fs::write(out, format!("{header}{body}"))
        .with_context(|| format!("writing {}", out.display()))?;
    let lang_names: Vec<&str> = reg.iter().map(|p| p.name()).collect();
    logger::summary(&format!(
        "✓ wrote full config ({} languages: {}) to {}",
        lang_names.len(),
        lang_names.join(", "),
        out.display()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::AnalyzeArgs;
    use std::path::PathBuf;

    fn args(input: PathBuf, config: Vec<String>) -> AnalyzeArgs {
        AnalyzeArgs {
            input,
            plugins: Vec::new(),
            config,
            ignore_paths: Vec::new(),
            git_branch: None,
            git_commit: None,
            git_dirty_files: None,
            git_origin: None,
        }
    }

    #[test]
    fn export_writes_project_and_languages_sections() {
        let dir = tempfile::tempdir().unwrap();
        // A partial project config: one override; everything else inherits defaults.
        let cfg = dir.path().join("code-ranker.toml");
        std::fs::write(
            &cfg,
            format!(
                "version = \"{}\"\n[plugins.base.ignore]\ntests = false\n",
                code_ranker_graph::version::CONFIG_VERSION
            ),
        )
        .unwrap();
        let out = dir.path().join("full.toml");

        export_full_config(
            &args(dir.path().to_path_buf(), vec![cfg.display().to_string()]),
            &out,
        )
        .unwrap();

        let doc: toml::Table = std::fs::read_to_string(&out).unwrap().parse().unwrap();
        let project = doc["project"].as_table().unwrap();
        let languages = doc["languages"].as_table().unwrap();

        // [project]: the override wins, the rest is inherited from the built-in
        // defaults. The per-language sections live under `plugins.base`.
        let base = &project["plugins"]["base"];
        assert_eq!(base["ignore"]["tests"].as_bool(), Some(false));
        assert_eq!(base["ignore"]["gitignore"].as_bool(), Some(true));
        assert!(project["output"]["json"]["path"].as_str().is_some());
        // [languages]: every registered language is present.
        assert!(!languages.is_empty(), "at least one language registered");
        // Python language config has its doc_lang field.
        if let Some(py) = languages.get("python") {
            assert_eq!(py["doc_lang"].as_str(), Some("python"));
            assert!(!py["principles"].as_array().unwrap().is_empty());
        }
    }
}
