//! The plugin registry — the single place that names concrete language plugins.
//! Everything else works only through the `LanguagePlugin` trait. Add a language
//! by writing a `code-split-plugin-<lang>` crate and adding one line to
//! [`registry`].

use anyhow::{Result, bail};
use code_split_plugin_api::{Graph, LanguagePlugin, Level, PluginInput};
use std::path::Path;

pub fn registry() -> Vec<Box<dyn LanguagePlugin>> {
    vec![
        Box::new(code_split_plugin_rust::RustPlugin),
        Box::new(code_split_plugin_python::PythonPlugin),
        Box::new(code_split_plugin_javascript::JavascriptPlugin),
        Box::new(code_split_plugin_typescript::TypescriptPlugin),
    ]
}

/// Comma-separated canonical plugin names, for help/error messages.
pub fn names() -> String {
    registry()
        .iter()
        .map(|p| p.name().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Parse the workspace with the named plugin at the `"files"` level, returning
/// the structural graph and the plugin's level descriptors.
pub fn analyze(name: &str, workspace: &Path, input: &PluginInput) -> Result<(Graph, Vec<Level>)> {
    let reg = registry();
    match reg.iter().find(|p| p.name() == name) {
        Some(p) => {
            let graph = p.analyze(workspace, "files", input)?;
            Ok((graph, p.levels()))
        }
        None => bail!("unknown plugin {name:?}; built-in plugins are: {}", names()),
    }
}

/// Tool/toolchain versions the matching plugin wants recorded in the snapshot.
pub fn versions(name: &str, workspace: &Path, input: &PluginInput) -> Vec<(String, String)> {
    registry()
        .iter()
        .find(|p| p.name() == name)
        .map(|p| p.versions(workspace, input))
        .unwrap_or_default()
}

/// Auto-detect the plugin from workspace markers. Errors if none or more than
/// one matches.
pub fn detect(workspace: &Path, input: &PluginInput) -> Result<String> {
    let reg = registry();
    let found: Vec<&str> = reg
        .iter()
        .filter(|p| p.detect(workspace, input))
        .map(|p| p.name())
        .collect();
    match found.as_slice() {
        [one] => Ok((*one).to_string()),
        [] => bail!(
            "could not auto-detect a plugin in {}: no project marker found — pass --plugin {}",
            workspace.display(),
            names()
        ),
        _ => bail!(
            "ambiguous project in {}: markers for multiple plugins found ({}) — pass --plugin to choose",
            workspace.display(),
            found.join(", ")
        ),
    }
}
