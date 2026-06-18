//! Preset catalog, metric thresholds, and the `[specs.<key>]` description
//! overrides a plugin applies over the central `builtin.toml` attribute specs.

use code_ranker_plugin_api::level::AttributeSpec;
use code_ranker_plugin_api::plugin::Preset;
use serde::Deserialize;
use std::collections::BTreeMap;
use toml::Table;

/// One `[[presets]]` entry as read from config. Mirrors the data shape of the
/// CLI's generic preset catalog; the plugin turns it into a
/// `code_ranker_plugin_api::plugin::Preset`, deriving `doc_url` from a `slug`.
#[derive(Debug, Clone, Deserialize)]
pub struct PresetCfg {
    pub id: String,
    pub title: String,
    pub sort_metric: String,
    #[serde(default)]
    pub connections: Vec<String>,
    pub slug: String,
    pub prompt: String,
}

/// One `[specs.<key>]` entry: per-language overrides applied over the central
/// `builtin.toml` attribute specs. Only the fields a language tweaks are set;
/// the rest are left untouched on the inherited spec.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SpecOverride {
    #[serde(default)]
    pub description: Option<String>,
}

/// One `[thresholds.<key>]` row.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ThresholdCfg {
    pub info: f64,
    pub warning: f64,
}

/// Read the `[[presets]]` array from a merged config (empty if absent).
pub fn presets(cfg: &Table) -> Vec<PresetCfg> {
    cfg.get("presets")
        .cloned()
        .map(|v| v.try_into().expect("[[presets]] shape"))
        .unwrap_or_default()
}

/// Read a top-level string key from a merged config.
fn string_field<'a>(cfg: &'a Table, key: &str) -> Option<&'a str> {
    cfg.get(key)?.as_str()
}

/// Build the fully-resolved [`Preset`] list from a merged config: the common
/// catalog (from `defaults.toml`) plus any language-specific presets, in that
/// order (the merge-by-`id` already yields it). Each `doc_url` resolves to
/// `{doc_base}/{doc_lang}/{slug}.md` and `label` is the `id`.
///
/// `doc_base` (the host/repo prefix, common) lives in `defaults.toml`; each
/// `<lang>.toml` supplies `doc_lang` (its principle-corpus language). If either
/// is absent the `doc_url` is left `None`.
pub fn resolved_presets(cfg: &Table) -> Vec<Preset> {
    let base = string_field(cfg, "doc_base");
    let lang = string_field(cfg, "doc_lang");
    presets(cfg)
        .into_iter()
        .map(|p| Preset {
            doc_url: base
                .zip(lang)
                .map(|(b, l)| format!("{b}/{l}/{}.md", p.slug)),
            label: p.id.clone(),
            id: p.id,
            title: p.title,
            prompt: p.prompt,
            sort_metric: p.sort_metric,
            connections: p.connections,
        })
        .collect()
}

/// Read the `[thresholds]` table from a merged config as `key → (info, warning)`
/// (empty if absent).
pub fn thresholds(cfg: &Table) -> BTreeMap<String, ThresholdCfg> {
    cfg.get("thresholds")
        .cloned()
        .map(|v| v.try_into().expect("[thresholds] shape"))
        .unwrap_or_default()
}

/// Read the `[specs]` table from a merged config as `key → override`
/// (empty if absent).
pub fn spec_overrides(cfg: &Table) -> BTreeMap<String, SpecOverride> {
    cfg.get("specs")
        .cloned()
        .map(|v| v.try_into().expect("[specs] shape"))
        .unwrap_or_default()
}

/// Apply a config's `[specs.<key>]` description overrides over the central
/// builtin metric specs — the shared body of every plugin's `metric_specs`. A
/// language refines a metric's description (e.g. enumerating the exact Halstead
/// operators/operands it counts) without restating the rest of the spec; an
/// override whose key isn't a known metric is ignored.
pub fn apply_spec_overrides(
    mut defaults: BTreeMap<String, AttributeSpec>,
    cfg: &Table,
) -> BTreeMap<String, AttributeSpec> {
    for (key, ov) in spec_overrides(cfg) {
        if let Some(spec) = defaults.get_mut(&key)
            && let Some(desc) = ov.description
        {
            spec.description = Some(desc);
        }
    }
    defaults
}
