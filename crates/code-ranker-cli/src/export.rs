//! `--export-full-config` — dump the fully-resolved configuration as one TOML
//! document with two top-level sections:
//!   - `[project]` — the merged project config: the built-in defaults
//!     (`config/defaults.toml`) ⊕ the discovered / `--config` project config;
//!   - `[plugin]`  — the active plugin's merged language config: its inheritance
//!     chain `defaults.toml ⊕ [base] ⊕ <lang>.toml`.
//!
//! Diagnostic: it shows EVERY effective parameter so a user can see what they may
//! override. The two sections use different schemas (and the project / plugin
//! `presets` shapes collide), so the file is a human-facing dump, not directly
//! reusable as a single `--config`.

use crate::cli::AnalyzeArgs;
use crate::{config, logger, plugin};
use anyhow::{Context, Result};
use std::path::Path;

/// Write the full effective configuration for `args` (`--plugin` / `--config` /
/// input) to `out`, then return — no analysis runs.
pub(crate) fn export_full_config(args: &AnalyzeArgs, out: &Path) -> Result<()> {
    // The workspace the config / plugin are resolved against. A best-effort
    // canonicalize keeps auto-detection working for a relative input.
    let workspace = args
        .input
        .canonicalize()
        .unwrap_or_else(|_| args.input.clone());

    // Project config exactly as analysis resolves it (built-in defaults ⊕ the
    // discovered / `--config` file). The merged raw table is `[project]`.
    let loaded = config::load(&workspace, &args.config, &args.ignore_paths, &[], &[])
        .context("configuration error")?;

    // The active plugin (explicit `--plugin` > config `plugin` > marker detection)
    // and its fully-merged language config table → `[plugin]`. Resolved through the
    // self-registered registry + the `LanguagePlugin::config` trait method — the CLI
    // never names a language.
    let plugin_name = plugin::resolve_plugin(
        args.plugin.as_deref(),
        loaded.config.plugin.as_deref(),
        &workspace,
    )?;
    let plugin_table = plugin::registry()
        .into_iter()
        .find(|p| p.name() == plugin_name)
        .with_context(|| {
            format!(
                "unknown plugin {plugin_name:?}; built-in plugins are: {}",
                crate::plugin::names()
            )
        })?
        .config();

    let mut doc = toml::Table::new();
    doc.insert("project".into(), toml::Value::Table(loaded.merged));
    doc.insert("plugin".into(), toml::Value::Table(plugin_table));
    let body = toml::to_string_pretty(&doc).context("serializing full config")?;

    let header = format!(
        "# code-ranker — full effective configuration (diagnostic dump).\n\
         # [project] = built-in defaults ⊕ {project_src}\n\
         # [plugin]  = merged config for plugin `{plugin_name}`\n\
         # Two schemas in one file: a human-facing view of every effective\n\
         # parameter, not directly reusable as a single --config.\n\n",
        project_src = loaded
            .source_file
            .as_deref()
            .unwrap_or("built-in defaults (no project config file found)"),
    );

    std::fs::write(out, format!("{header}{body}"))
        .with_context(|| format!("writing {}", out.display()))?;
    logger::info(&format!(
        "✓ wrote full config for plugin `{plugin_name}` to {}",
        out.display()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::AnalyzeArgs;
    use std::path::PathBuf;

    fn args(input: PathBuf, plugin: &str, config: Vec<String>) -> AnalyzeArgs {
        AnalyzeArgs {
            input,
            plugin: Some(plugin.to_string()),
            config,
            ignore_paths: Vec::new(),
            git_branch: None,
            git_commit: None,
            git_dirty_files: None,
            git_origin: None,
        }
    }

    #[test]
    fn export_writes_project_and_plugin_sections_with_merge() {
        let dir = tempfile::tempdir().unwrap();
        // A partial project config: one override; everything else inherits defaults.
        let cfg = dir.path().join("code-ranker.toml");
        std::fs::write(&cfg, "[ignore]\ntests = false\n").unwrap();
        let out = dir.path().join("full.toml");

        export_full_config(
            &args(
                dir.path().to_path_buf(),
                "python",
                vec![cfg.display().to_string()],
            ),
            &out,
        )
        .unwrap();

        let doc: toml::Table = std::fs::read_to_string(&out).unwrap().parse().unwrap();
        let project = doc["project"].as_table().unwrap();
        let plugin = doc["plugin"].as_table().unwrap();

        // [project]: the override wins, the rest is inherited from the built-in defaults.
        assert_eq!(project["ignore"]["tests"].as_bool(), Some(false));
        assert_eq!(project["ignore"]["gitignore"].as_bool(), Some(true));
        assert!(project["output"]["json"]["path"].as_str().is_some());
        // [plugin]: the resolved python language config (its presets catalog).
        assert_eq!(plugin["doc_lang"].as_str(), Some("python"));
        assert!(!plugin["presets"].as_array().unwrap().is_empty());
    }

    #[test]
    fn export_errors_on_unknown_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("full.toml");
        let err = export_full_config(&args(dir.path().to_path_buf(), "klingon", Vec::new()), &out)
            .unwrap_err()
            .to_string();
        assert!(err.contains("klingon"), "names the bad plugin: {err}");
    }
}
